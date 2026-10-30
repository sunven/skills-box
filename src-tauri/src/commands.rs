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

/// Forward task events to the renderer. A closed window must not fail a
/// read-only task or abort a mutation that may already be writing; the task
/// finishes and its result is discarded.
fn task_event_sink(on_event: Channel<TaskEvent>) -> EventSink {
    Arc::new(move |event| {
        let _ = on_event.send(event);
    })
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
    state.projects.add(&root)
}

#[tauri::command]
pub fn relocate_project(
    state: State<'_, AppState>,
    id: String,
    root: String,
) -> Result<RelocateProjectResult, AppError> {
    let _change = state.tasks.begin_registry_change()?;
    state.project_updates.relocate(&id, &root)
}

#[tauri::command]
pub fn forget_project(
    state: State<'_, AppState>,
    id: String,
) -> Result<ForgetProjectResult, AppError> {
    let _change = state.tasks.begin_registry_change()?;
    state.project_updates.forget(&id)
}

#[tauri::command]
pub fn set_active_scope(state: State<'_, AppState>, scope: ScopeId) -> Result<(), AppError> {
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
    read_with_timeout("库存", READ_TIMEOUT, async move {
        tauri::async_runtime::spawn_blocking(move || read_model.snapshot(&scope))
            .await
            .map_err(|error| AppError::skill_read_failed_context("库存", error))?
    })
    .await
}

#[tauri::command]
pub async fn get_skill_detail(
    state: State<'_, AppState>,
    scope: ScopeId,
    skill_id: String,
) -> Result<SkillDetail, AppError> {
    let read_model = Arc::clone(&state.read_model);
    read_with_timeout("skill 详情", READ_TIMEOUT, async move {
        tauri::async_runtime::spawn_blocking(move || read_model.detail(&scope, &skill_id))
            .await
            .map_err(|error| AppError::skill_read_failed_context("skill 详情", error))?
    })
    .await
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
            .map_err(|error| AppError::skill_read_failed_context("skill 内容", error))?
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
    on_event: Channel<TaskEvent>,
) -> Result<TaskId, AppError> {
    // Project install runs in the project root without `-g`.
    let sink = task_event_sink(on_event);
    state.tasks.start_install(scope, request, sink).await
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
    state.tasks.dismiss_interrupted_marker();
    Ok(())
}

/// Confirmed exit. Keeps the interruption marker, force-cancels even a sealed
/// mutation (the executor kills its whole process group), waits briefly, then
/// exits. The lock snapshot is retained for the next-startup verification.
#[tauri::command]
pub async fn quit_app(app: AppHandle, state: State<'_, AppState>) -> Result<(), AppError> {
    state.exiting.store(true, Ordering::SeqCst);
    let tasks = state.tasks.clone();
    let report = tasks.shutdown_mutations().await;
    if report.timed_out {
        // The executor already sent SIGKILL; the marker still needs to stay.
        eprintln!("skills-box: shutdown timed out waiting for a mutation to finish");
    }
    app.exit(0);
    Ok(())
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
    on_event: Channel<TaskEvent>,
) -> Result<TaskId, AppError> {
    let sink = task_event_sink(on_event);
    state.tasks.start_update(scope, skill_ids, sink).await
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
    on_event: Channel<TaskEvent>,
) -> Result<TaskId, AppError> {
    let sink = task_event_sink(on_event);
    state.tasks.start_uninstall(scope, skill_id, sink).await
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
