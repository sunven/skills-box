//! Read-only skill detail assembly and server-resolved skill roots.
//!
//! Detail combines the inventory scan with lock metadata and a capped read of
//! `SKILL.md` (1 MiB) for the description and warnings. Content browsing lives
//! in `content` and reads every file, including SKILL.md, through one path.
//! The frontend never supplies an absolute path.

use serde::Serialize;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::baseline::LocalModification;
use crate::error::AppError;
use crate::inventory::{AgentAttachment, AgentDir, ManagementState};
use crate::paths::AppPaths;

pub const SKILL_MD_LIMIT_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SkillMdStatus {
    Ok,
    Missing,
    TooLarge,
    Unreadable,
}

pub(crate) struct SkillMdRead {
    pub(crate) status: SkillMdStatus,
    pub(crate) content: Option<String>,
    pub(crate) error: Option<String>,
}

impl SkillMdRead {
    fn missing() -> Self {
        Self {
            status: SkillMdStatus::Missing,
            content: None,
            error: None,
        }
    }

    fn unreadable(error: impl Into<String>) -> Self {
        Self {
            status: SkillMdStatus::Unreadable,
            content: None,
            error: Some(error.into()),
        }
    }

    fn too_large() -> Self {
        Self {
            status: SkillMdStatus::TooLarge,
            content: None,
            error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDetail {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub source: Option<String>,
    pub source_type: Option<String>,
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    pub skill_path: Option<String>,
    pub installed_at: Option<String>,
    pub updated_at: Option<String>,
    pub canonical_path: Option<String>,
    /// Canonical path, or the agent copy path for copy-only skills.
    pub directory_path: Option<String>,
    pub directory_mtime_epoch_ms: Option<u64>,
    pub hash: Option<String>,
    pub management: ManagementState,
    pub local_modification: LocalModification,
    pub local_modification_detail: Option<String>,
    pub attachments: Vec<AgentAttachment>,
    pub warnings: Vec<String>,
}

pub fn read(paths: &AppPaths, skill_id: &str) -> Result<SkillDetail, AppError> {
    crate::read_model::ReadModel::new(paths.clone())
        .detail(&crate::scope::ScopeId::global(), skill_id)
}

/// One-shot injectable entry point for tests and fixtures. Production reads go
/// through the shared `ReadModel` held by `AppState`.
pub fn read_with_agents(
    paths: &AppPaths,
    agents: &[AgentDir],
    skill_id: &str,
) -> Result<SkillDetail, AppError> {
    crate::read_model::detail_with_agents(paths, agents, skill_id)
}

pub(crate) fn read_skill_md(directory: &Path) -> SkillMdRead {
    let path = directory.join("SKILL.md");

    let link_metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return SkillMdRead::missing();
        }
        Err(error) => {
            return SkillMdRead::unreadable(error.to_string());
        }
    };

    if link_metadata.file_type().is_symlink() {
        let resolved = match fs::canonicalize(&path) {
            Ok(resolved) => resolved,
            Err(error) => {
                return SkillMdRead::unreadable(format!("SKILL.md 符号链接不可解析：{error}"));
            }
        };
        let directory_resolved =
            fs::canonicalize(directory).unwrap_or_else(|_| directory.to_path_buf());
        if !resolved.starts_with(&directory_resolved) {
            return SkillMdRead::unreadable("SKILL.md 符号链接指向 skill 目录之外，已拒绝读取。");
        }
    }

    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) => {
            return SkillMdRead::unreadable(error.to_string());
        }
    };
    if !metadata.is_file() {
        return SkillMdRead::unreadable("SKILL.md 不是普通文件。");
    }
    if metadata.len() > SKILL_MD_LIMIT_BYTES {
        return SkillMdRead::too_large();
    }

    let file = match File::open(&path) {
        Ok(file) => file,
        Err(error) => {
            return SkillMdRead::unreadable(error.to_string());
        }
    };
    let mut bytes = Vec::new();
    if let Err(error) = file.take(SKILL_MD_LIMIT_BYTES + 1).read_to_end(&mut bytes) {
        return SkillMdRead::unreadable(error.to_string());
    }
    if bytes.len() as u64 > SKILL_MD_LIMIT_BYTES {
        return SkillMdRead::too_large();
    }
    let content = match String::from_utf8(bytes) {
        Ok(content) => content,
        Err(error) => {
            return SkillMdRead::unreadable(format!("SKILL.md 不是有效的 UTF-8：{error}"));
        }
    };
    SkillMdRead {
        status: SkillMdStatus::Ok,
        content: Some(content),
        error: None,
    }
}

/// Resolve and guard the physical skill folder used by content browsing and
/// `open_skill_folder`.
pub fn resolve_skill_root(
    paths: &AppPaths,
    agents: &[AgentDir],
    skill_id: &str,
) -> Result<PathBuf, AppError> {
    crate::read_model::resolve_root_with_agents(paths, agents, skill_id)
}

pub(crate) fn directory_mtime_epoch_ms(path: &Path) -> Option<u64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    modified
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::baseline::git_tree_hash;
    use crate::error::ErrorCode;
    use std::os::unix::fs::{PermissionsExt, symlink};

    struct Fixture {
        _tmp: tempfile::TempDir,
        paths: AppPaths,
        agents: Vec<AgentDir>,
        claude: PathBuf,
        canonical: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path().join("home");
            fs::create_dir_all(home.join(".agents/skills")).unwrap();
            fs::create_dir_all(home.join(".claude/skills")).unwrap();
            let canonical = home.join(".agents/skills");
            let claude = home.join(".claude/skills");
            let paths = AppPaths::new(&home, tmp.path().join("app-data"));
            let agents = vec![AgentDir {
                id: "claude-code".to_string(),
                display_name: "Claude Code".to_string(),
                universal: false,
                skills_dir: claude.clone(),
            }];
            Self {
                _tmp: tmp,
                paths,
                agents,
                claude,
                canonical,
            }
        }

        fn lock_entry(name: &str, hash: &str) -> serde_json::Value {
            serde_json::json!({
                "source": format!("acme/{name}"),
                "sourceType": "github",
                "sourceUrl": format!("https://github.com/acme/{name}.git"),
                "ref": "main",
                "skillPath": format!("skills/{name}/SKILL.md"),
                "skillFolderHash": hash,
                "installedAt": "2026-01-01T00:00:00.000Z",
                "updatedAt": "2026-02-03T04:05:06.000Z"
            })
        }

        fn write_lock(&self, entries: &[(&str, String)]) {
            let mut skills = serde_json::Map::new();
            for (name, hash) in entries {
                skills.insert((*name).to_string(), Self::lock_entry(name, hash));
            }
            let lock = serde_json::json!({"version": 3, "skills": skills});
            fs::create_dir_all(self.paths.agents_root()).unwrap();
            fs::write(self.paths.lock_path(), lock.to_string()).unwrap();
        }

        fn canonical_skill(&self, name: &str, body: &str) -> PathBuf {
            let dir = self.canonical.join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: {name} 的描述\n---\n{body}"),
            )
            .unwrap();
            dir
        }

        fn link(&self, root: &Path, name: &str, target: &Path) {
            symlink(target, root.join(name)).unwrap();
        }

        fn detail(&self, name: &str) -> SkillDetail {
            read_with_agents(&self.paths, &self.agents, name).unwrap()
        }
    }

    #[test]
    fn assembles_managed_detail_with_lock_metadata_and_rendered_source() {
        let fixture = Fixture::new();
        let dir = fixture.canonical_skill("alpha", "# Alpha\n\nBody\n");
        let hash = git_tree_hash(&dir).unwrap();
        fixture.write_lock(&[("alpha", hash.clone())]);
        fixture.link(&fixture.claude, "alpha", &dir);

        let detail = fixture.detail("alpha");
        assert_eq!(detail.id, "alpha");
        assert_eq!(detail.name, "alpha");
        assert_eq!(detail.description.as_deref(), Some("alpha 的描述"));
        assert_eq!(detail.source.as_deref(), Some("acme/alpha"));
        assert_eq!(detail.source_type.as_deref(), Some("github"));
        assert_eq!(detail.reference.as_deref(), Some("main"));
        assert_eq!(detail.skill_path.as_deref(), Some("skills/alpha/SKILL.md"));
        assert_eq!(
            detail.installed_at.as_deref(),
            Some("2026-01-01T00:00:00.000Z")
        );
        assert_eq!(
            detail.updated_at.as_deref(),
            Some("2026-02-03T04:05:06.000Z")
        );
        assert_eq!(
            detail.canonical_path.as_deref(),
            Some("~/.agents/skills/alpha")
        );
        assert_eq!(
            detail.directory_path.as_deref(),
            Some("~/.agents/skills/alpha")
        );
        assert_eq!(detail.hash.as_deref(), Some(hash.as_str()));
        assert_eq!(detail.management, ManagementState::Managed);
        assert_eq!(detail.local_modification, LocalModification::Clean);
        assert!(detail.directory_mtime_epoch_ms.is_some());
        assert_eq!(detail.attachments.len(), 1);
        assert_eq!(
            detail.attachments[0].mode,
            crate::inventory::AttachmentMode::Link
        );
        assert!(
            detail.attachments[0].detachable,
            "managed canonical + non-universal link is detachable"
        );
        let skill_md = read_skill_md(&dir);
        assert_eq!(skill_md.status, SkillMdStatus::Ok);
        assert!(skill_md.content.unwrap().contains("---\nname: alpha"));
        assert!(detail.warnings.is_empty(), "{:?}", detail.warnings);

        let json = serde_json::to_value(&detail).unwrap();
        assert_eq!(json["localModification"], "clean");
        assert_eq!(json["management"], "managed");
        assert_eq!(json["ref"], "main");
        assert!(json.get("skillMd").is_none());
        assert_eq!(json["attachments"][0]["agentId"], "claude-code");
        assert_eq!(json["attachments"][0]["detachable"], true);
        assert!(json["directoryMtimeEpochMs"].as_u64().unwrap() > 0);
    }

    #[test]
    fn copy_only_detail_uses_the_agent_path_and_has_no_lock_times() {
        let fixture = Fixture::new();
        let copy = fixture.claude.join("beta");
        fs::create_dir_all(&copy).unwrap();
        fs::write(
            copy.join("SKILL.md"),
            "---\nname: beta\ndescription: copy\n---\n# Beta\n",
        )
        .unwrap();
        fixture.write_lock(&[("beta", String::new())]);

        let detail = fixture.detail("beta");
        assert_eq!(detail.management, ManagementState::CopyOnly);
        assert!(detail.canonical_path.is_none());
        assert_eq!(
            detail.directory_path.as_deref(),
            Some("~/.claude/skills/beta")
        );
        assert!(detail.hash.is_none());
        assert_eq!(detail.local_modification, LocalModification::Unknown);
        assert!(detail.local_modification_detail.is_some());
        assert_eq!(detail.attachments.len(), 1);
        assert_eq!(
            detail.attachments[0].mode,
            crate::inventory::AttachmentMode::Copy
        );
    }

    #[test]
    fn unmanaged_detail_is_read_only_with_directory_mtime() {
        let fixture = Fixture::new();
        let dir = fixture.canonical_skill("gamma", "# Gamma\n");

        let detail = fixture.detail("gamma");
        assert_eq!(detail.management, ManagementState::Unmanaged);
        assert!(detail.source.is_none());
        assert!(detail.installed_at.is_none());
        assert!(detail.updated_at.is_none());
        assert!(detail.hash.is_none());
        assert!(detail.directory_mtime_epoch_ms.is_some());
        assert_eq!(detail.local_modification, LocalModification::Unknown);
        assert_eq!(
            detail.canonical_path.as_deref(),
            Some("~/.agents/skills/gamma")
        );
        assert!(dir.exists());
    }

    #[test]
    fn missing_skill_md_is_reported_and_does_not_fail_detail() {
        let fixture = Fixture::new();
        let dir = fixture.canonical.join("zeta");
        fs::create_dir_all(&dir).unwrap();
        fixture.write_lock(&[("zeta", "0".repeat(40))]);

        let detail = fixture.detail("zeta");
        let skill_md = read_skill_md(&dir);
        assert_eq!(skill_md.status, SkillMdStatus::Missing);
        assert!(skill_md.content.is_none());
        assert!(detail.description.is_none());
        assert!(
            detail
                .warnings
                .iter()
                .any(|warning| warning.contains("未找到 SKILL.md")),
            "{:?}",
            detail.warnings
        );
    }

    #[test]
    fn skill_md_over_one_mib_is_not_read() {
        let fixture = Fixture::new();
        let dir = fixture.canonical.join("huge");
        fs::create_dir_all(&dir).unwrap();
        let file = File::create(dir.join("SKILL.md")).unwrap();
        file.set_len(SKILL_MD_LIMIT_BYTES + 1).unwrap();
        fixture.write_lock(&[("huge", "0".repeat(40))]);

        let detail = fixture.detail("huge");
        let skill_md = read_skill_md(&dir);
        assert_eq!(skill_md.status, SkillMdStatus::TooLarge);
        assert!(skill_md.content.is_none());
        assert!(
            detail
                .warnings
                .iter()
                .any(|warning| warning.contains("1 MiB")),
            "{:?}",
            detail.warnings
        );
    }

    #[test]
    fn skill_md_at_exactly_one_mib_is_read() {
        let fixture = Fixture::new();
        let dir = fixture.canonical.join("boundary");
        fs::create_dir_all(&dir).unwrap();
        let file = File::create(dir.join("SKILL.md")).unwrap();
        file.set_len(SKILL_MD_LIMIT_BYTES).unwrap();
        fixture.write_lock(&[("boundary", "0".repeat(40))]);

        let skill_md = read_skill_md(&dir);
        assert_eq!(skill_md.status, SkillMdStatus::Ok);
        assert_eq!(
            skill_md.content.as_ref().unwrap().len() as u64,
            SKILL_MD_LIMIT_BYTES
        );
    }

    #[test]
    fn invalid_utf8_and_malformed_frontmatter_have_distinct_warnings() {
        let fixture = Fixture::new();
        let invalid = fixture.canonical.join("invalid");
        fs::create_dir_all(&invalid).unwrap();
        fs::write(invalid.join("SKILL.md"), [0xff, 0xfe, 0xfd]).unwrap();
        fixture.write_lock(&[("invalid", "0".repeat(40))]);
        let detail = fixture.detail("invalid");
        assert_eq!(read_skill_md(&invalid).status, SkillMdStatus::Unreadable);
        assert!(
            detail
                .warnings
                .iter()
                .any(|warning| warning.contains("UTF-8"))
        );

        let malformed = fixture.canonical.join("malformed");
        fs::create_dir_all(&malformed).unwrap();
        fs::write(
            malformed.join("SKILL.md"),
            "---\ndescription: \"unterminated\n---\n# Bad\n",
        )
        .unwrap();
        fixture.write_lock(&[("malformed", "0".repeat(40))]);
        let detail = fixture.detail("malformed");
        assert_eq!(read_skill_md(&malformed).status, SkillMdStatus::Ok);
        assert!(
            detail
                .warnings
                .iter()
                .any(|warning| warning.contains("frontmatter"))
        );
    }

    #[test]
    fn skill_md_symlink_outside_the_skill_folder_is_rejected() {
        let fixture = Fixture::new();
        let dir = fixture.canonical.join("sneaky");
        fs::create_dir_all(&dir).unwrap();
        let outside = fixture._tmp.path().join("secret.txt");
        fs::write(&outside, "secret").unwrap();
        symlink(&outside, dir.join("SKILL.md")).unwrap();
        fixture.write_lock(&[("sneaky", "0".repeat(40))]);

        let detail = fixture.detail("sneaky");
        assert_eq!(read_skill_md(&dir).status, SkillMdStatus::Unreadable);
        assert!(
            detail
                .warnings
                .iter()
                .any(|warning| warning.contains("目录之外"))
        );
    }

    #[test]
    fn skill_root_resolution_uses_server_paths_and_rejects_traversal() {
        let fixture = Fixture::new();
        let dir = fixture.canonical_skill("alpha", "# Alpha\n");
        fixture.write_lock(&[("alpha", git_tree_hash(&dir).unwrap())]);
        fixture.link(&fixture.claude, "alpha", &dir);

        let folder = resolve_skill_root(&fixture.paths, &fixture.agents, "alpha").unwrap();
        assert_eq!(folder, dir);

        let traversal =
            resolve_skill_root(&fixture.paths, &fixture.agents, "../alpha").unwrap_err();
        assert_eq!(traversal.code, ErrorCode::PathInvalid);

        let unknown = resolve_skill_root(&fixture.paths, &fixture.agents, "missing").unwrap_err();
        assert_eq!(unknown.code, ErrorCode::SkillNotFound);
    }

    #[test]
    fn skill_root_for_copy_only_uses_the_agent_copy() {
        let fixture = Fixture::new();
        let copy = fixture.claude.join("beta");
        fs::create_dir_all(&copy).unwrap();
        fs::write(copy.join("SKILL.md"), "---\nname: beta\n---\n").unwrap();
        fixture.write_lock(&[("beta", String::new())]);

        let folder = resolve_skill_root(&fixture.paths, &fixture.agents, "beta").unwrap();
        assert_eq!(folder, copy);
    }

    #[test]
    fn skill_root_rejects_ambiguous_copy_only_skills() {
        let fixture = Fixture::new();
        let claude_copy = fixture.claude.join("beta");
        fs::create_dir_all(&claude_copy).unwrap();
        fs::write(claude_copy.join("SKILL.md"), "# beta claude\n").unwrap();
        let gemini_root = fixture.paths.home().join(".gemini/skills");
        let gemini_copy = gemini_root.join("beta");
        fs::create_dir_all(&gemini_copy).unwrap();
        fs::write(gemini_copy.join("SKILL.md"), "# beta gemini\n").unwrap();
        fixture.write_lock(&[("beta", String::new())]);
        let mut agents = fixture.agents.clone();
        agents.push(AgentDir {
            id: "gemini-cli".to_string(),
            display_name: "Gemini CLI".to_string(),
            universal: false,
            skills_dir: gemini_root,
        });

        let error = resolve_skill_root(&fixture.paths, &agents, "beta").unwrap_err();
        assert_eq!(error.code, ErrorCode::SkillAmbiguous);
        assert!(error.detail.unwrap().contains("Gemini CLI"));
    }

    #[test]
    fn skill_root_rejects_an_agent_root_that_is_a_symlink() {
        let fixture = Fixture::new();
        let real = fixture._tmp.path().join("real-claude");
        fs::create_dir_all(real.join("beta")).unwrap();
        fs::write(real.join("beta/SKILL.md"), "---\nname: beta\n---\n").unwrap();
        fs::remove_dir_all(&fixture.claude).unwrap();
        symlink(&real, &fixture.claude).unwrap();
        fixture.write_lock(&[("beta", String::new())]);

        let error = resolve_skill_root(&fixture.paths, &fixture.agents, "beta").unwrap_err();
        assert_eq!(error.code, ErrorCode::PathInvalid);
    }

    #[test]
    fn skill_id_validation_rejects_paths() {
        let fixture = Fixture::new();
        for id in ["", "..", "a/b", "a\\b", ".hidden"] {
            let error = read_with_agents(&fixture.paths, &fixture.agents, id).unwrap_err();
            assert_eq!(error.code, ErrorCode::PathInvalid, "{id:?}");
        }
    }

    #[test]
    fn skill_md_file_permissions_do_not_affect_readability() {
        let fixture = Fixture::new();
        let dir = fixture.canonical_skill("alpha", "# Alpha\n");
        let md = dir.join("SKILL.md");
        fs::set_permissions(&md, fs::Permissions::from_mode(0o644)).unwrap();
        fixture.write_lock(&[("alpha", "0".repeat(40))]);
        let _ = fixture.detail("alpha");
        assert_eq!(read_skill_md(&dir).status, SkillMdStatus::Ok);
    }

    #[test]
    fn unreadable_skill_md_returns_a_degraded_detail_without_failing_other_skills() {
        use std::os::unix::fs::PermissionsExt;
        let fixture = Fixture::new();
        let alpha = fixture.canonical_skill("alpha", "# Alpha\n");
        fixture.canonical_skill("beta", "# Beta\n");
        fixture.write_lock(&[("alpha", "0".repeat(40)), ("beta", "1".repeat(40))]);
        let alpha_md = alpha.join("SKILL.md");
        fs::set_permissions(&alpha_md, fs::Permissions::from_mode(0o000)).unwrap();

        let detail = fixture.detail("alpha");
        assert_eq!(read_skill_md(&alpha).status, SkillMdStatus::Unreadable);
        fs::set_permissions(&alpha_md, fs::Permissions::from_mode(0o644)).unwrap();

        assert!(
            detail
                .warnings
                .iter()
                .any(|warning| warning.contains("无法读取 SKILL.md")),
            "{:?}",
            detail.warnings
        );

        let beta_dir = fixture.canonical.join("beta");
        let beta = fixture.detail("beta");
        assert_eq!(read_skill_md(&beta_dir).status, SkillMdStatus::Ok);
        assert!(beta.warnings.is_empty());
    }
}
