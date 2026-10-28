//! Update planning and per-skill execution helpers.
//!
//! Updates are rebuilt as `skills add <lock source> -g -s <skill>` with an
//! explicit `-a` for every currently attached, non-universal agent plus
//! `-a universal`. Detached agents are deliberately absent so they stay
//! detached, and the forbidden `update` / `check` / `--all` / `--copy` /
//! `-a '*'` forms are never produced.
//!
//! The frontend selects update-available candidates from the read-only update
//! report. Rust independently re-validates management state, canonical
//! presence and lock source resolution; anything else is skipped and reported
//! per skill instead of being written.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use crate::backup::BackupAttachment;
use crate::baseline::LocalModification;
use crate::error::AppError;
use crate::inventory::{self, AgentDir, AttachmentLocation, AttachmentMode, ManagementState};
use crate::lock::{LockEntry, LockFile};
use crate::paths::AppPaths;
use crate::process::{CommandSpec, ExecOutput};

use super::install::{
    INSTALL_TIMEOUT, InstallOutput, InstallPlan, InstallTarget, UNIVERSAL_AGENT_ID,
    install_command, parse_install_output, verify_install,
};
use super::manager::{TaskOverall, TaskSkillResult, TaskSkillStatus};
use super::source::validate_source;

#[derive(Debug, Clone)]
pub struct UpdateItem {
    pub name: String,
    /// Normalized `owner/repo`, with `#ref` appended only when the lock pinned
    /// a non-default branch/tag/commit.
    pub source: String,
    pub lock_source: String,
    pub reference: Option<String>,
    pub canonical: PathBuf,
    pub lock_hash: String,
    pub local_modification: LocalModification,
    pub local_modification_detail: Option<String>,
    /// Explicit `-a` ids in deterministic order, with `universal` last.
    pub agent_ids: Vec<String>,
    pub targets: Vec<InstallTarget>,
    /// Every physical attachment, used for the backup manifest.
    pub attachments: Vec<BackupAttachment>,
    /// Display names of copy attachments that will become symlinks.
    pub copy_agents: Vec<String>,
}

impl UpdateItem {
    pub fn install_plan(&self) -> InstallPlan {
        InstallPlan {
            source: self.source.clone(),
            skills: vec![self.name.clone()],
            agent_ids: self.agent_ids.clone(),
            targets: self.targets.clone(),
        }
    }

    pub fn command(&self, runtime: &crate::cli::CliRuntime, timeout: Duration) -> CommandSpec {
        install_command(runtime, &self.install_plan(), timeout)
    }
}

#[derive(Debug, Clone)]
pub struct UpdateSkip {
    pub name: String,
    pub error: AppError,
}

#[derive(Debug, Clone, Default)]
pub struct UpdatePlan {
    pub items: Vec<UpdateItem>,
    pub skipped: Vec<UpdateSkip>,
}

impl UpdatePlan {
    pub fn total(&self) -> u32 {
        (self.items.len() + self.skipped.len()) as u32
    }
}

pub fn update_command(
    item: &UpdateItem,
    runtime: &crate::cli::CliRuntime,
    timeout: Duration,
) -> CommandSpec {
    let _ = INSTALL_TIMEOUT;
    item.command(runtime, timeout)
}

/// Resolve the selected names into executable items plus per-skill skips.
/// A corrupt/non-v3 lock fails the whole plan; per-skill problems only skip
/// that name so update-all can continue.
pub fn plan_updates(
    paths: &AppPaths,
    agents: &[AgentDir],
    names: &[String],
) -> Result<UpdatePlan, AppError> {
    let lock = LockFile::read(&paths.lock_path())?;
    let baseline_store = crate::baseline::BaselineStore::new(paths);
    let mut plan = UpdatePlan::default();

    for name in names {
        let located = match inventory::locate_with_agents(paths, agents, name) {
            Ok(Some(located)) => located,
            Ok(None) => {
                plan.skipped.push(UpdateSkip {
                    name: name.clone(),
                    error: AppError::skill_not_found(name),
                });
                continue;
            }
            Err(error) => return Err(error),
        };
        if let Some(skip) = skip_for_management(&located.management, name) {
            plan.skipped.push(skip);
            continue;
        }
        let Some(canonical) = located.canonical_path.clone() else {
            plan.skipped.push(UpdateSkip {
                name: name.clone(),
                error: AppError::skill_not_found(name),
            });
            continue;
        };
        if !is_directory(&canonical) {
            plan.skipped.push(UpdateSkip {
                name: name.clone(),
                error: AppError::verify_canonical_missing(name, &canonical),
            });
            continue;
        }
        let Some(entry) = located
            .lock_entry
            .clone()
            .or_else(|| lock.get(name).cloned())
        else {
            plan.skipped.push(UpdateSkip {
                name: name.clone(),
                error: AppError::skill_unmanaged(name),
            });
            continue;
        };
        let source = match update_source(name, &entry) {
            Ok(source) => source,
            Err(error) => {
                plan.skipped.push(UpdateSkip {
                    name: name.clone(),
                    error,
                });
                continue;
            }
        };

        let agent_dirs: HashMap<&str, &AgentDir> = agents
            .iter()
            .map(|agent| (agent.id.as_str(), agent))
            .collect();
        let mut targets: HashMap<String, InstallTarget> = HashMap::new();
        let mut copy_agents: Vec<(String, String)> = Vec::new();
        let mut attachments = Vec::with_capacity(located.attachments.len());
        for attachment in &located.attachments {
            attachments.push(backup_attachment(attachment));
            let Some(agent) = agent_dirs.get(attachment.agent_id.as_str()) else {
                continue;
            };
            match attachment.mode {
                AttachmentMode::Link | AttachmentMode::Copy => {
                    if agent.universal {
                        continue;
                    }
                    if attachment.mode == AttachmentMode::Copy {
                        copy_agents
                            .push((attachment.agent_id.clone(), attachment.agent_name.clone()));
                    }
                    targets
                        .entry(attachment.agent_id.clone())
                        .or_insert(InstallTarget {
                            id: attachment.agent_id.clone(),
                            display_name: attachment.agent_name.clone(),
                            universal: false,
                            skills_dir: agent.skills_dir.clone(),
                        });
                }
                AttachmentMode::Native | AttachmentMode::Broken => {}
            }
        }
        attachments.sort_by(|left, right| left.agent_id.cmp(&right.agent_id));
        copy_agents.sort_by(|left, right| left.0.cmp(&right.0));

        let mut targets: Vec<InstallTarget> = targets.into_values().collect();
        targets.sort_by(|left, right| left.id.cmp(&right.id));
        let mut agent_ids: Vec<String> = targets.iter().map(|target| target.id.clone()).collect();
        agent_ids.push(UNIVERSAL_AGENT_ID.to_string());

        let assessment = baseline_store.assess(
            name,
            Some(&canonical),
            Some(entry.skill_folder_hash.as_str()),
        );
        plan.items.push(UpdateItem {
            name: name.clone(),
            source,
            lock_source: normalized_lock_source(&entry).unwrap_or_else(|| entry.source.clone()),
            reference: entry.reference.clone(),
            canonical,
            lock_hash: entry.skill_folder_hash.clone(),
            local_modification: assessment.state,
            local_modification_detail: assessment.detail,
            agent_ids,
            targets,
            attachments,
            copy_agents: copy_agents
                .into_iter()
                .map(|(_, display_name)| display_name)
                .collect(),
        });
    }
    Ok(plan)
}

fn skip_for_management(management: &ManagementState, name: &str) -> Option<UpdateSkip> {
    let error = match management {
        ManagementState::Managed => return None,
        ManagementState::CopyOnly => AppError::skill_copy_only(name),
        ManagementState::Unmanaged => AppError::skill_unmanaged(name),
    };
    Some(UpdateSkip {
        name: name.to_string(),
        error,
    })
}

fn backup_attachment(attachment: &AttachmentLocation) -> BackupAttachment {
    BackupAttachment {
        agent_id: attachment.agent_id.clone(),
        agent_name: attachment.agent_name.clone(),
        path: attachment.path.clone(),
        mode: attachment.mode,
    }
}

fn is_directory(path: &std::path::Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_dir())
        .unwrap_or(false)
}

pub fn normalized_lock_source(entry: &LockEntry) -> Option<String> {
    if !entry.source_type.eq_ignore_ascii_case("github") {
        return None;
    }
    for candidate in [entry.source.as_str(), entry.source_url.as_str()] {
        if let Ok(spec) = validate_source(candidate) {
            return Some(spec.normalized);
        }
    }
    None
}

/// Build the CLI source from the lock. v1 only installs default branches, but
/// a lock written by another tool may carry `ref`; it is re-validated and
/// appended as `owner/repo#ref` so the update follows the recorded ref.
fn update_source(name: &str, entry: &LockEntry) -> Result<String, AppError> {
    let Some(source) = normalized_lock_source(entry) else {
        return Err(AppError::skill_source_unresolvable(
            name,
            format!(
                "source={:?} sourceType={:?} sourceUrl={:?}",
                entry.source, entry.source_type, entry.source_url
            ),
        ));
    };
    match entry.reference.as_deref() {
        None => Ok(source),
        Some(raw) => {
            let reference = raw.trim();
            if reference.is_empty() || reference.eq_ignore_ascii_case("HEAD") {
                return Ok(source);
            }
            if !valid_ref(reference) {
                return Err(AppError::skill_source_unresolvable(
                    name,
                    format!("lock ref 不是安全的分支/标签名：{reference:?}"),
                ));
            }
            Ok(format!("{source}#{reference}"))
        }
    }
}

fn valid_ref(reference: &str) -> bool {
    !reference.is_empty()
        && reference.len() <= 200
        && !reference.starts_with('-')
        && !reference.starts_with('/')
        && !reference.ends_with('/')
        && !reference.contains("..")
        && reference.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | '/')
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateVerification {
    /// `Ok(())` means canonical, lock key and every explicit attachment passed.
    pub result: Result<(), AppError>,
    pub warnings: Vec<AppError>,
}

/// Interpret one `add --json` result and verify the filesystem afterwards.
/// The CLI's `installed` status is never trusted on its own.
pub fn verify_update(
    paths: &AppPaths,
    item: &UpdateItem,
    output: &ExecOutput,
) -> UpdateVerification {
    let cli_results = match parse_install_output(&output.stdout) {
        InstallOutput::Results(results) => results,
        InstallOutput::Fatal(detail) => {
            return UpdateVerification {
                result: Err(super::manager::classify_install_failure(
                    &detail,
                    &output.stderr,
                )),
                warnings: Vec::new(),
            };
        }
        InstallOutput::Unrecognized(detail) => {
            return UpdateVerification {
                result: Err(super::manager::classify_install_failure(
                    &output.stderr,
                    &detail,
                )),
                warnings: Vec::new(),
            };
        }
    };
    let Some(entry) = cli_results
        .iter()
        .find(|result| result.name.as_deref() == Some(item.name.as_str()))
        .or_else(|| (cli_results.len() == 1).then(|| &cli_results[0]))
    else {
        return UpdateVerification {
            result: Err(AppError::verify_failed(format!(
                "CLI 未返回 skill「{}」的结果。",
                item.name
            ))),
            warnings: Vec::new(),
        };
    };
    match entry.status {
        super::install::CliSkillStatus::Installed => {}
        super::install::CliSkillStatus::Skipped => {
            return UpdateVerification {
                result: Err(AppError::skill_not_in_source(
                    &item.name,
                    entry
                        .reason
                        .clone()
                        .unwrap_or_else(|| "CLI 跳过了该 skill。".to_string()),
                )),
                warnings: Vec::new(),
            };
        }
        super::install::CliSkillStatus::Failed => {
            return UpdateVerification {
                result: Err(AppError::skill_install_failed(
                    &item.name,
                    entry
                        .error
                        .clone()
                        .unwrap_or_else(|| "CLI 报告更新失败。".to_string()),
                )),
                warnings: Vec::new(),
            };
        }
    }

    let plan = item.install_plan();
    let verification = verify_install(paths, &plan, &cli_results);
    let result = match verification.results.first() {
        Some(result) if result.status == TaskSkillStatus::Installed => Ok(()),
        Some(result) => Err(result.error.clone().unwrap_or_else(|| {
            AppError::verify_failed(format!("更新后校验「{}」失败。", item.name))
        })),
        None => Err(AppError::verify_failed(format!(
            "更新后未生成 skill「{}」的校验结果。",
            item.name
        ))),
    };
    UpdateVerification {
        result,
        warnings: verification.warnings,
    }
}

pub fn overall_for_update(results: &[TaskSkillResult], cancelled: bool) -> TaskOverall {
    if cancelled {
        return TaskOverall::Cancelled;
    }
    let updated = results
        .iter()
        .filter(|result| result.status == TaskSkillStatus::Updated)
        .count();
    if updated > 0 && updated == results.len() {
        TaskOverall::Success
    } else if updated > 0 {
        TaskOverall::Partial
    } else {
        TaskOverall::Failed
    }
}

pub fn summary_message(overall: TaskOverall, results: &[TaskSkillResult]) -> String {
    let updated = results
        .iter()
        .filter(|result| result.status == TaskSkillStatus::Updated)
        .count();
    let unfinished = results.len().saturating_sub(updated);
    match overall {
        TaskOverall::Success => format!("更新完成：{updated} 个 skill 已更新。"),
        TaskOverall::Partial => {
            format!("部分更新完成：{updated} 个成功，{unfinished} 个未完成。")
        }
        TaskOverall::Failed => format!("更新失败：{unfinished} 个 skill 未完成。"),
        TaskOverall::Cancelled => format!("更新已取消：{updated} 个已更新。"),
    }
}

pub fn status_label(status: TaskSkillStatus) -> &'static str {
    match status {
        TaskSkillStatus::Installed => "installed",
        TaskSkillStatus::Updated => "updated",
        TaskSkillStatus::Skipped => "skipped",
        TaskSkillStatus::Failed => "failed",
    }
}

/// Names that may be retried: every failed skill, never skipped/ineligible
/// rows. Used by the frontend result view and kept here for contract clarity.
pub fn retryable_names(results: &[TaskSkillResult]) -> Vec<String> {
    results
        .iter()
        .filter(|result| result.status == TaskSkillStatus::Failed)
        .map(|result| result.name.clone())
        .collect()
}

pub const UPDATE_TIMEOUT: Duration = INSTALL_TIMEOUT;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::codes;
    use crate::inventory::AgentDir;
    use std::os::unix::fs::symlink;

    struct Fixture {
        _tmp: tempfile::TempDir,
        paths: AppPaths,
        agents: Vec<AgentDir>,
        canonical: PathBuf,
        qoder: PathBuf,
        claude: PathBuf,
        windsurf: PathBuf,
        gemini: PathBuf,
    }

    impl Fixture {
        fn new(entries: &[(&str, &str, Option<&str>)]) -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path().join("home");
            let paths = AppPaths::new(&home, tmp.path().join("app-data"));
            paths.ensure_dirs().unwrap();
            let canonical_root = paths.canonical_skills_dir();
            fs::create_dir_all(&canonical_root).unwrap();
            let qoder = home.join(".qoder/skills");
            let claude = home.join(".claude/skills");
            let windsurf = home.join(".codeium/windsurf/skills");
            let gemini = home.join(".gemini/skills");
            for dir in [&qoder, &claude, &windsurf, &gemini] {
                fs::create_dir_all(dir).unwrap();
            }
            // Codex shares the canonical store and must never receive `-a codex`.
            fs::create_dir_all(home.join(".codex")).unwrap();
            let canonical = canonical_root.join("alpha");
            fs::create_dir_all(&canonical).unwrap();
            fs::write(
                canonical.join("SKILL.md"),
                b"---\nname: alpha\ndescription: alpha\n---\n# alpha\n",
            )
            .unwrap();

            let mut skills = serde_json::Map::new();
            for (name, source, reference) in entries {
                let mut entry = serde_json::json!({
                    "source": source,
                    "sourceType": "github",
                    "sourceUrl": format!("https://github.com/{source}.git"),
                    "skillPath": format!("skills/{name}/SKILL.md"),
                    "skillFolderHash": "0123456789abcdef0123456789abcdef01234567",
                    "installedAt": "2026-01-01T00:00:00.000Z",
                    "updatedAt": "2026-01-01T00:00:00.000Z"
                });
                if let Some(reference) = reference {
                    entry["ref"] = serde_json::json!(reference);
                }
                skills.insert((*name).to_string(), entry);
            }
            fs::create_dir_all(paths.agents_root()).unwrap();
            fs::write(
                paths.lock_path(),
                serde_json::json!({"version":3,"skills":skills}).to_string(),
            )
            .unwrap();

            // Qoder links, Claude copies, Windsurf is detected but has no
            // attachment (detached), Gemini is universal-by-catalog.
            symlink(&canonical, qoder.join("alpha")).unwrap();
            fs::create_dir_all(claude.join("alpha")).unwrap();
            fs::write(claude.join("alpha/SKILL.md"), "# alpha copy\n").unwrap();
            symlink(&canonical, gemini.join("alpha")).unwrap();

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
                    id: "windsurf".to_string(),
                    display_name: "Windsurf".to_string(),
                    universal: false,
                    skills_dir: windsurf.clone(),
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
                canonical,
                qoder,
                claude,
                windsurf,
                gemini,
            }
        }
    }

    fn runtime() -> crate::cli::CliRuntime {
        crate::cli::CliRuntime {
            npx_path: PathBuf::from("/node/bin/npx"),
            node_dir: PathBuf::from("/node/bin"),
            git_dir: Some(PathBuf::from("/usr/bin")),
            cwd: PathBuf::from("/app-data/cli-cwd"),
            home: PathBuf::from("/Users/tester"),
            env: std::collections::BTreeMap::from([
                ("CI".to_string(), "true".to_string()),
                ("DISABLE_TELEMETRY".to_string(), "1".to_string()),
            ]),
            pinned_version: crate::cli::PINNED_CLI_VERSION.to_string(),
        }
    }

    fn plan(fixture: &Fixture, names: &[&str]) -> UpdatePlan {
        let names: Vec<String> = names.iter().map(|value| value.to_string()).collect();
        plan_updates(&fixture.paths, &fixture.agents, &names).unwrap()
    }

    #[test]
    fn builds_add_with_current_physical_agents_and_universal_only() {
        let fixture = Fixture::new(&[("alpha", "acme/skills", None)]);
        let plan = plan(&fixture, &["alpha"]);
        assert!(plan.skipped.is_empty());
        let item = &plan.items[0];
        assert_eq!(item.source, "acme/skills");
        assert_eq!(item.lock_source, "acme/skills");
        assert_eq!(item.canonical, fixture.canonical);
        assert_eq!(
            item.agent_ids,
            vec![
                "claude-code".to_string(),
                "qoder".to_string(),
                "universal".to_string()
            ],
            "detached windsurf, universal gemini/codex and native codex must be excluded"
        );
        assert_eq!(
            item.copy_agents,
            vec!["Claude Code".to_string()],
            "copy attachments are flagged for conversion"
        );
        assert!(!fixture.windsurf.join("alpha").exists());
        assert!(fixture.gemini.join("alpha").is_symlink());

        let spec = item.command(&runtime(), Duration::from_secs(5));
        assert_eq!(
            spec.args,
            vec![
                "-y",
                "skills@1.7.4",
                "add",
                "acme/skills",
                "-g",
                "-s",
                "alpha",
                "-a",
                "claude-code",
                "-a",
                "qoder",
                "-a",
                "universal",
                "-y",
                "--json"
            ]
        );
        for forbidden in ["update", "check", "remove", "--all", "--copy", "*"] {
            assert!(
                !spec.args.iter().any(|arg| arg == forbidden),
                "{forbidden} must never appear in update argv"
            );
        }
        // The backup manifest carries every physical attachment, but only the
        // copy is duplicated by the backup writer.
        assert_eq!(item.attachments.len(), 4);
        let claude = item
            .attachments
            .iter()
            .find(|attachment| attachment.agent_id == "claude-code")
            .unwrap();
        assert_eq!(claude.mode, AttachmentMode::Copy);
    }

    #[test]
    fn universal_copy_attachments_are_backed_up_but_not_reinstalled() {
        let fixture = Fixture::new(&[("alpha", "acme/skills", None)]);
        // Gemini is universal-by-catalog and mounted as a real copy. It must
        // stay out of the explicit `-a` list and out of the copy-conversion
        // warning while still appearing in the backup manifest.
        fs::remove_file(fixture.gemini.join("alpha")).unwrap();
        fs::create_dir_all(fixture.gemini.join("alpha")).unwrap();
        fs::write(fixture.gemini.join("alpha/SKILL.md"), "# gemini copy\n").unwrap();

        let item = &plan(&fixture, &["alpha"]).items[0];
        assert_eq!(
            item.agent_ids,
            vec![
                "claude-code".to_string(),
                "qoder".to_string(),
                "universal".to_string()
            ],
            "universal agents must only be covered by `-a universal`"
        );
        assert_eq!(
            item.copy_agents,
            vec!["Claude Code".to_string()],
            "a universal copy must not be advertised as a copy conversion"
        );
        let gemini = item
            .attachments
            .iter()
            .find(|attachment| attachment.agent_id == "gemini-cli")
            .expect("universal physical attachment in the backup manifest");
        assert_eq!(gemini.mode, AttachmentMode::Copy);
    }

    #[test]
    fn detached_agents_are_not_reattached_and_no_attachment_means_universal_only() {
        let fixture = Fixture::new(&[("alpha", "acme/skills", None)]);
        fs::remove_file(fixture.qoder.join("alpha")).unwrap();
        fs::remove_dir_all(fixture.claude.join("alpha")).unwrap();
        let plan = plan(&fixture, &["alpha"]);
        let item = &plan.items[0];
        assert_eq!(item.agent_ids, vec!["universal".to_string()]);
        assert!(item.targets.is_empty());
        assert!(item.copy_agents.is_empty());
        let spec = item.command(&runtime(), Duration::from_secs(5));
        assert_eq!(
            spec.args,
            vec![
                "-y",
                "skills@1.7.4",
                "add",
                "acme/skills",
                "-g",
                "-s",
                "alpha",
                "-a",
                "universal",
                "-y",
                "--json"
            ]
        );
    }

    #[test]
    fn lock_ref_is_validated_and_appended_to_the_source() {
        let fixture = Fixture::new(&[("alpha", "acme/skills", Some("dev"))]);
        let item = &plan(&fixture, &["alpha"]).items[0];
        assert_eq!(item.source, "acme/skills#dev");
        assert_eq!(item.reference.as_deref(), Some("dev"));
    }

    #[test]
    fn unsafe_refs_skip_the_skill() {
        let fixture = Fixture::new(&[("alpha", "acme/skills", Some("--all"))]);
        let plan = plan(&fixture, &["alpha"]);
        assert!(plan.items.is_empty());
        assert_eq!(plan.skipped[0].error.code, codes::SKILL_SOURCE_UNRESOLVABLE);
    }

    #[test]
    fn skips_copy_only_unmanaged_stale_and_non_github_sources() {
        let fixture = Fixture::new(&[
            ("alpha", "acme/skills", None),
            ("copy-only", "acme/skills", None),
            ("non-github", "acme/skills", None),
        ]);
        // copy-only: lock + a real copy in Claude, no canonical.
        fs::create_dir_all(fixture.claude.join("copy-only")).unwrap();
        fs::write(fixture.claude.join("copy-only/SKILL.md"), "copy\n").unwrap();
        let mut lock: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(fixture.paths.lock_path()).unwrap()).unwrap();
        lock["skills"]["copy-only"] = serde_json::json!({
            "source": "acme/skills",
            "sourceType": "github",
            "sourceUrl": "https://github.com/acme/skills.git",
            "skillPath": "skills/copy-only/SKILL.md",
            "skillFolderHash": "0123456789abcdef0123456789abcdef01234567",
            "installedAt": "2026-01-01T00:00:00.000Z",
            "updatedAt": "2026-01-01T00:00:00.000Z"
        });
        lock["skills"]["non-github"]["sourceType"] = serde_json::json!("local");
        lock["skills"]["non-github"]["source"] = serde_json::json!("/tmp/local");
        fs::write(fixture.paths.lock_path(), lock.to_string()).unwrap();
        let non_github = fixture.paths.canonical_skills_dir().join("non-github");
        fs::create_dir_all(&non_github).unwrap();
        fs::write(non_github.join("SKILL.md"), "non-github\n").unwrap();
        // Unmanaged has a canonical directory but no lock entry.
        let unmanaged = fixture.paths.canonical_skills_dir().join("unmanaged");
        fs::create_dir_all(&unmanaged).unwrap();
        fs::write(unmanaged.join("SKILL.md"), "unmanaged\n").unwrap();
        // Stale has a lock entry and no directory at all.

        let plan = plan(
            &fixture,
            &["alpha", "copy-only", "non-github", "unmanaged", "ghost"],
        );
        assert_eq!(plan.items.len(), 1);
        assert_eq!(plan.items[0].name, "alpha");
        let skipped: std::collections::HashMap<_, _> = plan
            .skipped
            .iter()
            .map(|skip| (skip.name.as_str(), skip.error.code.as_str()))
            .collect();
        assert_eq!(skipped["copy-only"], codes::SKILL_COPY_ONLY);
        assert_eq!(skipped["non-github"], codes::SKILL_SOURCE_UNRESOLVABLE);
        assert_eq!(skipped["unmanaged"], codes::SKILL_UNMANAGED);
        assert_eq!(skipped["ghost"], codes::SKILL_NOT_FOUND);
    }

    #[test]
    fn corrupt_lock_fails_the_whole_plan() {
        let fixture = Fixture::new(&[("alpha", "acme/skills", None)]);
        fs::write(fixture.paths.lock_path(), "{not json").unwrap();
        let error =
            plan_updates(&fixture.paths, &fixture.agents, &["alpha".to_string()]).unwrap_err();
        assert_eq!(error.code, codes::LOCK_CORRUPT);
    }

    #[test]
    fn retryable_names_include_only_failed_rows() {
        let results = vec![
            TaskSkillResult {
                name: "a".to_string(),
                status: TaskSkillStatus::Updated,
                error: None,
                backup_path: None,
            },
            TaskSkillResult {
                name: "b".to_string(),
                status: TaskSkillStatus::Failed,
                error: Some(AppError::verify_failed("x")),
                backup_path: None,
            },
            TaskSkillResult {
                name: "c".to_string(),
                status: TaskSkillStatus::Skipped,
                error: Some(AppError::skill_not_in_source("c", "x")),
                backup_path: None,
            },
        ];
        assert_eq!(retryable_names(&results), vec!["b".to_string()]);
        assert_eq!(overall_for_update(&results, false), TaskOverall::Partial);
    }

    // ------------------------------------------------------------------
    // Task execution with a temp HOME and a scripted executor.
    // ------------------------------------------------------------------

    use crate::baseline::fingerprint_folder;
    use crate::cli::{CliRuntime, CliRuntimeProvider, PINNED_CLI_VERSION};
    use crate::process::{CancelToken, CommandSpec, ExecError, ExecOutput, ProcessExecutor};
    use crate::tasks::conflicts::{ConflictInfo, ConflictProvider};
    use crate::tasks::manager::{
        EventSink, TaskEvent, TaskManager, TaskManagerConfig, TaskOverall, TaskPhase,
        TaskSkillStatus,
    };
    use std::sync::{Arc, Mutex};
    use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

    struct FakeRuntime;

    #[async_trait::async_trait]
    impl CliRuntimeProvider for FakeRuntime {
        async fn cli_runtime(&self) -> Result<CliRuntime, AppError> {
            Ok(CliRuntime {
                npx_path: PathBuf::from("/node/bin/npx"),
                node_dir: PathBuf::from("/node/bin"),
                git_dir: Some(PathBuf::from("/usr/bin")),
                cwd: PathBuf::from("/app-data/cli-cwd"),
                home: PathBuf::from("/Users/tester"),
                env: std::collections::BTreeMap::from([
                    ("CI".to_string(), "true".to_string()),
                    ("DISABLE_TELEMETRY".to_string(), "1".to_string()),
                ]),
                pinned_version: PINNED_CLI_VERSION.to_string(),
            })
        }
    }

    #[derive(Default)]
    struct NoConflicts;

    impl ConflictProvider for NoConflicts {
        fn lookup(&self, _names: &[String]) -> std::collections::HashMap<String, ConflictInfo> {
            std::collections::HashMap::new()
        }
    }

    struct UpdateResponse {
        output: ExecOutput,
        post: Option<Box<dyn FnOnce() + Send>>,
    }

    impl UpdateResponse {
        fn installed(name: &str) -> Self {
            Self {
                output: ExecOutput {
                    code: Some(0),
                    signal: None,
                    stdout: serde_json::json!([{
                        "name": name,
                        "status": "installed",
                        "source": "acme/skills"
                    }])
                    .to_string(),
                    stderr: String::new(),
                    duration: Duration::ZERO,
                },
                post: None,
            }
        }

        fn failed(name: &str, error: &str) -> Self {
            Self {
                output: ExecOutput {
                    code: Some(1),
                    signal: None,
                    stdout: serde_json::json!([{
                        "name": name,
                        "status": "failed",
                        "error": error
                    }])
                    .to_string(),
                    stderr: String::new(),
                    duration: Duration::ZERO,
                },
                post: None,
            }
        }

        fn with_post(mut self, post: impl FnOnce() + Send + 'static) -> Self {
            self.post = Some(Box::new(post));
            self
        }
    }

    #[derive(Default)]
    struct UpdateExecutor {
        responses: Mutex<std::collections::VecDeque<UpdateResponse>>,
        calls: Mutex<Vec<CommandSpec>>,
    }

    impl UpdateExecutor {
        fn new(responses: Vec<UpdateResponse>) -> Self {
            Self {
                responses: Mutex::new(responses.into()),
                ..Self::default()
            }
        }

        fn calls(&self) -> Vec<CommandSpec> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl ProcessExecutor for UpdateExecutor {
        async fn run(
            &self,
            spec: CommandSpec,
            _cancel: CancelToken,
        ) -> Result<ExecOutput, ExecError> {
            {
                let mut calls = self.calls.lock().unwrap();
                calls.push(spec);
            }
            let response = self
                .responses
                .lock()
                .unwrap()
                .pop_front()
                .expect("one scripted update response per CLI call");
            if let Some(post) = response.post {
                post();
            }
            Ok(response.output)
        }
    }

    fn sink() -> (EventSink, UnboundedReceiver<TaskEvent>) {
        let (sender, receiver) = unbounded_channel();
        let sink: EventSink = Arc::new(move |event| {
            let _ = sender.send(event);
        });
        (sink, receiver)
    }

    fn update_manager(paths: &AppPaths, executor: Arc<UpdateExecutor>) -> TaskManager {
        TaskManager::new(
            Arc::new(FakeRuntime),
            executor,
            Arc::new(NoConflicts),
            paths.clone(),
            TaskManagerConfig {
                preview_timeout: Duration::from_secs(60),
                install_timeout: Duration::from_secs(5),
            },
        )
    }

    async fn run_update_task(manager: &TaskManager, names: &[&str]) -> (Vec<TaskPhase>, TaskEvent) {
        let (sink, mut receiver) = sink();
        manager
            .start_update(names.iter().map(|name| name.to_string()).collect(), sink)
            .await
            .unwrap();
        let mut phases = Vec::new();
        let final_event = loop {
            let event = receiver.recv().await.expect("update event");
            phases.push(event.phase);
            if event.phase.is_final() {
                break event;
            }
        };
        (phases, final_event)
    }

    fn simulate_update(
        canonical: PathBuf,
        qoder: PathBuf,
        claude: PathBuf,
        lock_path: PathBuf,
        name: &'static str,
    ) -> impl FnOnce() + Send {
        move || {
            fs::write(canonical.join("SKILL.md"), format!("# {name} v2\n")).unwrap();
            let qoder_link = qoder.join(name);
            if fs::symlink_metadata(&qoder_link).is_ok() {
                let _ = fs::remove_file(&qoder_link);
                let _ = fs::remove_dir_all(&qoder_link);
            }
            symlink(&canonical, &qoder_link).unwrap();
            let claude_copy = claude.join(name);
            if fs::symlink_metadata(&claude_copy).is_ok() {
                let _ = fs::remove_file(&claude_copy);
                let _ = fs::remove_dir_all(&claude_copy);
            }
            symlink(&canonical, &claude_copy).unwrap();

            let raw = fs::read_to_string(&lock_path).unwrap();
            let mut lock: serde_json::Value = serde_json::from_str(&raw).unwrap();
            if let Some(entry) = lock["skills"].get_mut(name) {
                entry["skillFolderHash"] = serde_json::json!("b".repeat(40));
                entry["updatedAt"] = serde_json::json!("2026-02-02T00:00:00.000Z");
            }
            fs::write(&lock_path, lock.to_string()).unwrap();
        }
    }

    fn add_canonical(fixture: &Fixture, name: &str) -> PathBuf {
        let dir = fixture.paths.canonical_skills_dir().join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), format!("# {name} v1\n")).unwrap();
        dir
    }

    fn backup_dirs(fixture: &Fixture) -> Vec<PathBuf> {
        fs::read_dir(fixture.paths.backups_dir())
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn log_lines(fixture: &Fixture) -> Vec<serde_json::Value> {
        fs::read_to_string(fixture.paths.logs_dir().join("operations.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[tokio::test]
    async fn update_success_backs_up_runs_add_verifies_and_refreshes_the_baseline() {
        let fixture = Fixture::new(&[("alpha", "acme/skills", None)]);
        let post = simulate_update(
            fixture.canonical.clone(),
            fixture.qoder.clone(),
            fixture.claude.clone(),
            fixture.paths.lock_path(),
            "alpha",
        );
        let executor = Arc::new(UpdateExecutor::new(vec![
            UpdateResponse::installed("alpha").with_post(post),
        ]));
        let manager = update_manager(&fixture.paths, executor.clone());
        let (phases, final_event) = run_update_task(&manager, &["alpha"]).await;

        assert_eq!(final_event.phase, TaskPhase::Completed);
        assert_eq!(phases.first(), Some(&TaskPhase::Preparing));
        assert!(
            phases.contains(&TaskPhase::Backup),
            "backup phase must be reported: {phases:?}"
        );
        assert!(phases.contains(&TaskPhase::RunningCli));
        assert!(phases.contains(&TaskPhase::Verifying));
        assert_eq!(final_event.completed, Some(1));
        assert_eq!(final_event.total, Some(1));

        let result = final_event.result.expect("update result");
        assert_eq!(result.overall, TaskOverall::Success);
        assert_eq!(result.skills.len(), 1);
        assert_eq!(result.skills[0].name, "alpha");
        assert_eq!(result.skills[0].status, TaskSkillStatus::Updated);
        assert!(result.skills[0].error.is_none());
        let backup = result.skills[0].backup_path.clone().expect("skill backup");
        assert_eq!(result.backup_path.as_deref(), Some(backup.as_str()));
        let backed_up =
            fs::read_to_string(PathBuf::from(&backup).join("canonical/SKILL.md")).unwrap();
        assert!(backed_up.contains("# alpha"), "{backed_up:?}");
        assert!(
            !backed_up.contains("v2"),
            "backup must be the pre-update copy"
        );
        assert_eq!(
            fs::read_to_string(PathBuf::from(&backup).join("attachments/claude-code/SKILL.md"))
                .unwrap(),
            "# alpha copy\n"
        );
        let manifest: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(PathBuf::from(&backup).join("attachment-manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["action"], "update");
        assert_eq!(manifest["skill"], "alpha");
        assert_eq!(
            manifest["agents"],
            serde_json::json!(["claude-code", "qoder", "universal"])
        );

        assert_eq!(
            result
                .warnings
                .iter()
                .filter(|warning| warning.code == "update.copy_to_symlink")
                .count(),
            1
        );
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.code == "update.local_modified"),
            "{:?}",
            result.warnings
        );
        let log_path = result.log_path.expect("log path");
        assert!(PathBuf::from(&log_path).is_file());
        let lines = log_lines(&fixture);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["action"], "update");
        assert_eq!(lines[0]["skill"], "alpha");
        assert_eq!(
            lines[0]["agents"],
            serde_json::json!(["claude-code", "qoder", "universal"])
        );
        assert_eq!(lines[0]["result"], "updated");
        assert_eq!(lines[0]["backupPath"], backup);
        assert!(lines[0].get("error").is_none());
        assert!(lines[0]["time"].as_str().unwrap().ends_with('Z'));

        // Disk truth: canonical updated, both physical agents relinked, the
        // detached windsurf agent untouched.
        assert_eq!(
            fs::read_to_string(fixture.canonical.join("SKILL.md")).unwrap(),
            "# alpha v2\n"
        );
        assert!(fixture.qoder.join("alpha").is_symlink());
        assert!(fixture.claude.join("alpha").is_symlink());
        assert!(!fixture.windsurf.join("alpha").exists());
        assert!(fixture.gemini.join("alpha").is_symlink());

        let baseline = crate::baseline::BaselineStore::new(&fixture.paths)
            .load("alpha")
            .expect("verified baseline");
        assert!(baseline.verified);
        assert_eq!(
            baseline.content_hash,
            fingerprint_folder(&fixture.canonical).unwrap().content_hash
        );

        let calls = executor.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].args,
            vec![
                "-y",
                "skills@1.7.4",
                "add",
                "acme/skills",
                "-g",
                "-s",
                "alpha",
                "-a",
                "claude-code",
                "-a",
                "qoder",
                "-a",
                "universal",
                "-y",
                "--json"
            ]
        );
        for forbidden in ["update", "check", "remove", "--all", "--copy", "*"] {
            assert!(
                !calls[0].args.iter().any(|arg| arg == forbidden),
                "{forbidden} must never be spawned by update"
            );
        }
    }

    #[tokio::test]
    async fn update_all_continues_after_failures_and_skips_ineligible_skills() {
        let fixture = Fixture::new(&[
            ("alpha", "acme/skills", None),
            ("beta", "acme/skills", None),
            ("copy-only", "acme/skills", None),
        ]);
        let beta = add_canonical(&fixture, "beta");
        symlink(&beta, fixture.qoder.join("beta")).unwrap();
        fs::create_dir_all(fixture.claude.join("copy-only")).unwrap();
        fs::write(fixture.claude.join("copy-only/SKILL.md"), "copy\n").unwrap();

        let executor = Arc::new(UpdateExecutor::new(vec![
            UpdateResponse::installed("alpha").with_post(simulate_update(
                fixture.canonical.clone(),
                fixture.qoder.clone(),
                fixture.claude.clone(),
                fixture.paths.lock_path(),
                "alpha",
            )),
            UpdateResponse::failed("beta", "boom"),
        ]));
        let manager = update_manager(&fixture.paths, executor.clone());
        let (_phases, final_event) =
            run_update_task(&manager, &["alpha", "beta", "copy-only"]).await;

        assert_eq!(final_event.phase, TaskPhase::Completed);
        let result = final_event.result.unwrap();
        assert_eq!(result.overall, TaskOverall::Partial);
        let status = |name: &str| {
            result
                .skills
                .iter()
                .find(|skill| skill.name == name)
                .map(|skill| skill.status)
                .unwrap()
        };
        assert_eq!(status("alpha"), TaskSkillStatus::Updated);
        assert_eq!(status("beta"), TaskSkillStatus::Failed);
        assert_eq!(status("copy-only"), TaskSkillStatus::Skipped);
        let copy_only = result
            .skills
            .iter()
            .find(|skill| skill.name == "copy-only")
            .unwrap();
        assert_eq!(
            copy_only.error.as_ref().unwrap().code,
            crate::error::codes::SKILL_COPY_ONLY
        );
        assert!(copy_only.backup_path.is_none());
        assert_eq!(backup_dirs(&fixture).len(), 2, "alpha and beta only");
        assert_eq!(executor.calls().len(), 2);
        assert!(executor.calls()[1].args.contains(&"beta".to_string()));
        assert!(!executor.calls()[1].args.iter().any(|arg| arg == "--copy"));

        let lines = log_lines(&fixture);
        assert_eq!(lines.len(), 3);
        let line_for = |name: &str| {
            lines
                .iter()
                .find(|line| line["skill"] == name)
                .unwrap()
                .clone()
        };
        assert_eq!(line_for("alpha")["result"], "updated");
        assert_eq!(line_for("beta")["result"], "failed");
        assert_eq!(line_for("beta")["error"]["code"], "skill.install_failed");
        assert_eq!(line_for("copy-only")["result"], "skipped");
        assert!(line_for("copy-only").get("backupPath").is_none());
    }

    #[tokio::test]
    async fn corrupt_lock_rejects_the_update_before_a_task_starts() {
        let fixture = Fixture::new(&[("alpha", "acme/skills", None)]);
        fs::write(fixture.paths.lock_path(), "{not json").unwrap();
        let executor = Arc::new(UpdateExecutor::new(vec![UpdateResponse::installed("alpha")]));
        let manager = update_manager(&fixture.paths, executor.clone());
        let (sink, _receiver) = sink();
        let error = manager
            .start_update(vec!["alpha".to_string()], sink)
            .await
            .unwrap_err();
        assert_eq!(error.code, crate::error::codes::LOCK_CORRUPT);
        assert_eq!(executor.calls().len(), 0);
    }

    #[tokio::test]
    async fn backup_failure_aborts_before_the_cli_runs() {
        let fixture = Fixture::new(&[("alpha", "acme/skills", None)]);
        fs::remove_dir_all(fixture.paths.backups_dir()).unwrap();
        fs::write(fixture.paths.backups_dir(), b"not a directory").unwrap();
        let executor = Arc::new(UpdateExecutor::new(vec![UpdateResponse::installed(
            "alpha",
        )]));
        let manager = update_manager(&fixture.paths, executor.clone());
        let (_phases, final_event) = run_update_task(&manager, &["alpha"]).await;

        assert_eq!(final_event.phase, TaskPhase::Failed);
        let error = final_event.error.expect("backup error");
        assert_eq!(error.code, crate::error::codes::BACKUP_FAILED);
        assert_eq!(executor.calls().len(), 0);
        let lines = log_lines(&fixture);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["result"], "failed");
        assert_eq!(lines[0]["error"]["code"], "backup.failed");
    }

    #[tokio::test]
    async fn operation_log_failure_never_blocks_the_update() {
        let fixture = Fixture::new(&[("alpha", "acme/skills", None)]);
        fs::remove_dir_all(fixture.paths.logs_dir()).unwrap();
        fs::write(fixture.paths.logs_dir(), b"not a directory").unwrap();
        let executor = Arc::new(UpdateExecutor::new(vec![
            UpdateResponse::installed("alpha").with_post(simulate_update(
                fixture.canonical.clone(),
                fixture.qoder.clone(),
                fixture.claude.clone(),
                fixture.paths.lock_path(),
                "alpha",
            )),
        ]));
        let manager = update_manager(&fixture.paths, executor.clone());
        let (_phases, final_event) = run_update_task(&manager, &["alpha"]).await;

        assert_eq!(final_event.phase, TaskPhase::Completed);
        let result = final_event.result.unwrap();
        assert_eq!(result.overall, TaskOverall::Success);
        assert_eq!(result.skills[0].status, TaskSkillStatus::Updated);
        assert!(result.log_path.is_some());
        assert_eq!(executor.calls().len(), 1);
    }

    #[tokio::test]
    async fn cancellation_between_skills_keeps_completed_results() {
        let fixture = Fixture::new(&[
            ("alpha", "acme/skills", None),
            ("beta", "acme/skills", None),
        ]);
        let beta = add_canonical(&fixture, "beta");
        symlink(&beta, fixture.qoder.join("beta")).unwrap();
        let executor = Arc::new(UpdateExecutor::new(vec![
            UpdateResponse::installed("alpha").with_post(simulate_update(
                fixture.canonical.clone(),
                fixture.qoder.clone(),
                fixture.claude.clone(),
                fixture.paths.lock_path(),
                "alpha",
            )),
            UpdateResponse::installed("beta"),
        ]));
        let manager = update_manager(&fixture.paths, executor.clone());
        let (sink, mut receiver) = sink();
        let task_id = manager
            .start_update(vec!["alpha".to_string(), "beta".to_string()], sink)
            .await
            .unwrap();

        let mut final_event = None;
        while let Some(event) = receiver.recv().await {
            if event.phase == TaskPhase::Verifying
                && event.skill.as_deref() == Some("alpha")
                && event.completed == Some(1)
            {
                let ack = manager.cancel_task(&task_id).expect("cancel accepted");
                assert!(ack.accepted);
            }
            if event.phase.is_final() {
                final_event = Some(event);
                break;
            }
        }
        let final_event = final_event.expect("final update event");

        assert_eq!(final_event.phase, TaskPhase::Cancelled);
        let result = final_event.result.unwrap();
        assert_eq!(result.overall, TaskOverall::Cancelled);
        assert_eq!(result.skills.len(), 1);
        assert_eq!(result.skills[0].name, "alpha");
        assert_eq!(result.skills[0].status, TaskSkillStatus::Updated);
        assert_eq!(executor.calls().len(), 1);
    }
}
