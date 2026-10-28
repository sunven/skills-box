//! Tauri command boundary. Commands are thin: validate nothing from the
//! frontend beyond typed inputs, delegate to services, return typed results.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::AppState;
use crate::detach::DetachResult;
use crate::detail::{self, OpenTarget, SkillDetail};
use crate::env::EnvReport;
use crate::error::AppError;
use crate::inventory::InventorySnapshot;
use crate::paths::AppPaths;
use crate::tasks::{CancelAck, EventSink, InstallRequest, TaskEvent, TaskId};
use crate::update::UpdateCheckReport;

#[tauri::command]
pub async fn get_environment(
    state: State<'_, AppState>,
    refresh: Option<bool>,
) -> Result<EnvReport, AppError> {
    state.environment.get(refresh.unwrap_or(false)).await
}

/// Full read-model projection. Filesystem scanning happens on the blocking
/// pool so a large inventory never stalls the UI thread.
#[tauri::command]
pub async fn get_inventory(state: State<'_, AppState>) -> Result<InventorySnapshot, AppError> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || crate::inventory::read(&paths))
        .await
        .map_err(|error| {
            AppError::new("skill.read_failed", "读取库存失败。")
                .detail(error.to_string())
                .retryable(true)
        })?
}

#[tauri::command]
pub async fn get_skill_detail(
    state: State<'_, AppState>,
    skill_id: String,
) -> Result<SkillDetail, AppError> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || crate::detail::read(&paths, &skill_id))
        .await
        .map_err(|error| {
            AppError::new("skill.read_failed", "读取 skill 详情失败。")
                .detail(error.to_string())
                .retryable(true)
        })?
}

#[tauri::command]
pub async fn open_skill_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    skill_id: String,
) -> Result<(), AppError> {
    open_target(app, state, &skill_id, OpenTarget::Folder).await
}

#[tauri::command]
pub async fn open_skill_md(
    app: AppHandle,
    state: State<'_, AppState>,
    skill_id: String,
) -> Result<(), AppError> {
    open_target(app, state, &skill_id, OpenTarget::SkillMd).await
}

async fn open_target(
    app: AppHandle,
    state: State<'_, AppState>,
    skill_id: &str,
    target: OpenTarget,
) -> Result<(), AppError> {
    let paths = state.paths.clone();
    let skill_id = skill_id.to_string();
    let resolved = tauri::async_runtime::spawn_blocking(move || {
        let agents = crate::inventory::detected_agent_dirs(&paths);
        detail::resolve_open_target(&paths, &agents, &skill_id, target)
    })
    .await
    .map_err(|error| {
        AppError::new("skill.read_failed", "解析 skill 路径失败。")
            .detail(error.to_string())
            .retryable(true)
    })??;

    app.opener()
        .open_path(resolved.to_string_lossy().to_string(), None::<&str>)
        .map_err(|error| AppError::open_failed(error.to_string()))
}

/// Start a read-only install preview. Invalid sources are rejected before a
/// task is registered; all later phases stream through the per-task channel.
#[tauri::command]
pub async fn start_preview(
    state: State<'_, AppState>,
    source: String,
    on_event: Channel<TaskEvent>,
) -> Result<TaskId, AppError> {
    let sink: EventSink = Arc::new(move |event| {
        // A closed window must not fail a read-only task; the task simply ends.
        let _ = on_event.send(event);
    });
    state.tasks.start_preview(&source, sink).await
}

/// Read-only update check. Rust reads lock v3 and GitHub trees directly; no
/// `skills check` / `skills update` process is ever spawned.
#[tauri::command]
pub async fn check_updates(
    state: State<'_, AppState>,
    skill_ids: Option<Vec<String>>,
) -> Result<UpdateCheckReport, AppError> {
    state.updates.check(skill_ids).await
}

#[tauri::command]
pub async fn start_install(
    state: State<'_, AppState>,
    request: InstallRequest,
    on_event: Channel<TaskEvent>,
) -> Result<TaskId, AppError> {
    let sink: EventSink = Arc::new(move |event| {
        // A closed window must not abort a mutation that may already be
        // writing; the task still finishes and its result is discarded.
        let _ = on_event.send(event);
    });
    state.tasks.start_install(request, sink).await
}

#[tauri::command]
pub async fn cancel_task(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<CancelAck, AppError> {
    state.tasks.cancel_task(&task_id)
}

/// Serialized update execution. The frontend sends only update-candidate skill
/// ids; Rust re-validates them, backs up every skill, rebuilds `skills add`,
/// verifies the result and refreshes the baseline. `skills update` and
/// `skills check` are never spawned.
#[tauri::command]
pub async fn start_update(
    state: State<'_, AppState>,
    skill_ids: Vec<String>,
    on_event: Channel<TaskEvent>,
) -> Result<TaskId, AppError> {
    let sink: EventSink = Arc::new(move |event| {
        // A closed window must not abort a mutation that may already be
        // writing; the task still finishes and its result is discarded.
        let _ = on_event.send(event);
    });
    state.tasks.start_update(skill_ids, sink).await
}

/// Native detach from one agent. Runs through the same single-writer slot as
/// install/update/uninstall; no CLI is spawned. The frontend sends only the
/// inventory skill id and a catalog agent id; every path is server-resolved.
#[tauri::command]
pub async fn detach_agent(
    state: State<'_, AppState>,
    skill_id: String,
    agent_id: String,
) -> Result<DetachResult, AppError> {
    state.tasks.detach_agent(skill_id, agent_id).await
}

/// Serialized destructive uninstall. Only managed and copy-only skills are
/// accepted; Rust backs up lock/canonical/copies, runs `skills remove -g -s
/// <name> -y` without `-a`/`--all`, then verifies canonical, every attachment
/// path and the lock key on disk. Exit code 0 alone is never success.
#[tauri::command]
pub async fn start_uninstall(
    state: State<'_, AppState>,
    skill_id: String,
    on_event: Channel<TaskEvent>,
) -> Result<TaskId, AppError> {
    let sink: EventSink = Arc::new(move |event| {
        // A closed window must not abort a mutation that may already be
        // writing; the task still finishes and its result is discarded.
        let _ = on_event.send(event);
    });
    state.tasks.start_uninstall(skill_id, sink).await
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
    use crate::error::codes;

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
            assert_eq!(error.code, codes::PATH_INVALID, "{kind:?}");
        }
    }
}
