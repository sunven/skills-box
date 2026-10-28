//! Tauri command boundary. Commands are thin: validate nothing from the
//! frontend beyond typed inputs, delegate to services, return typed results.

use std::sync::Arc;

use tauri::State;
use tauri::ipc::Channel;

use crate::AppState;
use crate::env::EnvReport;
use crate::error::AppError;
use crate::inventory::InventorySnapshot;
use crate::tasks::{CancelAck, EventSink, TaskEvent, TaskId};

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

#[tauri::command]
pub async fn cancel_task(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<CancelAck, AppError> {
    state.tasks.cancel_task(&task_id)
}
