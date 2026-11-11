//! Uninstall planning and verification.
//!
//! Uninstall backs up the lock, canonical folder, copy attachments and
//! attachment manifest before touching the disk. Managed and copy-only skills
//! use the pinned CLI as `skills remove -g -s <name> -y` — never with
//! `-a` (per-agent remove has environment-dependent semantics) and never with
//! `--all`. Unmanaged skills use native removal of the located paths, because
//! CLI name normalization and scanning cannot safely address arbitrary folders
//! and symlinks without an installation record.
//!
//! Links to a copy are safe when they belong to this Skill's uninstall plan.
//! References outside that plan still block with `agent.dependency_conflict`
//! instead of leaving dangling links or expanding the deletion scope.
//!
//! The CLI exits 0 even when it removes nothing or only partially succeeds.
//! After it exits, the canonical folder, every physical attachment path and
//! the lock key are verified individually; a leftover is a `verify.*` failure,
//! never a success.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use crate::backup::{BackupAction, BackupAttachment, BackupStore};
use crate::baseline::BaselineStore;
use crate::cli::{CliRuntime, ScopedCli};
use crate::error::AppError;
use crate::error::cli::{self, CliAction};
#[cfg(test)]
use crate::inventory::AgentDir;
use crate::inventory::{AttachmentLocation, AttachmentMode, ManagementState};
#[cfg(test)]
use crate::lock::LockFile;
use crate::logs::{OperationEntry, OperationLog};
use crate::paths::AppPaths;
use crate::process::{CommandSpec, ExecOutput};

use super::install::{INSTALL_TIMEOUT, UNIVERSAL_AGENT_ID};
use super::manager::{TaskAction, TaskPhase, TaskSkillResult, TaskSkillStatus};
use super::session::{
    CancellationPolicy, FailureDisposition, MutationAction, Preflight, Reporter, SessionProfile,
    StartOutcome, UnitOutcome,
};

pub const UNINSTALL_TIMEOUT: Duration = INSTALL_TIMEOUT;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallPlan {
    pub name: String,
    pub management: ManagementState,
    pub marketplace: bool,
    marketplace_record: Option<crate::marketplace::records::Installation>,
    workflow_record: Option<crate::expert_packs::records::WorkflowInstallation>,
    /// Shared canonical folder; absent when only Agent attachments exist.
    pub canonical: Option<PathBuf>,
    /// Every physical attachment, sorted by agent id and path.
    pub attachments: Vec<AttachmentLocation>,
    /// Agent ids reported in the backup manifest and JSONL log. Native/shared
    /// agents are represented by `universal`; copy-only plans have no
    /// `universal` entry because no canonical folder exists.
    pub agent_ids: Vec<String>,
    /// Physical parent and raw link destination, in canonical/attachment order.
    /// Native deletion must keep addressing the paths that were backed up.
    native_path_state: Vec<(PathBuf, Option<PathBuf>)>,
}

impl UninstallPlan {
    pub fn backup_attachments(&self) -> Vec<BackupAttachment> {
        self.attachments
            .iter()
            .map(BackupAttachment::from)
            .collect()
    }

    pub fn total(&self) -> u32 {
        1
    }
}

/// Resolve an inventory id into an executable uninstall plan. This runs before
/// a task is registered so invalid paths and dependency conflicts reject
/// synchronously without spawning anything. A lock entry is optional.
///
/// The CLI command itself is built by [`ScopedCli::uninstall`]:
/// `npx -y skills@latest remove [-g] -s <name> -y`.
pub fn plan_uninstall(
    paths: &AppPaths,
    layout: &crate::scope_layout::ScopeLayout,
    skill_id: &str,
) -> Result<UninstallPlan, AppError> {
    let name = layout.scope.parse_skill_id(skill_id)?;
    // Same metacharacter rule as install/update; the CLI is never run through
    // a shell, but a name from a malicious on-disk folder must not reach argv.
    super::install::validate_skill_name(name)?;
    layout.ensure_mutation_ready()?;
    let Some(located) = layout.locate(paths, name)? else {
        return Err(AppError::skill_not_found(skill_id));
    };

    let mut attachments = located.attachments;
    attachments.sort_by(|left, right| {
        left.agent_id
            .cmp(&right.agent_id)
            .then_with(|| left.path.cmp(&right.path))
    });
    let canonical = located.canonical_path;
    let removal_paths: BTreeSet<&PathBuf> = canonical
        .iter()
        .chain(attachments.iter().map(|attachment| &attachment.path))
        .collect();
    // Reuse the shared ticket-08 scan so detach and uninstall can never
    // disagree about which symlinks depend on a copy directory. Uninstall
    // removes all of this Skill's attachments, unlike one-Agent Detach.
    for copy in attachments
        .iter()
        .filter(|attachment| attachment.mode == AttachmentMode::Copy)
    {
        let dependents = match crate::detach::find_copy_dependents(
            &layout.agents,
            &layout.canonical_dir,
            &copy.path,
        ) {
            Ok(dependents) => dependents
                .into_iter()
                .filter(|dependent| !removal_paths.contains(&dependent.path))
                .collect::<Vec<_>>(),
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

    let mut agent_ids: BTreeSet<String> = attachments
        .iter()
        .filter(|attachment| attachment.mode != AttachmentMode::Native)
        .map(|attachment| attachment.agent_id.clone())
        .collect();
    if canonical.is_some() {
        agent_ids.insert(UNIVERSAL_AGENT_ID.to_string());
    }

    let marketplace = located
        .lock_entry
        .as_ref()
        .is_some_and(|e| crate::expert_packs::is_native_source(&e.source_type));
    let mut native_path_state = Vec::new();
    if marketplace || located.management == ManagementState::Unmanaged {
        for path in canonical
            .iter()
            .chain(attachments.iter().map(|attachment| &attachment.path))
        {
            let parent = path
                .parent()
                .ok_or_else(|| AppError::path_invalid("卸载目标缺少父目录。"))?;
            let physical_parent = fs::canonicalize(parent)
                .map_err(|_| AppError::path_invalid("卸载目标的父目录无法读取。"))?;
            let metadata = fs::symlink_metadata(path)
                .map_err(|_| AppError::path_invalid("卸载目标无法读取。"))?;
            let link = if metadata.file_type().is_symlink() {
                Some(
                    fs::read_link(path)
                        .map_err(|_| AppError::path_invalid("卸载目标的符号链接无法读取。"))?,
                )
            } else {
                None
            };
            if let Some(root) = layout.project_root() {
                // Link attachments are only unlinked; their targets are not
                // read or deleted. Canonical/copy content is also backed up.
                let checked = if canonical.as_ref() == Some(path) || link.is_none() {
                    path.as_path()
                } else {
                    parent
                };
                crate::project_path::existing(root, checked)
                    .map_err(|_| AppError::path_invalid("卸载目标超出项目范围或无法读取。"))?;
            }
            native_path_state.push((physical_parent, link));
        }
    }

    Ok(UninstallPlan {
        name: located.name.clone(),
        management: located.management,
        marketplace,
        marketplace_record: crate::marketplace::records::Records::read(&layout.lock_path)?
            .skills
            .get(&located.name)
            .cloned(),
        workflow_record: crate::expert_packs::records::Records::read(&layout.lock_path)?
            .workflows
            .get(&located.name)
            .cloned(),
        canonical,
        attachments,
        agent_ids: agent_ids.into_iter().collect(),
        native_path_state,
    })
}

/// The pinned CLI cannot see extra agents, so their links and copies survive
/// `skills remove`. Remove them natively before verification; failures are
/// warnings here and become a `verify.attachment_present` result afterwards.
pub fn remove_extra_attachments(plan: &UninstallPlan, warnings: &mut Vec<AppError>) {
    for attachment in &plan.attachments {
        if crate::agents::is_catalog_agent(&attachment.agent_id) {
            continue;
        }
        match attachment.mode {
            AttachmentMode::Native | AttachmentMode::Broken => continue,
            AttachmentMode::Link | AttachmentMode::Copy => {}
        }
        match fs::symlink_metadata(&attachment.path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => warnings.push(AppError::agent_detach_failed(format!(
                "无法读取 {}：{error}",
                attachment.path.display()
            ))),
            Ok(metadata) if metadata.file_type().is_symlink() => {
                if let Err(error) = fs::remove_file(&attachment.path) {
                    warnings.push(AppError::agent_detach_failed(format!(
                        "无法删除符号链接 {}：{error}",
                        attachment.path.display()
                    )));
                }
            }
            Ok(metadata) if metadata.is_dir() => {
                if let Err(error) = fs::remove_dir_all(&attachment.path) {
                    warnings.push(AppError::agent_detach_failed(format!(
                        "无法删除 copy 目录 {}：{error}",
                        attachment.path.display()
                    )));
                }
            }
            Ok(_) => warnings.push(AppError::agent_detach_failed(format!(
                "目标既不是符号链接也不是目录：{}",
                attachment.path.display()
            ))),
        }
    }
}

/// Verify the filesystem and lock after `skills remove` exits. The CLI has no
/// JSON output and exits 0 on partial or empty removals, so this is the only
/// proof of success.
pub fn verify_uninstall(
    layout: &crate::scope_layout::ScopeLayout,
    plan: &UninstallPlan,
) -> Result<(), AppError> {
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

    let lock_present = match layout.lock() {
        Ok(lock) => lock.contains(&plan.name),
        Err(error) => {
            return Err(AppError::verify_failed(format!(
                "无法确认 lock 是否已清理：{}",
                error.message
            ))
            .detail(error.detail.unwrap_or_default()));
        }
    };
    if lock_present {
        leftovers.push(format!("lock key：{}", plan.name));
    }

    let detail = leftovers.join("；");
    if let Some(canonical) = canonical_present {
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
    }
}

/// Build the error for a CLI run that did not succeed or explicitly reported
/// failures. `remove` writes human text only and sets exit 0 even when it
/// removed nothing, so this complements — never replaces — disk verification.
pub fn classify_remove_output(output: &ExecOutput) -> Option<AppError> {
    cli::classify_remove_failure(output).map(|failure| failure.to_app_error(CliAction::Uninstall))
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
            .unwrap_or_else(|| AppError::cli_exit_nonzero("skills remove 执行失败。")));
    }
    verification?;
    match classify_remove_output(output) {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Uninstall as one session unit: backup in preflight, one `skills remove`,
/// then disk verification, baseline clearing and the operation log.
pub(crate) struct UninstallAction {
    paths: AppPaths,
    layout: crate::scope_layout::ScopeLayout,
    timeout: Duration,
}

impl UninstallAction {
    pub(crate) fn new(
        paths: AppPaths,
        layout: crate::scope_layout::ScopeLayout,
        timeout: Duration,
    ) -> Self {
        Self {
            paths,
            layout,
            timeout,
        }
    }

    fn finish(
        &self,
        plan: &UninstallPlan,
        outcome: Result<(), AppError>,
        mut warnings: Vec<AppError>,
        preflight: &Preflight,
    ) -> Result<UnitOutcome, AppError> {
        let status = if outcome.is_ok() {
            TaskSkillStatus::Removed
        } else {
            TaskSkillStatus::Failed
        };
        let error = outcome.err();
        if status == TaskSkillStatus::Removed
            && let Err(clear_error) =
                BaselineStore::for_scope(&self.paths, &self.layout.scope).clear(&plan.name)
        {
            warnings.push(AppError::baseline_clear_failed(clear_error));
        }

        let backup_display = self.paths.display(&preflight.snapshot);
        let log = OperationLog::for_scope(&self.paths, &self.layout.scope);
        let log_result = if status == TaskSkillStatus::Removed {
            "removed"
        } else {
            "failed"
        };
        let _ = log.append(&OperationEntry::new(
            "uninstall",
            &plan.name,
            plan.agent_ids.clone(),
            log_result,
            Some(backup_display.clone()),
            error.clone(),
        ));
        Ok(UnitOutcome {
            rows: vec![TaskSkillResult {
                name: plan.name.clone(),
                status,
                error,
                backup_path: Some(backup_display.clone()),
            }],
            warnings,
            backup_path: backup_display,
            final_progress: None,
        })
    }
}

impl MutationAction for UninstallAction {
    type Plan = UninstallPlan;
    type Unit = UninstallPlan;

    fn uses_cli(&self, plan: &Self::Plan) -> bool {
        !plan.marketplace && plan.management != ManagementState::Unmanaged
    }

    fn before_write(&self, plan: &Self::Unit) -> Result<(), AppError> {
        let registry = crate::projects::ProjectRegistry::new(self.paths.clone());
        let layout = crate::scope_layout::ScopeLayout::resolve_detected(
            &self.paths,
            &registry,
            &self.layout.scope,
        )?;
        if layout.root != self.layout.root
            || layout
                .project
                .as_ref()
                .map(|project| &project.check_revision)
                != self
                    .layout
                    .project
                    .as_ref()
                    .map(|project| &project.check_revision)
        {
            return Err(AppError::batch_target_changed(
                "项目根目录已变化，请重新确认。",
            ));
        }
        // Dependency protection must also run after backup for CLI plans:
        // an outside link may have started referencing a planned copy.
        let current = plan_uninstall(&self.paths, &layout, &layout.scope.skill_id(&plan.name))?;
        if current != *plan {
            return Err(AppError::batch_target_changed(
                "技能安装记录或卸载范围已变化，请重新确认。",
            ));
        }
        Ok(())
    }

    fn execute_native(
        &self,
        plan: &Self::Unit,
        preflight: &Preflight,
        reporter: &Reporter,
    ) -> Result<UnitOutcome, AppError> {
        reporter.progress(
            TaskPhase::RunningCli,
            format!("正在卸载技能 {}…", plan.name),
            Some(plan.name.as_str()),
        );
        let outcome = remove_unmanaged_paths(plan).and_then(|()| {
            if plan.workflow_record.is_some() {
                crate::expert_packs::records::Records::remove_workflow(
                    &self.layout.lock_path,
                    &plan.name,
                )?;
            } else if plan.marketplace {
                crate::marketplace::records::Records::remove(&self.layout.lock_path, &plan.name)?;
            }
            reporter.progress(
                TaskPhase::Verifying,
                "正在校验技能目录、Agent 挂载与安装记录…",
                Some(plan.name.as_str()),
            );
            verify_uninstall(&self.layout, plan)
        });
        self.finish(plan, outcome, Vec::new(), preflight)
    }

    fn profile(&self, plan: &Self::Plan) -> SessionProfile {
        SessionProfile {
            action: TaskAction::Uninstall,
            initial_message: "正在准备卸载…".to_string(),
            success_status: TaskSkillStatus::Removed,
            total: plan.total(),
            cancellation: CancellationPolicy::SealOnly,
        }
    }

    fn begin(&self, plan: &Self::Plan) -> Result<StartOutcome<Self::Unit>, AppError> {
        let log = OperationLog::for_scope(&self.paths, &self.layout.scope);
        Ok(StartOutcome {
            units: vec![plan.clone()],
            initial_rows: Vec::new(),
            log_path: Some(self.paths.display(log.path())),
        })
    }

    fn preflight(&self, plan: &Self::Unit, reporter: &Reporter) -> Result<Preflight, AppError> {
        reporter.progress(
            TaskPhase::Backup,
            format!("正在备份 {}…", plan.name),
            Some(plan.name.as_str()),
        );
        let backups = BackupStore::for_layout(&self.paths, &self.layout);
        let backup = backups
            .create(
                BackupAction::Uninstall,
                &plan.name,
                plan.canonical.as_deref(),
                &plan.backup_attachments(),
                &plan.agent_ids,
            )
            .inspect_err(|error| {
                let log = OperationLog::for_scope(&self.paths, &self.layout.scope);
                let _ = log.append(&OperationEntry::new(
                    "uninstall",
                    &plan.name,
                    plan.agent_ids.clone(),
                    "failed",
                    None,
                    Some(error.clone()),
                ));
            })?;
        Ok(Preflight {
            snapshot: backup.path,
            post_message: None,
            warnings: Vec::new(),
        })
    }

    fn command(&self, plan: &Self::Unit, runtime: &CliRuntime, reporter: &Reporter) -> CommandSpec {
        reporter.progress(
            TaskPhase::RunningCli,
            format!("正在执行 skills remove（{}）…", plan.name),
            Some(plan.name.as_str()),
        );
        ScopedCli::new(runtime, &self.layout).uninstall(&plan.name, self.timeout)
    }

    fn complete(
        &self,
        plan: &Self::Unit,
        output: ExecOutput,
        preflight: &Preflight,
        reporter: &Reporter,
    ) -> Result<UnitOutcome, AppError> {
        reporter.progress(
            TaskPhase::Verifying,
            "正在校验 canonical、agent 挂载与 lock…",
            Some(plan.name.as_str()),
        );
        let mut warnings = Vec::new();
        if output.success() {
            remove_extra_attachments(plan, &mut warnings);
        }
        let verification = verify_uninstall(&self.layout, plan);
        let outcome = evaluate_output(&output, verification);
        self.finish(plan, outcome, warnings, preflight)
    }

    fn on_infrastructure_failure(
        &self,
        plan: &Self::Unit,
        error: AppError,
        preflight: Option<&Preflight>,
        _reporter: &Reporter,
    ) -> FailureDisposition {
        super::session::unit_failure(
            &self.paths,
            &self.layout.scope,
            super::session::FailedUnit {
                action: "uninstall",
                name: &plan.name,
                agents: &plan.agent_ids,
            },
            error,
            preflight,
            None,
        )
    }

    fn on_cancelled(&self, plan: &Self::Unit, preflight: &Preflight, error: Option<AppError>) {
        super::session::log_unit_cancelled(
            &self.paths,
            &self.layout.scope,
            super::session::FailedUnit {
                action: "uninstall",
                name: &plan.name,
                agents: &plan.agent_ids,
            },
            &preflight.snapshot,
            error,
        );
    }
}

/// Exact located paths only: unlink a symlink without touching its target and
/// remove real directories recursively. No name normalization or lock writes.
fn remove_unmanaged_paths(plan: &UninstallPlan) -> Result<(), AppError> {
    let mut seen = BTreeSet::new();
    for path in plan
        .attachments
        .iter()
        .filter(|attachment| attachment.mode != AttachmentMode::Native)
        .map(|attachment| &attachment.path)
        .chain(plan.canonical.iter())
    {
        if !seen.insert(path) {
            continue;
        }
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            // Agent directories can alias the same physical path.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(AppError::verify_failed(format!(
                    "无法读取卸载目标 {}：{error}",
                    path.display()
                )));
            }
        };
        let result = if metadata.file_type().is_symlink() {
            fs::remove_file(path)
        } else if metadata.is_dir() {
            fs::remove_dir_all(path)
        } else {
            return Err(AppError::path_invalid("卸载目标既不是目录也不是符号链接。"));
        };
        result.map_err(|error| {
            AppError::verify_failed(format!("无法删除卸载目标 {}：{error}", path.display()))
        })?;
    }
    Ok(())
}

fn lexists(path: &std::path::Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use std::os::unix::fs::symlink;
    use std::path::Path;

    fn plan_uninstall(
        paths: &AppPaths,
        agents: &[AgentDir],
        name: &str,
    ) -> Result<UninstallPlan, AppError> {
        let layout = crate::scope_layout::ScopeLayout::global(paths, agents.to_vec());
        super::plan_uninstall(paths, &layout, name)
    }

    fn verify_uninstall(paths: &AppPaths, plan: &UninstallPlan) -> Result<(), AppError> {
        let layout = crate::scope_layout::ScopeLayout::global(paths, Vec::new());
        super::verify_uninstall(&layout, plan)
    }

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
            pinned_version: crate::cli::CLI_VERSION.to_string(),
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
        let fixture = Fixture::new();
        let layout =
            crate::scope_layout::ScopeLayout::global(&fixture.paths, fixture.agents.clone());
        let spec = ScopedCli::new(&runtime(), &layout).uninstall("alpha", Duration::from_secs(900));
        assert_eq!(spec.program, PathBuf::from("/node/bin/npx"));
        assert_eq!(
            spec.args,
            vec!["-y", "skills@latest", "remove", "-g", "-s", "alpha", "-y"]
        );
        assert_eq!(spec.cwd, PathBuf::from("/app-data/cli-cwd"));
        assert_eq!(spec.env["CI"], "true");
        assert_eq!(spec.timeout, Duration::from_secs(900));
        for forbidden in [
            "--all", "--copy", "-a", "*", "update", "check", "--json", "list",
        ] {
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
    fn unmanaged_skills_plan_their_physical_paths_without_a_lock_entry() {
        let fixture = Fixture::new();
        let plan = fixture.plan("unmanaged").unwrap();
        assert_eq!(plan.management, ManagementState::Unmanaged);
        assert_eq!(
            plan.canonical,
            Some(fixture.home.join(".agents/skills/unmanaged"))
        );
        assert_eq!(plan.agent_ids, vec!["universal"]);
    }

    #[test]
    fn invalid_and_unknown_skill_ids_are_rejected() {
        let fixture = Fixture::new();
        for id in ["", "..", "alpha/beta", ".hidden"] {
            let error = fixture.plan(id).unwrap_err();
            assert_eq!(error.code, ErrorCode::PathInvalid, "{id:?}");
        }
        for id in ["alpha;rm", "$(id)", "`id`", "a|b", "a&b", "a>b", "a*b"] {
            let error = fixture.plan(id).unwrap_err();
            assert_eq!(error.code, ErrorCode::SkillInvalidName, "{id:?}");
        }
        let error = fixture.plan("ghost").unwrap_err();
        assert_eq!(error.code, ErrorCode::SkillNotFound);
    }

    #[test]
    fn uninstall_includes_links_to_its_own_copy() {
        let fixture = Fixture::new();
        fs::remove_file(fixture.gemini.join("alpha")).unwrap();
        symlink(fixture.claude.join("alpha"), fixture.gemini.join("alpha")).unwrap();

        let plan = fixture
            .plan("alpha")
            .expect("the copy and its same-Skill link are both being uninstalled");
        assert!(plan.attachments.iter().any(|attachment| {
            attachment.path == fixture.claude.join("alpha")
                && attachment.mode == AttachmentMode::Copy
        }));
        assert!(plan.attachments.iter().any(|attachment| {
            attachment.path == fixture.gemini.join("alpha")
                && attachment.mode == AttachmentMode::Link
        }));
    }

    #[test]
    fn a_copy_referenced_by_a_different_skills_symlink_blocks_the_plan() {
        let fixture = Fixture::new();
        symlink(
            fixture.claude.join("alpha"),
            fixture.gemini.join("different-skill"),
        )
        .unwrap();

        let error = fixture.plan("alpha").unwrap_err();
        assert_eq!(error.code, ErrorCode::AgentDependencyConflict);
        assert!(error.message.contains("Claude Code"), "{error:?}");
        let detail = error.detail.expect("dependency detail");
        assert!(detail.contains("Gemini CLI"), "{detail}");
        assert!(
            detail.contains(".gemini/skills/different-skill"),
            "{detail}"
        );
    }

    #[test]
    fn outside_dependencies_block_copy_only_skills_too() {
        let fixture = Fixture::new();
        symlink(
            fixture.claude.join("copy-only"),
            fixture.qoder.join("different-skill"),
        )
        .unwrap();

        let error = fixture.plan("copy-only").unwrap_err();
        assert_eq!(error.code, ErrorCode::AgentDependencyConflict);
        assert!(error.message.contains("Claude Code"), "{error:?}");
        assert!(error.detail.unwrap().contains("Qoder"));
    }

    #[test]
    fn outside_copy_dependencies_also_block_unmanaged_uninstall() {
        let fixture = Fixture::new();
        remove_lock_key(&fixture.paths, "copy-only");
        symlink(
            fixture.claude.join("copy-only"),
            fixture.qoder.join("different-skill"),
        )
        .unwrap();
        assert_eq!(
            fixture.plan("copy-only").unwrap_err().code,
            ErrorCode::AgentDependencyConflict
        );
        assert!(fixture.claude.join("copy-only/SKILL.md").is_file());
    }

    #[test]
    fn copy_only_and_unmanaged_uninstall_include_their_own_dependent_links() {
        for unmanaged in [false, true] {
            let fixture = Fixture::new();
            if unmanaged {
                remove_lock_key(&fixture.paths, "copy-only");
            }
            symlink(
                fixture.claude.join("copy-only"),
                fixture.qoder.join("copy-only"),
            )
            .unwrap();
            let plan = fixture.plan("copy-only").unwrap();
            assert!(plan.canonical.is_none());
            assert_eq!(plan.attachments.len(), 2);
            assert_eq!(
                plan.management,
                if unmanaged {
                    ManagementState::Unmanaged
                } else {
                    ManagementState::CopyOnly
                }
            );
        }
    }

    #[test]
    fn a_canonical_link_to_the_same_skills_copy_is_inside_uninstall_scope() {
        let fixture = Fixture::new();
        symlink(
            fixture.claude.join("copy-only"),
            fixture.paths.canonical_skills_dir().join("copy-only"),
        )
        .unwrap();
        let plan = fixture.plan("copy-only").unwrap();
        assert_eq!(plan.management, ManagementState::Managed);
        assert!(plan.canonical.unwrap().is_symlink());
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
        assert_eq!(verification, Ok(()));
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

        let error = verify_uninstall(&fixture.paths, &plan).unwrap_err();
        assert_eq!(error.code, ErrorCode::VerifyCanonicalPresent);
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

        let error = verify_uninstall(&fixture.paths, &plan).unwrap_err();
        assert_eq!(error.code, ErrorCode::VerifyAttachmentPresent);
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

        let error = verify_uninstall(&fixture.paths, &plan).unwrap_err();
        assert_eq!(error.code, ErrorCode::VerifyAttachmentPresent);
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

        let error = verify_uninstall(&fixture.paths, &plan).unwrap_err();
        assert_eq!(error.code, ErrorCode::VerifyLockPresent);
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
            verify_uninstall(&fixture.paths, &plan),
            Ok(()),
            "a missing lock means there is no key left"
        );

        fs::write(fixture.paths.lock_path(), "{not json").unwrap();
        let error = verify_uninstall(&fixture.paths, &plan).unwrap_err();
        assert_eq!(error.code, ErrorCode::VerifyFailed);
    }

    #[test]
    fn exit_zero_without_changes_is_not_success() {
        let fixture = Fixture::new();
        let plan = fixture.plan("alpha").unwrap();
        // The CLI exited 0 but nothing was removed.
        let verification = verify_uninstall(&fixture.paths, &plan);
        assert_eq!(
            verification.unwrap_err().code,
            ErrorCode::VerifyCanonicalPresent
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
        assert_eq!(failure.code, ErrorCode::CliPartialFailure);
        assert!(failure.detail.unwrap().contains("Failed to remove"));

        let nonzero = classify_remove_output(&output(1, "", "boom")).expect("nonzero");
        assert_eq!(nonzero.code, ErrorCode::CliExitNonzero);
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
    use crate::error::ErrorCode;
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
                pinned_version: crate::cli::CLI_VERSION.to_string(),
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
        fn lookup(
            &self,
            _layout: &crate::scope_layout::ScopeLayout,
            _names: &[String],
        ) -> HashMap<String, ConflictInfo> {
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
            .start_uninstall(crate::scope::ScopeId::Global, name.to_string(), sink)
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
    async fn successful_uninstall_discards_only_its_backup_when_retention_is_off() {
        let fixture = ExecFixture::new();
        let settings = crate::settings::SettingsService::new(fixture.paths.clone(), None);
        settings
            .save_data_preferences(crate::settings::DataPreferences {
                retain_backups: false,
                operation_logging: false,
            })
            .unwrap();
        let older = crate::backup::BackupStore::new(&fixture.paths)
            .create(
                crate::backup::BackupAction::Uninstall,
                "older",
                None,
                &[],
                &[],
            )
            .unwrap();
        let paths = fixture.paths.clone();
        let remove = remove_all_post(fixture.home.clone(), fixture.paths.clone(), "alpha");
        fixture
            .executor
            .push(RemoveResponse::success(SUCCESS, move || {
                assert_eq!(
                    fs::read_dir(paths.backups_dir()).unwrap().count(),
                    2,
                    "backup must exist before mutation"
                );
                remove();
            }));
        let (_, event) = run_task(&fixture.manager, "alpha").await;
        let result = event.result.unwrap();
        assert_eq!(result.overall, TaskOverall::Success);
        assert!(result.backup_path.is_none());
        assert!(result.skills.iter().all(|row| row.backup_path.is_none()));
        assert!(result.log_path.is_none());
        assert!(older.path.exists());
        assert_eq!(
            fs::read_dir(fixture.paths.backups_dir()).unwrap().count(),
            1
        );
        assert!(!fixture.paths.logs_dir().join("operations.jsonl").exists());
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
        assert_eq!(result.action, crate::tasks::manager::TaskAction::Uninstall);
        assert!(result.started_at > 0, "startedAt is populated");
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
            vec!["-y", "skills@latest", "remove", "-g", "-s", "alpha", "-y"]
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
        assert_eq!(error.code, ErrorCode::VerifyAttachmentPresent);
        assert!(error.message.contains("Claude Code"));
        assert!(result.backup_path.is_some());
        assert!(result.log_path.is_some());
        assert!(
            BaselineStore::new(&fixture.paths).load("alpha").is_some(),
            "baseline is only removed after verified success"
        );
        let lines = fixture.log_lines();
        assert_eq!(lines[0]["result"], "failed");
        assert_eq!(
            lines[0]["error"]["code"],
            ErrorCode::VerifyAttachmentPresent.as_str()
        );
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
            ErrorCode::VerifyCanonicalPresent
        );
        assert!(fixture.canonical.exists());
        assert_eq!(fixture.executor.calls().len(), 1);
        let lines = fixture.log_lines();
        assert_eq!(lines[0]["result"], "failed");
        assert_eq!(
            lines[0]["error"]["code"],
            ErrorCode::VerifyCanonicalPresent.as_str()
        );
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
        assert_eq!(error.code, ErrorCode::CliExitNonzero);
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
            ErrorCode::VerifyLockPresent
        );
        let lines = fixture.log_lines();
        assert_eq!(
            lines[0]["error"]["code"],
            ErrorCode::VerifyLockPresent.as_str()
        );
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
        assert_eq!(error.code, ErrorCode::BackupFailed);
        assert_eq!(fixture.executor.calls().len(), 0);
        assert!(fixture.canonical.is_dir());
        let lines = fixture.log_lines();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["result"], "failed");
        assert_eq!(lines[0]["error"]["code"], ErrorCode::BackupFailed.as_str());
        assert!(lines[0].get("backupPath").is_none());
    }

    #[tokio::test]
    async fn unmanaged_uninstall_backs_up_and_preserves_lock_without_cli_runtime() {
        let fixture = ExecFixture::with_runtime(Arc::new(HangingRuntime));
        let canonical = fixture.paths.canonical_skills_dir().join("unmanaged");
        symlink(&canonical, fixture.qoder.join("unmanaged")).unwrap();
        fs::create_dir_all(fixture.claude.join("unmanaged")).unwrap();
        fs::write(fixture.claude.join("unmanaged/SKILL.md"), "local copy").unwrap();
        let lock_before = fs::read(fixture.paths.lock_path()).unwrap();
        let (_, event) = tokio::time::timeout(
            Duration::from_secs(5),
            run_task(&fixture.manager, "unmanaged"),
        )
        .await
        .expect("native uninstall must not resolve CLI runtime");
        let result = event.result.unwrap();
        assert_eq!(result.overall, TaskOverall::Success);
        assert!(!canonical.exists());
        assert!(!fixture.qoder.join("unmanaged").is_symlink());
        assert!(!fixture.claude.join("unmanaged").exists());
        assert!(fixture.executor.calls().is_empty());
        assert_eq!(fs::read(fixture.paths.lock_path()).unwrap(), lock_before);
        let backup = fixture.backups().pop().unwrap();
        assert!(backup.join("canonical/SKILL.md").is_file());
        assert_eq!(
            fs::read_to_string(backup.join("attachments/claude-code/SKILL.md")).unwrap(),
            "local copy"
        );
        assert_eq!(fixture.log_lines()[0]["result"], "removed");
        assert!(fixture.canonical.is_dir());
    }

    #[tokio::test]
    async fn unmanaged_canonical_symlink_uninstall_preserves_its_source_and_similar_names() {
        let fixture = ExecFixture::new();
        let canonical = fixture.paths.canonical_skills_dir().join("unmanaged");
        let source = fixture.home.join("plugin-source");
        fs::rename(&canonical, &source).unwrap();
        let canonical = canonical.with_file_name("unmanaged skill");
        symlink(&source, &canonical).unwrap();
        let neighbor = fixture.paths.canonical_skills_dir().join("unmanaged-skill");
        fs::create_dir_all(&neighbor).unwrap();
        fs::write(neighbor.join("SKILL.md"), "different skill").unwrap();
        let (_, event) = run_task(&fixture.manager, "unmanaged skill").await;
        assert_eq!(event.result.unwrap().overall, TaskOverall::Success);
        assert!(!canonical.is_symlink());
        assert!(source.join("SKILL.md").is_file());
        assert_eq!(
            fs::read_to_string(neighbor.join("SKILL.md")).unwrap(),
            "different skill"
        );
        assert!(fixture.executor.calls().is_empty());
        assert!(fixture.backups()[0].join("canonical/SKILL.md").is_file());
    }

    #[tokio::test]
    async fn unmanaged_extra_agent_only_copy_uninstalls_without_creating_a_lock() {
        let fixture = ExecFixture::new();
        let canonical = fixture.paths.canonical_skills_dir().join("unmanaged");
        let target = fixture.home.join(".acme/skills/unmanaged");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::rename(&canonical, &target).unwrap();
        fs::write(fixture.paths.agents_config_path(), r#"{"version":1,"agents":[{"id":"acme","displayName":"Acme","skillsDir":"~/.acme/skills"}]}"#).unwrap();
        fs::remove_file(fixture.paths.lock_path()).unwrap();
        let (_, event) = run_task(&fixture.manager, "unmanaged").await;
        assert_eq!(event.result.unwrap().overall, TaskOverall::Success);
        assert!(!target.exists());
        assert!(!fixture.paths.lock_path().exists());
        assert!(fixture.executor.calls().is_empty());
        let backup = &fixture.backups()[0];
        assert!(backup.join("attachments/acme/SKILL.md").is_file());
        assert!(backup.join("lock-absent.txt").is_file());
    }

    #[tokio::test]
    async fn unmanaged_uninstall_rechecks_adoption_and_link_destination_after_backup() {
        for adopted in [false, true] {
            let fixture = ExecFixture::new();
            let canonical = fixture.paths.canonical_skills_dir().join("unmanaged");
            let source = fixture.home.join("source");
            fs::rename(&canonical, &source).unwrap();
            symlink(&source, &canonical).unwrap();
            let layout = crate::scope_layout::ScopeLayout::resolve_detected(
                &fixture.paths,
                &crate::projects::ProjectRegistry::new(fixture.paths.clone()),
                &crate::scope::ScopeId::Global,
            )
            .unwrap();
            let plan = super::plan_uninstall(&fixture.paths, &layout, "unmanaged").unwrap();
            let backup = BackupStore::for_layout(&fixture.paths, &layout)
                .create(
                    BackupAction::Uninstall,
                    &plan.name,
                    plan.canonical.as_deref(),
                    &plan.backup_attachments(),
                    &plan.agent_ids,
                )
                .unwrap();
            if adopted {
                write_lock(
                    &fixture.paths,
                    &["alpha", "copy-only", "other", "unmanaged"],
                );
            } else {
                fs::remove_file(&canonical).unwrap();
                symlink(&fixture.canonical, &canonical).unwrap();
            }
            let action =
                UninstallAction::new(fixture.paths.clone(), layout, Duration::from_secs(5));
            assert_eq!(
                action.before_write(&plan).unwrap_err().code,
                ErrorCode::BatchTargetChanged
            );
            assert!(canonical.is_symlink());
            assert!(source.join("SKILL.md").is_file());
            assert!(backup.path.join("canonical/SKILL.md").is_file());
        }
    }

    #[tokio::test]
    async fn unmanaged_uninstall_backup_failure_preserves_files() {
        let fixture = ExecFixture::new();
        fs::remove_dir(fixture.paths.backups_dir()).unwrap();
        fs::write(fixture.paths.backups_dir(), "blocked").unwrap();
        let (_, event) = run_task(&fixture.manager, "unmanaged").await;
        assert_eq!(event.error.unwrap().code, ErrorCode::BackupFailed);
        assert!(
            fixture
                .paths
                .canonical_skills_dir()
                .join("unmanaged/SKILL.md")
                .is_file()
        );
        assert!(fixture.executor.calls().is_empty());
    }

    #[tokio::test]
    async fn outside_copy_dependencies_block_before_a_task_starts() {
        let fixture = ExecFixture::new();
        symlink(
            fixture.claude.join("alpha"),
            fixture.gemini.join("different-skill"),
        )
        .unwrap();

        let (sink, mut receiver) = sink();
        let error = fixture
            .manager
            .start_uninstall(crate::scope::ScopeId::Global, "alpha".to_string(), sink)
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::AgentDependencyConflict);
        assert_eq!(fixture.executor.calls().len(), 0);
        assert!(fixture.backups().is_empty());
        assert!(fixture.canonical.is_dir());
        assert!(fixture.claude.join("alpha").is_dir());
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn same_skill_copy_links_uninstall_with_backup_and_no_leftovers() {
        for unmanaged in [false, true] {
            let fixture = ExecFixture::new();
            let codex = fixture.home.join(".codex/skills");
            fs::create_dir_all(&codex).unwrap();
            fs::remove_file(fixture.gemini.join("alpha")).unwrap();
            for dir in [&codex, &fixture.gemini] {
                symlink("../../.claude/skills/alpha", dir.join("alpha")).unwrap();
            }
            if unmanaged {
                remove_lock_key(&fixture.paths, "alpha");
            } else {
                let home = fixture.home.clone();
                let paths = fixture.paths.clone();
                let codex_path = codex.join("alpha");
                fixture
                    .executor
                    .push(RemoveResponse::success(SUCCESS, move || {
                        remove_all_post(home, paths, "alpha")();
                        // The CLI does not know the Codex compatibility directory;
                        // native Extra-agent cleanup must remove this broken link.
                        assert!(codex_path.is_symlink());
                    }));
            }
            let (_, event) = run_task(&fixture.manager, "alpha").await;
            assert_eq!(event.result.unwrap().overall, TaskOverall::Success);
            for path in [
                &fixture.canonical,
                &fixture.claude.join("alpha"),
                &fixture.gemini.join("alpha"),
                &fixture.qoder.join("alpha"),
                &codex.join("alpha"),
            ] {
                assert!(fs::symlink_metadata(path).is_err(), "{}", path.display());
            }
            let backup = &fixture.backups()[0];
            assert!(backup.join("canonical/SKILL.md").is_file());
            assert!(backup.join("attachments/claude-code/SKILL.md").is_file());
            let manifest = fs::read_to_string(backup.join(BACKUP_MANIFEST_FILE)).unwrap();
            assert!(manifest.contains("codex-legacy"));
            assert!(manifest.contains(".codex/skills/alpha"));
            assert_eq!(fixture.executor.calls().len(), usize::from(!unmanaged));
            assert_eq!(fixture.log_lines()[0]["result"], "removed");
            assert!(
                fixture
                    .paths
                    .canonical_skills_dir()
                    .join("other/SKILL.md")
                    .is_file()
            );
        }
    }

    #[tokio::test]
    async fn newly_added_outside_copy_links_block_after_backup() {
        for unmanaged in [false, true] {
            let fixture = ExecFixture::new();
            if unmanaged {
                remove_lock_key(&fixture.paths, "alpha");
            }
            let layout = crate::scope_layout::ScopeLayout::resolve_detected(
                &fixture.paths,
                &crate::projects::ProjectRegistry::new(fixture.paths.clone()),
                &crate::scope::ScopeId::Global,
            )
            .unwrap();
            let plan = super::plan_uninstall(&fixture.paths, &layout, "alpha").unwrap();
            let backup = BackupStore::for_layout(&fixture.paths, &layout)
                .create(
                    BackupAction::Uninstall,
                    &plan.name,
                    plan.canonical.as_deref(),
                    &plan.backup_attachments(),
                    &plan.agent_ids,
                )
                .unwrap();
            symlink(
                fixture.claude.join("alpha"),
                fixture.gemini.join("different-skill"),
            )
            .unwrap();
            let action =
                UninstallAction::new(fixture.paths.clone(), layout, Duration::from_secs(5));
            assert_eq!(
                action.before_write(&plan).unwrap_err().code,
                ErrorCode::AgentDependencyConflict
            );
            assert!(fixture.canonical.join("SKILL.md").is_file());
            assert!(fixture.claude.join("alpha/SKILL.md").is_file());
            assert!(
                backup
                    .path
                    .join("attachments/claude-code/SKILL.md")
                    .is_file()
            );
            assert!(fixture.executor.calls().is_empty());
        }
    }

    #[tokio::test]
    async fn uninstall_is_cancellable_before_the_cli_spawns() {
        let fixture = ExecFixture::with_runtime(Arc::new(HangingRuntime));
        let lock_before = fs::read(fixture.paths.lock_path()).unwrap();
        let (sink, mut receiver) = sink();
        let task_id = fixture
            .manager
            .start_uninstall(crate::scope::ScopeId::Global, "alpha".to_string(), sink)
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
            .start_uninstall(crate::scope::ScopeId::Global, "alpha".to_string(), sink)
            .await
            .unwrap();
        loop {
            let event = receiver.recv().await.unwrap();
            if event.phase == TaskPhase::RunningCli {
                break;
            }
        }
        let error = fixture.manager.cancel_task(&task_id).unwrap_err();
        assert_eq!(error.code, ErrorCode::TaskCannotCancel);

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

#[cfg(test)]
mod project_uninstall_tests {
    use super::*;
    use crate::baseline::BaselineStore;
    use crate::cli::{CLI_VERSION, CliRuntime, CliRuntimeProvider};
    use crate::error::ErrorCode;
    use crate::process::{CancelToken, ExecError, ProcessExecutor};
    use crate::projects::{Project, ProjectRegistry};
    use crate::scope::ScopeId;
    use crate::tasks::conflicts::{ConflictInfo, ConflictProvider};
    use crate::tasks::manager::{
        EventSink, TaskEvent, TaskManager, TaskManagerConfig, TaskOverall, TaskPhase,
        TaskSkillStatus,
    };
    use std::os::unix::fs::symlink;
    use std::sync::{Arc, Mutex};
    use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

    struct ProjectFixture {
        _tmp: tempfile::TempDir,
        paths: AppPaths,
        root: PathBuf,
        canonical: PathBuf,
        claude: PathBuf,
        acme: PathBuf,
        global_canonical: PathBuf,
        global_qoder_link: PathBuf,
        project: Project,
    }

    impl ProjectFixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path().join("home");
            let paths = AppPaths::new(&home, tmp.path().join("app-data"));
            paths.ensure_dirs().unwrap();
            // Global skill with the same name must stay untouched.
            let global_canonical = paths.canonical_skills_dir().join("alpha");
            fs::create_dir_all(&global_canonical).unwrap();
            fs::write(global_canonical.join("SKILL.md"), "# global alpha\n").unwrap();
            let global_qoder = home.join(".qoder/skills");
            fs::create_dir_all(&global_qoder).unwrap();
            let global_qoder_link = global_qoder.join("alpha");
            symlink(&global_canonical, &global_qoder_link).unwrap();
            fs::create_dir_all(paths.agents_root()).unwrap();
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
            BaselineStore::new(&paths)
                .record_from_dir("alpha", &global_canonical, true)
                .unwrap();

            let root = tmp.path().join("projects/app");
            let canonical = root.join(".agents/skills/alpha");
            fs::create_dir_all(&canonical).unwrap();
            fs::write(canonical.join("SKILL.md"), "# project alpha\n").unwrap();
            let claude = root.join(".claude/skills");
            fs::create_dir_all(claude.join("alpha")).unwrap();
            fs::write(claude.join("alpha/SKILL.md"), "# project copy\n").unwrap();
            fs::create_dir_all(root.join(".qoder/skills")).unwrap();
            symlink(&canonical, root.join(".qoder/skills/alpha")).unwrap();
            let acme = root.join(".acme/skills");
            fs::create_dir_all(&acme).unwrap();
            symlink(&canonical, acme.join("alpha")).unwrap();
            fs::create_dir_all(paths.app_data()).unwrap();
            fs::write(
                paths.agents_config_path(),
                r#"{
                    "version": 1,
                    "agents": [
                        {"id": "acme", "displayName": "Acme", "skillsDir": "~/.acme/skills", "projectSkillsDir": ".acme/skills"}
                    ]
                }"#,
            )
            .unwrap();
            fs::write(
                root.join("skills-lock.json"),
                serde_json::json!({
                    "version": 1,
                    "skills": {
                        "alpha": {
                            "source": "acme/skills",
                            "sourceType": "github",
                            "sourceUrl": "https://github.com/acme/skills.git",
                            "skillPath": "skills/alpha/SKILL.md",
                            "computedHash": "a".repeat(64)
                        }
                    }
                })
                .to_string(),
            )
            .unwrap();
            let root = fs::canonicalize(root).unwrap();
            let project = Project {
                id: uuid::Uuid::new_v4(),
                root,
                display_name: "app".to_string(),
                added_at: 1,
                last_opened_at: 1,
                check_revision: "initial".into(),
            };
            Self {
                _tmp: tmp,
                paths,
                root: project.root.clone(),
                canonical,
                claude,
                acme,
                global_canonical,
                global_qoder_link,
                project,
            }
        }

        fn layout(&self) -> crate::scope_layout::ScopeLayout {
            crate::scope_layout::ScopeLayout::project(&self.paths, self.project.clone())
        }
    }

    #[test]
    fn project_uninstall_plan_is_scoped_and_includes_extra_attachments() {
        let fixture = ProjectFixture::new();
        let layout = fixture.layout();
        let plan = super::plan_uninstall(&fixture.paths, &layout, &layout.scope.skill_id("alpha"))
            .unwrap();
        assert_eq!(
            plan.canonical.as_deref(),
            Some(fs::canonicalize(&fixture.canonical).unwrap().as_path())
        );
        assert!(
            plan.attachments
                .iter()
                .any(|attachment| attachment.agent_id == "acme"),
            "extra project link joins the backup/verification set: {:?}",
            plan.attachments
        );
        assert!(plan.canonical.as_ref().unwrap().starts_with(&fixture.root));
        assert!(fixture.global_canonical.is_dir());
    }

    #[test]
    fn project_uninstall_command_omits_global_flag_and_uses_the_project_root_as_cwd() {
        let fixture = ProjectFixture::new();
        let runtime = CliRuntime {
            npx_path: PathBuf::from("/node/bin/npx"),
            node_dir: PathBuf::from("/node/bin"),
            git_dir: Some(PathBuf::from("/usr/bin")),
            cwd: PathBuf::from("/app-data/cli-cwd"),
            home: fixture.paths.home().to_path_buf(),
            env: std::collections::BTreeMap::new(),
            pinned_version: CLI_VERSION.to_string(),
        };
        let spec =
            ScopedCli::new(&runtime, &fixture.layout()).uninstall("alpha", Duration::from_secs(5));
        assert_eq!(spec.cwd, fixture.root);
        assert_eq!(
            spec.args,
            vec!["-y", "skills@latest", "remove", "-s", "alpha", "-y"]
        );
        for forbidden in ["-g", "-a", "--all", "--copy", "*", "update", "check"] {
            assert!(
                !spec.args.iter().any(|arg| arg == forbidden),
                "{forbidden} must never appear in a project uninstall"
            );
        }
    }

    struct FakeRuntime;

    #[async_trait::async_trait]
    impl CliRuntimeProvider for FakeRuntime {
        async fn cli_runtime(&self) -> Result<CliRuntime, AppError> {
            Ok(CliRuntime {
                npx_path: PathBuf::from("/node/bin/npx"),
                node_dir: PathBuf::from("/node/bin"),
                git_dir: Some(PathBuf::from("/usr/bin")),
                cwd: PathBuf::from("/app-data/cli-cwd"),
                home: PathBuf::from("/tmp/skills-box-home"),
                env: std::collections::BTreeMap::new(),
                pinned_version: CLI_VERSION.to_string(),
            })
        }
    }

    #[derive(Default)]
    struct NoConflicts;

    impl ConflictProvider for NoConflicts {
        fn lookup(
            &self,
            _layout: &crate::scope_layout::ScopeLayout,
            _names: &[String],
        ) -> std::collections::HashMap<String, ConflictInfo> {
            std::collections::HashMap::new()
        }
    }

    struct Executor {
        calls: Mutex<Vec<CommandSpec>>,
        post: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    }

    #[async_trait::async_trait]
    impl ProcessExecutor for Executor {
        async fn run(
            &self,
            spec: CommandSpec,
            _cancel: CancelToken,
        ) -> Result<ExecOutput, ExecError> {
            self.calls.lock().unwrap().push(spec);
            if let Some(post) = self.post.lock().unwrap().take() {
                post();
            }
            Ok(ExecOutput {
                code: Some(0),
                signal: None,
                stdout: include_str!("../../tests/fixtures/cli/remove-success.txt").to_string(),
                stderr: String::new(),
                duration: Duration::ZERO,
            })
        }
    }

    fn sink() -> (EventSink, UnboundedReceiver<TaskEvent>) {
        let (sender, receiver) = unbounded_channel();
        let sink: EventSink = Arc::new(move |event| {
            let _ = sender.send(event);
        });
        (sink, receiver)
    }

    fn remove_tree(path: &std::path::Path) {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                fs::remove_file(path).unwrap();
            }
            Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path).unwrap(),
            Ok(_) => fs::remove_file(path).unwrap(),
            Err(_) => {}
        }
    }

    async fn run_project_uninstall(
        fixture: &ProjectFixture,
        leave_canonical: bool,
    ) -> (TaskManager, Arc<Executor>, ScopeId, TaskEvent) {
        let root = fixture.root.clone();
        let canonical = fixture.canonical.clone();
        let post = move || {
            for relative in [
                ".qoder/skills/alpha",
                ".claude/skills/alpha",
                ".acme/skills/alpha",
            ] {
                remove_tree(&root.join(relative));
            }
            if !leave_canonical {
                remove_tree(&canonical);
            }
            let lock_path = root.join("skills-lock.json");
            let mut lock: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(&lock_path).unwrap()).unwrap();
            lock["skills"].as_object_mut().unwrap().remove("alpha");
            fs::write(&lock_path, lock.to_string()).unwrap();
        };
        let executor = Arc::new(Executor {
            calls: Mutex::new(Vec::new()),
            post: Mutex::new(Some(Box::new(post))),
        });
        let projects = Arc::new(ProjectRegistry::new(fixture.paths.clone()));
        let entry = projects.add(fixture.root.to_str().unwrap()).unwrap();
        let scope = entry.project.scope();
        let manager = TaskManager::with_projects(
            Arc::new(FakeRuntime),
            executor.clone(),
            Arc::new(NoConflicts),
            fixture.paths.clone(),
            projects,
            TaskManagerConfig {
                preview_timeout: Duration::from_secs(60),
                install_timeout: Duration::from_secs(5),
            },
        );
        let (sink, mut receiver) = sink();
        manager
            .start_uninstall(scope.clone(), scope.skill_id("alpha"), sink)
            .await
            .unwrap();
        let final_event = loop {
            let event = receiver.recv().await.unwrap();
            if event.phase.is_final() {
                break event;
            }
        };
        (manager, executor, scope, final_event)
    }

    #[tokio::test]
    async fn project_unmanaged_uninstall_preserves_global_scope_and_project_lock() {
        let fixture = ProjectFixture::new();
        let lock_path = fixture.root.join("skills-lock.json");
        fs::write(&lock_path, r#"{"version":1,"skills":{}}"#).unwrap();
        let lock_before = fs::read(&lock_path).unwrap();
        let (_, executor, scope, event) = run_project_uninstall(&fixture, false).await;
        let result = event.result.unwrap();
        assert_eq!(result.overall, TaskOverall::Success);
        assert_eq!(result.scope, scope);
        assert!(!fixture.canonical.exists());
        assert!(!fixture.claude.join("alpha").exists());
        assert!(!fixture.acme.join("alpha").is_symlink());
        assert_eq!(fs::read(&lock_path).unwrap(), lock_before);
        assert!(fixture.global_canonical.is_dir());
        assert!(fixture.global_qoder_link.is_symlink());
        assert!(executor.calls.lock().unwrap().is_empty());
        assert!(
            fixture
                .paths
                .backups_dir()
                .join(format!("project-{}", scope.project_id().unwrap()))
                .is_dir()
        );
    }

    #[tokio::test]
    async fn project_uninstall_only_removes_the_project_scope_and_backs_it_up() {
        let fixture = ProjectFixture::new();
        let (_manager, executor, scope, final_event) = run_project_uninstall(&fixture, false).await;
        assert_eq!(final_event.phase, TaskPhase::Completed);
        let result = final_event.result.unwrap();
        assert_eq!(result.scope, scope);
        assert_eq!(result.overall, TaskOverall::Success);
        assert_eq!(result.skills[0].status, TaskSkillStatus::Removed);

        let calls = executor.calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].cwd, fixture.root);
        assert_eq!(
            calls[0].args,
            vec!["-y", "skills@latest", "remove", "-s", "alpha", "-y"]
        );

        // Project scope is fully gone, including the extra link.
        assert!(!fixture.canonical.exists());
        assert!(!fixture.root.join(".qoder/skills/alpha").exists());
        assert!(!fixture.claude.join("alpha").exists());
        assert!(!fixture.acme.join("alpha").exists());
        let lock: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(fixture.root.join("skills-lock.json")).unwrap(),
        )
        .unwrap();
        assert!(lock["skills"].get("alpha").is_none());

        // Global scope with the same skill name is untouched.
        assert!(fixture.global_canonical.is_dir());
        assert!(fixture.global_qoder_link.is_symlink());
        assert!(
            crate::lock::LockFile::read(&fixture.paths.lock_path())
                .unwrap()
                .contains("alpha")
        );
        assert!(
            BaselineStore::new(&fixture.paths).load("alpha").is_some(),
            "global baseline is untouched"
        );

        // Scope-isolated backup with the project lock and copy attachment.
        let backup = PathBuf::from(result.backup_path.unwrap());
        assert_eq!(
            backup.parent().unwrap(),
            fixture
                .paths
                .backups_dir()
                .join(format!("project-{}", scope.project_id().unwrap()))
        );
        assert!(backup.join("skills-lock.json").is_file());
        assert!(!backup.join("skill-lock.json").exists());
        assert_eq!(
            fs::read_to_string(backup.join("canonical/SKILL.md")).unwrap(),
            "# project alpha\n"
        );
        assert_eq!(
            fs::read_to_string(backup.join("attachments/claude-code/SKILL.md")).unwrap(),
            "# project copy\n"
        );
        let manifest: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(backup.join("attachment-manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["scope"], serde_json::json!(scope.as_string()));
        assert_eq!(manifest["action"], "uninstall");

        let log = fs::read_to_string(fixture.paths.logs_dir().join("operations.jsonl")).unwrap();
        let line: serde_json::Value = serde_json::from_str(log.lines().next().unwrap()).unwrap();
        assert_eq!(line["scope"], scope.as_string());
        assert!(!fixture.paths.interrupted_marker_path().exists());
    }

    #[tokio::test]
    async fn a_failed_project_uninstall_never_touches_the_global_scope() {
        let fixture = ProjectFixture::new();
        let (_manager, _executor, scope, final_event) = run_project_uninstall(&fixture, true).await;
        let result = final_event.result.unwrap();
        assert_eq!(result.scope, scope);
        assert_eq!(result.overall, TaskOverall::Failed);
        assert_eq!(
            result.skills[0].error.as_ref().unwrap().code,
            ErrorCode::VerifyCanonicalPresent
        );
        assert!(
            fixture.canonical.exists(),
            "failed removal leaves the project canonical"
        );
        assert!(fixture.global_canonical.is_dir());
        assert!(fixture.global_qoder_link.is_symlink());
        assert!(
            crate::lock::LockFile::read(&fixture.paths.lock_path())
                .unwrap()
                .contains("alpha")
        );
        assert!(BaselineStore::new(&fixture.paths).load("alpha").is_some());
    }
}
