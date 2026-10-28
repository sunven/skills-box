//! Path guards for server-resolved skill locations.
//!
//! The frontend only ever sends a skill id; Rust resolves the real folder from
//! the inventory scan. Before handing any path to the opener we verify that it
//! is lexically and physically inside the canonical store or a detected agent
//! skills directory. Agent skills roots that are themselves symlinks are
//! rejected because they make containment checks ambiguous.

use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootKind {
    Canonical,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowedRoot {
    pub path: PathBuf,
    pub kind: RootKind,
}

impl AllowedRoot {
    pub fn canonical(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            kind: RootKind::Canonical,
        }
    }

    pub fn agent(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            kind: RootKind::Agent,
        }
    }
}

/// Skill ids are directory names, never paths. Reject separators, traversal,
/// NUL bytes, hidden entries (the inventory skips them), and overlong names.
pub fn validate_skill_id(skill_id: &str) -> Result<(), AppError> {
    let invalid = skill_id.is_empty()
        || skill_id.len() > 255
        || skill_id == "."
        || skill_id == ".."
        || skill_id.starts_with('.')
        || skill_id.contains(['/', '\\', '\0']);
    if invalid {
        return Err(AppError::path_invalid(
            "skillId 必须是 inventory 中的单个目录名。",
        ));
    }
    Ok(())
}

/// Lexically normalize an absolute path (`/a/./b/../c` -> `/a/c`). Returns
/// `None` when the path is relative or traversal escapes the root.
pub fn normalize_absolute(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => normalized.push(Path::new("/")),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::Normal(part) => normalized.push(part),
            Component::Prefix(_) => return None,
        }
    }
    Some(normalized)
}

/// Verify `candidate` is inside one of the allowed roots.
///
/// Both the lexical path and the resolved path (when it exists) must be inside
/// an allowed root. An agent skills root that is a symlink is rejected before
/// any containment check.
pub fn ensure_allowed(
    candidate: &Path,
    roots: &[AllowedRoot],
) -> Result<AllowedRoot, AppError> {
    let normalized = normalize_absolute(candidate)
        .ok_or_else(|| AppError::path_invalid("路径必须是绝对路径。"))?;

    let root = roots
        .iter()
        .find(|root| {
            normalize_absolute(&root.path)
                .map(|root_path| normalized.starts_with(&root_path))
                .unwrap_or(false)
        })
        .cloned()
        .ok_or_else(|| {
            AppError::path_outside_allowed_root(candidate.display().to_string())
        })?;

    if root.kind == RootKind::Agent {
        let metadata = fs::symlink_metadata(&root.path).map_err(|error| {
            AppError::path_invalid(format!(
                "无法检查 agent skills 目录：{}（{error}）",
                root.path.display()
            ))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(AppError::path_invalid(format!(
                "agent skills 目录本身是符号链接，已拒绝打开：{}",
                root.path.display()
            )));
        }
    }

    if candidate.exists() {
        let resolved = fs::canonicalize(candidate).map_err(|error| {
            AppError::path_invalid(format!(
                "无法解析路径 {}：{error}",
                candidate.display()
            ))
        })?;
        let physically_allowed = roots.iter().any(|root| {
            fs::canonicalize(&root.path)
                .map(|root_path| resolved.starts_with(&root_path))
                .unwrap_or(false)
        });
        if !physically_allowed {
            return Err(AppError::path_outside_allowed_root(format!(
                "{} -> {}",
                candidate.display(),
                resolved.display()
            )));
        }
    }

    Ok(root)
}

/// True when `candidate` is lexically inside at least one root. Useful for
/// tests and for callers that only need the cheap check.
pub fn is_within(candidate: &Path, roots: &[AllowedRoot]) -> bool {
    let Some(normalized) = normalize_absolute(candidate) else {
        return false;
    };
    roots.iter().any(|root| {
        normalize_absolute(&root.path)
            .map(|root_path| normalized.starts_with(root_path))
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn setup() -> (
        tempfile::TempDir,
        PathBuf,
        PathBuf,
        Vec<AllowedRoot>,
    ) {
        let tmp = tempfile::tempdir().unwrap();
        let canonical = tmp.path().join("home/.agents/skills");
        let agent = tmp.path().join("home/.claude/skills");
        fs::create_dir_all(&canonical).unwrap();
        fs::create_dir_all(&agent).unwrap();
        fs::create_dir_all(canonical.join("alpha")).unwrap();
        fs::create_dir_all(agent.join("copy-only")).unwrap();
        let roots = vec![
            AllowedRoot::canonical(canonical.clone()),
            AllowedRoot::agent(agent.clone()),
        ];
        (tmp, canonical, agent, roots)
    }

    #[test]
    fn rejects_path_traversal_and_separators_in_skill_ids() {
        for id in [
            "", ".", "..", "../alpha", "alpha/beta", "alpha\\beta", ".hidden",
            "alpha\0beta",
        ] {
            let error = validate_skill_id(id).unwrap_err();
            assert_eq!(error.code, crate::error::codes::PATH_INVALID, "{id:?}");
        }
        for id in ["alpha", "ce-review", "skill_1.2"] {
            validate_skill_id(id).unwrap();
        }
    }

    #[test]
    fn accepts_children_of_canonical_and_agent_roots() {
        let (_tmp, canonical, agent, roots) = setup();
        assert_eq!(
            ensure_allowed(&canonical.join("alpha"), &roots).unwrap().kind,
            RootKind::Canonical
        );
        assert_eq!(
            ensure_allowed(&agent.join("copy-only"), &roots)
                .unwrap()
                .kind,
            RootKind::Agent
        );
    }

    #[test]
    fn rejects_paths_outside_allowed_roots() {
        let (tmp, canonical, _agent, roots) = setup();
        let error = ensure_allowed(&tmp.path().join("elsewhere/alpha"), &roots).unwrap_err();
        assert_eq!(
            error.code,
            crate::error::codes::PATH_OUTSIDE_ALLOWED_ROOT
        );

        let traversal = canonical.join("../evil");
        let error = ensure_allowed(&traversal, &roots).unwrap_err();
        assert_eq!(
            error.code,
            crate::error::codes::PATH_OUTSIDE_ALLOWED_ROOT
        );
    }

    #[test]
    fn rejects_agent_root_that_is_itself_a_symlink() {
        let (tmp, canonical, agent, mut roots) = setup();
        let real_agent = tmp.path().join("real-agent-skills");
        fs::create_dir_all(real_agent.join("alpha")).unwrap();
        fs::remove_dir_all(&agent).unwrap();
        symlink(&real_agent, &agent).unwrap();
        roots[1] = AllowedRoot::agent(agent.clone());

        let error = ensure_allowed(&agent.join("alpha"), &roots).unwrap_err();
        assert_eq!(error.code, crate::error::codes::PATH_INVALID);
        assert!(error.detail.as_deref().unwrap().contains("符号链接"));

        // Canonical children remain usable.
        ensure_allowed(&canonical.join("alpha"), &roots).unwrap();
    }

    #[test]
    fn rejects_leaf_symlinks_that_resolve_outside_allowed_roots() {
        let (tmp, _canonical, agent, roots) = setup();
        let outside = tmp.path().join("outside-copy");
        fs::create_dir_all(&outside).unwrap();
        symlink(&outside, agent.join("escaped")).unwrap();

        let error = ensure_allowed(&agent.join("escaped"), &roots).unwrap_err();
        assert_eq!(
            error.code,
            crate::error::codes::PATH_OUTSIDE_ALLOWED_ROOT
        );
    }

    #[test]
    fn allows_a_canonical_root_that_is_a_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real-store");
        let root = tmp.path().join("home/.agents/skills");
        fs::create_dir_all(real.join("alpha")).unwrap();
        fs::create_dir_all(root.parent().unwrap()).unwrap();
        symlink(&real, &root).unwrap();
        let roots = vec![AllowedRoot::canonical(root.clone())];
        ensure_allowed(&root.join("alpha"), &roots).unwrap();
    }

    #[test]
    fn normalization_and_within_helpers_are_lexical() {
        let roots = vec![AllowedRoot::canonical("/Users/tester/.agents/skills")];
        assert!(is_within(
            Path::new("/Users/tester/.agents/skills/alpha"),
            &roots
        ));
        assert!(!is_within(
            Path::new("/Users/tester/.agents/skills/../evil"),
            &roots
        ));
        assert!(!is_within(
            Path::new("/Users/tester/.agents/skills-other/alpha"),
            &roots
        ));
        assert_eq!(
            normalize_absolute(Path::new("/a/./b/../c")),
            Some(PathBuf::from("/a/c"))
        );
        assert_eq!(normalize_absolute(Path::new("relative/path")), None);
    }
}
