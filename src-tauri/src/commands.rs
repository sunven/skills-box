//! Tauri command boundary. Commands are thin: validate nothing from the
//! frontend beyond typed inputs, delegate to services, return typed results.

use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::AppState;
use crate::detail::{self, OpenTarget, SkillDetail};
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
