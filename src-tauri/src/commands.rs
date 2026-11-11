//! Tauri command boundary. Commands are thin: validate nothing from the
//! frontend beyond typed inputs, delegate to services, return typed results.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::AppState;
use crate::agents::extras::{ExtraAgent, RemoveExtraAgentResult, UserExtraAgent};
use crate::app_update::Snapshot as AppUpdateSnapshot;
use crate::attach::AttachResult;
use crate::content::{self, SkillDirListing, SkillFileContent};
use crate::detach::DetachResult;
use crate::detail::SkillDetail;
use crate::env::EnvReport;
use crate::error::AppError;
use crate::inventory::InventorySnapshot;
use crate::paths::AppPaths;
use crate::projects::{ProjectEntry, ProjectList};
use crate::scope::ScopeId;
use crate::tasks::{ActiveMutationInfo, CancelAck, EventSink, InstallRequest, TaskEvent, TaskId};
use crate::timeouts::READ_TIMEOUT;
use crate::update::UpdateCheckResponse;
use crate::update::project_checks::{ForgetProjectResult, RelocateProjectResult};
use crate::update::state::UpdateCheckState;

#[tauri::command]
pub async fn get_app_data(state: State<'_, AppState>) -> Result<crate::app_data::Report, AppError> {
    let service = state.app_data.clone();
    let paths = state.paths.clone();
    let settings = state.settings.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut report = service.report(&paths)?;
        report.preferences = settings.data_preferences()?;
        Ok(report)
    })
    .await
    .map_err(|e| crate::app_data::error(e.to_string()))?
}

#[tauri::command]
pub fn save_data_preferences(
    state: State<'_, AppState>,
    preferences: crate::settings::DataPreferences,
) -> Result<crate::settings::DataPreferences, AppError> {
    let _write = state.app_updates.configuration_write()?;
    state.settings.save_data_preferences(preferences)
}

#[tauri::command]
pub async fn preview_app_data_cleanup(
    state: State<'_, AppState>,
    selection: crate::app_data::Selection,
) -> Result<crate::app_data::Preview, AppError> {
    let service = state.app_data.clone();
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || service.preview(&paths, selection))
        .await
        .map_err(|e| crate::app_data::error(e.to_string()))?
}

#[tauri::command]
pub async fn clean_app_data(
    state: State<'_, AppState>,
    preview_id: String,
) -> Result<crate::app_data::CleanupResult, AppError> {
    let permit = state.tasks.try_acquire_mutation()?;
    let cleanup = state.paths.data_coordination.cleanup()?;
    let configuration = state.app_updates.configuration_write()?;
    let service = state.app_data.clone();
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (_permit, _cleanup, _configuration) = (permit, cleanup, configuration);
        service.clean(&paths, &preview_id)
    })
    .await
    .map_err(|e| crate::app_data::error(e.to_string()))?
}

#[tauri::command]
pub async fn reveal_backup(
    state: State<'_, AppState>,
    app: AppHandle,
    id: String,
) -> Result<(), AppError> {
    let service = state.app_data.clone();
    let paths = state.paths.clone();
    let path = tauri::async_runtime::spawn_blocking(move || service.backup_directory(&paths, &id))
        .await
        .map_err(|e| crate::app_data::error(e.to_string()))??;
    app.opener()
        .open_path(path.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| AppError::open_failed(e.to_string()))
}

#[tauri::command]
pub fn get_github_settings(
    state: State<'_, AppState>,
) -> Result<crate::settings::GitHubSettingsStatus, AppError> {
    state.settings.status()
}

#[tauri::command]
pub fn save_github_token(
    state: State<'_, AppState>,
    token: String,
) -> Result<crate::settings::GitHubSettingsStatus, AppError> {
    let _write = state.app_updates.configuration_write()?;
    state.settings.save(&token)
}

#[tauri::command]
pub fn clear_github_token(
    state: State<'_, AppState>,
) -> Result<crate::settings::GitHubSettingsStatus, AppError> {
    let _write = state.app_updates.configuration_write()?;
    state.settings.clear()
}

#[tauri::command]
pub async fn test_github_auth(
    state: State<'_, AppState>,
) -> Result<crate::update::api::GitHubAuthTestResult, AppError> {
    let result = state
        .github_api
        .test_auth()
        .await
        .map_err(|error| error.to_app_error())?;
    state
        .discovery
        .auth_succeeded(&result.context, result.revision, result.remaining);
    Ok(result)
}

/// Forward task events to the renderer. A closed window must not fail a
/// read-only task or abort a mutation that may already be writing; the task
/// finishes and its result is discarded.
fn task_event_sink(on_event: Channel<TaskEvent>) -> EventSink {
    Arc::new(move |event| {
        let _ = on_event.send(event);
    })
}

#[tauri::command]
pub fn get_cli_commands(state: State<'_, AppState>) -> Vec<crate::cli::commands::CliCommandRecord> {
    state.cli_commands.records()
}

#[tauri::command]
pub async fn preview_cli_commands(
    state: State<'_, AppState>,
    scope: ScopeId,
    request: crate::cli::preview::CliCommandRequest,
) -> Result<crate::cli::preview::CliCommandPreview, AppError> {
    state.tasks.preview_cli_commands(scope, request).await
}

/// Run one read operation under the spec's 30-second hard cap. A stuck local
/// scan or IPC bridge becomes a retryable `skill.read_timeout` instead of a
/// spinner that never resolves.
pub(crate) async fn read_with_timeout<T, F>(
    what: &str,
    timeout: Duration,
    future: F,
) -> Result<T, AppError>
where
    F: std::future::Future<Output = Result<T, AppError>>,
{
    match tokio::time::timeout(timeout, future).await {
        Ok(result) => result,
        Err(_) => Err(AppError::skill_read_timeout(what, timeout)),
    }
}

#[tauri::command]
pub async fn get_environment(
    state: State<'_, AppState>,
    refresh: Option<bool>,
) -> Result<EnvReport, AppError> {
    let refresh = refresh.unwrap_or(false);
    if refresh {
        state.read_model.invalidate();
    }
    state.environment.get(refresh).await
}

/// Registry snapshot with per-entry availability and the restored active
/// scope. A corrupt registry is an empty list plus an app-level warning; the
/// global scope stays usable.
#[tauri::command]
pub fn list_projects(state: State<'_, AppState>) -> ProjectList {
    state.projects.list()
}

#[tauri::command]
pub fn add_project(state: State<'_, AppState>, root: String) -> Result<ProjectEntry, AppError> {
    let _write = state.app_updates.configuration_write()?;
    state.projects.add(&root)
}

#[tauri::command]
pub fn relocate_project(
    state: State<'_, AppState>,
    id: String,
    root: String,
) -> Result<RelocateProjectResult, AppError> {
    let _change = state.tasks.begin_registry_change()?;
    let result = state.project_updates.relocate(&id, &root)?;
    state.discovery.refresh();
    Ok(result)
}

#[tauri::command]
pub fn forget_project(
    state: State<'_, AppState>,
    id: String,
) -> Result<ForgetProjectResult, AppError> {
    let _change = state.tasks.begin_registry_change()?;
    let result = state.project_updates.forget(&id)?;
    state.discovery.refresh();
    Ok(result)
}

#[tauri::command]
pub fn set_active_scope(state: State<'_, AppState>, scope: ScopeId) -> Result<(), AppError> {
    let _write = state.app_updates.configuration_write()?;
    state.projects.set_active_scope(&scope)
}

/// Full read-model projection. Filesystem scanning happens on the blocking
/// pool so a large inventory never stalls the UI thread, under a 30s cap.
#[tauri::command]
pub async fn get_inventory(
    state: State<'_, AppState>,
    scope: ScopeId,
) -> Result<InventorySnapshot, AppError> {
    let read_model = Arc::clone(&state.read_model);
    blocking_read("库存", move || read_model.snapshot(&scope)).await
}

#[tauri::command]
pub async fn get_skill_detail(
    state: State<'_, AppState>,
    scope: ScopeId,
    skill_id: String,
) -> Result<SkillDetail, AppError> {
    let read_model = Arc::clone(&state.read_model);
    blocking_read("skill 详情", move || read_model.detail(&scope, &skill_id)).await
}

/// Lazy directory listing for the content tab. The frontend sends a skill id
/// and a relative path only; the skill root is resolved on the server.
#[tauri::command]
pub async fn list_skill_dir(
    state: State<'_, AppState>,
    scope: ScopeId,
    skill_id: String,
    path: String,
) -> Result<SkillDirListing, AppError> {
    let paths = state.paths.clone();
    let read_model = Arc::clone(&state.read_model);
    blocking_read("skill 内容目录", move || {
        let root = read_model.resolve_root(&scope, &skill_id)?;
        content::list(&paths, &root, &path)
    })
    .await
}

/// Read-only file preview. Oversized, binary and unreadable files are values,
/// not errors, so the preview can explain the state.
#[tauri::command]
pub async fn read_skill_file(
    state: State<'_, AppState>,
    scope: ScopeId,
    skill_id: String,
    path: String,
) -> Result<SkillFileContent, AppError> {
    let paths = state.paths.clone();
    let read_model = Arc::clone(&state.read_model);
    blocking_read("skill 文件", move || {
        let root = read_model.resolve_root(&scope, &skill_id)?;
        content::read_file(&paths, &root, &path)
    })
    .await
}

#[tauri::command]
pub async fn open_skill_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    scope: ScopeId,
    skill_id: String,
) -> Result<(), AppError> {
    let read_model = Arc::clone(&state.read_model);
    let resolved = blocking_read("skill 路径", move || {
        read_model.resolve_root(&scope, &skill_id)
    })
    .await?;
    app.opener()
        .open_path(resolved.to_string_lossy().to_string(), None::<&str>)
        .map_err(|error| AppError::open_failed(error.to_string()))
}

#[tauri::command]
pub async fn open_skill_file(
    app: AppHandle,
    state: State<'_, AppState>,
    scope: ScopeId,
    skill_id: String,
    path: String,
) -> Result<(), AppError> {
    let read_model = Arc::clone(&state.read_model);
    let resolved = blocking_read("skill 文件路径", move || {
        let root = read_model.resolve_root(&scope, &skill_id)?;
        content::resolve_open_path(&root, &path)
    })
    .await?;
    app.opener()
        .open_path(resolved.to_string_lossy().to_string(), None::<&str>)
        .map_err(|error| AppError::open_failed(error.to_string()))
}

/// External links are the only URL surface. The frontend never gets the
/// opener plugin permission; every link is re-validated here and must be an
/// absolute https URL with a host. http, file, javascript, data, ssh, mailto
/// and relative targets are rejected.
pub fn validate_external_url(raw: &str) -> Result<(), AppError> {
    if raw.is_empty() || raw.trim() != raw {
        return Err(AppError::open_url_rejected("链接包含首尾空白或为空。"));
    }
    let parsed = reqwest::Url::parse(raw)
        .map_err(|error| AppError::open_url_rejected(format!("无法解析链接：{error}")))?;
    if parsed.scheme() != "https" {
        return Err(AppError::open_url_rejected(format!(
            "协议为 {}。",
            parsed.scheme()
        )));
    }
    if parsed.host_str().is_none() {
        return Err(AppError::open_url_rejected("链接缺少主机名。"));
    }
    if raw
        .split_once("://")
        .is_some_and(|(_, rest)| rest.starts_with('/'))
    {
        return Err(AppError::open_url_rejected("链接缺少主机名。"));
    }
    Ok(())
}

#[tauri::command]
pub fn open_external_url(app: AppHandle, url: String) -> Result<(), AppError> {
    validate_external_url(&url)?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| AppError::open_failed(error.to_string()))
}

/// Run one blocking read on the blocking pool under the spec's 30-second cap.
async fn blocking_read<T, F>(what: &str, work: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
{
    read_with_timeout(what, READ_TIMEOUT, async move {
        tauri::async_runtime::spawn_blocking(work)
            .await
            .map_err(|error| AppError::skill_read_failed_context(what, error))?
    })
    .await
}

/// Start a read-only install preview. Invalid sources are rejected before a
/// task is registered; all later phases stream through the per-task channel.
#[tauri::command]
pub async fn start_preview(
    state: State<'_, AppState>,
    scope: ScopeId,
    source: String,
    on_event: Channel<TaskEvent>,
) -> Result<TaskId, AppError> {
    // Project preview uses the project root as cwd and never `-g`.
    let sink = task_event_sink(on_event);
    state.tasks.start_preview(scope, &source, sink).await
}

/// Read-only update check, scoped. Global reads lock v3 and GitHub trees;
/// project scope parses `skills-lock.json` and compares CLI content hashes
/// against per-commit archives. No `skills check` / `skills update` process is
/// ever spawned for checking.
#[tauri::command]
pub async fn check_updates(
    state: State<'_, AppState>,
    scope: ScopeId,
    skill_ids: Option<Vec<String>>,
    check_revision: Option<String>,
) -> Result<UpdateCheckResponse, AppError> {
    match &scope {
        ScopeId::Global => {
            let full_check = skill_ids.is_none();
            let report = state.updates.check(skill_ids).await?;
            Ok(UpdateCheckResponse::Applied {
                scope,
                check_revision: None,
                state: full_check.then(|| UpdateCheckState::from_report(&report)),
                report,
            })
        }
        ScopeId::Project(_) => {
            state
                .project_updates
                .check(scope, skill_ids, check_revision)
                .await
        }
    }
}

/// Persisted check summary for every scope, for the scope switcher. One file
/// read instead of one request per project.
#[tauri::command]
pub fn get_update_check_states(state: State<'_, AppState>) -> BTreeMap<String, UpdateCheckState> {
    state.project_updates.read_states()
}

/// Last persisted full-check summary for one scope, if any. Read-only; no
/// network or CLI. Version-1 state files are read as the global scope.
#[tauri::command]
pub fn get_update_check_state(
    state: State<'_, AppState>,
    scope: ScopeId,
) -> Option<UpdateCheckState> {
    state.project_updates.read_state(&scope)
}

#[tauri::command]
pub async fn start_install(
    state: State<'_, AppState>,
    scope: ScopeId,
    request: InstallRequest,
    command_preview_id: Option<String>,
    on_event: Channel<TaskEvent>,
) -> Result<TaskId, AppError> {
    // Project install runs in the project root without `-g`.
    let discovery = state.discovery.clone();
    let source = crate::tasks::validate_source(&request.source)?.identity();
    let sink: EventSink = Arc::new(move |mut event| {
        if let Some(result) = &mut event.result {
            if let Err(error) = discovery.acknowledge_installed(&source, &result.skills) {
                result.warnings.push(error);
            }
            discovery.refresh();
        }
        let _ = on_event.send(event);
    });
    state
        .tasks
        .start_install_with_preview(scope, request, command_preview_id, sink)
        .await
}

#[tauri::command]
pub async fn cancel_task(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<CancelAck, AppError> {
    state.tasks.cancel_task(&task_id)
}

/// Current in-flight mutation for the exit confirmation dialog. `None` means
/// closing the window is safe (previews are read-only).
#[tauri::command]
pub fn get_active_mutation(state: State<'_, AppState>) -> Option<ActiveMutationInfo> {
    state.tasks.active_mutation()
}

/// Acknowledge the startup interruption warning. The marker is removed; the
/// retained backup snapshots are never deleted.
#[tauri::command]
pub fn dismiss_interrupted_mutation(state: State<'_, AppState>) -> Result<(), AppError> {
    let _write = state.app_updates.configuration_write()?;
    state.tasks.dismiss_interrupted_marker();
    Ok(())
}

/// Confirmed exit. Keeps the interruption marker, force-cancels even a sealed
/// mutation (the executor kills its whole process group), waits briefly, then
/// exits. The lock snapshot is retained for the next-startup verification.
#[tauri::command]
pub async fn quit_app(app: AppHandle, state: State<'_, AppState>) -> Result<(), AppError> {
    let _exit_guard = state.app_updates.exit_guard()?;
    state.app_updates.begin_shutdown();
    state.exiting.store(true, Ordering::SeqCst);
    state.discovery.shutdown_and_wait().await;
    let tasks = state.tasks.clone();
    if tasks
        .active_mutation()
        .is_some_and(|task| task.kind == crate::interrupted::MutationKind::Batch)
    {
        tasks.shutdown_batch().await;
    } else {
        let report = tasks.shutdown_mutations().await;
        if report.timed_out {
            eprintln!("skills-box: shutdown timed out waiting for a mutation to finish");
        }
    }
    app.exit(0);
    Ok(())
}

#[tauri::command]
pub fn get_app_update_state(state: State<'_, AppState>) -> AppUpdateSnapshot {
    state.app_updates.snapshot()
}

#[tauri::command]
pub fn check_app_update(state: State<'_, AppState>) -> Result<AppUpdateSnapshot, AppError> {
    state.app_updates.check()
}

#[tauri::command]
pub fn start_app_update(state: State<'_, AppState>) -> Result<AppUpdateSnapshot, AppError> {
    state.app_updates.update()
}

/// Serialized update execution. The frontend sends only update-candidate skill
/// ids; Rust re-validates them, backs up every skill, rebuilds `skills add`,
/// verifies the result and refreshes the baseline. `skills update` and
/// `skills check` are never spawned.
#[tauri::command]
pub async fn start_update(
    state: State<'_, AppState>,
    scope: ScopeId,
    skill_ids: Vec<String>,
    command_preview_id: Option<String>,
    on_event: Channel<TaskEvent>,
) -> Result<TaskId, AppError> {
    let sink = task_event_sink(on_event);
    state
        .tasks
        .start_update_with_preview(scope, skill_ids, command_preview_id, sink)
        .await
}

#[tauri::command]
pub async fn list_stale_records(
    state: State<'_, AppState>,
    scope: ScopeId,
) -> Result<Vec<crate::tasks::stale_records::StaleRecord>, AppError> {
    state.tasks.stale_records(scope).await
}

/// Resolve the current selected targets into a read-only, server-owned plan.
#[tauri::command]
pub async fn preview_batch(
    state: State<'_, AppState>,
    scope: ScopeId,
    request: crate::tasks::batch::BatchRequest,
) -> Result<crate::tasks::batch::BatchPreview, AppError> {
    let (updates, checked_records) = if request.action == crate::tasks::batch::BatchAction::Update {
        let names = request
            .skill_ids
            .iter()
            .map(|id| scope.parse_skill_id(id).map(str::to_string))
            .collect::<Result<Vec<_>, _>>()?;
        let records = state.tasks.batch_update_records(scope.clone()).await?;
        let response = match &scope {
            ScopeId::Global => UpdateCheckResponse::Applied {
                scope: scope.clone(),
                check_revision: None,
                report: state.updates.check(Some(names)).await?,
                state: None,
            },
            ScopeId::Project(_) => {
                state
                    .project_updates
                    .check(scope.clone(), Some(names), None)
                    .await?
            }
        };
        let updates = match response {
            UpdateCheckResponse::Applied { report, .. } => report
                .skills
                .into_iter()
                .map(|r| (r.name.clone(), r))
                .collect(),
            UpdateCheckResponse::Invalidated { .. } => {
                return Err(AppError::batch_preview_expired());
            }
        };
        (updates, Some(records))
    } else {
        (std::collections::HashMap::new(), None)
    };
    state
        .tasks
        .preview_checked_batch(scope, request, updates, checked_records)
        .await
}

#[tauri::command]
pub async fn refresh_batch_preview(
    state: State<'_, AppState>,
    preview_id: String,
) -> Result<crate::tasks::batch::BatchPreview, AppError> {
    state.tasks.refresh_batch_preview(preview_id).await
}

#[tauri::command]
pub async fn start_batch(
    state: State<'_, AppState>,
    scope: ScopeId,
    confirmation: crate::tasks::batch::BatchConfirmation,
    on_event: Channel<TaskEvent>,
) -> Result<TaskId, AppError> {
    state
        .tasks
        .start_batch(scope, confirmation, task_event_sink(on_event))
        .await
}

/// Native detach from one agent. Runs through the same single-writer slot as
/// install/update/uninstall; no CLI is spawned. The frontend sends only the
/// inventory skill id and a catalog agent id; every path is server-resolved.
#[tauri::command]
pub async fn detach_agent(
    state: State<'_, AppState>,
    scope: ScopeId,
    skill_id: String,
    agent_id: String,
) -> Result<DetachResult, AppError> {
    state.tasks.detach_agent(scope, skill_id, agent_id).await
}

/// Native attach: add one agent attachment to an already-installed managed
/// skill. Serialized with install/update/uninstall/detach; no CLI is spawned.
#[tauri::command]
pub async fn attach_agent(
    state: State<'_, AppState>,
    scope: ScopeId,
    skill_id: String,
    agent_id: String,
) -> Result<AttachResult, AppError> {
    state.tasks.attach_agent(scope, skill_id, agent_id).await
}

/// Persist a user-defined extra agent in the app-data `agents.json`.
#[tauri::command]
pub async fn add_extra_agent(
    state: State<'_, AppState>,
    agent: UserExtraAgent,
) -> Result<ExtraAgent, AppError> {
    state.tasks.add_extra_agent(agent).await
}

/// Change or clear a user extra agent's project skills directory. The project
/// guard rejects the change while registered projects may still hold mounts.
#[tauri::command]
pub async fn update_extra_agent(
    state: State<'_, AppState>,
    agent_id: String,
    project_skills_dir: Option<String>,
) -> Result<ExtraAgent, AppError> {
    state
        .tasks
        .update_extra_agent(agent_id, project_skills_dir)
        .await
}

/// Remove a user-defined extra agent, detaching its skills first.
#[tauri::command]
pub async fn remove_extra_agent(
    state: State<'_, AppState>,
    agent_id: String,
) -> Result<RemoveExtraAgentResult, AppError> {
    state.tasks.remove_extra_agent(agent_id).await
}

/// Serialized destructive uninstall. Only managed and copy-only skills are
/// accepted; Rust backs up lock/canonical/copies, runs `skills remove -g -s
/// <name> -y` without `-a`/`--all`, then verifies canonical, every attachment
/// path and the lock key on disk. Exit code 0 alone is never success.
#[tauri::command]
pub async fn start_uninstall(
    state: State<'_, AppState>,
    scope: ScopeId,
    skill_id: String,
    command_preview_id: Option<String>,
    on_event: Channel<TaskEvent>,
) -> Result<TaskId, AppError> {
    let sink = task_event_sink(on_event);
    state
        .tasks
        .start_uninstall_with_preview(scope, skill_id, command_preview_id, sink)
        .await
}

/// Open one of the app-data directories (backups / logs / cache / baselines).
/// The frontend sends a fixed kind, never a path.
#[tauri::command]
pub async fn reveal_app_data(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: String,
) -> Result<(), AppError> {
    let paths = state.paths.clone();
    let directory = tauri::async_runtime::spawn_blocking(move || {
        let directory = app_data_dir(&paths, &kind)?;
        std::fs::create_dir_all(&directory).map_err(|error| {
            AppError::path_app_data(format!("{}：{error}", directory.display()))
        })?;
        Ok::<PathBuf, AppError>(directory)
    })
    .await
    .map_err(|error| AppError::path_app_data(format!("无法准备应用数据目录：{error}")))??;

    app.opener()
        .open_path(directory.to_string_lossy().to_string(), None::<&str>)
        .map_err(|error| AppError::open_failed(error.to_string()))
}

pub fn app_data_dir(paths: &AppPaths, kind: &str) -> Result<PathBuf, AppError> {
    match kind {
        "backups" => Ok(paths.backups_dir()),
        "logs" => Ok(paths.logs_dir()),
        "cache" => Ok(paths.cache_dir()),
        "baselines" => Ok(paths.baselines_dir()),
        _ => Err(AppError::path_invalid(format!(
            "未知的应用数据目录：{kind}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    #[test]
    fn app_data_kinds_map_to_their_directories() {
        let paths = AppPaths::new("/Users/tester", "/tmp/skills-box-app-data");
        assert_eq!(
            app_data_dir(&paths, "backups").unwrap(),
            paths.backups_dir()
        );
        assert_eq!(app_data_dir(&paths, "logs").unwrap(), paths.logs_dir());
        assert_eq!(app_data_dir(&paths, "cache").unwrap(), paths.cache_dir());
        assert_eq!(
            app_data_dir(&paths, "baselines").unwrap(),
            paths.baselines_dir()
        );
    }

    #[test]
    fn unknown_or_path_like_kinds_are_rejected() {
        let paths = AppPaths::new("/Users/tester", "/tmp/skills-box-app-data");
        for kind in ["", "../backups", "/tmp", "BACKUPS", "*"] {
            let error = app_data_dir(&paths, kind).unwrap_err();
            assert_eq!(error.code, ErrorCode::PathInvalid, "{kind:?}");
        }
    }

    #[tokio::test]
    async fn read_timeout_reports_a_retryable_skill_read_timeout() {
        let pending = std::future::pending::<Result<(), AppError>>();
        let error = read_with_timeout("库存", Duration::from_millis(20), pending)
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::SkillReadTimeout);
        assert!(error.retryable);
        assert!(error.message.contains("库存"));
    }

    #[tokio::test]
    async fn read_timeout_passes_through_fast_results_and_errors() {
        let value = read_with_timeout("库存", Duration::from_secs(1), async { Ok(7_u8) })
            .await
            .unwrap();
        assert_eq!(value, 7);

        let error = read_with_timeout("库存", Duration::from_secs(1), async {
            Err::<(), _>(AppError::lock_corrupt("bad"))
        })
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::LockCorrupt);
    }

    #[test]
    fn external_urls_are_https_only_and_absolute() {
        for url in [
            "https://example.com/docs",
            "HTTPS://example.com",
            "https://example.com",
            "https://example.com/path?q=1#frag",
        ] {
            validate_external_url(url).unwrap_or_else(|error| panic!("{url}: {error:?}"));
        }
        for url in [
            "",
            " https://example.com",
            "https://example.com ",
            "http://example.com",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,x",
            "ssh://git@example.com/repo",
            "mailto:a@example.com",
            "tel:123",
            "/relative/path",
            "example.com",
            "https://",
            "https:///path",
            "\nhttps://example.com",
        ] {
            let error = validate_external_url(url).unwrap_err();
            assert_eq!(error.code, ErrorCode::OpenFailed, "{url:?}");
            assert!(!error.retryable);
        }
    }
}

#[tauri::command]
pub async fn get_discovery_state(
    state: State<'_, AppState>,
) -> Result<crate::discovery::DiscoveryState, AppError> {
    let service = state.discovery.clone();
    read_with_timeout("发现记录", READ_TIMEOUT, async move {
        tauri::async_runtime::spawn_blocking(move || service.get_state())
            .await
            .map_err(|_| AppError::task_internal("发现记录读取失败", "blocking worker failed"))
    })
    .await
}
#[tauri::command]
pub async fn start_discovery_scan(
    state: State<'_, AppState>,
    manual: Option<bool>,
) -> Result<crate::discovery::DiscoveryProgress, AppError> {
    let service = state.discovery.clone();
    tauri::async_runtime::spawn_blocking(move || service.start(manual.unwrap_or(false)))
        .await
        .map_err(|_| AppError::task_internal("发现扫描无法启动", "blocking worker failed"))?
}
#[tauri::command]
pub fn acknowledge_discoveries(
    state: State<'_, AppState>,
    ids: Option<Vec<crate::discovery::DiscoveryId>>,
) -> Result<(), AppError> {
    let _write = state.app_updates.configuration_write()?;
    state.discovery.acknowledge(ids)
}
#[tauri::command]
pub fn set_discovery_ignored(
    state: State<'_, AppState>,
    id: crate::discovery::DiscoveryId,
    ignored: bool,
) -> Result<(), AppError> {
    let _write = state.app_updates.configuration_write()?;
    state.discovery.set_ignored(id, ignored)
}
#[tauri::command]
pub fn invalidate_discovery_source(
    state: State<'_, AppState>,
    source: String,
) -> Result<(), AppError> {
    let _write = state.app_updates.configuration_write()?;
    state.discovery.invalidate(source)
}

#[tauri::command]
pub async fn list_marketplace_categories() -> Result<Vec<crate::marketplace::provider::Category>, AppError> {
    crate::marketplace::provider::categories().await
}
#[tauri::command]
pub async fn list_marketplace_skills(
    keyword: String,
    page: u32,
    sort: crate::marketplace::provider::Sort,
    category: String,
    api_key: crate::marketplace::provider::ApiKeyFilter,
) -> Result<crate::marketplace::provider::Listing, AppError> {
    crate::marketplace::provider::list(&keyword, page, sort, &category, api_key).await
}
#[tauri::command]
pub async fn get_marketplace_skill(
    coordinate: crate::marketplace::Coordinate,
) -> Result<crate::marketplace::provider::Detail, AppError> {
    crate::marketplace::provider::detail(&coordinate).await
}
#[tauri::command]
pub async fn list_marketplace_files(
    coordinate: crate::marketplace::Coordinate,
    version: String,
) -> Result<crate::marketplace::files::Listing, AppError> {
    crate::marketplace::files::list(&coordinate, &version).await
}
#[tauri::command]
pub async fn read_marketplace_file(
    coordinate: crate::marketplace::Coordinate,
    version: String,
    path: String,
) -> Result<crate::content::SkillFileContent, AppError> {
    crate::marketplace::files::read(&coordinate, &version, &path).await
}
#[tauri::command]
pub async fn get_marketplace_cli(
    state: State<'_, AppState>,
) -> Result<crate::marketplace::runtime::Status, AppError> {
    Ok(state.tasks.marketplace_cli().await)
}
#[tauri::command]
pub async fn prepare_marketplace_install(
    state: State<'_, AppState>,
    scope: ScopeId,
    coordinate: crate::marketplace::Coordinate,
    agents: Vec<String>,
) -> Result<crate::tasks::marketplace::Preview, AppError> {
    state
        .tasks
        .prepare_marketplace(scope, coordinate, agents)
        .await
}
#[tauri::command]
pub async fn start_marketplace_install(
    state: State<'_, AppState>,
    scope: ScopeId,
    preview_id: String,
    on_event: Channel<TaskEvent>,
) -> Result<TaskId, AppError> {
    state
        .tasks
        .start_marketplace(scope, preview_id, task_event_sink(on_event))
        .await
}
