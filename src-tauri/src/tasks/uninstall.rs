//! Uninstall planning and verification.
//!
//! Uninstall is the only destructive mutation. It is limited to managed and
//! copy-only skills (a lock entry exists), backs up the lock, canonical folder,
//! copy attachments and attachment manifest before touching the disk, and is
//! executed by the pinned CLI as `skills remove -g -s <name> -y` — never with
//! `-a` (per-agent remove has environment-dependent semantics) and never with
//! `--all`.
//!
//! A copy attachment shared with another agent through a symlink would leave
//! dangling links behind, so planning blocks with `agent.dependency_conflict`
//! instead of rewriting other agents' attachments.
//!
//! The CLI exits 0 even when it removes nothing or only partially succeeds.
//! After it exits, the canonical folder, every physical attachment path and
//! the lock key are verified individually; a leftover is a `verify.*` failure,
//! never a success.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use crate::backup::BackupAttachment;
use crate::cli::CliRuntime;
use crate::error::AppError;
use crate::inventory::{self, AgentDir, AttachmentLocation, AttachmentMode, ManagementState};
use crate::lock::LockFile;
use crate::paths::AppPaths;
use crate::process::{CommandSpec, ExecOutput};
use crate::security;

use super::install::{INSTALL_TIMEOUT, UNIVERSAL_AGENT_ID};

pub const UNINSTALL_TIMEOUT: Duration = INSTALL_TIMEOUT;

#[derive(Debug, Clone)]
pub struct UninstallPlan {
    pub name: String,
    pub management: ManagementState,
    /// Shared canonical folder; `None` for copy-only skills.
    pub canonical: Option<PathBuf>,
    /// Every physical attachment, sorted by agent id and path.
    pub attachments: Vec<AttachmentLocation>,
    /// Agent ids reported in the backup manifest and JSONL log. Native/shared
    /// agents are represented by `universal`; copy-only plans have no
    /// `universal` entry because no canonical folder exists.
    pub agent_ids: Vec<String>,
}

impl UninstallPlan {
    pub fn backup_attachments(&self) -> Vec<BackupAttachment> {
        self.attachments
            .iter()
            .map(|attachment| BackupAttachment {
                agent_id: attachment.agent_id.clone(),
                agent_name: attachment.agent_name.clone(),
                path: attachment.path.clone(),
                mode: attachment.mode,
            })
            .collect()
    }

    pub fn total(&self) -> u32 {
        1
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyDependents {
    /// Display name of the agent whose copy would be deleted.
    pub copy_agent: String,
    /// `"Agent · path"` entries of symlinks resolving into the copy.
    pub dependents: Vec<String>,
}

/// Resolve an inventory id into an executable uninstall plan. This runs before
/// a task is registered so unmanaged skills and dependency conflicts reject
/// synchronously without spawning anything.
pub fn plan_uninstall(
    paths: &AppPaths,
    agents: &[AgentDir],
    skill_id: &str,
) -> Result<UninstallPlan, AppError> {
    security::validate_skill_id(skill_id)?;
    let Some(located) = inventory::locate_with_agents(paths, agents, skill_id)? else {
        return Err(AppError::skill_not_found(skill_id));
    };
    if located.management == ManagementState::Unmanaged {
        return Err(AppError::skill_unmanaged(skill_id));
    }

    let mut attachments = located.attachments;
    attachments.sort_by(|left, right| {
        left.agent_id
            .cmp(&right.agent_id)
            .then_with(|| left.path.cmp(&right.path))
    });
    if let Some(conflict) = find_copy_dependents(agents, &attachments)
        .into_iter()
        .next()
    {
        return Err(AppError::agent_dependency_conflict(
            &conflict.copy_agent,
            &conflict.dependents,
        ));
    }

    let canonical = located.canonical_path;
    let mut agent_ids: BTreeSet<String> = attachments
        .iter()
        .filter(|attachment| attachment.mode != AttachmentMode::Native)
        .map(|attachment| attachment.agent_id.clone())
        .collect();
    if canonical.is_some() {
        agent_ids.insert(UNIVERSAL_AGENT_ID.to_string());
    }

    Ok(UninstallPlan {
        name: located.name,
        management: located.management,
        canonical,
        attachments,
        agent_ids: agent_ids.into_iter().collect(),
    })
}

/// Find symlinks in any detected agent directory that resolve into a copy
/// attachment. Deleting such a copy would leave another agent dangling.
pub fn find_copy_dependents(
    agents: &[AgentDir],
    attachments: &[AttachmentLocation],
) -> Vec<CopyDependents> {
    let mut conflicts = Vec::new();
    for copy in attachments
        .iter()
        .filter(|attachment| attachment.mode == AttachmentMode::Copy)
    {
        let Ok(copy_target) = fs::canonicalize(&copy.path) else {
            continue;
        };
        let mut dependents: BTreeSet<String> = BTreeSet::new();
        for agent in agents {
            let entries = match fs::read_dir(&agent.skills_dir) {
                Ok(entries) => entries,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path == copy.path {
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
                if resolved == copy_target || resolved.starts_with(&copy_target) {
                    dependents.insert(format!("{} · {}", agent.display_name, path.display()));
                }
            }
        }
        if !dependents.is_empty() {
            conflicts.push(CopyDependents {
                copy_agent: copy.agent_name.clone(),
                dependents: dependents.into_iter().collect(),
            });
        }
    }
    conflicts
}

/// `npx -y skills@1.7.4 remove -g -s <name> -y`.
///
/// No `-a` (per-agent remove may delete the canonical store and lock) and no
/// `--all`. `remove` has no JSON output; the result is verified on disk.
pub fn uninstall_command(runtime: &CliRuntime, name: &str, timeout: Duration) -> CommandSpec {
    CommandSpec::new(
        &runtime.npx_path,
        vec![
            "-y".to_string(),
            format!("skills@{}", runtime.pinned_version),
            "remove".to_string(),
            "-g".to_string(),
            "-s".to_string(),
            name.to_string(),
            "-y".to_string(),
        ],
        &runtime.cwd,
        runtime.env.clone(),
        timeout,
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallVerification {
    /// `Ok(())` only when the canonical folder, every attachment path and the
    /// lock key are gone.
    pub result: Result<(), AppError>,
    pub warnings: Vec<AppError>,
}

/// Verify the filesystem and lock after `skills remove` exits. The CLI has no
/// JSON output and exits 0 on partial or empty removals, so this is the only
/// proof of success.
pub fn verify_uninstall(paths: &AppPaths, plan: &UninstallPlan) -> UninstallVerification {
    let mut leftovers: Vec<String> = Vec::new();
    let mut canonical_present = None;
    if let Some(canonical) = &plan.canonical {
        if lexists(canonical) {
            leftovers.push(format!("canonical：{}", canonical.display()));
            canonical_present = Some(canonical.clone());
        }
    }

    let mut attachments_present = Vec::new();
    let mut seen_paths: BTreeSet<PathBuf> = BTreeSet::new();
    for attachment in &plan.attachments {
        if !seen_paths.insert(attachment.path.clone()) {
            continue;
        }
        if lexists(&attachment.path) {
            leftovers.push(format!(
                "{}：{}",
                attachment.agent_name,
                attachment.path.display()
            ));
            attachments_present.push(attachment.clone());
        }
    }

    let lock = match LockFile::read(&paths.lock_path()) {
        Ok(lock) => lock,
        Err(error) => {
            return UninstallVerification {
                result: Err(AppError::verify_failed(format!(
                    "无法确认 lock 是否已清理：{}",
                    error.message
                ))
                .detail(error.detail.unwrap_or_default())),
                warnings: Vec::new(),
            };
        }
    };
    let lock_present = lock.contains(&plan.name);
    if lock_present {
        leftovers.push(format!("lock key：{}", plan.name));
    }

    let detail = leftovers.join("；");
    let result = if let Some(canonical) = canonical_present {
        Err(AppError::verify_canonical_present(&plan.name, &canonical)
            .detail(format!("仍存在：{detail}")))
    } else if let Some(attachment) = attachments_present.first() {
        Err(
            AppError::verify_attachment_present(&plan.name, &attachment.agent_name, &attachment.path)
                .detail(format!("仍存在：{detail}")),
        )
    } else if lock_present {
        Err(AppError::verify_lock_present(&plan.name).detail(detail))
    } else {
        Ok(())
    };

    UninstallVerification {
        result,
        warnings: Vec::new(),
    }
}

/// Build the error for a CLI run that did not succeed or explicitly reported
/// failures. `remove` writes human text only and sets exit 0 even when it
/// removed nothing, so this complements — never replaces — disk verification.
pub fn classify_remove_output(output: &ExecOutput) -> Option<AppError> {
    let combined = if output.stderr_trimmed().is_empty() {
        output.stdout_trimmed().to_string()
    } else if output.stdout_trimmed().is_empty() {
        output.stderr_trimmed().to_string()
    } else {
        format!(
            "{}\n{}",
            output.stdout_trimmed(),
            output.stderr_trimmed()
        )
    };
    let lower = combined.to_lowercase();
    let failed = !output.success()
        || lower.contains("failed to remove")
        || lower.contains("could not remove");
    if !failed {
        return None;
    }
    let detail = super::parser::truncate_chars(&combined, 16 * 1024);
    if detail.is_empty() {
        Some(AppError::cli_failed("skills remove 未返回任何输出。"))
    } else {
        Some(AppError::cli_failed(detail))
    }
}

fn lexists(path: &std::path::Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::codes;
    use std::os::unix::fs::symlink;
    use std::path::Path;

    struct Fixture {
        _tmp: tempfile::TempDir,
        home: PathBuf,
        paths: AppPaths,
        agents: Vec<AgentDir>,
        canonical: PathBuf,
        qoder: PathBuf,
        claude: PathBuf,
        gemini: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path().join("home");
            let paths = AppPaths::new(&home, tmp.path().join("app-data"));
            paths.ensure_dirs().unwrap();
            let canonical_root = paths.canonical_skills_dir();
            fs::create_dir_all(&canonical_root).unwrap();
            let qoder = home.join(".qoder/skills");
            let claude = home.join(".claude/skills");
            let gemini = home.join(".gemini/skills");
            for dir in [&qoder, &claude, &gemini] {
                fs::create_dir_all(dir).unwrap();
            }

            let canonical = canonical_root.join("alpha");
            fs::create_dir_all(&canonical).unwrap();
            fs::write(canonical.join("SKILL.md"), "# alpha\n").unwrap();
            symlink(&canonical, qoder.join("alpha")).unwrap();
            fs::create_dir_all(claude.join("alpha")).unwrap();
            fs::write(claude.join("alpha/SKILL.md"), "# alpha copy\n").unwrap();
            symlink(&canonical, gemini.join("alpha")).unwrap();

            // Copy-only: lock entry + a real copy, no canonical folder.
            fs::create_dir_all(claude.join("copy-only")).unwrap();
            fs::write(claude.join("copy-only/SKILL.md"), "# copy-only\n").unwrap();

            // Unmanaged: a canonical folder without a lock entry.
            let unmanaged = canonical_root.join("unmanaged");
            fs::create_dir_all(&unmanaged).unwrap();
            fs::write(unmanaged.join("SKILL.md"), "# unmanaged\n").unwrap();

            write_lock(&paths, &["alpha", "copy-only"]);

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
                    skills_dir: canonical_root,
                },
            ];
            Self {
                _tmp: tmp,
                home,
                paths,
                agents,
                canonical,
                qoder,
                claude,
                gemini,
            }
        }

        fn plan(&self, name: &str) -> Result<UninstallPlan, AppError> {
            plan_uninstall(&self.paths, &self.agents, name)
        }
    }

    fn write_lock(paths: &AppPaths, names: &[&str]) {
        let mut skills = serde_json::Map::new();
        for name in names {
            skills.insert(
                (*name).to_string(),
                serde_json::json!({
                    "source": "acme/skills",
                    "sourceType": "github",
                    "sourceUrl": "https://github.com/acme/skills.git",
                    "skillPath": format!("skills/{name}/SKILL.md"),
                    "skillFolderHash": "0123456789abcdef0123456789abcdef01234567",
                    "installedAt": "2026-01-01T00:00:00.000Z",
                    "updatedAt": "2026-01-01T00:00:00.000Z"
                }),
            );
        }
        fs::create_dir_all(paths.agents_root()).unwrap();
        fs::write(
            paths.lock_path(),
            serde_json::json!({ "version": 3, "skills": skills }).to_string(),
        )
        .unwrap();
    }

    fn runtime() -> CliRuntime {
        CliRuntime {
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

    fn remove_path(path: &Path) {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                fs::remove_file(path).unwrap();
            }
            Ok(metadata) if metadata.is_dir() => {
                fs::remove_dir_all(path).unwrap();
            }
            Ok(_) => fs::remove_file(path).unwrap(),
            Err(_) => {}
        }
    }

    fn remove_lock_key(paths: &AppPaths, name: &str) {
        let raw = fs::read_to_string(paths.lock_path()).unwrap();
        let mut lock: serde_json::Value = serde_json::from_str(&raw).unwrap();
        lock["skills"].as_object_mut().unwrap().remove(name);
        fs::write(paths.lock_path(), lock.to_string()).unwrap();
    }

    fn remove_skill(fixture: &Fixture, name: &str) {
        remove_path(&fixture.canonical.with_file_name(name));
        remove_path(&fixture.qoder.join(name));
        remove_path(&fixture.claude.join(name));
        remove_path(&fixture.gemini.join(name));
        remove_lock_key(&fixture.paths, name);
    }

    #[test]
    fn uninstall_command_is_names_only_and_never_uses_forbidden_flags() {
        let spec = uninstall_command(&runtime(), "alpha", Duration::from_secs(900));
        assert_eq!(spec.program, PathBuf::from("/node/bin/npx"));
        assert_eq!(
            spec.args,
            vec![
                "-y",
                "skills@1.7.4",
                "remove",
                "-g",
                "-s",
                "alpha",
                "-y"
            ]
        );
        assert_eq!(spec.cwd, PathBuf::from("/app-data/cli-cwd"));
        assert_eq!(spec.env["CI"], "true");
        assert_eq!(spec.timeout, Duration::from_secs(900));
        for forbidden in ["--all", "--copy", "-a", "*", "update", "check", "--json"] {
            assert!(
                !spec.args.iter().any(|arg| arg == forbidden),
                "{forbidden} must never appear in uninstall argv"
            );
        }
    }

    #[test]
    fn managed_skills_plan_canonical_all_attachments_and_universal() {
        let fixture = Fixture::new();
        let plan = fixture.plan("alpha").unwrap();
        assert_eq!(plan.name, "alpha");
        assert_eq!(plan.management, ManagementState::Managed);
        assert_eq!(plan.canonical.as_deref(), Some(fixture.canonical.as_path()));
        assert_eq!(
            plan.attachments
                .iter()
                .map(|attachment| (attachment.agent_id.as_str(), attachment.mode))
                .collect::<Vec<_>>(),
            vec![
                ("claude-code", AttachmentMode::Copy),
                ("codex", AttachmentMode::Native),
                ("gemini-cli", AttachmentMode::Link),
                ("qoder", AttachmentMode::Link),
            ]
        );
        assert_eq!(
            plan.agent_ids,
            vec![
                "claude-code".to_string(),
                "gemini-cli".to_string(),
                "qoder".to_string(),
                "universal".to_string()
            ],
            "native/shared agents are represented by universal"
        );
        assert_eq!(plan.backup_attachments().len(), 4);
        assert_eq!(plan.total(), 1);

        // Other skills are untouched by planning itself.
        assert!(fixture.home.join(".agents/skills/unmanaged/SKILL.md").is_file());
        assert!(fixture.claude.join("copy-only").is_dir());
    }

    #[test]
    fn copy_only_skills_can_be_uninstalled_without_a_universal_entry() {
        let fixture = Fixture::new();
        let plan = fixture.plan("copy-only").unwrap();
        assert_eq!(plan.management, ManagementState::CopyOnly);
        assert!(plan.canonical.is_none());
        assert_eq!(plan.attachments.len(), 1);
        assert_eq!(plan.attachments[0].agent_id, "claude-code");
        assert_eq!(plan.attachments[0].mode, AttachmentMode::Copy);
        assert_eq!(plan.agent_ids, vec!["claude-code".to_string()]);
    }

    #[test]
    fn unmanaged_skills_are_rejected() {
        let fixture = Fixture::new();
        let error = fixture.plan("unmanaged").unwrap_err();
        assert_eq!(error.code, codes::SKILL_UNMANAGED);
        assert!(error.message.contains("不可更新或卸载"), "{error}");
    }

    #[test]
    fn invalid_and_unknown_skill_ids_are_rejected() {
        let fixture = Fixture::new();
        for id in ["", "..", "alpha/beta", ".hidden"] {
            let error = fixture.plan(id).unwrap_err();
            assert_eq!(error.code, codes::PATH_INVALID, "{id:?}");
        }
        let error = fixture.plan("ghost").unwrap_err();
        assert_eq!(error.code, codes::SKILL_NOT_FOUND);
    }

    #[test]
    fn a_copy_referenced_by_another_agents_symlink_blocks_the_plan() {
        let fixture = Fixture::new();
        // Gemini now points at Claude's copy instead of the canonical folder.
        fs::remove_file(fixture.gemini.join("alpha")).unwrap();
        symlink(fixture.claude.join("alpha"), fixture.gemini.join("alpha")).unwrap();

        let error = fixture.plan("alpha").unwrap_err();
        assert_eq!(error.code, codes::AGENT_DEPENDENCY_CONFLICT);
        assert!(error.message.contains("Claude Code"), "{error:?}");
        let detail = error.detail.expect("dependency detail");
        assert!(detail.contains("Gemini CLI"), "{detail}");
        assert!(detail.contains(".gemini/skills/alpha"), "{detail}");
    }

    #[test]
    fn cross_agent_dependencies_block_copy_only_skills_too() {
        let fixture = Fixture::new();
        symlink(
            fixture.claude.join("copy-only"),
            fixture.qoder.join("copy-only"),
        )
        .unwrap();

        let error = fixture.plan("copy-only").unwrap_err();
        assert_eq!(error.code, codes::AGENT_DEPENDENCY_CONFLICT);
        assert!(error.message.contains("Claude Code"), "{error:?}");
        assert!(error.detail.unwrap().contains("Qoder"));
    }

    #[test]
    fn copy_dependents_ignore_links_to_the_canonical_store_and_sibling_names() {
        let fixture = Fixture::new();
        // Qoder links the canonical folder, not Claude's copy, and a prefix
        // sibling must not be treated as a dependent.
        symlink(
            &fixture.canonical,
            fixture.qoder.join("alpha-copy"),
        )
        .unwrap();
        let plan = fixture.plan("alpha").unwrap();
        assert!(find_copy_dependents(&fixture.agents, &plan.attachments).is_empty());
    }

    #[test]
    fn verification_passes_only_after_canonical_attachments_and_lock_key_are_gone() {
        let fixture = Fixture::new();
        let plan = fixture.plan("alpha").unwrap();
        remove_skill(&fixture, "alpha");

        let verification = verify_uninstall(&fixture.paths, &plan);
        assert_eq!(verification.result, Ok(()));
        assert!(!fixture.canonical.exists());
        assert!(!fixture.qoder.join("alpha").exists());
        assert!(!fixture.claude.join("alpha").exists());
        assert!(!fixture.gemini.join("alpha").exists());
        let lock = LockFile::read(&fixture.paths.lock_path()).unwrap();
        assert!(!lock.contains("alpha"));
        assert!(lock.contains("copy-only"), "unrelated entries survive");
        assert!(fixture.claude.join("copy-only").is_dir());
    }

    #[test]
    fn a_remaining_canonical_folder_fails_verification() {
        let fixture = Fixture::new();
        let plan = fixture.plan("alpha").unwrap();
        remove_path(&fixture.qoder.join("alpha"));
        remove_path(&fixture.claude.join("alpha"));
        remove_path(&fixture.gemini.join("alpha"));
        remove_lock_key(&fixture.paths, "alpha");

        let error = verify_uninstall(&fixture.paths, &plan)
            .result
            .unwrap_err();
        assert_eq!(error.code, codes::VERIFY_CANONICAL_PRESENT);
        assert!(error.message.contains("canonical"));
        assert!(error.detail.unwrap().contains("alpha"));
    }

    #[test]
    fn a_remaining_copy_attachment_fails_verification() {
        let fixture = Fixture::new();
        let plan = fixture.plan("alpha").unwrap();
        remove_path(&fixture.canonical);
        remove_path(&fixture.qoder.join("alpha"));
        remove_path(&fixture.gemini.join("alpha"));
        remove_lock_key(&fixture.paths, "alpha");
        // Claude's copy remains; verification must not claim success.
        assert!(fixture.claude.join("alpha").is_dir());

        let error = verify_uninstall(&fixture.paths, &plan)
            .result
            .unwrap_err();
        assert_eq!(error.code, codes::VERIFY_ATTACHMENT_PRESENT);
        assert!(error.message.contains("Claude Code"));
        assert!(error.detail.unwrap().contains(".claude/skills/alpha"));
    }

    #[test]
    fn broken_symlink_attachments_still_count_as_present() {
        let fixture = Fixture::new();
        let plan = fixture.plan("alpha").unwrap();
        remove_path(&fixture.canonical);
        remove_path(&fixture.claude.join("alpha"));
        remove_path(&fixture.gemini.join("alpha"));
        remove_lock_key(&fixture.paths, "alpha");
        // Qoder's symlink now dangles but still exists on disk.
        assert!(fs::symlink_metadata(fixture.qoder.join("alpha")).is_ok());

        let error = verify_uninstall(&fixture.paths, &plan)
            .result
            .unwrap_err();
        assert_eq!(error.code, codes::VERIFY_ATTACHMENT_PRESENT);
        assert!(error.message.contains("Qoder"));
    }

    #[test]
    fn a_remaining_lock_key_fails_verification() {
        let fixture = Fixture::new();
        let plan = fixture.plan("alpha").unwrap();
        remove_path(&fixture.canonical);
        remove_path(&fixture.qoder.join("alpha"));
        remove_path(&fixture.claude.join("alpha"));
        remove_path(&fixture.gemini.join("alpha"));

        let error = verify_uninstall(&fixture.paths, &plan)
            .result
            .unwrap_err();
        assert_eq!(error.code, codes::VERIFY_LOCK_PRESENT);
        assert!(error.message.contains("lock"));
    }

    #[test]
    fn a_missing_lock_after_removal_is_success_and_a_corrupt_one_is_not() {
        let fixture = Fixture::new();
        let plan = fixture.plan("alpha").unwrap();
        remove_path(&fixture.canonical);
        remove_path(&fixture.qoder.join("alpha"));
        remove_path(&fixture.claude.join("alpha"));
        remove_path(&fixture.gemini.join("alpha"));
        fs::remove_file(fixture.paths.lock_path()).unwrap();
        assert_eq!(
            verify_uninstall(&fixture.paths, &plan).result,
            Ok(()),
            "a missing lock means there is no key left"
        );

        fs::write(fixture.paths.lock_path(), "{not json").unwrap();
        let error = verify_uninstall(&fixture.paths, &plan)
            .result
            .unwrap_err();
        assert_eq!(error.code, codes::VERIFY_FAILED);
    }

    #[test]
    fn exit_zero_without_changes_is_not_success() {
        let fixture = Fixture::new();
        let plan = fixture.plan("alpha").unwrap();
        // The CLI exited 0 but nothing was removed.
        let verification = verify_uninstall(&fixture.paths, &plan);
        assert_eq!(
            verification.result.unwrap_err().code,
            codes::VERIFY_CANONICAL_PRESENT
        );
    }

    #[test]
    fn remove_output_classification_handles_success_missing_partial_and_nonzero() {
        const SUCCESS: &str = include_str!("../../tests/fixtures/cli/remove-success.txt");
        const MISSING: &str = include_str!("../../tests/fixtures/cli/remove-missing.txt");
        const PARTIAL: &str = include_str!("../../tests/fixtures/cli/remove-failure-perm.txt");

        let output = |code: i32, stdout: &str, stderr: &str| ExecOutput {
            code: Some(code),
            signal: None,
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            duration: Duration::ZERO,
        };
        assert!(classify_remove_output(&output(0, SUCCESS, "")).is_none());
        assert!(classify_remove_output(&output(0, MISSING, "")).is_none());

        let failure = classify_remove_output(&output(0, PARTIAL, "")).expect("partial failure");
        assert_eq!(failure.code, codes::CLI_FAILED);
        assert!(failure.detail.unwrap().contains("Failed to remove"));

        let nonzero = classify_remove_output(&output(1, "", "boom")).expect("nonzero");
        assert_eq!(nonzero.code, codes::CLI_FAILED);
        assert_eq!(nonzero.detail.as_deref(), Some("boom"));
    }
}
