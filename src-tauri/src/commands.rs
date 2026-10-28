//! Tauri command boundary. Commands are thin: validate nothing from the
//! frontend beyond typed inputs, delegate to services, return typed results.

use tauri::State;

use crate::AppState;
use crate::env::EnvReport;
use crate::error::AppError;
use crate::inventory::InventorySnapshot;

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
