//! One gate for Agent attachment eligibility and target resolution.
//!
//! Attach, detach and install's extra-agent linking used to re-decode the
//! management × agent × attachment-mode matrix and the target path guards.
//! This module owns the shared matrix; the mutation modules still perform the
//! symlink/unlink, backup, dependency scan, verification and logging.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::AppError;
use crate::inventory::{
    self, AgentDir, AttachmentLocation, AttachmentMode, LocatedSkill, ManagementState,
};
use crate::project_path::{self, Existence, PathError};
use crate::security::normalize_absolute;

/// Path context for one scope's attachment targets. Project scope passes its
/// canonicalized root so every target is proven to stay inside it; global
/// scope has no project root.
#[derive(Debug, Clone, Copy, Default)]
pub struct TargetContext<'a> {
    pub project_root: Option<&'a Path>,
}

impl<'a> TargetContext<'a> {
    pub fn global() -> Self {
        Self { project_root: None }
    }

    pub fn project(root: &'a Path) -> Self {
        Self {
            project_root: Some(root),
        }
    }

    pub fn for_layout(layout: &'a crate::scope_layout::ScopeLayout) -> Self {
        Self {
            project_root: layout.project_root(),
        }
    }

    fn ensure_inside(&self, path: &Path) -> Result<(), AppError> {
        let Some(root) = self.project_root else {
            return Ok(());
        };
        let target = project_path::lexical(root, path).map_err(|error| match error {
            PathError::InvalidRoot => AppError::path_invalid("项目根路径无法规范化。"),
            PathError::LexicalEscape(target) => {
                AppError::path_outside_allowed_root(format!("{} 位于项目根之外", target.display()))
            }
            _ => AppError::path_invalid(format!("路径无效：{}", path.display())),
        })?;
        // Preserve attachment's historical root-resolution fallback. Missing
        // targets still require their nearest existing ancestor to stay inside.
        let root = normalize_absolute(root).expect("root checked by lexical containment");
        let root_real = fs::canonicalize(&root).unwrap_or(root);
        project_path::resolve(&target, &root_real, Existence::AllowMissing).map_err(|error| {
            match error {
                PathError::Inspect { path, error } => {
                    AppError::path_invalid(format!("无法检查目标路径 {}：{error}", path.display()))
                }
                PathError::Resolve { path, error } => AppError::path_invalid(format!(
                    "无法解析目标路径的父目录 {}：{error}",
                    path.display()
                )),
                PathError::PhysicalEscape(_) => AppError::path_outside_allowed_root(format!(
                    "{} 的符号链接解析后位于项目根之外",
                    target.display()
                )),
                _ => AppError::path_invalid(format!(
                    "无法检查目标路径的父目录：{}",
                    target.display()
                )),
            }
        })?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetState {
    Absent,
    SymlinkToCanonical,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentDirectoryState {
    Ready,
    /// The directory does not exist; the caller may create it.
    Missing(String),
    Symlink,
    NotDirectory,
    Unreadable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAttachment {
    pub agent: AgentDir,
    pub canonical: PathBuf,
    pub target: PathBuf,
    pub existing: Option<AttachmentLocation>,
    pub target_state: TargetState,
    pub directory_state: AgentDirectoryState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetachEligibility {
    Detachable,
    Unmanaged,
    CopyOnly,
    NativeAgent,
    NativeAttachment,
    Broken,
    NotAttached,
}

pub fn find_agent<'a>(agents: &'a [AgentDir], agent_id: &str) -> Result<&'a AgentDir, AppError> {
    agents
        .iter()
        .find(|agent| agent.id == agent_id)
        .ok_or_else(|| AppError::agent_not_detected(agent_id))
}

/// Resolve a native attach target. The caller has already validated the skill
/// id/name, read the lock and located the skill.
pub fn resolve_attach(
    canonical_dir: &Path,
    located: &LocatedSkill,
    agents: &[AgentDir],
    agent_id: &str,
    context: TargetContext<'_>,
) -> Result<ResolvedAttachment, AppError> {
    resolve_attach_inner(canonical_dir, located, agents, agent_id, context, true)
}

/// Copy to project previews an attachment before its canonical folder is
/// published. All other native Attach gates remain the same.
pub(crate) fn resolve_copy_attach(
    canonical_dir: &Path,
    located: &LocatedSkill,
    agents: &[AgentDir],
    agent_id: &str,
    context: TargetContext<'_>,
) -> Result<ResolvedAttachment, AppError> {
    resolve_attach_inner(canonical_dir, located, agents, agent_id, context, false)
}

fn resolve_attach_inner(
    canonical_dir: &Path,
    located: &LocatedSkill,
    agents: &[AgentDir],
    agent_id: &str,
    context: TargetContext<'_>,
    require_canonical: bool,
) -> Result<ResolvedAttachment, AppError> {
    let skill_id = located.name.as_str();
    let canonical = managed_canonical(located, skill_id)?;
    if require_canonical
        && !fs::metadata(canonical)
            .map(|metadata| metadata.is_dir())
            .unwrap_or(false)
    {
        return Err(AppError::verify_canonical_missing(skill_id, canonical));
    }

    let agent = find_agent(agents, agent_id)?;
    ensure_not_universal(agent, canonical_dir)?;
    context.ensure_inside(&agent.skills_dir)?;
    if let Some(existing) = located
        .attachments
        .iter()
        .find(|attachment| attachment.agent_id == agent_id)
    {
        return Err(match existing.mode {
            AttachmentMode::Native => AppError::agent_native(&agent.display_name),
            AttachmentMode::Broken => AppError::agent_broken(&agent.display_name, &existing.path),
            AttachmentMode::Link | AttachmentMode::Copy => {
                AppError::agent_already_attached(&agent.display_name)
            }
        });
    }

    let target = strict_target(&agent.skills_dir, skill_id, canonical, false)?;
    context.ensure_inside(&target)?;
    let directory_state = match inspect_agent_dir(&agent.skills_dir) {
        AgentDirectoryState::Symlink => {
            return Err(AppError::path_invalid(format!(
                "agent skills 目录本身是符号链接：{}",
                agent.skills_dir.display()
            )));
        }
        AgentDirectoryState::NotDirectory => {
            return Err(AppError::path_invalid(format!(
                "agent skills 路径不是目录：{}",
                agent.skills_dir.display()
            )));
        }
        AgentDirectoryState::Unreadable(error) => {
            return Err(AppError::agent_attach_failed(format!(
                "无法检查 {}：{error}",
                agent.skills_dir.display()
            )));
        }
        state @ (AgentDirectoryState::Ready | AgentDirectoryState::Missing(_)) => state,
    };
    let target_state = attach_target_state(&target)?;

    Ok(ResolvedAttachment {
        agent: agent.clone(),
        canonical: canonical.to_path_buf(),
        target,
        existing: None,
        target_state,
        directory_state,
    })
}

/// Resolve a detach target. The caller has already validated the skill
/// id/name, read the lock and located the skill.
pub fn resolve_detach(
    canonical_dir: &Path,
    located: &LocatedSkill,
    agents: &[AgentDir],
    agent_id: &str,
    context: TargetContext<'_>,
) -> Result<ResolvedAttachment, AppError> {
    let skill_id = located.name.as_str();
    let canonical = managed_canonical(located, skill_id)?;
    let agent = find_agent(agents, agent_id)?;
    ensure_not_universal(agent, canonical_dir)?;
    context.ensure_inside(&agent.skills_dir)?;
    let existing = located
        .attachments
        .iter()
        .find(|attachment| attachment.agent_id == agent_id)
        .ok_or_else(|| AppError::agent_not_attached(&agent.display_name))?;
    match existing.mode {
        AttachmentMode::Native => return Err(AppError::agent_native(&agent.display_name)),
        AttachmentMode::Broken => {
            return Err(AppError::agent_broken(&agent.display_name, &existing.path));
        }
        AttachmentMode::Link | AttachmentMode::Copy => {}
    }

    // Detach validates the agent directory before the target path, matching
    // the historical error order.
    let directory_state = match inspect_agent_dir(&agent.skills_dir) {
        AgentDirectoryState::Symlink => {
            return Err(AppError::path_invalid(format!(
                "agent skills 目录本身是符号链接，已拒绝断开：{}",
                agent.skills_dir.display()
            )));
        }
        AgentDirectoryState::NotDirectory => {
            return Err(AppError::path_invalid(format!(
                "agent skills 路径不是目录：{}",
                agent.skills_dir.display()
            )));
        }
        AgentDirectoryState::Missing(error) | AgentDirectoryState::Unreadable(error) => {
            return Err(AppError::path_invalid(format!(
                "无法检查 agent skills 目录 {}：{error}",
                agent.skills_dir.display()
            )));
        }
        AgentDirectoryState::Ready => AgentDirectoryState::Ready,
    };
    let target = strict_target(&agent.skills_dir, skill_id, canonical, true)?;
    context.ensure_inside(&target)?;
    let target_state = detach_target_state(&target, canonical);

    Ok(ResolvedAttachment {
        agent: agent.clone(),
        canonical: canonical.to_path_buf(),
        target,
        existing: Some(existing.clone()),
        target_state,
        directory_state,
    })
}

/// Resolve one extra-agent link for install. This keeps the historical
/// relaxed profile: no direct-child/canonical-target guards, and a missing
/// agent directory is reported as `Missing` for the caller to create.
pub fn resolve_install_link(
    canonical: &Path,
    agent: &AgentDir,
    name: &str,
    context: TargetContext<'_>,
) -> Result<ResolvedAttachment, AppError> {
    context.ensure_inside(&agent.skills_dir)?;
    let directory_state = match inspect_agent_dir(&agent.skills_dir) {
        AgentDirectoryState::Symlink => {
            return Err(AppError::agent_attach_failed(format!(
                "额外 agent 的 skills 目录本身是符号链接：{}",
                agent.skills_dir.display()
            )));
        }
        AgentDirectoryState::NotDirectory => {
            return Err(AppError::agent_attach_failed(format!(
                "额外 agent 的 skills 路径不是目录：{}",
                agent.skills_dir.display()
            )));
        }
        AgentDirectoryState::Unreadable(error) => {
            return Err(AppError::agent_attach_failed(format!(
                "无法检查 {}：{error}",
                agent.skills_dir.display()
            )));
        }
        state @ (AgentDirectoryState::Ready | AgentDirectoryState::Missing(_)) => state,
    };
    let target = agent.skills_dir.join(name);
    context.ensure_inside(&target)?;
    let target_state = install_target_state(&target, canonical)?;

    Ok(ResolvedAttachment {
        agent: agent.clone(),
        canonical: canonical.to_path_buf(),
        target,
        existing: None,
        target_state,
        directory_state,
    })
}

/// The row-level rule shared by detach and the inventory projection. The UI
/// only needs `Detachable`; detach maps each rejection to its existing error.
pub fn detach_eligibility(
    management: ManagementState,
    has_canonical: bool,
    universal_or_shared: bool,
    attachment: Option<AttachmentMode>,
) -> DetachEligibility {
    if management == ManagementState::Unmanaged {
        return DetachEligibility::Unmanaged;
    }
    if management == ManagementState::CopyOnly || !has_canonical {
        return DetachEligibility::CopyOnly;
    }
    if universal_or_shared {
        return DetachEligibility::NativeAgent;
    }
    match attachment {
        None => DetachEligibility::NotAttached,
        Some(AttachmentMode::Native) => DetachEligibility::NativeAttachment,
        Some(AttachmentMode::Broken) => DetachEligibility::Broken,
        Some(AttachmentMode::Link | AttachmentMode::Copy) => DetachEligibility::Detachable,
    }
}

fn managed_canonical<'a>(located: &'a LocatedSkill, skill_id: &str) -> Result<&'a Path, AppError> {
    match located.management {
        ManagementState::Managed => {}
        ManagementState::CopyOnly => return Err(AppError::skill_copy_only(skill_id)),
        ManagementState::Unmanaged => return Err(AppError::skill_unmanaged(skill_id)),
    }
    located
        .canonical_path
        .as_deref()
        .ok_or_else(|| AppError::skill_copy_only(skill_id))
}

fn ensure_not_universal(agent: &AgentDir, canonical_dir: &Path) -> Result<(), AppError> {
    if agent.universal || inventory::same_dir(&agent.skills_dir, canonical_dir) {
        return Err(AppError::agent_native(&agent.display_name));
    }
    Ok(())
}

fn inspect_agent_dir(agent_dir: &Path) -> AgentDirectoryState {
    match fs::symlink_metadata(agent_dir) {
        Ok(metadata) if metadata.file_type().is_symlink() => AgentDirectoryState::Symlink,
        Ok(metadata) if !metadata.is_dir() => AgentDirectoryState::NotDirectory,
        Ok(_) => AgentDirectoryState::Ready,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            AgentDirectoryState::Missing(error.to_string())
        }
        Err(error) => AgentDirectoryState::Unreadable(error.to_string()),
    }
}

fn strict_target(
    agent_dir: &Path,
    name: &str,
    canonical: &Path,
    detached: bool,
) -> Result<PathBuf, AppError> {
    let root = normalize_absolute(agent_dir)
        .ok_or_else(|| AppError::path_invalid("agent skills 目录必须是绝对路径。"))?;
    let target = normalize_absolute(&root.join(name)).ok_or_else(|| {
        AppError::path_invalid(if detached {
            "断链目标路径无效。"
        } else {
            "挂载目标路径无效。"
        })
    })?;
    if target.parent() != Some(root.as_path()) {
        return Err(AppError::path_outside_allowed_root(format!(
            "{} 不是 agent 目录的直接子项",
            target.display()
        )));
    }
    if normalize_absolute(canonical).as_deref() == Some(target.as_path()) {
        return Err(AppError::path_invalid(if detached {
            format!("断链目标不能是 canonical 目录：{}", target.display())
        } else {
            format!("挂载目标不能是 canonical 目录：{}", target.display())
        }));
    }
    Ok(target)
}

fn attach_target_state(target: &Path) -> Result<TargetState, AppError> {
    // Preserve attach's historical behavior: any existing target is rejected,
    // while a metadata error is treated as absent and left to the symlink call.
    if fs::symlink_metadata(target).is_ok() {
        return Err(AppError::agent_attach_failed(format!(
            "{} 已存在，已停止（不覆盖现有内容）。",
            target.display()
        )));
    }
    Ok(TargetState::Absent)
}

fn detach_target_state(target: &Path, canonical: &Path) -> TargetState {
    match fs::symlink_metadata(target) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let expected = fs::canonicalize(canonical).unwrap_or_else(|_| canonical.to_path_buf());
            match fs::canonicalize(target) {
                Ok(resolved) if resolved == expected => TargetState::SymlinkToCanonical,
                _ => TargetState::Other,
            }
        }
        Ok(_) => TargetState::Other,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => TargetState::Absent,
        Err(_) => TargetState::Other,
    }
}

fn install_target_state(target: &Path, canonical: &Path) -> Result<TargetState, AppError> {
    match fs::symlink_metadata(target) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let expected = fs::canonicalize(canonical).unwrap_or_else(|_| canonical.to_path_buf());
            match fs::canonicalize(target) {
                Ok(resolved) if resolved == expected => Ok(TargetState::SymlinkToCanonical),
                Ok(resolved) => Err(AppError::agent_attach_failed(format!(
                    "{} 已存在且指向 {}，已停止（不覆盖现有内容）。",
                    target.display(),
                    resolved.display()
                ))),
                Err(error) => Err(AppError::agent_attach_failed(format!(
                    "{} 是断链，已停止（不覆盖现有内容）：{error}",
                    target.display()
                ))),
            }
        }
        Ok(_) => Err(AppError::agent_attach_failed(format!(
            "{} 已存在且不是符号链接，已停止（不覆盖现有内容）。",
            target.display()
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(TargetState::Absent),
        Err(error) => Err(AppError::agent_attach_failed(format!(
            "无法检查 {}：{error}",
            target.display()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use crate::paths::AppPaths;
    use std::os::unix::fs::symlink;

    // Test shims keep the pre-scope call shape where the context is global.
    fn resolve_attach(
        paths: &AppPaths,
        located: &LocatedSkill,
        agents: &[AgentDir],
        agent_id: &str,
    ) -> Result<ResolvedAttachment, AppError> {
        super::resolve_attach(
            &paths.canonical_skills_dir(),
            located,
            agents,
            agent_id,
            TargetContext::global(),
        )
    }

    fn resolve_detach(
        paths: &AppPaths,
        located: &LocatedSkill,
        agents: &[AgentDir],
        agent_id: &str,
    ) -> Result<ResolvedAttachment, AppError> {
        super::resolve_detach(
            &paths.canonical_skills_dir(),
            located,
            agents,
            agent_id,
            TargetContext::global(),
        )
    }

    fn resolve_install_link(
        canonical: &Path,
        agent: &AgentDir,
        name: &str,
    ) -> Result<ResolvedAttachment, AppError> {
        super::resolve_install_link(canonical, agent, name, TargetContext::global())
    }

    struct Fixture {
        _tmp: tempfile::TempDir,
        paths: AppPaths,
        canonical: PathBuf,
        agent_dir: PathBuf,
        agent: AgentDir,
    }

    impl Fixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path().join("home");
            let paths = AppPaths::new(&home, tmp.path().join("app-data"));
            paths.ensure_dirs().unwrap();
            let canonical = paths.canonical_skills_dir().join("alpha");
            fs::create_dir_all(&canonical).unwrap();
            fs::write(canonical.join("SKILL.md"), "---\nname: alpha\n---\n").unwrap();
            let agent_dir = home.join(".claude/skills");
            fs::create_dir_all(&agent_dir).unwrap();
            let agent = AgentDir {
                id: "claude-code".to_string(),
                display_name: "Claude Code".to_string(),
                universal: false,
                skills_dir: agent_dir.clone(),
            };
            Self {
                _tmp: tmp,
                paths,
                canonical,
                agent_dir,
                agent,
            }
        }

        fn located(
            &self,
            management: ManagementState,
            attachments: Vec<AttachmentLocation>,
        ) -> LocatedSkill {
            LocatedSkill {
                name: "alpha".to_string(),
                management,
                canonical_path: (management == ManagementState::Managed)
                    .then(|| self.canonical.clone()),
                primary_path: Some(self.canonical.clone()),
                attachments,
                lock_entry: None,
            }
        }

        fn attachment(&self, mode: AttachmentMode) -> AttachmentLocation {
            AttachmentLocation {
                agent_id: self.agent.id.clone(),
                agent_name: self.agent.display_name.clone(),
                path: self.agent_dir.join("alpha"),
                mode,
            }
        }
    }

    #[test]
    fn attach_resolves_managed_target_and_reports_missing_directory() {
        let fixture = Fixture::new();
        let missing_dir = fixture.paths.home().join(".missing/skills");
        let agent = AgentDir {
            skills_dir: missing_dir.clone(),
            ..fixture.agent.clone()
        };
        let located = fixture.located(ManagementState::Managed, Vec::new());
        let resolved = resolve_attach(
            &fixture.paths,
            &located,
            std::slice::from_ref(&agent),
            &agent.id,
        )
        .unwrap();
        assert_eq!(resolved.target, missing_dir.join("alpha"));
        assert_eq!(resolved.target_state, TargetState::Absent);
        assert!(matches!(
            resolved.directory_state,
            AgentDirectoryState::Missing(_)
        ));

        let resolved = resolve_attach(
            &fixture.paths,
            &located,
            std::slice::from_ref(&fixture.agent),
            &fixture.agent.id,
        )
        .unwrap();
        assert_eq!(resolved.directory_state, AgentDirectoryState::Ready);
        assert_eq!(resolved.target_state, TargetState::Absent);
    }

    #[test]
    fn attach_rejects_management_agent_existing_and_paths() {
        let fixture = Fixture::new();
        let managed = fixture.located(ManagementState::Managed, Vec::new());

        let error = resolve_attach(
            &fixture.paths,
            &fixture.located(ManagementState::CopyOnly, Vec::new()),
            std::slice::from_ref(&fixture.agent),
            &fixture.agent.id,
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::SkillCopyOnly);
        let error = resolve_attach(
            &fixture.paths,
            &fixture.located(ManagementState::Unmanaged, Vec::new()),
            std::slice::from_ref(&fixture.agent),
            &fixture.agent.id,
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::SkillUnmanaged);

        let universal = AgentDir {
            universal: true,
            ..fixture.agent.clone()
        };
        let error = resolve_attach(
            &fixture.paths,
            &managed,
            std::slice::from_ref(&universal),
            &universal.id,
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::AgentNative);

        let shared = AgentDir {
            skills_dir: fixture.paths.canonical_skills_dir(),
            ..fixture.agent.clone()
        };
        let error = resolve_attach(
            &fixture.paths,
            &managed,
            std::slice::from_ref(&shared),
            &shared.id,
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::AgentNative);

        for (mode, expected) in [
            (AttachmentMode::Native, ErrorCode::AgentNative),
            (AttachmentMode::Broken, ErrorCode::AgentBroken),
            (AttachmentMode::Link, ErrorCode::AgentAlreadyAttached),
            (AttachmentMode::Copy, ErrorCode::AgentAlreadyAttached),
        ] {
            let located = fixture.located(ManagementState::Managed, vec![fixture.attachment(mode)]);
            let error = resolve_attach(
                &fixture.paths,
                &located,
                std::slice::from_ref(&fixture.agent),
                &fixture.agent.id,
            )
            .unwrap_err();
            assert_eq!(error.code, expected, "{mode:?}");
        }

        fs::write(fixture.agent_dir.join("alpha"), "occupied").unwrap();
        let error = resolve_attach(
            &fixture.paths,
            &managed,
            std::slice::from_ref(&fixture.agent),
            &fixture.agent.id,
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::AgentAttachFailed);

        let mut escaped = fixture.located(ManagementState::Managed, Vec::new());
        escaped.name = "../alpha".to_string();
        let error = resolve_attach(
            &fixture.paths,
            &escaped,
            std::slice::from_ref(&fixture.agent),
            &fixture.agent.id,
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::PathOutsideAllowedRoot);

        let agent_link = fixture.paths.home().join(".linked-skills");
        symlink(&fixture.agent_dir, &agent_link).unwrap();
        let linked = AgentDir {
            skills_dir: agent_link,
            ..fixture.agent.clone()
        };
        let error = resolve_attach(
            &fixture.paths,
            &managed,
            std::slice::from_ref(&linked),
            &linked.id,
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::PathInvalid);
    }

    #[test]
    fn detach_resolves_link_and_copy_and_rejects_ineligible_rows() {
        let fixture = Fixture::new();
        symlink(&fixture.canonical, fixture.agent_dir.join("alpha")).unwrap();
        let link = fixture.located(
            ManagementState::Managed,
            vec![fixture.attachment(AttachmentMode::Link)],
        );
        let resolved = resolve_detach(
            &fixture.paths,
            &link,
            std::slice::from_ref(&fixture.agent),
            &fixture.agent.id,
        )
        .unwrap();
        assert_eq!(resolved.target_state, TargetState::SymlinkToCanonical);
        assert_eq!(resolved.existing.unwrap().mode, AttachmentMode::Link);

        fs::remove_file(fixture.agent_dir.join("alpha")).unwrap();
        fs::create_dir_all(fixture.agent_dir.join("alpha")).unwrap();
        let copy = fixture.located(
            ManagementState::Managed,
            vec![fixture.attachment(AttachmentMode::Copy)],
        );
        let resolved = resolve_detach(
            &fixture.paths,
            &copy,
            std::slice::from_ref(&fixture.agent),
            &fixture.agent.id,
        )
        .unwrap();
        assert_eq!(resolved.target_state, TargetState::Other);
        assert_eq!(resolved.existing.unwrap().mode, AttachmentMode::Copy);

        for (located, expected) in [
            (
                fixture.located(ManagementState::CopyOnly, Vec::new()),
                ErrorCode::SkillCopyOnly,
            ),
            (
                fixture.located(ManagementState::Unmanaged, Vec::new()),
                ErrorCode::SkillUnmanaged,
            ),
            (
                fixture.located(ManagementState::Managed, Vec::new()),
                ErrorCode::AgentNotAttached,
            ),
            (
                fixture.located(
                    ManagementState::Managed,
                    vec![fixture.attachment(AttachmentMode::Native)],
                ),
                ErrorCode::AgentNative,
            ),
            (
                fixture.located(
                    ManagementState::Managed,
                    vec![fixture.attachment(AttachmentMode::Broken)],
                ),
                ErrorCode::AgentBroken,
            ),
        ] {
            let error = resolve_detach(
                &fixture.paths,
                &located,
                std::slice::from_ref(&fixture.agent),
                &fixture.agent.id,
            )
            .unwrap_err();
            assert_eq!(error.code, expected);
        }

        let universal = AgentDir {
            universal: true,
            ..fixture.agent.clone()
        };
        let error = resolve_detach(
            &fixture.paths,
            &copy,
            std::slice::from_ref(&universal),
            &universal.id,
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::AgentNative);

        let missing = AgentDir {
            skills_dir: fixture.paths.home().join(".missing/skills"),
            ..fixture.agent.clone()
        };
        let error = resolve_detach(
            &fixture.paths,
            &copy,
            std::slice::from_ref(&missing),
            &missing.id,
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::PathInvalid);
    }

    #[test]
    fn install_link_is_idempotent_and_rejects_conflicts() {
        let fixture = Fixture::new();
        let link = fixture.agent_dir.join("alpha");
        symlink(&fixture.canonical, &link).unwrap();
        let resolved = resolve_install_link(&fixture.canonical, &fixture.agent, "alpha").unwrap();
        assert_eq!(resolved.target_state, TargetState::SymlinkToCanonical);
        assert_eq!(resolved.directory_state, AgentDirectoryState::Ready);

        fs::remove_file(&link).unwrap();
        let resolved = resolve_install_link(&fixture.canonical, &fixture.agent, "alpha").unwrap();
        assert_eq!(resolved.target_state, TargetState::Absent);

        let missing = AgentDir {
            skills_dir: fixture.paths.home().join(".missing/skills"),
            ..fixture.agent.clone()
        };
        let resolved = resolve_install_link(&fixture.canonical, &missing, "alpha").unwrap();
        assert!(matches!(
            resolved.directory_state,
            AgentDirectoryState::Missing(_)
        ));
        assert_eq!(resolved.target_state, TargetState::Absent);

        fs::write(&link, "occupied").unwrap();
        let error = resolve_install_link(&fixture.canonical, &fixture.agent, "alpha").unwrap_err();
        assert_eq!(error.code, ErrorCode::AgentAttachFailed);

        fs::remove_file(&link).unwrap();
        let other = fixture.paths.home().join("other");
        fs::create_dir_all(&other).unwrap();
        symlink(&other, &link).unwrap();
        let error = resolve_install_link(&fixture.canonical, &fixture.agent, "alpha").unwrap_err();
        assert_eq!(error.code, ErrorCode::AgentAttachFailed);

        fs::remove_file(&link).unwrap();
        symlink(fixture.paths.home().join("missing"), &link).unwrap();
        let error = resolve_install_link(&fixture.canonical, &fixture.agent, "alpha").unwrap_err();
        assert_eq!(error.code, ErrorCode::AgentAttachFailed);

        let agent_link = fixture.paths.home().join(".linked-skills");
        symlink(&fixture.agent_dir, &agent_link).unwrap();
        let linked = AgentDir {
            skills_dir: agent_link,
            ..fixture.agent.clone()
        };
        let error = resolve_install_link(&fixture.canonical, &linked, "alpha").unwrap_err();
        assert_eq!(error.code, ErrorCode::AgentAttachFailed);
    }

    #[test]
    fn project_target_context_guards_lexical_and_symlink_escapes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("project");
        let outside = tmp.path().join("outside");
        fs::create_dir_all(root.join(".acme/skills")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        symlink(&outside, root.join("escape")).unwrap();
        symlink(root.join(".acme/skills"), root.join("inside-link")).unwrap();
        let context = TargetContext::project(&root);

        assert!(context.ensure_inside(&root.join(".acme/skills")).is_ok());
        assert!(
            context.ensure_inside(&root.join(".missing/skills")).is_ok(),
            "a missing directory inside the root is a valid attach target"
        );
        assert!(
            context
                .ensure_inside(&root.join("inside-link/deeper"))
                .is_ok()
        );

        let lexical = context.ensure_inside(&outside).unwrap_err();
        assert_eq!(lexical.code, ErrorCode::PathOutsideAllowedRoot);
        let resolved = context
            .ensure_inside(&root.join("escape/skills"))
            .unwrap_err();
        assert_eq!(resolved.code, ErrorCode::PathOutsideAllowedRoot);
    }

    #[test]
    fn detach_eligibility_matches_the_row_matrix() {
        use DetachEligibility::*;
        assert_eq!(
            detach_eligibility(ManagementState::Unmanaged, true, false, None),
            Unmanaged
        );
        assert_eq!(
            detach_eligibility(ManagementState::CopyOnly, false, false, None),
            CopyOnly
        );
        assert_eq!(
            detach_eligibility(ManagementState::Managed, false, false, None),
            CopyOnly
        );
        assert_eq!(
            detach_eligibility(
                ManagementState::Managed,
                true,
                true,
                Some(AttachmentMode::Link)
            ),
            NativeAgent
        );
        assert_eq!(
            detach_eligibility(ManagementState::Managed, true, false, None),
            NotAttached
        );
        assert_eq!(
            detach_eligibility(
                ManagementState::Managed,
                true,
                false,
                Some(AttachmentMode::Native)
            ),
            NativeAttachment
        );
        assert_eq!(
            detach_eligibility(
                ManagementState::Managed,
                true,
                false,
                Some(AttachmentMode::Broken)
            ),
            Broken
        );
        assert_eq!(
            detach_eligibility(
                ManagementState::Managed,
                true,
                false,
                Some(AttachmentMode::Link)
            ),
            Detachable
        );
        assert_eq!(
            detach_eligibility(
                ManagementState::Managed,
                true,
                false,
                Some(AttachmentMode::Copy)
            ),
            Detachable
        );
    }
}

#[cfg(test)]
mod canonical_target_tests {
    use super::*;
    use crate::error::ErrorCode;
    use crate::paths::AppPaths;

    fn resolve_detach(
        paths: &AppPaths,
        located: &LocatedSkill,
        agents: &[AgentDir],
        agent_id: &str,
    ) -> Result<ResolvedAttachment, AppError> {
        super::resolve_detach(
            &paths.canonical_skills_dir(),
            located,
            agents,
            agent_id,
            TargetContext::global(),
        )
    }

    #[test]
    fn detach_rejects_a_canonical_target_path() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        paths.ensure_dirs().unwrap();
        let agent_dir = home.join(".claude/skills");
        fs::create_dir_all(&agent_dir).unwrap();
        let canonical = agent_dir.join("alpha");
        fs::create_dir_all(&canonical).unwrap();
        let agent = AgentDir {
            id: "claude-code".to_string(),
            display_name: "Claude Code".to_string(),
            universal: false,
            skills_dir: agent_dir.clone(),
        };
        let located = LocatedSkill {
            name: "alpha".to_string(),
            management: ManagementState::Managed,
            canonical_path: Some(canonical.clone()),
            primary_path: Some(canonical),
            attachments: vec![AttachmentLocation {
                agent_id: agent.id.clone(),
                agent_name: agent.display_name.clone(),
                path: agent_dir.join("alpha"),
                mode: AttachmentMode::Link,
            }],
            lock_entry: None,
        };
        let error =
            resolve_detach(&paths, &located, std::slice::from_ref(&agent), &agent.id).unwrap_err();
        assert_eq!(error.code, ErrorCode::PathInvalid);
        assert!(error.detail.as_deref().unwrap().contains("canonical"));
    }
}
