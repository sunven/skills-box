//! Install execution: request validation, the pinned CLI command, its
//! `--json` result parser, the pre-spawn lock snapshot, and the post-install
//! verifier.
//!
//! The CLI's `installed` status is never trusted on its own: after the process
//! exits we check the canonical folder, the lock key, and every selected
//! non-universal agent attachment with `lstat`. A silent symlink-to-copy
//! fallback is reported as a warning instead of a failure.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;

use crate::agents::catalog;
#[cfg(test)]
use crate::agents::{DetectionContext, resolve_global_skills_dir};
use crate::attachment_target::{self, AgentDirectoryState, TargetState};
use crate::backup::{BackupAction, BackupStore};
use crate::baseline::{BaselineStore, write_warning};
use crate::cli::{CliRuntime, ScopedCli};
#[cfg(test)]
use crate::env::capture_process_env;
#[cfg(test)]
use crate::env::child_env::{ChildEnvParams, build_child_env};
use crate::error::AppError;
use crate::error::cli::{self, CliAction};
use crate::inventory::{AgentDir, ManagementState};
use crate::lock::ScopedLock;
use crate::paths::AppPaths;
use crate::process::{CommandSpec, ExecOutput};

use super::conflicts::find_conflicts;
use super::manager::{TaskAction, TaskPhase, TaskSkillResult, TaskSkillStatus};
use super::session::{
    CancellationPolicy, FailureDisposition, MutationAction, Preflight, ProgressMessage, Reporter,
    SessionProfile, StartOutcome, UnitOutcome,
};
use super::source::validate_source;

pub const INSTALL_TIMEOUT: Duration = crate::timeouts::WRITE_TIMEOUT;
pub const UNIVERSAL_AGENT_ID: &str = "universal";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallRequest {
    pub source: String,
    pub skills: Vec<String>,
    pub agents: Vec<String>,
    /// Explicit user retry of failed items from a previous partial install.
    /// Only a same-source managed/stale lock entry may be overwritten; plain
    /// installs keep rejecting every same-name conflict.
    #[serde(default)]
    pub retry: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallTarget {
    pub id: String,
    pub display_name: String,
    pub universal: bool,
    pub skills_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPlan {
    /// Normalized `owner/repo`.
    pub source: String,
    pub skills: Vec<String>,
    /// Explicit `-a` ids in request order, with `universal` appended last.
    pub agent_ids: Vec<String>,
    /// Selected agents resolved to absolute skills directories.
    pub targets: Vec<InstallTarget>,
    /// True only for an explicit retry request (see [`InstallRequest::retry`]).
    pub retry: bool,
}

impl InstallPlan {
    pub fn total(&self) -> u32 {
        self.skills.len() as u32
    }
}

/// Validate a full install request server-side. The frontend supplies only
/// semantic values (source, skill names, agent ids); everything else is
/// resolved here.
pub fn validate_request(
    paths: &AppPaths,
    layout: &crate::scope_layout::ScopeLayout,
    request: &InstallRequest,
) -> Result<InstallPlan, AppError> {
    let source = validate_source(&request.source)?;
    let skills = validate_skill_names(&request.skills)?;
    let (agent_ids, targets) = if layout.is_global() {
        validate_agents(paths, &request.agents)?
    } else {
        validate_project_agents(paths, layout, &request.agents)?
    };
    Ok(InstallPlan {
        source: source.normalized,
        skills,
        agent_ids,
        targets,
        retry: request.retry,
    })
}

/// Reject the CLI flags and wildcard forms the app must never send, plus
/// path-like, control-character or shell-metacharacter names. The executor
/// never uses a shell, so the metacharacter rule is defense in depth against a
/// malicious repository controlling a skill folder name.
pub fn validate_skill_names(raw: &[String]) -> Result<Vec<String>, AppError> {
    let mut seen = HashSet::new();
    let mut names = Vec::new();
    for value in raw {
        let name = value.trim();
        validate_skill_name(name)?;
        if seen.insert(name.to_string()) {
            names.push(name.to_string());
        }
    }
    if names.is_empty() {
        return Err(AppError::skill_invalid_name("至少需要一个 skill 名。"));
    }
    Ok(names)
}

/// The single skill-name rule shared by install/update and the native
/// detach/uninstall entry points. `value` must already be the exact name (no
/// trimming) so an inventory id can never resolve to a different directory.
pub fn validate_skill_name(value: &str) -> Result<(), AppError> {
    let invalid = value.is_empty()
        || value.len() > 255
        || value == "*"
        || value == "."
        || value == ".."
        || value.starts_with('-')
        || value.starts_with('*')
        || value.contains(['/', '\\', '\0'])
        || value.chars().any(char::is_control)
        || value.chars().any(is_shell_metacharacter);
    if invalid {
        return Err(AppError::skill_invalid_name(format!(
            "无效的 skill 名：{value:?}"
        )));
    }
    Ok(())
}

fn is_shell_metacharacter(character: char) -> bool {
    matches!(
        character,
        ';' | '|'
            | '&'
            | '$'
            | '`'
            | '<'
            | '>'
            | '"'
            | '\''
            | '('
            | ')'
            | '*'
            | '?'
            | '!'
            | '{'
            | '}'
            | '['
            | ']'
            | '~'
            | '#'
    )
}

/// Validate, normalize and dedupe requested agent ids, preserving input order.
/// `validate` runs before dedupe so every raw entry is checked exactly as the
/// caller's error precedence expects.
fn collect_agent_ids(
    raw: &[String],
    mut validate: impl FnMut(&str) -> Result<(), AppError>,
) -> Result<Vec<String>, AppError> {
    let mut seen = HashSet::new();
    let mut order = Vec::new();
    for value in raw {
        let id = value.trim();
        let invalid = id.is_empty()
            || id.len() > 64
            || id == "*"
            || id.starts_with('-')
            || !id.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
            });
        if invalid {
            return Err(AppError::agent_invalid(format!(
                "无效的 agent 标识：{value:?}"
            )));
        }
        if id == UNIVERSAL_AGENT_ID {
            continue;
        }
        validate(id)?;
        if seen.insert(id.to_string()) {
            order.push(id.to_string());
        }
    }
    Ok(order)
}

pub(crate) fn cli_agent_ids(targets: &[InstallTarget]) -> Vec<String> {
    targets
        .iter()
        .filter(|target| crate::agents::is_catalog_agent(&target.id))
        .map(|target| target.id.clone())
        .chain(std::iter::once(UNIVERSAL_AGENT_ID.to_string()))
        .collect()
}

pub(crate) fn validate_selected_agents(
    paths: &AppPaths,
    layout: &crate::scope_layout::ScopeLayout,
    agents: &[String],
) -> Result<(Vec<String>, Vec<InstallTarget>), AppError> {
    if layout.is_global() {
        validate_agents(paths, agents)
    } else {
        validate_project_agents(paths, layout, agents)
    }
}

fn validate_agents(
    paths: &AppPaths,
    raw: &[String],
) -> Result<(Vec<String>, Vec<InstallTarget>), AppError> {
    let order = collect_agent_ids(raw, |id| {
        if let Some(definition) = catalog()
            .agents
            .iter()
            .find(|definition| definition.id == id)
        {
            if definition.global_skills_dir.is_none() {
                return Err(AppError::agent_unsupported_global(&definition.display_name));
            }
        } else if crate::agents::extras::find(paths, id).is_none() {
            return Err(AppError::agent_invalid(format!("未知 agent：{id}")));
        }
        Ok(())
    })?;

    let detected = crate::inventory::detected_agent_dirs(paths);
    let mut targets = Vec::new();
    for id in order {
        let target = detected
            .iter()
            .find(|agent| agent.id == id)
            .ok_or_else(|| {
                let display_name = catalog()
                    .agents
                    .iter()
                    .find(|definition| definition.id == id)
                    .map(|definition| definition.display_name.clone())
                    .or_else(|| {
                        crate::agents::extras::find(paths, &id).map(|extra| extra.display_name)
                    })
                    .unwrap_or_else(|| id.clone());
                AppError::agent_not_detected(&display_name)
            })?;
        targets.push(InstallTarget {
            id: target.id.clone(),
            display_name: target.display_name.clone(),
            universal: target.universal,
            skills_dir: target.skills_dir.clone(),
        });
    }

    // Extra agents are linked natively after the CLI runs, so they never
    // appear in the CLI argument list.
    Ok((cli_agent_ids(&targets), targets))
}

/// Project install targets: globally detected catalog agents plus configured
/// extras with a valid `projectSkillsDir`, whether or not the directory exists.
/// Universal agents are not separate targets because they read canonical.
fn validate_project_agents(
    paths: &AppPaths,
    layout: &crate::scope_layout::ScopeLayout,
    raw: &[String],
) -> Result<(Vec<String>, Vec<InstallTarget>), AppError> {
    let order = collect_agent_ids(raw, |_| Ok(()))?;

    let available = layout.install_targets(&crate::inventory::detected_agent_dirs(paths));
    let mut targets = Vec::new();
    for id in order {
        let target = available
            .iter()
            .find(|agent| agent.id == id)
            .ok_or_else(|| {
                let catalog_definition = catalog()
                    .agents
                    .iter()
                    .find(|definition| definition.id == id);
                let extra = crate::agents::extras::find(paths, &id);
                let display_name = catalog_definition
                    .map(|definition| definition.display_name.clone())
                    .or_else(|| extra.as_ref().map(|extra| extra.display_name.clone()))
                    .unwrap_or_else(|| id.clone());
                match catalog_definition {
                    Some(definition) if definition.universal => {
                        AppError::agent_unsupported_global(&display_name)
                    }
                    Some(_) => AppError::agent_not_detected(&display_name),
                    None => match extra {
                        Some(_) => AppError::agent_not_detected(&display_name),
                        None => AppError::agent_invalid(format!("未知 agent：{id}")),
                    },
                }
            })?;
        targets.push(InstallTarget {
            id: target.id.clone(),
            display_name: target.display_name.clone(),
            universal: false,
            skills_dir: target.skills_dir.clone(),
        });
    }

    let agent_ids = cli_agent_ids(&targets);
    Ok((agent_ids, targets))
}

/// Resolve known agent ids to absolute global skills directories without their
/// config roots existing. Used by install fixtures.
#[cfg(test)]
pub fn resolve_agent_dirs(paths: &AppPaths, ids: &[String]) -> Vec<AgentDir> {
    let parent = capture_process_env();
    let env = build_child_env(ChildEnvParams {
        home: paths.home(),
        node_dir: None,
        git_dir: None,
        parent: &parent,
    });
    let context = DetectionContext::new(paths.home(), paths.cli_cwd(), env);
    ids.iter()
        .filter_map(|id| {
            let definition = match catalog()
                .agents
                .iter()
                .find(|definition| &definition.id == id)
            {
                Some(definition) => definition.clone(),
                None => crate::agents::extras::find(paths, id)?.definition(),
            };
            let skills_dir = resolve_global_skills_dir(&definition, &context)?;
            Some(AgentDir {
                id: definition.id.clone(),
                display_name: definition.display_name.clone(),
                universal: definition.universal,
                skills_dir,
            })
        })
        .collect()
}

/// Create the symlinks the pinned CLI cannot create. Extra agents are never in
/// `agent_ids`, so after `skills add` lands the canonical folder skills-box
/// links them itself. Idempotent: an existing symlink to the canonical folder
/// is left untouched, and nothing is ever overwritten.
pub fn ensure_extra_attachments(
    layout: &crate::scope_layout::ScopeLayout,
    plan: &InstallPlan,
    installed: &[String],
) -> HashMap<String, AppError> {
    let canonical_root = &layout.canonical_dir;
    let mut errors = HashMap::new();
    for name in installed {
        let canonical = canonical_root.join(name);
        if !fs::metadata(&canonical)
            .map(|metadata| metadata.is_dir())
            .unwrap_or(false)
        {
            continue;
        }
        for target in plan
            .targets
            .iter()
            .filter(|target| !target.universal && !crate::agents::is_catalog_agent(&target.id))
        {
            if let Err(error) = ensure_extra_link(layout, &canonical, target, name) {
                errors.entry(name.clone()).or_insert(error);
            }
        }
    }
    errors
}

fn ensure_extra_link(
    layout: &crate::scope_layout::ScopeLayout,
    canonical: &std::path::Path,
    target: &InstallTarget,
    name: &str,
) -> Result<(), AppError> {
    let agent = AgentDir {
        id: target.id.clone(),
        display_name: target.display_name.clone(),
        universal: target.universal,
        skills_dir: target.skills_dir.clone(),
    };
    let resolved = attachment_target::resolve_install_link(
        canonical,
        &agent,
        name,
        attachment_target::TargetContext::for_layout(layout),
    )?;
    if matches!(resolved.directory_state, AgentDirectoryState::Missing(_)) {
        fs::create_dir_all(&resolved.agent.skills_dir).map_err(|error| {
            AppError::agent_attach_failed(format!(
                "无法创建 {}：{error}",
                resolved.agent.skills_dir.display()
            ))
        })?;
    }
    match resolved.target_state {
        TargetState::SymlinkToCanonical => Ok(()),
        TargetState::Absent =>
        {
            #[cfg(unix)]
            std::os::unix::fs::symlink(canonical, &resolved.target).map_err(|error| {
                AppError::agent_attach_failed(format!(
                    "无法创建 {}：{error}",
                    resolved.target.display()
                ))
            })
        }
        TargetState::Other => Err(AppError::agent_attach_failed(format!(
            "{} 已存在，已停止（不覆盖现有内容）。",
            resolved.target.display()
        ))),
    }
}

/// Reject same-name managed/copy-only/unmanaged skills before spawning. The
/// CLI would otherwise overwrite them silently.
///
/// `retry_source` is only `Some(owner/repo)` for an explicit retry of a
/// previous partial install: a managed (including stale-lock) entry from the
/// same source may be overwritten, because this is exactly the skill that the
/// earlier attempt partially wrote. Every other duplicate stays rejected.
pub fn conflicts_for(
    paths: &AppPaths,
    layout: &crate::scope_layout::ScopeLayout,
    skills: &[String],
    retry_source: Option<&str>,
) -> Result<Option<AppError>, AppError> {
    let conflicts = find_conflicts(paths, layout, skills)?;
    for name in skills {
        if let Some(conflict) = conflicts.get(name) {
            let retryable_managed =
                retry_source.is_some()
                    && conflict.management == ManagementState::Managed
                    && conflict.source.as_deref().zip(retry_source).is_some_and(
                        |(existing, retry)| {
                            super::source::source_identity(existing)
                                == super::source::source_identity(retry)
                        },
                    );
            if retryable_managed {
                continue;
            }
            let management = match conflict.management {
                ManagementState::Managed => "已安装",
                ManagementState::CopyOnly => "仅副本",
                ManagementState::Unmanaged => "未受管",
            };
            let source = conflict.source.as_deref().unwrap_or("未知");
            return Ok(Some(AppError::skill_conflict(
                name,
                format!("{management}；现有来源：{source}"),
            )));
        }
    }
    Ok(None)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliSkillStatus {
    Installed,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliSkillResult {
    pub name: Option<String>,
    pub status: CliSkillStatus,
    pub error: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallOutput {
    Results(Vec<CliSkillResult>),
    /// A fatal preflight object without a `name` (clone failure, no SKILL.md,
    /// invalid agents, ...).
    Fatal(String),
    /// Not a JSON array, an empty array, or an unknown entry shape.
    Unrecognized(String),
}

/// Parse one `add --json` result and map infrastructure failures for the
/// action. Install and update share this mapping so their error precedence
/// cannot drift.
pub(crate) fn add_results(
    output: &ExecOutput,
    action: CliAction,
) -> Result<Vec<CliSkillResult>, AppError> {
    match parse_install_output(&output.stdout) {
        InstallOutput::Results(results) => Ok(results),
        InstallOutput::Fatal(detail) => {
            Err(cli::classify_add_failure(&detail, &output.stderr).to_app_error(action))
        }
        InstallOutput::Unrecognized(detail) => {
            let stderr = output.stderr.trim();
            let detail = if stderr.is_empty() {
                detail
            } else {
                format!("{detail}\n{stderr}")
            };
            Err(AppError::cli_invalid_json(detail))
        }
    }
}

pub fn parse_install_output(raw: &str) -> InstallOutput {
    let value: serde_json::Value = match serde_json::from_str(raw.trim()) {
        Ok(value) => value,
        Err(error) => {
            return InstallOutput::Unrecognized(format!("CLI 输出不是 JSON：{error}"));
        }
    };
    let Some(entries) = value.as_array() else {
        return InstallOutput::Unrecognized("CLI 输出不是 JSON 数组。".to_string());
    };
    if entries.is_empty() {
        return InstallOutput::Unrecognized("CLI 返回了空结果。".to_string());
    }

    let mut results = Vec::new();
    for entry in entries {
        let Some(object) = entry.as_object() else {
            return InstallOutput::Unrecognized("CLI 结果包含非对象条目。".to_string());
        };
        let Some(status) = object.get("status").and_then(serde_json::Value::as_str) else {
            return InstallOutput::Unrecognized("CLI 结果缺少 status。".to_string());
        };
        let name = object
            .get("name")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        let error = object
            .get("error")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        let reason = object
            .get("reason")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);

        match status {
            "installed" => {
                if name.is_none() {
                    return InstallOutput::Unrecognized("installed 结果缺少 name。".to_string());
                }
                results.push(CliSkillResult {
                    name,
                    status: CliSkillStatus::Installed,
                    error: None,
                    reason: None,
                });
            }
            "skipped" => {
                if name.is_none() {
                    return InstallOutput::Unrecognized("skipped 结果缺少 name。".to_string());
                }
                results.push(CliSkillResult {
                    name,
                    status: CliSkillStatus::Skipped,
                    error: None,
                    reason,
                });
            }
            "failed" => {
                if name.is_none() {
                    return InstallOutput::Fatal(
                        error.unwrap_or_else(|| {
                            "skills CLI 预检失败（未提供错误信息）。".to_string()
                        }),
                    );
                }
                results.push(CliSkillResult {
                    name,
                    status: CliSkillStatus::Failed,
                    error,
                    reason: None,
                });
            }
            other => {
                return InstallOutput::Unrecognized(format!("CLI 返回了未知状态：{other}"));
            }
        }
    }
    InstallOutput::Results(results)
}

/// Snapshot the scope lock into `backups/<ms>-install-<task>/` before the CLI
/// runs. Install is task-scoped, so the snapshot is lock-only (no canonical or
/// attachments) and carries the same manifest as every other backup. A missing
/// lock is a normal fresh-machine state and is recorded as absent.
pub fn snapshot_lock(
    paths: &AppPaths,
    layout: &crate::scope_layout::ScopeLayout,
    task_id: &str,
) -> Result<PathBuf, AppError> {
    let snapshot = BackupStore::for_layout(paths, layout).create(
        BackupAction::Install,
        task_id,
        None,
        &[],
        &[],
    )?;
    Ok(snapshot.path)
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InstallVerification {
    pub results: Vec<TaskSkillResult>,
    pub warnings: Vec<AppError>,
}

/// Verify every requested skill after the CLI exits. `installed` entries are
/// checked on disk; `failed`/`skipped` entries are authoritative for their own
/// outcome. Filesystem checks are best-effort and never panic.
pub fn verify_install(
    layout: &crate::scope_layout::ScopeLayout,
    plan: &InstallPlan,
    cli_results: &[CliSkillResult],
) -> InstallVerification {
    let by_name: HashMap<&str, &CliSkillResult> = cli_results
        .iter()
        .filter_map(|result| result.name.as_deref().map(|name| (name, result)))
        .collect();
    let lock = layout.lock();
    let mut verification = InstallVerification::default();

    for name in &plan.skills {
        let entry = by_name.get(name.as_str());
        let outcome = match entry {
            None => Err(AppError::verify_failed(format!(
                "CLI 未返回 skill「{name}」的结果。"
            ))),
            Some(result) => match result.status {
                CliSkillStatus::Installed => {
                    verify_installed(layout, plan, &lock, name, &mut verification.warnings)
                }
                CliSkillStatus::Skipped => Err(AppError::skill_not_in_source(
                    name,
                    result
                        .reason
                        .clone()
                        .unwrap_or_else(|| "CLI 跳过了该 skill。".to_string()),
                )),
                CliSkillStatus::Failed => Err(AppError::skill_install_failed(
                    name,
                    result
                        .error
                        .clone()
                        .unwrap_or_else(|| "CLI 报告安装失败。".to_string()),
                )),
            },
        };

        let (status, error) = match outcome {
            Ok(()) => (TaskSkillStatus::Installed, None),
            Err(error) if error.code == crate::error::ErrorCode::SkillNotInSource => {
                (TaskSkillStatus::Skipped, Some(error))
            }
            Err(error) => (TaskSkillStatus::Failed, Some(error)),
        };
        verification.results.push(TaskSkillResult {
            name: name.clone(),
            status,
            error,
            backup_path: None,
        });
    }
    verification
}

fn verify_installed(
    layout: &crate::scope_layout::ScopeLayout,
    plan: &InstallPlan,
    lock: &Result<ScopedLock, AppError>,
    name: &str,
    warnings: &mut Vec<AppError>,
) -> Result<(), AppError> {
    let canonical = layout.canonical_dir.join(name);
    match fs::symlink_metadata(&canonical) {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {}
        _ => return Err(AppError::verify_canonical_missing(name, &canonical)),
    }

    match lock {
        Ok(lock) => match lock.get(name) {
            None => return Err(AppError::verify_lock_missing(name)),
            Some(entry) => {
                // Update sources may carry `#ref`; the lock stores the base
                // repository. A different base means the CLI wrote a lock
                // entry for an unexpected repository.
                let expected = plan.source.split('#').next().unwrap_or(&plan.source);
                if super::source::source_identity(&entry.source)
                    != super::source::source_identity(expected)
                {
                    return Err(AppError::verify_lock_mismatch(
                        name,
                        expected,
                        &entry.source,
                    ));
                }
            }
        },
        Err(error) => {
            return Err(AppError::verify_failed(format!(
                "lock 无法读取：{}",
                error.message
            )));
        }
    }

    let canonical_resolved = fs::canonicalize(&canonical).unwrap_or_else(|_| canonical.clone());
    for target in plan.targets.iter().filter(|target| !target.universal) {
        let attachment = target.skills_dir.join(name);
        match fs::symlink_metadata(&attachment) {
            Err(_) => {
                return Err(AppError::verify_attachment_missing(
                    name,
                    &target.display_name,
                    &attachment,
                ));
            }
            Ok(metadata) if metadata.file_type().is_symlink() => {
                match fs::canonicalize(&attachment) {
                    Ok(resolved) if resolved == canonical_resolved => {}
                    Ok(resolved) => {
                        return Err(AppError::verify_attachment_mode(
                            name,
                            &target.display_name,
                            &attachment,
                            format!("符号链接指向 {}", resolved.display()),
                        ));
                    }
                    Err(error) => {
                        return Err(AppError::verify_attachment_mode(
                            name,
                            &target.display_name,
                            &attachment,
                            format!("符号链接无法解析：{error}"),
                        ));
                    }
                }
            }
            Ok(metadata) if metadata.file_type().is_dir() => {
                warnings.push(AppError::verify_copy_fallback(
                    name,
                    &target.display_name,
                    &attachment,
                ));
            }
            Ok(_) => {
                return Err(AppError::verify_attachment_mode(
                    name,
                    &target.display_name,
                    &attachment,
                    "既不是符号链接也不是目录",
                ));
            }
        }
    }
    Ok(())
}

/// Install as one session unit: lock snapshot and conflict re-check in
/// preflight, one `skills add`, then disk verification, baseline writes and
/// the lock snapshot as the task backup.
pub(crate) struct InstallAction {
    paths: AppPaths,
    layout: crate::scope_layout::ScopeLayout,
    timeout: Duration,
    task_id: String,
}

impl InstallAction {
    pub(crate) fn new(
        paths: AppPaths,
        layout: crate::scope_layout::ScopeLayout,
        timeout: Duration,
        task_id: String,
    ) -> Self {
        Self {
            paths,
            layout,
            timeout,
            task_id,
        }
    }
}

impl MutationAction for InstallAction {
    type Plan = InstallPlan;
    type Unit = InstallPlan;

    fn profile(&self, plan: &Self::Plan) -> SessionProfile {
        SessionProfile {
            action: TaskAction::Install,
            initial_message: "正在准备安装…".to_string(),
            success_status: TaskSkillStatus::Installed,
            total: plan.total(),
            cancellation: CancellationPolicy::SealOnly,
        }
    }

    fn begin(&self, plan: &Self::Plan) -> Result<StartOutcome<Self::Unit>, AppError> {
        Ok(StartOutcome {
            units: vec![plan.clone()],
            initial_rows: Vec::new(),
            log_path: None,
        })
    }

    fn preflight(&self, plan: &Self::Unit, _reporter: &Reporter) -> Result<Preflight, AppError> {
        // The app never writes the lock, but a corrupt lock for this scope must
        // block the install before the CLI spawns.
        self.layout.lock()?;
        let retry_source = plan.retry.then_some(plan.source.as_str());
        if let Some(conflict) =
            conflicts_for(&self.paths, &self.layout, &plan.skills, retry_source)?
        {
            return Err(conflict);
        }
        let snapshot = snapshot_lock(&self.paths, &self.layout, &self.task_id)?;
        Ok(Preflight {
            snapshot: snapshot.clone(),
            post_message: Some(ProgressMessage {
                phase: TaskPhase::Preparing,
                message: format!("已快照 lock：{}", self.paths.display(&snapshot)),
                skill: None,
                completed: 0,
            }),
            warnings: Vec::new(),
        })
    }

    fn command(&self, plan: &Self::Unit, runtime: &CliRuntime, reporter: &Reporter) -> CommandSpec {
        reporter.progress(TaskPhase::RunningCli, "正在执行 skills add…", None);
        ScopedCli::new(runtime, &self.layout).install(
            &plan.source,
            &plan.skills,
            &plan.agent_ids,
            self.timeout,
        )
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
            None,
        );
        let cli_results = add_results(&output, CliAction::Install)?;

        let installed_names: Vec<String> = cli_results
            .iter()
            .filter(|result| result.status == CliSkillStatus::Installed)
            .filter_map(|result| result.name.clone())
            .collect();
        let link_errors = ensure_extra_attachments(&self.layout, plan, &installed_names);
        let mut verification = verify_install(&self.layout, plan, &cli_results);
        for result in &mut verification.results {
            if let Some(error) = link_errors.get(&result.name) {
                result.status = TaskSkillStatus::Failed;
                result.error = Some(error.clone());
            }
        }
        for (index, result) in verification.results.iter().enumerate() {
            reporter.progress_at(
                TaskPhase::Verifying,
                format!("已校验 {}", result.name),
                index as u32 + 1,
                Some(result.name.as_str()),
            );
        }

        let mut warnings = verification.warnings;
        let baselines = BaselineStore::for_scope(&self.paths, &self.layout.scope);
        for result in &verification.results {
            if result.status != TaskSkillStatus::Installed {
                continue;
            }
            let canonical = self.layout.canonical_dir.join(&result.name);
            if let Err(error) = baselines.record_from_dir(&result.name, &canonical, true) {
                warnings.push(write_warning(error));
            }
        }

        Ok(UnitOutcome {
            rows: verification.results,
            warnings,
            backup_path: self.paths.display(&preflight.snapshot),
            final_progress: None,
        })
    }

    fn on_infrastructure_failure(
        &self,
        _unit: &Self::Unit,
        error: AppError,
        _preflight: Option<&Preflight>,
        _reporter: &Reporter,
    ) -> FailureDisposition {
        FailureDisposition::TaskFailed(error)
    }

    fn on_cancelled(&self, _unit: &Self::Unit, _preflight: &Preflight, _error: Option<AppError>) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use std::collections::BTreeMap;
    use std::path::Path;

    /// Global-scope shims for the pre-scope test fixtures.
    fn validate_request(
        paths: &AppPaths,
        request: &InstallRequest,
    ) -> Result<InstallPlan, AppError> {
        let layout = crate::scope_layout::ScopeLayout::global(
            paths,
            crate::inventory::detected_agent_dirs(paths),
        );
        super::validate_request(paths, &layout, request)
    }

    fn verify_install(
        paths: &AppPaths,
        plan: &InstallPlan,
        cli_results: &[CliSkillResult],
    ) -> InstallVerification {
        let layout = crate::scope_layout::ScopeLayout::global(paths, Vec::new());
        super::verify_install(&layout, plan, cli_results)
    }

    fn conflicts_for(
        paths: &AppPaths,
        skills: &[String],
        retry_source: Option<&str>,
    ) -> Result<Option<AppError>, AppError> {
        let layout = crate::scope_layout::ScopeLayout::global(paths, Vec::new());
        super::conflicts_for(paths, &layout, skills, retry_source)
    }

    fn ensure_extra_attachments(
        paths: &AppPaths,
        plan: &InstallPlan,
        installed: &[String],
    ) -> HashMap<String, AppError> {
        let layout = crate::scope_layout::ScopeLayout::global(paths, Vec::new());
        super::ensure_extra_attachments(&layout, plan, installed)
    }

    const SUCCESS: &str = include_str!("../../tests/fixtures/cli/install-success.txt");
    const PARTIAL: &str = include_str!("../../tests/fixtures/cli/install-partial.txt");
    const COPY: &str = include_str!("../../tests/fixtures/cli/install-copy.txt");
    const FAILED: &str = include_str!("../../tests/fixtures/cli/install-failed.txt");
    const EMPTY: &str = include_str!("../../tests/fixtures/cli/install-empty.txt");
    const FATAL_CLONE: &str = include_str!("../../tests/fixtures/cli/install-fatal-clone.txt");
    const NO_SKILLS: &str = include_str!("../../tests/fixtures/cli/install-noskills.txt");

    fn runtime() -> CliRuntime {
        CliRuntime {
            npx_path: PathBuf::from("/node/bin/npx"),
            node_dir: PathBuf::from("/node/bin"),
            git_dir: Some(PathBuf::from("/usr/bin")),
            cwd: PathBuf::from("/app-data/cli-cwd"),
            home: PathBuf::from("/Users/tester"),
            env: BTreeMap::from([
                ("CI".to_string(), "true".to_string()),
                ("DISABLE_TELEMETRY".to_string(), "1".to_string()),
            ]),
            pinned_version: crate::cli::CLI_VERSION.to_string(),
        }
    }

    fn request(source: &str, skills: &[&str], agents: &[&str]) -> InstallRequest {
        InstallRequest {
            source: source.to_string(),
            skills: skills.iter().map(|value| value.to_string()).collect(),
            agents: agents.iter().map(|value| value.to_string()).collect(),
            retry: false,
        }
    }

    fn fixture() -> (tempfile::TempDir, AppPaths) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(home.join(".qoder")).unwrap();
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        (tmp, paths)
    }

    fn plan(skills: &[&str], agents: &[&str]) -> InstallPlan {
        InstallPlan {
            source: "acme/skills".to_string(),
            skills: skills.iter().map(|value| value.to_string()).collect(),
            agent_ids: agents
                .iter()
                .map(|value| value.to_string())
                .chain(std::iter::once(UNIVERSAL_AGENT_ID.to_string()))
                .collect(),
            targets: vec![InstallTarget {
                id: "qoder".to_string(),
                display_name: "Qoder".to_string(),
                universal: false,
                skills_dir: PathBuf::from("/Users/tester/.qoder/skills"),
            }],
            retry: false,
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

    fn install_state(paths: &AppPaths, name: &str, agent_dir: Option<&Path>, copy: bool) {
        let canonical = paths.canonical_skills_dir().join(name);
        fs::create_dir_all(&canonical).unwrap();
        fs::write(canonical.join("SKILL.md"), format!("# {name}\n")).unwrap();
        if let Some(agent_dir) = agent_dir {
            fs::create_dir_all(agent_dir).unwrap();
            let attachment = agent_dir.join(name);
            if copy {
                fs::create_dir_all(&attachment).unwrap();
            } else {
                #[cfg(unix)]
                std::os::unix::fs::symlink(&canonical, &attachment).unwrap();
            }
        }
    }

    fn verification_plan(home: &Path, skills: &[&str], agents: &[&str]) -> InstallPlan {
        let ids: Vec<String> = agents.iter().map(|value| value.to_string()).collect();
        let targets = resolve_agent_dirs(&AppPaths::new(home, home.join("app-data")), &ids)
            .into_iter()
            .map(|agent| InstallTarget {
                id: agent.id,
                display_name: agent.display_name,
                universal: agent.universal,
                skills_dir: agent.skills_dir,
            })
            .collect();
        InstallPlan {
            source: "acme/skills".to_string(),
            skills: skills.iter().map(|value| value.to_string()).collect(),
            agent_ids: agents
                .iter()
                .map(|value| value.to_string())
                .chain(std::iter::once(UNIVERSAL_AGENT_ID.to_string()))
                .collect(),
            targets,
            retry: false,
        }
    }

    #[test]
    fn install_command_is_exact_and_never_uses_forbidden_flags() {
        let (_tmp, paths) = fixture();
        let layout = crate::scope_layout::ScopeLayout::global(&paths, Vec::new());
        let plan = plan(&["find-skills", "to-spec"], &["claude-code"]);
        let spec = ScopedCli::new(&runtime(), &layout).install(
            &plan.source,
            &plan.skills,
            &plan.agent_ids,
            Duration::from_secs(900),
        );
        assert_eq!(spec.program, PathBuf::from("/node/bin/npx"));
        assert_eq!(
            spec.args,
            vec![
                "-y",
                "skills@latest",
                "add",
                "acme/skills",
                "-g",
                "-s",
                "find-skills",
                "-s",
                "to-spec",
                "-a",
                "claude-code",
                "-a",
                "universal",
                "-y",
                "--json"
            ]
        );
        assert_eq!(spec.cwd, PathBuf::from("/app-data/cli-cwd"));
        assert_eq!(spec.env["CI"], "true");
        assert_eq!(spec.timeout, Duration::from_secs(900));
        for forbidden in ["--all", "--copy", "*", "update", "check", "remove", "list"] {
            assert!(
                !spec.args.iter().any(|arg| arg == forbidden),
                "{forbidden} must never be part of install"
            );
        }
        assert_eq!(spec.args[2], "add");
        assert_eq!(spec.args.last().map(String::as_str), Some("--json"));
        assert_eq!(
            spec.args.iter().filter(|arg| *arg == "-a").count(),
            2,
            "exactly one -a per selected agent plus universal"
        );
    }

    #[test]
    fn validation_normalizes_source_dedupes_and_appends_universal() {
        let (_tmp, paths) = fixture();
        let plan = validate_request(
            &paths,
            &request(
                "https://github.com/acme/skills/",
                &["alpha", "alpha", "beta"],
                &["qoder", "universal", "qoder"],
            ),
        )
        .unwrap();
        assert_eq!(plan.source, "acme/skills");
        assert_eq!(plan.skills, vec!["alpha", "beta"]);
        assert_eq!(plan.agent_ids, vec!["qoder", "universal"]);
        assert_eq!(plan.targets.len(), 1);
        assert_eq!(
            plan.targets[0].skills_dir,
            paths.home().join(".qoder/skills")
        );
    }

    #[test]
    fn validation_rejects_copy_all_wildcard_and_flag_like_names() {
        let (_tmp, paths) = fixture();
        for forbidden in [
            "--copy", "--all", "*", "-s", "a/b", "..", "", "alpha;rm", "$(id)", "`id`", "a|b",
            "a&b", "a>b", "a<b", "a\"b", "a'b", "a(b)", "a*b", "a?b", "a{b}", "a[b]", "a~b", "a#b",
        ] {
            let error = validate_request(&paths, &request("acme/skills", &[forbidden], &["qoder"]))
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::SkillInvalidName, "{forbidden:?}");
        }
        let error = validate_request(&paths, &request("acme/skills", &[], &["qoder"])).unwrap_err();
        assert_eq!(error.code, ErrorCode::SkillInvalidName);
    }

    #[test]
    fn validation_rejects_unknown_wildcard_and_unsupported_agents() {
        let (_tmp, paths) = fixture();
        for unknown in [
            "*",
            "--all",
            "not-an-agent",
            "-a",
            "qoder;rm",
            "../qoder",
            "qoder/../../x",
            "$(id)",
        ] {
            let error = validate_request(&paths, &request("acme/skills", &["alpha"], &[unknown]))
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::AgentInvalid, "{unknown}");
        }
        let error = validate_request(
            &paths,
            &request("acme/skills", &["alpha"], &["promptscript"]),
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::AgentUnsupportedGlobal);
        let error =
            validate_request(&paths, &request("acme/skills", &["alpha"], &["eve"])).unwrap_err();
        assert_eq!(error.code, ErrorCode::AgentUnsupportedGlobal);
    }

    #[test]
    fn validation_rejects_agent_that_is_not_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        let error =
            validate_request(&paths, &request("acme/skills", &["alpha"], &["qoder"])).unwrap_err();
        assert_eq!(error.code, ErrorCode::AgentNotDetected);
    }

    #[test]
    fn parser_reads_success_partial_failures_and_fatal_errors() {
        let InstallOutput::Results(results) = parse_install_output(SUCCESS) else {
            panic!("success must parse");
        };
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name.as_deref(), Some("find-skills"));
        assert_eq!(results[0].status, CliSkillStatus::Installed);

        // `mode: "copy"` is informational only; mount truth comes from lstat.
        let InstallOutput::Results(results) = parse_install_output(COPY) else {
            panic!("copy fixture must parse");
        };
        assert_eq!(results[0].status, CliSkillStatus::Installed);

        let InstallOutput::Results(results) = parse_install_output(PARTIAL) else {
            panic!("partial must parse");
        };
        assert_eq!(results[0].status, CliSkillStatus::Skipped);
        assert_eq!(
            results[0].reason.as_deref(),
            Some("No matching skill found in source")
        );
        assert_eq!(results[1].status, CliSkillStatus::Installed);

        let InstallOutput::Results(results) = parse_install_output(FAILED) else {
            panic!("per-skill failure must parse");
        };
        assert_eq!(results[0].status, CliSkillStatus::Failed);
        assert!(
            results[0]
                .error
                .as_deref()
                .unwrap()
                .contains("PromptScript")
        );

        let InstallOutput::Fatal(error) = parse_install_output(FATAL_CLONE) else {
            panic!("fatal clone failure must be fatal");
        };
        assert!(error.contains("Failed to clone"));

        let InstallOutput::Fatal(error) = parse_install_output(NO_SKILLS) else {
            panic!("no-skills preflight must be fatal");
        };
        assert!(error.contains("No valid skills"));

        assert!(matches!(
            parse_install_output(EMPTY),
            InstallOutput::Unrecognized(_)
        ));
        assert!(matches!(
            parse_install_output("garbage"),
            InstallOutput::Unrecognized(_)
        ));
        assert!(matches!(
            parse_install_output(r#"[{"status":"installed"}]"#),
            InstallOutput::Unrecognized(_)
        ));
        assert!(matches!(
            parse_install_output(r#"[{"name":"x","status":"weird"}]"#),
            InstallOutput::Unrecognized(_)
        ));
    }

    #[test]
    fn verifier_checks_canonical_lock_and_agent_link() {
        let (tmp, paths) = fixture();
        let plan = verification_plan(paths.home(), &["find-skills"], &["qoder"]);
        let agent_dir = tmp.path().join("home/.qoder/skills");
        install_state(&paths, "find-skills", Some(&agent_dir), false);
        write_lock(&paths, &["find-skills"]);

        let result = verify_install(&paths, &plan, &parse_results(SUCCESS));
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].status, TaskSkillStatus::Installed);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn verifier_fails_missing_canonical_attachment_and_lock() {
        let (tmp, paths) = fixture();
        let plan = verification_plan(paths.home(), &["find-skills"], &["qoder"]);
        let cli = parse_results(SUCCESS);

        let result = verify_install(&paths, &plan, &cli);
        assert_eq!(result.results[0].status, TaskSkillStatus::Failed);
        assert_eq!(
            result.results[0].error.as_ref().unwrap().code,
            ErrorCode::VerifyCanonicalMissing
        );

        let agent_dir = tmp.path().join("home/.qoder/skills");
        install_state(&paths, "find-skills", None, false);
        write_lock(&paths, &["find-skills"]);
        let result = verify_install(&paths, &plan, &cli);
        assert_eq!(
            result.results[0].error.as_ref().unwrap().code,
            ErrorCode::VerifyAttachmentMissing
        );

        let _ = fs::remove_dir_all(&agent_dir);
        fs::create_dir_all(&agent_dir).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(paths.home().join("missing"), agent_dir.join("find-skills"))
            .unwrap();
        let result = verify_install(&paths, &plan, &cli);
        assert_eq!(
            result.results[0].error.as_ref().unwrap().code,
            ErrorCode::VerifyAttachmentMode
        );
        #[cfg(unix)]
        fs::remove_file(agent_dir.join("find-skills")).unwrap();

        let result = verify_install(&paths, &plan, &cli);
        assert_eq!(
            result.results[0].error.as_ref().unwrap().code,
            ErrorCode::VerifyAttachmentMissing
        );

        install_state(&paths, "find-skills", Some(&agent_dir), false);
        fs::remove_file(paths.lock_path()).unwrap();
        let result = verify_install(&paths, &plan, &cli);
        assert_eq!(
            result.results[0].error.as_ref().unwrap().code,
            ErrorCode::VerifyLockMissing
        );
    }

    #[test]
    fn verifier_reports_copy_fallback_as_a_warning() {
        let (tmp, paths) = fixture();
        let plan = verification_plan(paths.home(), &["find-skills"], &["qoder"]);
        let agent_dir = tmp.path().join("home/.qoder/skills");
        install_state(&paths, "find-skills", Some(&agent_dir), true);
        write_lock(&paths, &["find-skills"]);

        let result = verify_install(&paths, &plan, &parse_results(SUCCESS));
        assert_eq!(result.results[0].status, TaskSkillStatus::Installed);
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(result.warnings[0].code, ErrorCode::VerifyCopyFallback);
    }

    #[test]
    fn verifier_marks_skipped_and_cli_failed_entries_without_checking_disk() {
        let (_tmp, paths) = fixture();
        let plan = verification_plan(paths.home(), &["no-such-skill", "find-skills"], &["qoder"]);
        let result = verify_install(&paths, &plan, &parse_results(PARTIAL));
        assert_eq!(result.results[0].name, "no-such-skill");
        assert_eq!(result.results[0].status, TaskSkillStatus::Skipped);
        assert_eq!(
            result.results[0].error.as_ref().unwrap().code,
            ErrorCode::SkillNotInSource
        );
        assert_eq!(result.results[1].status, TaskSkillStatus::Failed);
        assert_eq!(
            result.results[1].error.as_ref().unwrap().code,
            ErrorCode::VerifyCanonicalMissing
        );
    }

    #[test]
    fn retry_permits_only_same_source_managed_or_stale_names() {
        let (_tmp, paths) = fixture();
        install_state(&paths, "alpha", None, false);
        write_lock(&paths, &["alpha"]);
        // Plain duplicate is always rejected.
        let error = conflicts_for(&paths, &["alpha".to_string()], None)
            .unwrap()
            .expect("plain duplicate must conflict");
        assert_eq!(error.code, ErrorCode::SkillConflict);
        // A retry from the same source may overwrite the managed entry.
        assert!(
            conflicts_for(&paths, &["alpha".to_string()], Some("acme/skills"))
                .unwrap()
                .is_none()
        );
        // A different source stays a conflict even for a retry.
        let error = conflicts_for(&paths, &["alpha".to_string()], Some("other/repo"))
            .unwrap()
            .expect("different source must conflict");
        assert_eq!(error.code, ErrorCode::SkillConflict);

        // Stale lock entries reserve the name too, but a same-source retry is
        // the partial-install case this flag exists for.
        fs::remove_dir_all(paths.canonical_skills_dir()).unwrap();
        let error = conflicts_for(&paths, &["alpha".to_string()], None)
            .unwrap()
            .expect("stale lock must conflict");
        assert_eq!(error.code, ErrorCode::SkillConflict);
        assert!(
            conflicts_for(&paths, &["alpha".to_string()], Some("acme/skills"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn verifier_rejects_a_lock_entry_from_a_different_source() {
        let (tmp, paths) = fixture();
        let plan = verification_plan(paths.home(), &["find-skills"], &["qoder"]);
        let agent_dir = tmp.path().join("home/.qoder/skills");
        install_state(&paths, "find-skills", Some(&agent_dir), false);
        write_lock(&paths, &["find-skills"]);
        let raw = fs::read_to_string(paths.lock_path())
            .unwrap()
            .replace("acme/skills", "other/repo");
        fs::write(paths.lock_path(), raw).unwrap();

        let result = verify_install(&paths, &plan, &parse_results(SUCCESS));
        assert_eq!(result.results[0].status, TaskSkillStatus::Failed);
        assert_eq!(
            result.results[0].error.as_ref().unwrap().code,
            ErrorCode::VerifyLockMismatch
        );
    }

    #[test]
    fn validation_accepts_detected_extra_agents_without_cli_args() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let workbook = home.join(".workbuddy");
        fs::create_dir_all(workbook.join("skills")).unwrap();
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));

        let plan =
            validate_request(&paths, &request("acme/skills", &["alpha"], &["workbuddy"])).unwrap();
        assert_eq!(plan.agent_ids, vec![UNIVERSAL_AGENT_ID.to_string()]);
        assert_eq!(plan.targets.len(), 1);
        assert_eq!(plan.targets[0].id, "workbuddy");
        assert_eq!(plan.targets[0].skills_dir, workbook.join("skills"));
        let layout = crate::scope_layout::ScopeLayout::global(&paths, Vec::new());

        let spec = ScopedCli::new(&runtime(), &layout).install(
            &plan.source,
            &plan.skills,
            &plan.agent_ids,
            Duration::from_secs(900),
        );
        assert!(
            !spec.args.iter().any(|arg| arg == "workbuddy"),
            "extra agents must never reach the CLI"
        );

        fs::remove_dir_all(home.join(".workbuddy")).unwrap();
        let error = validate_request(&paths, &request("acme/skills", &["alpha"], &["workbuddy"]))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::AgentNotDetected);
    }

    #[test]
    fn extra_attachments_are_linked_natively_and_never_overwritten() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let workbook = home.join(".workbuddy/skills");
        fs::create_dir_all(&workbook).unwrap();
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        install_state(&paths, "find-skills", None, false);

        let plan = InstallPlan {
            source: "acme/skills".to_string(),
            skills: vec!["find-skills".to_string()],
            agent_ids: vec![UNIVERSAL_AGENT_ID.to_string()],
            targets: vec![InstallTarget {
                id: "workbuddy".to_string(),
                display_name: "WorkBuddy".to_string(),
                universal: false,
                skills_dir: workbook.clone(),
            }],
            retry: false,
        };

        let errors = ensure_extra_attachments(&paths, &plan, &["find-skills".to_string()]);
        assert!(errors.is_empty(), "{errors:?}");
        let link = workbook.join("find-skills");
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::canonicalize(&link).unwrap(),
            fs::canonicalize(paths.canonical_skills_dir().join("find-skills")).unwrap()
        );

        // Idempotent on the second run.
        assert!(ensure_extra_attachments(&paths, &plan, &["find-skills".to_string()]).is_empty());

        // A foreign directory at the same name is reported, not overwritten.
        let foreign = paths.canonical_skills_dir().join("other");
        fs::create_dir_all(&foreign).unwrap();
        fs::create_dir_all(workbook.join("other")).unwrap();
        let other_plan = InstallPlan {
            source: "acme/skills".to_string(),
            skills: vec!["other".to_string()],
            agent_ids: vec![UNIVERSAL_AGENT_ID.to_string()],
            targets: vec![InstallTarget {
                id: "workbuddy".to_string(),
                display_name: "WorkBuddy".to_string(),
                universal: false,
                skills_dir: workbook.clone(),
            }],
            retry: false,
        };
        let errors = ensure_extra_attachments(&paths, &other_plan, &["other".to_string()]);
        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors[&"other".to_string()].code,
            ErrorCode::AgentAttachFailed
        );
    }

    #[test]
    fn project_install_runs_in_the_root_without_global_and_keeps_universal() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let root = tmp.path().join("projects/app");
        fs::create_dir_all(home.join(".claude/skills")).unwrap();
        fs::create_dir_all(root.join(".agents/skills")).unwrap();
        fs::create_dir_all(root.join(".claude/skills")).unwrap();
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        let project = crate::projects::Project {
            id: uuid::Uuid::new_v4(),
            root: root.clone(),
            display_name: "app".to_string(),
            added_at: 1,
            last_opened_at: 1,
            check_revision: "initial".into(),
        };
        let layout = crate::scope_layout::ScopeLayout::project(&paths, project);

        let plan = super::validate_request(
            &paths,
            &layout,
            &request("acme/skills", &["alpha"], &["claude-code"]),
        )
        .unwrap();
        assert_eq!(
            plan.agent_ids,
            vec!["claude-code".to_string(), UNIVERSAL_AGENT_ID.to_string()]
        );
        assert_eq!(
            plan.targets[0].skills_dir,
            layout.root.join(".claude/skills")
        );

        let spec = ScopedCli::new(&runtime(), &layout).install(
            &plan.source,
            &plan.skills,
            &plan.agent_ids,
            Duration::from_secs(900),
        );
        assert_eq!(
            spec.args,
            vec![
                "-y",
                "skills@latest",
                "add",
                "acme/skills",
                "-s",
                "alpha",
                "-a",
                "claude-code",
                "-a",
                "universal",
                "-y",
                "--json"
            ]
        );
        assert_eq!(spec.cwd, layout.root);
        assert!(!spec.args.iter().any(|arg| arg == "-g"));
    }

    #[test]
    fn project_workbuddy_install_uses_native_attachments_after_canonical_cli_install() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(home.join(".workbuddy")).unwrap();
        let root = tmp.path().join("project");
        fs::create_dir_all(&root).unwrap();
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        let project = crate::projects::Project {
            id: uuid::Uuid::new_v4(),
            root,
            display_name: "project".to_string(),
            added_at: 1,
            last_opened_at: 1,
            check_revision: "initial".into(),
        };
        let layout = crate::scope_layout::ScopeLayout::project(&paths, project);
        let plan = super::validate_request(
            &paths,
            &layout,
            &request("acme/skills", &["alpha"], &["workbuddy"]),
        )
        .unwrap();
        assert_eq!(plan.targets.len(), 1);
        assert_eq!(plan.targets[0].id, "workbuddy");
        assert_eq!(plan.targets[0].display_name, "WorkBuddy");
        assert_eq!(
            plan.targets[0].skills_dir,
            layout.root.join(".workbuddy/skills")
        );
        assert!(!plan.targets[0].skills_dir.exists());
        let spec = ScopedCli::new(&runtime(), &layout).install(
            &plan.source,
            &plan.skills,
            &plan.agent_ids,
            Duration::from_secs(900),
        );
        assert_eq!(spec.cwd, layout.root);
        assert_eq!(
            spec.args,
            vec![
                "-y",
                "skills@latest",
                "add",
                "acme/skills",
                "-s",
                "alpha",
                "-a",
                "universal",
                "-y",
                "--json",
            ]
        );
        let canonical = layout.canonical_dir.join("alpha");
        fs::create_dir_all(&canonical).unwrap();
        fs::write(canonical.join("SKILL.md"), "# alpha\n").unwrap();
        let errors = super::ensure_extra_attachments(&layout, &plan, &["alpha".to_string()]);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            fs::read_link(layout.root.join(".workbuddy/skills/alpha")).unwrap(),
            canonical
        );
        assert!(!layout.root.join(".codebuddy/skills").exists());
    }

    #[test]
    fn install_lock_snapshots_are_scope_isolated() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let root = tmp.path().join("projects/app");
        fs::create_dir_all(root.join(".agents/skills")).unwrap();
        fs::write(
            root.join("skills-lock.json"),
            r#"{"version":1,"skills":{}}"#,
        )
        .unwrap();
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        paths.ensure_dirs().unwrap();
        fs::create_dir_all(paths.agents_root()).unwrap();
        fs::write(paths.lock_path(), r#"{"version":3,"skills":{}}"#).unwrap();

        let global = crate::scope_layout::ScopeLayout::global(&paths, Vec::new());
        let global_snapshot = super::snapshot_lock(&paths, &global, "task-global").unwrap();
        assert_eq!(global_snapshot.parent().unwrap(), paths.backups_dir());
        assert!(global_snapshot.join("skill-lock.json").is_file());
        assert!(!global_snapshot.to_string_lossy().contains("project-"));
        let manifest: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(global_snapshot.join(crate::backup::BACKUP_MANIFEST_FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["action"], "install");

        let project = crate::projects::Project {
            id: uuid::Uuid::new_v4(),
            root: root.clone(),
            display_name: "app".to_string(),
            added_at: 1,
            last_opened_at: 1,
            check_revision: "initial".into(),
        };
        let layout = crate::scope_layout::ScopeLayout::project(&paths, project);
        let project_snapshot = super::snapshot_lock(&paths, &layout, "task-project").unwrap();
        let project_root = paths
            .backups_dir()
            .join(format!("project-{}", layout.scope.project_id().unwrap()));
        assert_eq!(project_snapshot.parent().unwrap(), project_root);
        assert!(project_snapshot.join("skills-lock.json").is_file());
        assert!(!project_snapshot.join("skill-lock.json").exists());
    }

    #[test]
    fn add_results_appends_stderr_to_unrecognized_output() {
        let output = ExecOutput {
            code: Some(0),
            signal: None,
            stdout: "not json".to_string(),
            stderr: "  npm warning  ".to_string(),
            duration: Duration::ZERO,
        };
        let error = super::add_results(&output, CliAction::Update).unwrap_err();
        assert_eq!(error.code, ErrorCode::CliInvalidJson);
        let detail = error.detail.unwrap();
        assert!(detail.contains("CLI 输出不是 JSON"));
        assert!(detail.contains("npm warning"));
        assert!(!detail.ends_with('\n'));
    }

    #[test]
    fn project_extra_link_is_created_inside_the_project_and_verified() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let root = tmp.path().join("projects/app");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(root.join(".agents/skills")).unwrap();
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
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
        let project = crate::projects::Project {
            id: uuid::Uuid::new_v4(),
            root: root.clone(),
            display_name: "app".to_string(),
            added_at: 1,
            last_opened_at: 1,
            check_revision: "initial".into(),
        };
        let layout = crate::scope_layout::ScopeLayout::project(&paths, project);
        let canonical = layout.canonical_dir.join("find-skills");
        fs::create_dir_all(&canonical).unwrap();
        fs::write(canonical.join("SKILL.md"), "# find-skills\n").unwrap();
        fs::write(
            layout.lock_path.clone(),
            serde_json::json!({
                "version": 1,
                "skills": {
                    "find-skills": {
                        "source": "acme/skills",
                        "sourceType": "github",
                        "sourceUrl": "https://github.com/acme/skills.git",
                        "skillPath": "skills/find-skills/SKILL.md",
                        "computedHash": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    }
                }
            })
            .to_string(),
        )
        .unwrap();

        let extra_dir = layout.root.join(".acme/skills");
        assert!(!extra_dir.exists());
        let plan = InstallPlan {
            source: "acme/skills".to_string(),
            skills: vec!["find-skills".to_string()],
            agent_ids: vec![UNIVERSAL_AGENT_ID.to_string()],
            targets: vec![InstallTarget {
                id: "acme".to_string(),
                display_name: "Acme".to_string(),
                universal: false,
                skills_dir: extra_dir.clone(),
            }],
            retry: false,
        };

        let errors = super::ensure_extra_attachments(&layout, &plan, &["find-skills".to_string()]);
        assert!(errors.is_empty(), "{errors:?}");
        let link = extra_dir.join("find-skills");
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::canonicalize(&link).unwrap(),
            fs::canonicalize(&canonical).unwrap()
        );
        assert!(
            !paths.canonical_skills_dir().exists(),
            "global store untouched"
        );

        let results = vec![CliSkillResult {
            name: Some("find-skills".to_string()),
            status: CliSkillStatus::Installed,
            error: None,
            reason: None,
        }];
        let verification = super::verify_install(&layout, &plan, &results);
        assert_eq!(
            verification.results[0].status,
            TaskSkillStatus::Installed,
            "{:?}",
            verification.results[0].error
        );
    }

    fn parse_results(raw: &str) -> Vec<CliSkillResult> {
        match parse_install_output(raw) {
            InstallOutput::Results(results) => results,
            other => panic!("expected results, got {other:?}"),
        }
    }
}
