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

/// `npx -y skills@1.7.4 remove -g -s <name> -y`./// Resolve an inventory id into an executable uninstall plan. This runs before
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
    // Reuse the shared ticket-08 scan so detach and uninstall can never
    // disagree about which symlinks depend on a copy directory.
    for copy in attachments
        .iter()
        .filter(|attachment| attachment.mode == AttachmentMode::Copy)
    {
        let dependents = match crate::detach::find_copy_dependents(
            agents,
            &paths.canonical_skills_dir(),
            &copy.path,
        ) {
            Ok(dependents) => dependents,
            // An unresolvable copy has nothing to compare against; planning
            // stays permissive and post-uninstall verification still runs.
            Err(_) => continue,
        };
        if !dependents.is_empty() {
            let list = dependents
                .iter()
                .map(|dependent| {
                    format!("{}（{}）", dependent.label, paths.display(&dependent.path))
                })
                .collect::<Vec<_>>();
            return Err(AppError::agent_dependency_conflict(&copy.agent_name, &list));
        }
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
    if let Some(canonical) = &plan.canonical
        && lexists(canonical)
    {
        leftovers.push(format!("canonical：{}", canonical.display()));
        canonical_present = Some(canonical.clone());
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
        Err(AppError::verify_attachment_present(
            &plan.name,
            &attachment.agent_name,
            &attachment.path,
        )
        .detail(format!("仍存在：{detail}")))
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
        format!("{}\n{}", output.stdout_trimmed(), output.stderr_trimmed())
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

/// Combine the CLI result with disk verification. Exit code 0 is never enough:
/// a leftover fails with `verify.*`. A non-zero exit is reported as a CLI
/// failure even when the filesystem happens to look clean.
pub fn evaluate_output(
    output: &ExecOutput,
    verification: Result<(), AppError>,
) -> Result<(), AppError> {
    if !output.success() {
        return Err(classify_remove_output(output)
            .unwrap_or_else(|| AppError::cli_failed("skills remove 执行失败。")));
    }
    verification?;
    match classify_remove_output(output) {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

pub fn summary_message(overall: super::manager::TaskOverall, name: &str) -> String {
    use super::manager::TaskOverall;
    match overall {
        TaskOverall::Success => format!("已彻底卸载「{name}」。"),
        TaskOverall::Partial => format!("卸载部分完成：「{name}」仍有残留。"),
        TaskOverall::Failed => format!("卸载未完成：「{name}」仍保留部分文件或 lock 条目。"),
        TaskOverall::Cancelled => "卸载已取消。".to_string(),
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

    pub(super) fn write_lock(paths: &AppPaths, names: &[&str]) {
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

    pub(super) fn remove_path(path: &Path) {
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

    pub(super) fn remove_lock_key(paths: &AppPaths, name: &str) {
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
            vec!["-y", "skills@1.7.4", "remove", "-g", "-s", "alpha", "-y"]
        );
        assert_eq!(spec.cwd, PathBuf::from("/app-data/cli-cwd"));
        assert_eq!(spec.env["CI"], "true");
        assert_eq!(spec.timeout, Duration::from_secs(900));
        for forbidden in ["--all", "--copy", "-a", "*", "update", "check", "--json", "list"] {
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
        assert!(
            fixture
                .home
                .join(".agents/skills/unmanaged/SKILL.md")
                .is_file()
        );
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
        symlink(&fixture.canonical, fixture.qoder.join("alpha-copy")).unwrap();
        let plan = fixture.plan("alpha").unwrap();
        for copy in plan
            .attachments
            .iter()
            .filter(|attachment| attachment.mode == AttachmentMode::Copy)
        {
            let dependents = crate::detach::find_copy_dependents(
                &fixture.agents,
                &fixture.paths.canonical_skills_dir(),
                &copy.path,
            )
            .unwrap();
            assert!(dependents.is_empty(), "{dependents:?}");
        }
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

        let error = verify_uninstall(&fixture.paths, &plan).result.unwrap_err();
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

        let error = verify_uninstall(&fixture.paths, &plan).result.unwrap_err();
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

        let error = verify_uninstall(&fixture.paths, &plan).result.unwrap_err();
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

        let error = verify_uninstall(&fixture.paths, &plan).result.unwrap_err();
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
        let error = verify_uninstall(&fixture.paths, &plan).result.unwrap_err();
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

#[cfg(test)]
mod execution_tests {
    use super::tests::{remove_lock_key, remove_path, write_lock};
    use super::*;
    use crate::backup::BACKUP_MANIFEST_FILE;
    use crate::baseline::BaselineStore;
    use crate::cli::CliRuntimeProvider;
    use crate::error::codes;
    use crate::process::{CancelToken, ExecError, ProcessExecutor};
    use crate::tasks::conflicts::{ConflictInfo, ConflictProvider};
    use crate::tasks::manager::{
        EventSink, TaskEvent, TaskManager, TaskManagerConfig, TaskOverall, TaskPhase,
        TaskSkillStatus,
    };
    use std::collections::HashMap;
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

    const SUCCESS: &str = include_str!("../../tests/fixtures/cli/remove-success.txt");

    struct ExecRuntime;

    #[async_trait::async_trait]
    impl CliRuntimeProvider for ExecRuntime {
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
                pinned_version: crate::cli::PINNED_CLI_VERSION.to_string(),
            })
        }
    }

    struct HangingRuntime;

    #[async_trait::async_trait]
    impl CliRuntimeProvider for HangingRuntime {
        async fn cli_runtime(&self) -> Result<CliRuntime, AppError> {
            std::future::pending().await
        }
    }

    #[derive(Default)]
    struct NoConflicts;

    impl ConflictProvider for NoConflicts {
        fn lookup(&self, _names: &[String]) -> HashMap<String, ConflictInfo> {
            HashMap::new()
        }
    }

    struct RemoveResponse {
        output: ExecOutput,
        delay: Duration,
        post: Option<Box<dyn FnOnce() + Send>>,
    }

    impl RemoveResponse {
        fn success(stdout: &str, post: impl FnOnce() + Send + 'static) -> Self {
            Self {
                output: ExecOutput {
                    code: Some(0),
                    signal: None,
                    stdout: stdout.to_string(),
                    stderr: String::new(),
                    duration: Duration::ZERO,
                },
                delay: Duration::ZERO,
                post: Some(Box::new(post)),
            }
        }

        fn output(code: i32, stdout: &str, stderr: &str) -> Self {
            Self {
                output: ExecOutput {
                    code: Some(code),
                    signal: None,
                    stdout: stdout.to_string(),
                    stderr: stderr.to_string(),
                    duration: Duration::ZERO,
                },
                delay: Duration::ZERO,
                post: None,
            }
        }

        fn delayed(mut self, delay: Duration) -> Self {
            self.delay = delay;
            self
        }
    }

    #[derive(Default)]
    struct RemoveExecutor {
        responses: Mutex<Vec<RemoveResponse>>,
        calls: Mutex<Vec<CommandSpec>>,
    }

    impl RemoveExecutor {
        fn push(&self, response: RemoveResponse) {
            self.responses.lock().unwrap().push(response);
        }

        fn calls(&self) -> Vec<CommandSpec> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl ProcessExecutor for RemoveExecutor {
        async fn run(
            &self,
            spec: CommandSpec,
            _cancel: CancelToken,
        ) -> Result<ExecOutput, ExecError> {
            self.calls.lock().unwrap().push(spec);
            let response = self
                .responses
                .lock()
                .unwrap()
                .pop()
                .expect("one scripted remove response");
            if !response.delay.is_zero() {
                tokio::time::sleep(response.delay).await;
            }
            if let Some(post) = response.post {
                post();
            }
            Ok(response.output)
        }
    }

    struct ExecFixture {
        _tmp: tempfile::TempDir,
        home: PathBuf,
        paths: AppPaths,
        canonical: PathBuf,
        qoder: PathBuf,
        claude: PathBuf,
        gemini: PathBuf,
        executor: Arc<RemoveExecutor>,
        manager: TaskManager,
    }

    impl ExecFixture {
        fn new() -> Self {
            Self::with_runtime(Arc::new(ExecRuntime))
        }

        fn with_runtime(runtime: Arc<dyn CliRuntimeProvider>) -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path().join("home");
            let paths = AppPaths::new(&home, tmp.path().join("app-data"));
            paths.ensure_dirs().unwrap();
            for relative in [".qoder/skills", ".claude/skills", ".gemini/skills"] {
                fs::create_dir_all(home.join(relative)).unwrap();
            }
            let canonical_root = paths.canonical_skills_dir();
            fs::create_dir_all(&canonical_root).unwrap();

            let canonical = canonical_root.join("alpha");
            fs::create_dir_all(&canonical).unwrap();
            fs::write(canonical.join("SKILL.md"), "# alpha\n").unwrap();
            symlink(&canonical, home.join(".qoder/skills/alpha")).unwrap();
            fs::create_dir_all(home.join(".claude/skills/alpha")).unwrap();
            fs::write(home.join(".claude/skills/alpha/SKILL.md"), "# alpha copy\n").unwrap();
            symlink(&canonical, home.join(".gemini/skills/alpha")).unwrap();

            fs::create_dir_all(home.join(".claude/skills/copy-only")).unwrap();
            fs::write(
                home.join(".claude/skills/copy-only/SKILL.md"),
                "# copy-only\n",
            )
            .unwrap();

            let unmanaged = canonical_root.join("unmanaged");
            fs::create_dir_all(&unmanaged).unwrap();
            fs::write(unmanaged.join("SKILL.md"), "# unmanaged\n").unwrap();

            let other = canonical_root.join("other");
            fs::create_dir_all(&other).unwrap();
            fs::write(other.join("SKILL.md"), "# other\n").unwrap();
            symlink(&other, home.join(".qoder/skills/other")).unwrap();

            write_lock(&paths, &["alpha", "copy-only", "other"]);
            BaselineStore::new(&paths)
                .record_from_dir("alpha", &canonical, true)
                .unwrap();

            let executor = Arc::new(RemoveExecutor::default());
            let manager = TaskManager::new(
                runtime,
                executor.clone(),
                Arc::new(NoConflicts),
                paths.clone(),
                TaskManagerConfig {
                    preview_timeout: Duration::from_secs(60),
                    install_timeout: Duration::from_secs(5),
                },
            );
            let qoder = home.join(".qoder/skills");
            let claude = home.join(".claude/skills");
            let gemini = home.join(".gemini/skills");
            Self {
                _tmp: tmp,
                home,
                paths,
                canonical,
                qoder,
                claude,
                gemini,
                executor,
                manager,
            }
        }

        fn backups(&self) -> Vec<PathBuf> {
            fs::read_dir(self.paths.backups_dir())
                .map(|entries| {
                    entries
                        .filter_map(Result::ok)
                        .map(|entry| entry.path())
                        .collect()
                })
                .unwrap_or_default()
        }

        fn log_lines(&self) -> Vec<serde_json::Value> {
            fs::read_to_string(self.paths.logs_dir().join("operations.jsonl"))
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        }
    }

    fn sink() -> (EventSink, UnboundedReceiver<TaskEvent>) {
        let (sender, receiver) = unbounded_channel();
        let sink: EventSink = Arc::new(move |event| {
            let _ = sender.send(event);
        });
        (sink, receiver)
    }

    async fn run_task(manager: &TaskManager, name: &str) -> (Vec<TaskPhase>, TaskEvent) {
        let (sink, mut receiver) = sink();
        manager
            .start_uninstall(name.to_string(), sink)
            .await
            .unwrap();
        let mut phases = Vec::new();
        let final_event = loop {
            let event = receiver.recv().await.expect("uninstall event");
            phases.push(event.phase);
            if event.phase.is_final() {
                break event;
            }
        };
        (phases, final_event)
    }

    fn remove_agent_paths(home: &Path, name: &str) {
        for relative in [".qoder/skills", ".claude/skills", ".gemini/skills"] {
            remove_path(&home.join(relative).join(name));
        }
    }

    fn remove_all_post(
        home: PathBuf,
        paths: AppPaths,
        name: &'static str,
    ) -> impl FnOnce() + Send + 'static {
        move || {
            remove_agent_paths(&home, name);
            remove_path(&paths.canonical_skills_dir().join(name));
            remove_lock_key(&paths, name);
        }
    }

    #[tokio::test]
    async fn uninstall_success_backs_up_removes_verifies_and_logs() {
        let fixture = ExecFixture::new();
        fixture.executor.push(RemoveResponse::success(
            SUCCESS,
            remove_all_post(fixture.home.clone(), fixture.paths.clone(), "alpha"),
        ));

        let (phases, final_event) = run_task(&fixture.manager, "alpha").await;
        assert_eq!(final_event.phase, TaskPhase::Completed);
        assert_eq!(phases.first(), Some(&TaskPhase::Preparing));
        assert!(phases.contains(&TaskPhase::Backup), "{phases:?}");
        assert!(phases.contains(&TaskPhase::RunningCli), "{phases:?}");
        assert!(phases.contains(&TaskPhase::Verifying), "{phases:?}");
        assert_eq!(final_event.completed, Some(1));
        assert_eq!(final_event.total, Some(1));

        let result = final_event.result.expect("uninstall result");
        assert_eq!(result.overall, TaskOverall::Success);
        assert_eq!(result.skills.len(), 1);
        assert_eq!(result.skills[0].name, "alpha");
        assert_eq!(result.skills[0].status, TaskSkillStatus::Removed);
        assert!(result.skills[0].error.is_none());
        let backup = result.skills[0].backup_path.clone().expect("skill backup");
        assert_eq!(result.backup_path.as_deref(), Some(backup.as_str()));

        let backup_dir = PathBuf::from(&backup);
        let directory_name = backup_dir.file_name().unwrap().to_string_lossy();
        let (stamp, skill) = directory_name
            .split_once("-uninstall-")
            .expect("timestamp-uninstall-skill layout");
        assert!(stamp.chars().all(|character| character.is_ascii_digit()));
        assert_eq!(skill, "alpha");
        assert!(backup_dir.join("skill-lock.json").is_file());
        assert_eq!(
            fs::read_to_string(backup_dir.join("canonical/SKILL.md")).unwrap(),
            "# alpha\n"
        );
        assert_eq!(
            fs::read_to_string(backup_dir.join("attachments/claude-code/SKILL.md")).unwrap(),
            "# alpha copy\n"
        );
        let manifest: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(backup_dir.join(BACKUP_MANIFEST_FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["action"], "uninstall");
        assert_eq!(manifest["skill"], "alpha");
        assert_eq!(manifest["canonicalPresent"], true);
        assert_eq!(
            manifest["agents"],
            serde_json::json!(["claude-code", "gemini-cli", "qoder", "universal"])
        );

        // Disk truth: everything for alpha is gone, nothing else is touched.
        assert!(!fixture.canonical.exists());
        assert!(!fixture.qoder.join("alpha").exists());
        assert!(!fixture.claude.join("alpha").exists());
        assert!(!fixture.gemini.join("alpha").exists());
        let lock = LockFile::read(&fixture.paths.lock_path()).unwrap();
        assert!(!lock.contains("alpha"));
        assert!(lock.contains("copy-only"));
        assert!(lock.contains("other"));
        assert!(fixture.claude.join("copy-only").is_dir());
        assert!(
            fixture
                .home
                .join(".agents/skills/unmanaged/SKILL.md")
                .is_file()
        );
        assert!(fixture.canonical.with_file_name("other").is_dir());
        assert!(fixture.qoder.join("other").is_symlink());
        assert!(
            BaselineStore::new(&fixture.paths).load("alpha").is_none(),
            "baseline is part of the delete scope"
        );

        let log_path = result.log_path.expect("log path");
        assert!(PathBuf::from(&log_path).is_file());
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        let lines = fixture.log_lines();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["action"], "uninstall");
        assert_eq!(lines[0]["skill"], "alpha");
        assert_eq!(
            lines[0]["agents"],
            serde_json::json!(["claude-code", "gemini-cli", "qoder", "universal"])
        );
        assert_eq!(lines[0]["result"], "removed");
        assert_eq!(lines[0]["backupPath"], backup);
        assert!(lines[0].get("error").is_none());
        assert!(lines[0]["time"].as_str().unwrap().ends_with('Z'));

        let calls = fixture.executor.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].program, PathBuf::from("/node/bin/npx"));
        assert_eq!(
            calls[0].args,
            vec!["-y", "skills@1.7.4", "remove", "-g", "-s", "alpha", "-y"]
        );
        for forbidden in ["--all", "--copy", "-a", "*", "update", "check"] {
            assert!(
                !calls[0].args.iter().any(|arg| arg == forbidden),
                "{forbidden} must never be spawned by uninstall"
            );
        }
    }

    #[tokio::test]
    async fn copy_only_skills_uninstall_and_back_up_their_copies() {
        let fixture = ExecFixture::new();
        fixture.executor.push(RemoveResponse::success(
            SUCCESS,
            remove_all_post(fixture.home.clone(), fixture.paths.clone(), "copy-only"),
        ));

        let (_phases, final_event) = run_task(&fixture.manager, "copy-only").await;
        assert_eq!(final_event.phase, TaskPhase::Completed);
        let result = final_event.result.unwrap();
        assert_eq!(result.overall, TaskOverall::Success);
        assert_eq!(result.skills[0].status, TaskSkillStatus::Removed);
        assert!(!fixture.claude.join("copy-only").exists());
        let backup = PathBuf::from(result.backup_path.unwrap());
        assert!(!backup.join("canonical").exists());
        assert_eq!(
            fs::read_to_string(backup.join("attachments/claude-code/SKILL.md")).unwrap(),
            "# copy-only\n"
        );
        let manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(backup.join(BACKUP_MANIFEST_FILE)).unwrap())
                .unwrap();
        assert_eq!(manifest["action"], "uninstall");
        assert_eq!(manifest["canonicalPresent"], false);
        assert_eq!(manifest["agents"], serde_json::json!(["claude-code"]));
        let lines = fixture.log_lines();
        assert_eq!(lines[0]["agents"], serde_json::json!(["claude-code"]));
        assert_eq!(lines[0]["result"], "removed");
        assert!(
            !LockFile::read(&fixture.paths.lock_path())
                .unwrap()
                .contains("copy-only")
        );
    }

    #[tokio::test]
    async fn partial_removal_is_a_verify_failure_not_success() {
        let fixture = ExecFixture::new();
        let home = fixture.home.clone();
        let paths = fixture.paths.clone();
        let canonical = fixture.canonical.clone();
        fixture
            .executor
            .push(RemoveResponse::success(SUCCESS, move || {
                remove_path(&home.join(".qoder/skills/alpha"));
                remove_path(&home.join(".gemini/skills/alpha"));
                remove_path(&canonical);
                remove_lock_key(&paths, "alpha");
                // Claude's copy is left behind.
            }));

        let (_phases, final_event) = run_task(&fixture.manager, "alpha").await;
        assert_eq!(final_event.phase, TaskPhase::Completed);
        let result = final_event.result.unwrap();
        assert_eq!(result.overall, TaskOverall::Failed);
        assert_eq!(result.skills[0].status, TaskSkillStatus::Failed);
        let error = result.skills[0].error.clone().expect("verify error");
        assert_eq!(error.code, codes::VERIFY_ATTACHMENT_PRESENT);
        assert!(error.message.contains("Claude Code"));
        assert!(result.backup_path.is_some());
        assert!(result.log_path.is_some());
        assert!(
            BaselineStore::new(&fixture.paths).load("alpha").is_some(),
            "baseline is only removed after verified success"
        );
        let lines = fixture.log_lines();
        assert_eq!(lines[0]["result"], "failed");
        assert_eq!(lines[0]["error"]["code"], codes::VERIFY_ATTACHMENT_PRESENT);
    }

    #[tokio::test]
    async fn exit_zero_without_changes_is_a_failure() {
        let fixture = ExecFixture::new();
        fixture
            .executor
            .push(RemoveResponse::success(SUCCESS, || {}));

        let (_phases, final_event) = run_task(&fixture.manager, "alpha").await;
        let result = final_event.result.unwrap();
        assert_eq!(result.overall, TaskOverall::Failed);
        assert_eq!(
            result.skills[0].error.as_ref().unwrap().code,
            codes::VERIFY_CANONICAL_PRESENT
        );
        assert!(fixture.canonical.exists());
        assert_eq!(fixture.executor.calls().len(), 1);
        let lines = fixture.log_lines();
        assert_eq!(lines[0]["result"], "failed");
        assert_eq!(lines[0]["error"]["code"], codes::VERIFY_CANONICAL_PRESENT);
    }

    #[tokio::test]
    async fn a_nonzero_remove_exit_is_a_cli_failure() {
        let fixture = ExecFixture::new();
        fixture
            .executor
            .push(RemoveResponse::output(1, "", "permission denied"));

        let (_phases, final_event) = run_task(&fixture.manager, "alpha").await;
        let result = final_event.result.unwrap();
        assert_eq!(result.overall, TaskOverall::Failed);
        let error = result.skills[0].error.as_ref().unwrap();
        assert_eq!(error.code, codes::CLI_FAILED);
        assert!(
            error
                .detail
                .as_deref()
                .unwrap()
                .contains("permission denied")
        );
        assert!(fixture.canonical.exists());
    }

    #[tokio::test]
    async fn a_surviving_lock_key_is_a_verify_failure() {
        let fixture = ExecFixture::new();
        let home = fixture.home.clone();
        let canonical = fixture.canonical.clone();
        fixture
            .executor
            .push(RemoveResponse::success(SUCCESS, move || {
                remove_agent_paths(&home, "alpha");
                remove_path(&canonical);
                // The lock key is deliberately left behind.
            }));

        let (_phases, final_event) = run_task(&fixture.manager, "alpha").await;
        let result = final_event.result.unwrap();
        assert_eq!(result.overall, TaskOverall::Failed);
        assert_eq!(
            result.skills[0].error.as_ref().unwrap().code,
            codes::VERIFY_LOCK_PRESENT
        );
        let lines = fixture.log_lines();
        assert_eq!(lines[0]["error"]["code"], codes::VERIFY_LOCK_PRESENT);
    }

    #[tokio::test]
    async fn backup_failure_aborts_before_the_cli_runs() {
        let fixture = ExecFixture::new();
        fs::remove_dir_all(fixture.paths.backups_dir()).unwrap();
        fs::write(fixture.paths.backups_dir(), b"not a directory").unwrap();
        fixture
            .executor
            .push(RemoveResponse::output(0, SUCCESS, ""));

        let (_phases, final_event) = run_task(&fixture.manager, "alpha").await;
        assert_eq!(final_event.phase, TaskPhase::Failed);
        let error = final_event.error.expect("backup error");
        assert_eq!(error.code, codes::BACKUP_FAILED);
        assert_eq!(fixture.executor.calls().len(), 0);
        assert!(fixture.canonical.is_dir());
        let lines = fixture.log_lines();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["result"], "failed");
        assert_eq!(lines[0]["error"]["code"], codes::BACKUP_FAILED);
        assert!(lines[0].get("backupPath").is_none());
    }

    #[tokio::test]
    async fn unmanaged_skills_are_rejected_before_a_task_starts() {
        let fixture = ExecFixture::new();
        let (sink, mut receiver) = sink();
        let error = fixture
            .manager
            .start_uninstall("unmanaged".to_string(), sink)
            .await
            .unwrap_err();
        assert_eq!(error.code, codes::SKILL_UNMANAGED);
        assert_eq!(fixture.executor.calls().len(), 0);
        assert!(fixture.backups().is_empty());
        assert!(receiver.try_recv().is_err(), "no task should be created");
    }

    #[tokio::test]
    async fn cross_agent_copy_dependencies_block_before_a_task_starts() {
        let fixture = ExecFixture::new();
        fs::remove_file(fixture.gemini.join("alpha")).unwrap();
        symlink(fixture.claude.join("alpha"), fixture.gemini.join("alpha")).unwrap();

        let (sink, mut receiver) = sink();
        let error = fixture
            .manager
            .start_uninstall("alpha".to_string(), sink)
            .await
            .unwrap_err();
        assert_eq!(error.code, codes::AGENT_DEPENDENCY_CONFLICT);
        assert_eq!(fixture.executor.calls().len(), 0);
        assert!(fixture.backups().is_empty());
        assert!(fixture.canonical.is_dir());
        assert!(fixture.claude.join("alpha").is_dir());
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn uninstall_is_cancellable_before_the_cli_spawns() {
        let fixture = ExecFixture::with_runtime(Arc::new(HangingRuntime));
        let lock_before = fs::read(fixture.paths.lock_path()).unwrap();
        let (sink, mut receiver) = sink();
        let task_id = fixture
            .manager
            .start_uninstall("alpha".to_string(), sink)
            .await
            .unwrap();
        let event = receiver.recv().await.unwrap();
        assert_eq!(event.phase, TaskPhase::Preparing);

        let ack = fixture.manager.cancel_task(&task_id).unwrap();
        assert!(ack.accepted);
        let final_event = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let event = receiver.recv().await.unwrap();
                if event.phase.is_final() {
                    return event;
                }
            }
        })
        .await
        .expect("cancel should finish the task");
        assert_eq!(final_event.phase, TaskPhase::Cancelled);
        assert_eq!(fixture.executor.calls().len(), 0);
        assert!(fixture.canonical.is_dir());
        assert_eq!(
            fs::read(fixture.paths.lock_path()).unwrap(),
            lock_before,
            "cancelling before spawn must not touch the lock"
        );
        assert!(
            !fixture.paths.interrupted_marker_path().exists(),
            "cancel clears the interruption marker"
        );
    }

    #[tokio::test]
    async fn uninstall_cannot_be_cancelled_after_the_cli_spawns() {
        let fixture = ExecFixture::new();
        let home = fixture.home.clone();
        let paths = fixture.paths.clone();
        fixture.executor.push(
            RemoveResponse::success(SUCCESS, move || {
                remove_agent_paths(&home, "alpha");
                remove_path(&paths.canonical_skills_dir().join("alpha"));
                remove_lock_key(&paths, "alpha");
            })
            .delayed(Duration::from_millis(150)),
        );

        let (sink, mut receiver) = sink();
        let task_id = fixture
            .manager
            .start_uninstall("alpha".to_string(), sink)
            .await
            .unwrap();
        loop {
            let event = receiver.recv().await.unwrap();
            if event.phase == TaskPhase::RunningCli {
                break;
            }
        }
        let error = fixture.manager.cancel_task(&task_id).unwrap_err();
        assert_eq!(error.code, codes::TASK_CANNOT_CANCEL);

        let final_event = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let event = receiver.recv().await.unwrap();
                if event.phase.is_final() {
                    return event;
                }
            }
        })
        .await
        .expect("uninstall should finish");
        assert_eq!(final_event.phase, TaskPhase::Completed);
        assert_eq!(
            final_event.result.unwrap().skills[0].status,
            TaskSkillStatus::Removed
        );
    }
}
