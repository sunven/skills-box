pub mod agents;
pub mod cli;
pub mod commands;
pub mod env;
pub mod error;
pub mod inventory;
pub mod lock;
pub mod paths;
pub mod process;
pub mod tasks;

use std::sync::Arc;

use cli::CliRuntimeProvider;
use env::EnvironmentService;
use paths::AppPaths;
use process::tokio_executor::TokioExecutor;
use tasks::{InventoryConflictProvider, TaskManager, TaskManagerConfig};

pub struct AppState {
    pub paths: AppPaths,
    pub environment: Arc<EnvironmentService>,
    pub tasks: TaskManager,
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
            commands::start_preview,
            commands::cancel_task
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
