//! Native attach: create a skill's attachment in one more agent without
//! reinstalling it. Inverse of detach; the canonical store and lock entry stay
//! untouched.
//!
//! Only managed skills with a canonical folder can be attached. Copy-only and
//! unmanaged skills, universal/shared agents, broken links and agents that
//! already have the skill are rejected; the frontend never offers them. Every
//! path is resolved on the server from the inventory and guarded: the target
//! must be exactly one entry inside that agent's skills directory and the
//! directory itself may not be a symlink.

use std::fs;
use std::path::PathBuf;

use serde::Serialize;

use crate::error::AppError;
use crate::inventory::{self, AgentDir, AttachmentMode, ManagementState};
use crate::lock::LockFile;
use crate::logs::{OperationEntry, OperationLog};
use crate::paths::AppPaths;
use crate::security::{self, normalize_absolute};

const ATTACH_ACTION: &str = "attach";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachResult {
    pub skill_id: String,
    pub agent_id: String,
    pub agent_name: String,
    pub attachment_path: String,
    pub canonical_path: String,
    pub log_path: String,
}

pub fn attach_agent(
    paths: &AppPaths,
    agents: &[AgentDir],
    skill_id: &str,
    agent_id: &str,
) -> Result<AttachResult, AppError> {
    security::validate_skill_id(skill_id)?;
    super::tasks::install::validate_skill_name(skill_id)?;
    // A corrupt or non-v3 lock blocks every mutation, including attach.
    LockFile::read(&paths.lock_path())?;

    let located = inventory::locate_with_agents(paths, agents, skill_id)?
        .ok_or_else(|| AppError::skill_not_found(skill_id))?;
    match located.management {
        ManagementState::Managed => {}
        ManagementState::CopyOnly => return Err(AppError::skill_copy_only(skill_id)),
        ManagementState::Unmanaged => return Err(AppError::skill_unmanaged(skill_id)),
    }
    let canonical = located
        .canonical_path
        .clone()
        .ok_or_else(|| AppError::skill_copy_only(skill_id))?;
    if !fs::metadata(&canonical)
        .map(|metadata| metadata.is_dir())
        .unwrap_or(false)
    {
        return Err(AppError::verify_canonical_missing(skill_id, &canonical));
    }

    let agent = agents
        .iter()
        .find(|agent| agent.id == agent_id)
        .ok_or_else(|| AppError::agent_not_detected(agent_id))?;
    if agent.universal || same_dir(&agent.skills_dir, &paths.canonical_skills_dir()) {
        return Err(AppError::agent_native(&agent.display_name));
    }
    if let Some(existing) = located
        .attachments
        .iter()
        .find(|attachment| attachment.agent_id == agent_id)
    {
        return match existing.mode {
            AttachmentMode::Native => Err(AppError::agent_native(&agent.display_name)),
            AttachmentMode::Broken => {
                Err(AppError::agent_broken(&agent.display_name, &existing.path))
            }
            AttachmentMode::Link | AttachmentMode::Copy => {
                Err(AppError::agent_already_attached(&agent.display_name))
            }
        };
    }

    let target = prepare_target(&agent.skills_dir, skill_id, &canonical)?;
    prepare_directory(&agent.skills_dir)?;
    if fs::symlink_metadata(&target).is_ok() {
        return Err(AppError::agent_attach_failed(format!(
            "{} 已存在，已停止（不覆盖现有内容）。",
            target.display()
        )));
    }

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&canonical, &target).map_err(|error| {
            AppError::agent_attach_failed(format!("无法创建符号链接 {}：{error}", target.display()))
        })?;
    }

    let created = fs::symlink_metadata(&target)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false);
    if !created {
        return Err(AppError::agent_attach_failed(format!(
            "创建后未找到符号链接：{}",
            target.display()
        )));
    }

    let log = OperationLog::new(paths);
    let log_path = paths.display(log.path());
    let _ = log.append(&OperationEntry::new(
        ATTACH_ACTION,
        skill_id,
        vec![agent_id.to_string()],
        "attached",
        None,
        None,
    ));
    Ok(AttachResult {
        skill_id: skill_id.to_string(),
        agent_id: agent_id.to_string(),
        agent_name: agent.display_name.clone(),
        attachment_path: paths.display(&target),
        canonical_path: paths.display(&canonical),
        log_path,
    })
}

fn prepare_directory(agent_dir: &std::path::Path) -> Result<(), AppError> {
    match fs::symlink_metadata(agent_dir) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(AppError::path_invalid(format!(
            "agent skills 目录本身是符号链接：{}",
            agent_dir.display()
        ))),
        Ok(metadata) if !metadata.is_dir() => Err(AppError::path_invalid(format!(
            "agent skills 路径不是目录：{}",
            agent_dir.display()
        ))),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir_all(agent_dir)
            .map_err(|error| {
                AppError::agent_attach_failed(format!("无法创建 {}：{error}", agent_dir.display()))
            }),
        Err(error) => Err(AppError::agent_attach_failed(format!(
            "无法检查 {}：{error}",
            agent_dir.display()
        ))),
    }
}

fn prepare_target(
    agent_dir: &std::path::Path,
    skill_id: &str,
    canonical: &std::path::Path,
) -> Result<PathBuf, AppError> {
    let root = normalize_absolute(agent_dir)
        .ok_or_else(|| AppError::path_invalid("agent skills 目录必须是绝对路径。"))?;
    let target = normalize_absolute(&root.join(skill_id))
        .ok_or_else(|| AppError::path_invalid("挂载目标路径无效。"))?;
    if target.parent() != Some(root.as_path()) {
        return Err(AppError::path_outside_allowed_root(format!(
            "{} 不是 agent 目录的直接子项",
            target.display()
        )));
    }
    if normalize_absolute(canonical).as_deref() == Some(target.as_path()) {
        return Err(AppError::path_invalid(format!(
            "挂载目标不能是 canonical 目录：{}",
            target.display()
        )));
    }
    Ok(target)
}

fn same_dir(left: &std::path::Path, right: &std::path::Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::codes;

    struct Fixture {
        _tmp: tempfile::TempDir,
        paths: AppPaths,
        agents: Vec<AgentDir>,
        claude: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path().join("home");
            let paths = AppPaths::new(&home, tmp.path().join("app-data"));
            paths.ensure_dirs().unwrap();
            let canonical = paths.canonical_skills_dir().join("alpha");
            fs::create_dir_all(&canonical).unwrap();
            fs::write(
                canonical.join("SKILL.md"),
                "---\nname: alpha\ndescription: alpha\n---\n",
            )
            .unwrap();
            fs::create_dir_all(home.join(".claude/skills")).unwrap();
            fs::write(
                paths.lock_path(),
                serde_json::json!({
                    "version": 3,
                    "skills": {
                        "alpha": {
                            "source": "acme/skills",
                            "sourceType": "github",
                            "sourceUrl": "https://github.com/acme/skills.git",
                            "skillPath": "skills/alpha/SKILL.md",
                            "skillFolderHash": "0123456789abcdef0123456789abcdef01234567",
                            "installedAt": "2026-01-01T00:00:00.000Z",
                            "updatedAt": "2026-01-01T00:00:00.000Z"
                        }
                    }
                })
                .to_string(),
            )
            .unwrap();
            let claude = home.join(".claude/skills");
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
            }
        }
    }

    #[test]
    fn attaches_managed_skill_with_a_symlink_and_rejects_repeats() {
        let fixture = Fixture::new();
        let result = attach_agent(&fixture.paths, &fixture.agents, "alpha", "claude-code").unwrap();
        assert_eq!(result.agent_name, "Claude Code");
        let link = fixture.claude.join("alpha");
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::canonicalize(&link).unwrap(),
            fs::canonicalize(fixture.paths.canonical_skills_dir().join("alpha")).unwrap()
        );

        let error =
            attach_agent(&fixture.paths, &fixture.agents, "alpha", "claude-code").unwrap_err();
        assert_eq!(error.code, codes::AGENT_ALREADY_ATTACHED);
    }

    #[test]
    fn rejects_unmanaged_unknown_and_occupied_targets() {
        let fixture = Fixture::new();
        fs::remove_file(fixture.paths.lock_path()).unwrap();
        let error =
            attach_agent(&fixture.paths, &fixture.agents, "alpha", "claude-code").unwrap_err();
        assert_eq!(error.code, codes::SKILL_UNMANAGED);

        let fixture = Fixture::new();
        let error = attach_agent(&fixture.paths, &fixture.agents, "alpha", "nope").unwrap_err();
        assert_eq!(error.code, codes::AGENT_NOT_DETECTED);

        let fixture = Fixture::new();
        // A plain file in the agent directory is invisible to the inventory
        // but still occupies the attachment path; attach must not overwrite it.
        fs::write(fixture.claude.join("alpha"), "noise").unwrap();
        let error =
            attach_agent(&fixture.paths, &fixture.agents, "alpha", "claude-code").unwrap_err();
        assert_eq!(error.code, codes::AGENT_ATTACH_FAILED);
    }

    #[test]
    fn rejects_universal_agents_and_copy_only_skills() {
        let fixture = Fixture::new();
        let canonical = fixture.paths.canonical_skills_dir();
        let agents = vec![AgentDir {
            id: "codex".to_string(),
            display_name: "Codex".to_string(),
            universal: true,
            skills_dir: canonical.clone(),
        }];
        let error = attach_agent(&fixture.paths, &agents, "alpha", "codex").unwrap_err();
        assert_eq!(error.code, codes::AGENT_NATIVE);

        // Copy-only beta: a lock entry and an agent copy, no canonical folder.
        let copy = fixture.claude.join("beta");
        fs::create_dir_all(&copy).unwrap();
        fs::write(copy.join("SKILL.md"), "---\nname: beta\n---\n").unwrap();
        let raw = fs::read_to_string(fixture.paths.lock_path())
            .unwrap()
            .replace("\"alpha\"", "\"beta\"");
        fs::write(fixture.paths.lock_path(), raw).unwrap();
        fs::remove_dir_all(canonical.join("alpha")).unwrap();

        let error =
            attach_agent(&fixture.paths, &fixture.agents, "beta", "claude-code").unwrap_err();
        assert_eq!(error.code, codes::SKILL_COPY_ONLY);
    }
}
