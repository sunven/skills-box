pub mod agents;
pub mod baseline;
pub mod cli;
pub mod commands;
pub mod detail;
pub mod env;
pub mod error;
pub mod inventory;
pub mod lock;
pub mod paths;
pub mod process;
pub mod security;
pub mod tasks;
pub mod update;

use std::sync::Arc;

use cli::CliRuntimeProvider;
use env::EnvironmentService;
use paths::AppPaths;
use process::tokio_executor::TokioExecutor;
use tasks::{InventoryConflictProvider, TaskManager, TaskManagerConfig};
use update::UpdateService;

pub struct AppState {
    pub paths: AppPaths,
    pub environment: Arc<EnvironmentService>,
    pub tasks: TaskManager,
    pub updates: UpdateService,
}

impl AppState {
    pub fn new(paths: AppPaths) -> Self {
        let environment = Arc::new(EnvironmentService::from_process(paths.clone()));
        let runtime: Arc<dyn CliRuntimeProvider> = environment.clone();
        let tasks = TaskManager::new(
            runtime,
            Arc::new(TokioExecutor),
            Arc::new(InventoryConflictProvider::new(paths.clone())),
            TaskManagerConfig::default(),
        );
        Self {
            updates: UpdateService::new(paths.clone()),
            paths,
            environment,
            tasks,
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let paths = AppPaths::discover();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::new(paths))
        .invoke_handler(tauri::generate_handler![
            commands::get_environment,
            commands::get_inventory,
            commands::get_skill_detail,
            commands::open_skill_folder,
            commands::open_skill_md,
            commands::start_preview,
            commands::check_updates,
            commands::cancel_task
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
