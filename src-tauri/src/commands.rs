//! Tauri command boundary. Commands are thin: validate nothing from the
//! frontend beyond typed inputs, delegate to services, return typed results.

use tauri::State;

use crate::AppState;
use crate::env::EnvReport;
use crate::error::AppError;

#[tauri::command]
pub async fn get_environment(
    state: State<'_, AppState>,
    refresh: Option<bool>,
) -> Result<EnvReport, AppError> {
    state.environment.get(refresh.unwrap_or(false)).await
}
