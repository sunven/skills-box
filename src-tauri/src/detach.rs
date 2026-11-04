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

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::attachment_target::{self, TargetContext};
use crate::backup::{BackupAction, BackupAttachment, BackupStore};
use crate::error::AppError;
use crate::inventory::{self, AgentDir, AttachmentMode};
use crate::logs::{OperationEntry, OperationLog};
use crate::paths::AppPaths;
use crate::scope_layout::ScopeLayout;

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
    pub unattached: bool,
}

pub fn detach_agent(
    paths: &AppPaths,
    layout: &ScopeLayout,
    skill_id: &str,
    agent_id: &str,
) -> Result<DetachResult, AppError> {
    detach_agent_checked(paths, layout, skill_id, agent_id, &|_| {}, &|| Ok(()))
}

pub(crate) fn detach_agent_checked(
    paths: &AppPaths,
    layout: &ScopeLayout,
    skill_id: &str,
    agent_id: &str,
    record_snapshot: &dyn Fn(&std::path::Path),
    before_write: &dyn Fn() -> Result<(), AppError>,
) -> Result<DetachResult, AppError> {
    let name = layout.scope.parse_skill_id(skill_id)?.to_string();
    // Same metacharacter rule as install/update; detach never spawns a shell,
    // but a malicious on-disk folder name must not reach the log or argv paths.
    super::tasks::install::validate_skill_name(&name)?;
    // A corrupt lock for this scope blocks every mutation, including detach.
    layout.ensure_mutation_ready()?;

    let located = layout
        .locate(paths, &name)?
        .ok_or_else(|| AppError::skill_not_found(skill_id))?;
    let resolved = attachment_target::resolve_detach(
        &layout.canonical_dir,
        &located,
        &layout.agents,
        agent_id,
        TargetContext::for_layout(layout),
    )?;
    let agent = &resolved.agent;
    let canonical = &resolved.canonical;
    let target = &resolved.target;
    let attachment = resolved
        .existing
        .as_ref()
        .expect("resolve_detach always returns an attachment");

    let log = OperationLog::for_scope(paths, &layout.scope);
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
        attachment_path: paths.display(target),
        canonical_path: paths.display(canonical),
        backup_path: None,
        log_path,
        unattached: remaining_agents.is_empty(),
        remaining_agents,
    };

    // The physical entry decides how to delete: never follow a symlink.
    let metadata = match fs::symlink_metadata(target) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(AppError::agent_not_attached(&agent.display_name)
                .detail(format!("{}（{error}）", target.display())));
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
            before_write()?;
            if let Err(error) = fs::remove_file(target) {
                let error = AppError::agent_detach_failed(format!(
                    "无法删除符号链接 {}：{error}",
                    target.display()
                ));
                log_failure(&log, skill_id, agent_id, None, &error);
                return Err(error);
            }
        }
        AttachmentMode::Copy => {
            let target_real = fs::canonicalize(target).map_err(|error| {
                AppError::agent_detach_failed(format!(
                    "无法解析 copy 目录 {}：{error}",
                    target.display()
                ))
            })?;
            let canonical_real =
                fs::canonicalize(canonical).unwrap_or_else(|_| canonical.to_path_buf());
            if target_real == canonical_real {
                let error = AppError::path_invalid(format!(
                    "断链目标不能是 canonical 目录：{}",
                    target.display()
                ));
                log_failure(&log, skill_id, agent_id, None, &error);
                return Err(error);
            }

            let dependents = find_copy_dependents(&layout.agents, canonical, target)?;
            if !dependents.is_empty() {
                let list = dependents
                    .iter()
                    .map(|dependent| {
                        format!("{}（{}）", dependent.label, paths.display(&dependent.path))
                    })
                    .collect::<Vec<_>>();
                let error = AppError::agent_dependency_conflict(&agent.display_name, &list);
                log_failure(&log, skill_id, agent_id, None, &error);
                return Err(error);
            }

            let backup_attachment = BackupAttachment {
                agent_id: agent.id.clone(),
                agent_name: agent.display_name.clone(),
                path: target.clone(),
                mode: AttachmentMode::Copy,
            };
            let backup = match BackupStore::for_layout(paths, layout).create(
                BackupAction::Detach,
                skill_id,
                None,
                std::slice::from_ref(&backup_attachment),
                std::slice::from_ref(&agent.id),
            ) {
                Ok(backup) => backup,
                Err(error) => {
                    log_failure(&log, skill_id, agent_id, None, &error);
                    return Err(error);
                }
            };
            record_snapshot(&backup.path);
            let backup_display = paths.display(&backup.path);
            result.backup_path = Some(backup_display.clone());
            if let Err(error) = before_write() {
                log_failure(&log, skill_id, agent_id, Some(backup_display), &error);
                return Err(error);
            }

            // Re-lstat immediately before deleting. A path that changed into
            // a symlink must never be followed by remove_dir_all.
            let metadata = match fs::symlink_metadata(target) {
                Ok(metadata) => metadata,
                Err(error) => {
                    let error = AppError::agent_detach_failed(format!(
                        "备份后无法读取 {}：{error}",
                        target.display()
                    ));
                    log_failure(&log, skill_id, agent_id, Some(backup_display), &error);
                    return Err(error);
                }
            };
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                let error = AppError::agent_detach_failed(format!(
                    "copy 目录在备份后发生了变化，已停止删除：{}",
                    target.display()
                ));
                log_failure(&log, skill_id, agent_id, Some(backup_display), &error);
                return Err(error);
            }
            if let Err(error) = fs::remove_dir_all(target) {
                let error = AppError::agent_detach_failed(format!(
                    "无法删除 copy 目录 {}：{error}",
                    target.display()
                ));
                log_failure(&log, skill_id, agent_id, Some(backup_display), &error);
                return Err(error);
            }
        }
        AttachmentMode::Native | AttachmentMode::Broken => unreachable!("mode was filtered above"),
    }

    if fs::symlink_metadata(target).is_ok() {
        let error =
            AppError::agent_detach_failed(format!("删除后目标仍然存在：{}", target.display()));
        log_failure(&log, skill_id, agent_id, result.backup_path.clone(), &error);
        return Err(error);
    }

    let _ = log.append(&OperationEntry::new(
        DETACH_ACTION,
        skill_id,
        vec![agent_id.to_string()],
        "detached",
        result.backup_path.clone(),
        None,
    ));
    Ok(result)
}

/// A symlink whose realpath is a copy directory. Shared with the uninstall
/// flow so both destructive actions use the same cross-agent dependency scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependentLink {
    /// Agent display name, or `canonical` for the shared store.
    pub label: String,
    pub path: PathBuf,
}

/// Find every symlink (inside any detected agent skills directory or the
/// canonical store) whose realpath is the copy directory being removed.
/// Dangling symlinks cannot resolve and are ignored.
pub fn find_copy_dependents(
    agents: &[AgentDir],
    canonical_dir: &Path,
    target: &Path,
) -> Result<Vec<DependentLink>, AppError> {
    let target_real = fs::canonicalize(target).map_err(|error| {
        AppError::agent_detach_failed(format!("无法解析 copy 目录 {}：{error}", target.display()))
    })?;

    let mut roots: Vec<(String, PathBuf)> = Vec::new();
    for agent in agents {
        if inventory::same_dir(&agent.skills_dir, canonical_dir) {
            continue;
        }
        roots.push((agent.display_name.clone(), agent.skills_dir.clone()));
    }
    roots.push(("canonical".to_string(), canonical_dir.to_path_buf()));

    let mut seen_roots: BTreeSet<PathBuf> = BTreeSet::new();
    let mut seen_links: BTreeSet<PathBuf> = BTreeSet::new();
    let mut dependents = Vec::new();
    for (label, root) in roots {
        // Read-only scan: follow a symlinked root because a link inside it can
        // still depend on the copy. Deletion targets are guarded separately.
        let Ok(metadata) = fs::metadata(&root) else {
            continue;
        };
        if !metadata.is_dir() {
            continue;
        }
        let root_real = fs::canonicalize(&root).unwrap_or_else(|_| root.clone());
        if !seen_roots.insert(root_real) {
            continue;
        }
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path == target {
                continue;
            }
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if !metadata.file_type().is_symlink() {
                continue;
            }
            let Ok(resolved) = fs::canonicalize(&path) else {
                continue;
            };
            if resolved == target_real && seen_links.insert(path.clone()) {
                dependents.push(DependentLink {
                    label: label.clone(),
                    path,
                });
            }
        }
    }
    Ok(dependents)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{AgentDir, ManagementState};
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
            Self::with_shared_agent(true)
        }

        fn without_shared_agent() -> Self {
            Self::with_shared_agent(false)
        }

        fn with_shared_agent(shared: bool) -> Self {
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
            let mut agents = vec![
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
            ];
            if shared {
                agents.push(AgentDir {
                    id: "codex".to_string(),
                    display_name: "Codex".to_string(),
                    universal: true,
                    skills_dir: canonical_root.clone(),
                });
            }
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
            let layout = ScopeLayout::global(&self.paths, self.agents.clone());
            super::detach_agent(&self.paths, &layout, skill, agent)
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
        assert_eq!(result.canonical_path, fixture.paths.display(&canonical));
        assert!(result.backup_path.is_none());
        assert!(!result.unattached);
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
        let layout =
            crate::scope_layout::ScopeLayout::global(&fixture.paths, fixture.agents.clone());
        assert_eq!(
            layout
                .locate(&fixture.paths, "alpha")
                .unwrap()
                .unwrap()
                .attachments
                .len(),
            2,
            "native codex + qoder link remain; claude is gone"
        );
    }

    #[test]
    fn copy_detach_without_dependents_backs_up_then_deletes_the_copy() {
        let fixture = Fixture::without_shared_agent();
        let canonical = fixture.managed_skill("alpha");
        let copy = fixture.copy(&fixture.claude, "alpha");
        let copy_before = fs::read(copy.join("SKILL.md")).unwrap();
        let canonical_before = fs::read(canonical.join("SKILL.md")).unwrap();
        let lock_before = fs::read(fixture.paths.lock_path()).unwrap();

        let result = fixture.detach("alpha", "claude-code").unwrap();

        assert_eq!(result.mode, AttachmentMode::Copy);
        assert!(result.unattached, "no other agent is attached");
        assert!(result.remaining_agents.is_empty());
        assert!(
            fs::symlink_metadata(&copy).is_err(),
            "copy directory is gone"
        );
        assert_eq!(
            fs::read(canonical.join("SKILL.md")).unwrap(),
            canonical_before
        );
        assert_eq!(fs::read(fixture.paths.lock_path()).unwrap(), lock_before);

        let backup_path = result.backup_path.clone().expect("copy detach backup");
        assert!(
            backup_path.contains("detach-alpha"),
            "timestamp-action-skill layout: {backup_path}"
        );
        let backups = fixture.backup_dirs();
        assert_eq!(backups.len(), 1);
        let backup = &backups[0];
        assert_eq!(
            fs::read(backup.join("skill-lock.json")).unwrap(),
            lock_before
        );
        assert_eq!(
            fs::read(backup.join("attachments/claude-code/SKILL.md")).unwrap(),
            copy_before
        );
        assert!(
            !backup.join("canonical").exists(),
            "detach must not copy the canonical store"
        );
        let manifest: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(backup.join("attachment-manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["action"], "detach");
        assert_eq!(manifest["skill"], "alpha");
        assert_eq!(manifest["canonicalPresent"], false);
        assert_eq!(manifest["agents"], serde_json::json!(["claude-code"]));
        assert_eq!(manifest["attachments"][0]["mode"], "copy");
        assert_eq!(manifest["attachments"][0]["backedUp"], true);

        let lines = fixture.log_lines();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["action"], "detach");
        assert_eq!(lines[0]["result"], "detached");
        assert_eq!(lines[0]["agents"], serde_json::json!(["claude-code"]));
        assert_eq!(lines[0]["backupPath"], backup_path);
    }

    #[test]
    fn copy_detach_with_a_cross_agent_symlink_dependency_is_blocked_without_changes() {
        let fixture = Fixture::new();
        fixture.managed_skill("alpha");
        let copy = fixture.copy(&fixture.claude, "alpha");
        // Gemini reads Claude's copy through a relative symlink, like the real
        // HyperFrames layout; deleting the copy would dangle it.
        symlink("../../.claude/skills/alpha", fixture.gemini.join("alpha")).unwrap();
        let lock_before = fs::read(fixture.paths.lock_path()).unwrap();
        let copy_before = fs::read(copy.join("SKILL.md")).unwrap();

        let error = fixture.detach("alpha", "claude-code").unwrap_err();

        assert_eq!(error.code, crate::error::ErrorCode::AgentDependencyConflict);
        let detail = error.detail.as_deref().unwrap_or_default();
        assert!(detail.contains("Gemini CLI"), "{detail}");
        assert!(detail.contains("~/.gemini/skills/alpha"), "{detail}");
        assert!(copy.join("SKILL.md").is_file(), "copy must stay intact");
        assert_eq!(fs::read(copy.join("SKILL.md")).unwrap(), copy_before);
        assert_eq!(fs::read(fixture.paths.lock_path()).unwrap(), lock_before);
        assert!(
            fixture.backup_dirs().is_empty(),
            "a blocked detach must not create a backup"
        );
        let lines = fixture.log_lines();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["action"], "detach");
        assert_eq!(lines[0]["result"], "failed");
        assert_eq!(
            lines[0]["error"]["code"],
            crate::error::ErrorCode::AgentDependencyConflict.as_str()
        );
        assert!(lines[0].get("backupPath").is_none());
    }

    #[test]
    fn symlinks_to_canonical_are_not_copy_dependencies() {
        let fixture = Fixture::without_shared_agent();
        let canonical = fixture.managed_skill("alpha");
        let copy = fixture.copy(&fixture.claude, "alpha");
        fixture.link(&fixture.qoder, "alpha", &canonical);

        let result = fixture.detach("alpha", "claude-code").unwrap();

        assert_eq!(result.mode, AttachmentMode::Copy);
        assert_eq!(result.remaining_agents, vec!["Qoder".to_string()]);
        assert!(!copy.exists());
        assert!(fixture.qoder.join("alpha").is_symlink());
    }

    #[test]
    fn universal_native_broken_and_unattached_agents_are_rejected() {
        let fixture = Fixture::new();
        let canonical = fixture.managed_skill("alpha");
        fixture.link(&fixture.claude, "alpha", &canonical);
        // A universal agent may carry a legacy physical link; it still reads
        // the canonical store and must not be offered a detach.
        fixture.link(&fixture.gemini, "alpha", &canonical);
        // A broken attachment is a read-only anomaly.
        fixture.write_lock(&["alpha", "beta"]);
        let beta = fixture.canonical_root.join("beta");
        fs::create_dir_all(&beta).unwrap();
        fs::write(beta.join("SKILL.md"), "# beta\n").unwrap();
        symlink(
            fixture.qoder.join("missing-target"),
            fixture.qoder.join("beta"),
        )
        .unwrap();

        let universal = fixture.detach("alpha", "gemini-cli").unwrap_err();
        assert_eq!(universal.code, crate::error::ErrorCode::AgentNative);
        let shared = fixture.detach("alpha", "codex").unwrap_err();
        assert_eq!(shared.code, crate::error::ErrorCode::AgentNative);
        let broken = fixture.detach("beta", "qoder").unwrap_err();
        assert_eq!(broken.code, crate::error::ErrorCode::AgentBroken);
        let not_attached = fixture.detach("alpha", "qoder").unwrap_err();
        assert_eq!(not_attached.code, crate::error::ErrorCode::AgentNotAttached);
        let unknown = fixture.detach("alpha", "not-an-agent").unwrap_err();
        assert_eq!(unknown.code, crate::error::ErrorCode::AgentNotDetected);

        // Nothing was removed or backed up.
        assert!(fixture.claude.join("alpha").is_symlink());
        assert!(fixture.gemini.join("alpha").is_symlink());
        assert!(fs::symlink_metadata(fixture.qoder.join("beta")).is_ok());
        assert!(fixture.backup_dirs().is_empty());
    }

    #[test]
    fn copy_only_and_unmanaged_skills_are_rejected() {
        let fixture = Fixture::without_shared_agent();
        fixture.write_lock(&["beta"]);
        let copy = fixture.copy(&fixture.claude, "beta");
        let gamma = fixture.canonical_root.join("gamma");
        fs::create_dir_all(&gamma).unwrap();
        fs::write(gamma.join("SKILL.md"), "# gamma\n").unwrap();

        let copy_only = fixture.detach("beta", "claude-code").unwrap_err();
        assert_eq!(copy_only.code, crate::error::ErrorCode::SkillCopyOnly);
        assert!(copy.join("SKILL.md").is_file());

        let unmanaged = fixture.detach("gamma", "claude-code").unwrap_err();
        assert_eq!(unmanaged.code, crate::error::ErrorCode::SkillUnmanaged);
        assert!(gamma.join("SKILL.md").is_file());

        assert!(fixture.backup_dirs().is_empty());
    }

    #[test]
    fn path_guards_reject_traversal_and_symlinked_agent_roots() {
        let fixture = Fixture::new();
        let canonical = fixture.managed_skill("alpha");
        fixture.link(&fixture.claude, "alpha", &canonical);

        for skill in ["../alpha", "alpha/../../qoder/skills/alpha", "", ".hidden"] {
            let error = fixture.detach(skill, "claude-code").unwrap_err();
            assert_eq!(
                error.code,
                crate::error::ErrorCode::PathInvalid,
                "skill id {skill:?}"
            );
        }
        for skill in ["alpha;rm", "$(id)", "`id`", "a|b", "a*b"] {
            let error = fixture.detach(skill, "claude-code").unwrap_err();
            assert_eq!(
                error.code,
                crate::error::ErrorCode::SkillInvalidName,
                "skill id {skill:?}"
            );
        }
        // An agent skills root that is a symlink makes containment ambiguous.
        let real = fixture._tmp.path().join("real-claude-skills");
        fs::create_dir_all(real.join("alpha")).unwrap();
        fs::write(real.join("alpha/SKILL.md"), "# copy\n").unwrap();
        fs::remove_dir_all(&fixture.claude).unwrap();
        symlink(&real, &fixture.claude).unwrap();
        let error = fixture.detach("alpha", "claude-code").unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::PathInvalid);
        assert!(error.detail.as_deref().unwrap().contains("符号链接"));
        assert!(real.join("alpha/SKILL.md").is_file());
    }

    #[test]
    fn detaching_the_last_attachment_reports_unattached_and_keeps_the_managed_skill() {
        let fixture = Fixture::without_shared_agent();
        let canonical = fixture.managed_skill("alpha");
        fixture.link(&fixture.claude, "alpha", &canonical);

        let result = fixture.detach("alpha", "claude-code").unwrap();

        assert!(result.unattached);
        assert!(result.remaining_agents.is_empty());
        let detail = crate::detail::read_with_agents(&fixture.paths, &fixture.agents, "alpha")
            .expect("detail after detach");
        assert_eq!(detail.management, ManagementState::Managed);
        assert!(detail.attachments.is_empty(), "detail shows 未挂载");
        assert_eq!(
            detail.canonical_path.as_deref(),
            Some(fixture.paths.display(&canonical).as_str())
        );
    }

    #[test]
    fn corrupt_lock_blocks_detach_before_touching_the_attachment() {
        let fixture = Fixture::new();
        let canonical = fixture.managed_skill("alpha");
        fixture.link(&fixture.claude, "alpha", &canonical);
        fs::write(fixture.paths.lock_path(), "{not json").unwrap();

        let error = fixture.detach("alpha", "claude-code").unwrap_err();

        assert_eq!(error.code, crate::error::ErrorCode::LockCorrupt);
        assert!(fixture.claude.join("alpha").is_symlink());
        assert!(fixture.backup_dirs().is_empty());
        assert!(fixture.log_lines().is_empty());
    }

    #[test]
    fn backup_failure_aborts_copy_detach_before_deleting() {
        let fixture = Fixture::without_shared_agent();
        let canonical = fixture.managed_skill("alpha");
        let copy = fixture.copy(&fixture.claude, "alpha");
        let lock_before = fs::read(fixture.paths.lock_path()).unwrap();
        fs::remove_dir_all(fixture.paths.backups_dir()).unwrap();
        fs::write(fixture.paths.backups_dir(), b"not a directory").unwrap();

        let error = fixture.detach("alpha", "claude-code").unwrap_err();

        assert_eq!(error.code, crate::error::ErrorCode::BackupFailed);
        assert!(copy.join("SKILL.md").is_file(), "copy must survive");
        assert!(canonical.join("SKILL.md").is_file());
        assert_eq!(fs::read(fixture.paths.lock_path()).unwrap(), lock_before);
        let lines = fixture.log_lines();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["result"], "failed");
        assert_eq!(
            lines[0]["error"]["code"],
            crate::error::ErrorCode::BackupFailed.as_str()
        );
        assert!(lines[0].get("backupPath").is_none());
    }

    #[test]
    fn project_detach_unlinks_only_the_project_attachment() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let root = tmp.path().join("projects/app");
        fs::create_dir_all(&home).unwrap();
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        paths.ensure_dirs().unwrap();
        let canonical = root.join(".agents/skills/alpha");
        fs::create_dir_all(&canonical).unwrap();
        fs::write(
            canonical.join("SKILL.md"),
            "---\nname: alpha\ndescription: alpha\n---\n",
        )
        .unwrap();
        let lock_path = root.join("skills-lock.json");
        fs::write(
            lock_path.clone(),
            serde_json::json!({
                "version": 1,
                "skills": {
                    "alpha": {
                        "source": "acme/skills",
                        "sourceType": "github",
                        "sourceUrl": "https://github.com/acme/skills.git",
                        "skillPath": "skills/alpha/SKILL.md",
                        "computedHash": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        let claude = root.join(".claude/skills");
        fs::create_dir_all(&claude).unwrap();
        symlink(&canonical, claude.join("alpha")).unwrap();
        let project = crate::projects::Project {
            id: uuid::Uuid::new_v4(),
            root: root.clone(),
            display_name: "app".to_string(),
            added_at: 1,
            last_opened_at: 1,
            check_revision: "initial".into(),
        };
        let layout = ScopeLayout::project(&paths, project);
        let lock_before = fs::read(&lock_path).unwrap();

        let result = super::detach_agent(
            &paths,
            &layout,
            &layout.scope.skill_id("alpha"),
            "claude-code",
        )
        .unwrap();

        assert!(!claude.join("alpha").exists());
        assert!(canonical.join("SKILL.md").is_file());
        assert_eq!(fs::read(&lock_path).unwrap(), lock_before);
        assert_eq!(
            result.canonical_path,
            fs::canonicalize(&canonical).unwrap().display().to_string()
        );
        assert!(result.unattached);
        assert!(result.remaining_agents.is_empty());
        assert!(
            !paths.canonical_skills_dir().exists(),
            "global canonical store untouched"
        );
    }
}
