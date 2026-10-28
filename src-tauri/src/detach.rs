//! Native detach: remove a skill from one agent while keeping the canonical
//! store and the lock entry intact.
//!
//! Symlink attachments are unlinked directly. Copy attachments require a
//! realpath dependency scan (no other agent symlink may resolve to the copy)
//! and a mandatory backup snapshot before deletion. Universal/native, broken,
//! copy-only and unmanaged rows are rejected; the frontend never offers them.
//!
//! Every target is resolved on the server from the inventory and guarded:
//! it must be exactly one entry inside that agent's catalog skills directory,
//! the directory itself may not be a symlink, and the target may not be the
//! canonical store. Symlinks are never followed when deleting.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::AppError;
use crate::inventory::{self, AgentDir, ManagementState, AttachmentMode};
use crate::lock::LockFile;
use crate::logs::{OperationEntry, OperationLog};
use crate::paths::AppPaths;
use crate::security::{self, normalize_absolute};

const DETACH_ACTION: &str = "detach";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetachResult {
    pub skill_id: String,
    pub agent_id: String,
    pub agent_name: String,
    pub mode: AttachmentMode,
    pub attachment_path: String,
    pub canonical_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_path: Option<String>,
    pub log_path: String,
    /// Display names of the attachments that remain after this detach.
    pub remaining_agents: Vec<String>,
    /// True when no agent attachment remains; the skill is still managed.
    pub unmounted: bool,
}

pub fn detach_agent(
    paths: &AppPaths,
    agents: &[AgentDir],
    skill_id: &str,
    agent_id: &str,
) -> Result<DetachResult, AppError> {
    security::validate_skill_id(skill_id)?;
    // A corrupt or non-v3 lock blocks every mutation, including detach.
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

    let agent = agents
        .iter()
        .find(|agent| agent.id == agent_id)
        .ok_or_else(|| AppError::agent_not_detected(agent_id))?;
    // Universal agents read the canonical store directly. Even when a legacy
    // physical link exists, removing it does not detach the skill from them.
    if agent.universal || same_dir(&agent.skills_dir, &paths.canonical_skills_dir()) {
        return Err(AppError::agent_native(&agent.display_name));
    }
    let attachment = located
        .attachments
        .iter()
        .find(|attachment| attachment.agent_id == agent_id)
        .ok_or_else(|| AppError::agent_not_attached(&agent.display_name))?;
    match attachment.mode {
        AttachmentMode::Native => return Err(AppError::agent_native(&agent.display_name)),
        AttachmentMode::Broken => {
            return Err(AppError::agent_broken(
                &agent.display_name,
                &attachment.path,
            ));
        }
        AttachmentMode::Link | AttachmentMode::Copy => {}
    }

    let target = validate_target(&agent.skills_dir, skill_id, &canonical)?;
    let log = OperationLog::new(paths);
    let log_path = paths.display(log.path());
    let remaining_agents: Vec<String> = located
        .attachments
        .iter()
        .filter(|attachment| attachment.agent_id != agent_id)
        .map(|attachment| attachment.agent_name.clone())
        .collect();
    let mut result = DetachResult {
        skill_id: skill_id.to_string(),
        agent_id: agent_id.to_string(),
        agent_name: agent.display_name.clone(),
        mode: attachment.mode,
        attachment_path: paths.display(&target),
        canonical_path: paths.display(&canonical),
        backup_path: None,
        log_path,
        unmounted: remaining_agents.is_empty(),
        remaining_agents,
    };

    // The physical entry decides how to delete: never follow a symlink.
    let metadata = match fs::symlink_metadata(&target) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(AppError::agent_not_attached(&agent.display_name).detail(
                format!("{}（{error}）", target.display()),
            ));
        }
        Err(error) => {
            let error =
                AppError::agent_detach_failed(format!("无法读取 {}：{error}", target.display()));
            log_failure(&log, skill_id, agent_id, None, &error);
            return Err(error);
        }
    };
    let mode = if metadata.file_type().is_symlink() {
        AttachmentMode::Link
    } else if metadata.is_dir() {
        AttachmentMode::Copy
    } else {
        let error = AppError::agent_detach_failed(format!(
            "目标既不是符号链接也不是目录：{}",
            target.display()
        ));
        log_failure(&log, skill_id, agent_id, None, &error);
        return Err(error);
    };
    if mode != attachment.mode {
        let error = AppError::agent_detach_failed(format!(
            "挂载状态已变化（库存为 {:?}，当前为 {mode:?}）；请刷新后重试。",
            attachment.mode
        ));
        log_failure(&log, skill_id, agent_id, None, &error);
        return Err(error);
    }
    result.mode = mode;

    match mode {
        AttachmentMode::Link => {
            if let Err(error) = fs::remove_file(&target) {
                let error = AppError::agent_detach_failed(format!(
                    "无法删除符号链接 {}：{error}",
                    target.display()
                ));
                log_failure(&log, skill_id, agent_id, None, &error);
                return Err(error);
            }
        }
        AttachmentMode::Copy => {
            let error = AppError::agent_detach_failed("copy 断开尚未实现");
            log_failure(&log, skill_id, agent_id, None, &error);
            return Err(error);
        }
        AttachmentMode::Native | AttachmentMode::Broken => unreachable!("mode was filtered above"),
    }

    if fs::symlink_metadata(&target).is_ok() {
        let error = AppError::agent_detach_failed(format!(
            "删除后目标仍然存在：{}",
            target.display()
        ));
        log_failure(&log, skill_id, agent_id, None, &error);
        return Err(error);
    }

    let _ = log.append(&OperationEntry::new(
        DETACH_ACTION,
        skill_id,
        vec![agent_id.to_string()],
        "detached",
        None,
        None,
    ));
    Ok(result)
}

fn validate_target(
    agent_dir: &Path,
    skill_id: &str,
    canonical: &Path,
) -> Result<PathBuf, AppError> {
    let metadata = fs::symlink_metadata(agent_dir).map_err(|error| {
        AppError::path_invalid(format!(
            "无法检查 agent skills 目录 {}：{error}",
            agent_dir.display()
        ))
    })?;
    if metadata.file_type().is_symlink() {
        return Err(AppError::path_invalid(format!(
            "agent skills 目录本身是符号链接，已拒绝断开：{}",
            agent_dir.display()
        )));
    }
    if !metadata.is_dir() {
        return Err(AppError::path_invalid(format!(
            "agent skills 路径不是目录：{}",
            agent_dir.display()
        )));
    }

    let root = normalize_absolute(agent_dir)
        .ok_or_else(|| AppError::path_invalid("agent skills 目录必须是绝对路径。"))?;
    let target = normalize_absolute(&root.join(skill_id))
        .ok_or_else(|| AppError::path_invalid("断链目标路径无效。"))?;
    if target.parent() != Some(root.as_path()) {
        return Err(AppError::path_outside_allowed_root(format!(
            "{} 不是 agent 目录的直接子项",
            target.display()
        )));
    }
    let canonical_normalized = normalize_absolute(canonical);
    if canonical_normalized.as_deref() == Some(target.as_path()) {
        return Err(AppError::path_invalid(format!(
            "断链目标不能是 canonical 目录：{}",
            target.display()
        )));
    }
    Ok(target)
}

fn log_failure(
    log: &OperationLog,
    skill_id: &str,
    agent_id: &str,
    backup_path: Option<String>,
    error: &AppError,
) {
    // The operation log is best effort and must never mask the real error.
    let _ = log.append(&OperationEntry::new(
        DETACH_ACTION,
        skill_id,
        vec![agent_id.to_string()],
        "failed",
        backup_path,
        Some(error.clone()),
    ));
}

fn same_dir(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{self, AgentDir};
    use std::os::unix::fs::symlink;

    struct Fixture {
        _tmp: tempfile::TempDir,
        paths: AppPaths,
        agents: Vec<AgentDir>,
        canonical_root: PathBuf,
        claude: PathBuf,
        qoder: PathBuf,
        gemini: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path().join("home");
            let paths = AppPaths::new(&home, tmp.path().join("app-data"));
            paths.ensure_dirs().unwrap();
            fs::create_dir_all(paths.canonical_skills_dir()).unwrap();
            let claude = home.join(".claude/skills");
            let qoder = home.join(".qoder/skills");
            let gemini = home.join(".gemini/skills");
            for dir in [&claude, &qoder, &gemini] {
                fs::create_dir_all(dir).unwrap();
            }
            let canonical_root = paths.canonical_skills_dir();
            let agents = vec![
                AgentDir {
                    id: "claude-code".to_string(),
                    display_name: "Claude Code".to_string(),
                    universal: false,
                    skills_dir: claude.clone(),
                },
                AgentDir {
                    id: "qoder".to_string(),
                    display_name: "Qoder".to_string(),
                    universal: false,
                    skills_dir: qoder.clone(),
                },
                AgentDir {
                    id: "gemini-cli".to_string(),
                    display_name: "Gemini CLI".to_string(),
                    universal: true,
                    skills_dir: gemini.clone(),
                },
                AgentDir {
                    id: "codex".to_string(),
                    display_name: "Codex".to_string(),
                    universal: true,
                    skills_dir: canonical_root.clone(),
                },
            ];
            Self {
                _tmp: tmp,
                paths,
                agents,
                canonical_root,
                claude,
                qoder,
                gemini,
            }
        }

        fn lock_entry(name: &str) -> serde_json::Value {
            serde_json::json!({
                "source": format!("acme/{name}"),
                "sourceType": "github",
                "sourceUrl": format!("https://github.com/acme/{name}.git"),
                "skillPath": format!("skills/{name}/SKILL.md"),
                "skillFolderHash": "0123456789abcdef0123456789abcdef01234567",
                "installedAt": "2026-01-01T00:00:00.000Z",
                "updatedAt": "2026-02-03T04:05:06.000Z"
            })
        }

        fn write_lock(&self, names: &[&str]) {
            let mut skills = serde_json::Map::new();
            for name in names {
                skills.insert((*name).to_string(), Self::lock_entry(name));
            }
            let lock = serde_json::json!({
                "version": 3,
                "skills": skills,
                "dismissed": { "findSkillsPrompt": true }
            });
            fs::create_dir_all(self.paths.agents_root()).unwrap();
            fs::write(self.paths.lock_path(), lock.to_string()).unwrap();
        }

        fn managed_skill(&self, name: &str) -> PathBuf {
            let dir = self.canonical_root.join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: {name}\n---\n# {name}\n"),
            )
            .unwrap();
            self.write_lock(&[name]);
            dir
        }

        fn copy(&self, root: &Path, name: &str) -> PathBuf {
            let dir = root.join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: copy\n---\n# {name} copy\n"),
            )
            .unwrap();
            dir
        }

        fn link(&self, root: &Path, name: &str, target: &Path) {
            symlink(target, root.join(name)).unwrap();
        }

        fn detach(&self, skill: &str, agent: &str) -> Result<DetachResult, AppError> {
            detach_agent(&self.paths, &self.agents, skill, agent)
        }

        fn log_lines(&self) -> Vec<serde_json::Value> {
            fs::read_to_string(self.paths.logs_dir().join("operations.jsonl"))
                .map(|raw| {
                    raw.lines()
                        .map(|line| serde_json::from_str(line).unwrap())
                        .collect()
                })
                .unwrap_or_default()
        }

        fn backup_dirs(&self) -> Vec<PathBuf> {
            fs::read_dir(self.paths.backups_dir())
                .map(|entries| {
                    entries
                        .filter_map(Result::ok)
                        .map(|entry| entry.path())
                        .collect()
                })
                .unwrap_or_default()
        }
    }

    #[test]
    fn symlink_detach_unlinks_only_that_agent_and_keeps_canonical_and_lock() {
        let fixture = Fixture::new();
        let canonical = fixture.managed_skill("alpha");
        fixture.link(&fixture.claude, "alpha", &canonical);
        fixture.link(&fixture.qoder, "alpha", &canonical);
        let lock_before = fs::read(fixture.paths.lock_path()).unwrap();
        let canonical_before = fs::read(canonical.join("SKILL.md")).unwrap();

        let result = fixture.detach("alpha", "claude-code").unwrap();

        assert_eq!(result.skill_id, "alpha");
        assert_eq!(result.agent_id, "claude-code");
        assert_eq!(result.agent_name, "Claude Code");
        assert_eq!(result.mode, AttachmentMode::Link);
        assert_eq!(
            result.attachment_path,
            fixture.paths.display(&fixture.claude.join("alpha"))
        );
        assert_eq!(
            result.canonical_path,
            fixture.paths.display(&canonical)
        );
        assert!(result.backup_path.is_none());
        assert!(!result.unmounted);
        assert_eq!(
            result.remaining_agents,
            vec!["Qoder".to_string(), "Codex".to_string()]
        );

        assert!(fs::symlink_metadata(fixture.claude.join("alpha")).is_err());
        assert!(fixture.qoder.join("alpha").is_symlink());
        assert_eq!(
            fs::read(canonical.join("SKILL.md")).unwrap(),
            canonical_before
        );
        assert_eq!(fs::read(fixture.paths.lock_path()).unwrap(), lock_before);
        assert!(
            fixture.backup_dirs().is_empty(),
            "symlink detach must not create a backup"
        );

        let lines = fixture.log_lines();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["action"], "detach");
        assert_eq!(lines[0]["skill"], "alpha");
        assert_eq!(lines[0]["agents"], serde_json::json!(["claude-code"]));
        assert_eq!(lines[0]["result"], "detached");
        assert!(lines[0].get("backupPath").is_none());
        assert!(lines[0].get("error").is_none());
        assert!(lines[0]["time"].as_str().unwrap().ends_with('Z'));
        assert_eq!(
            inventory::locate_with_agents(&fixture.paths, &fixture.agents, "alpha")
                .unwrap()
                .unwrap()
                .attachments
                .len(),
            2,
            "native codex + qoder link remain; claude is gone"
        );
    }
}
