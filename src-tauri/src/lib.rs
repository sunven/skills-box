pub mod agents;
pub mod commands;
pub mod env;
pub mod error;
pub mod paths;
pub mod process;

use env::EnvironmentService;
use paths::AppPaths;

pub struct AppState {
    pub paths: AppPaths,
    pub environment: EnvironmentService,
}

impl AppState {
    pub fn new(paths: AppPaths) -> Self {
        Self {
            environment: EnvironmentService::from_process(paths.clone()),
            paths,
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let paths = AppPaths::discover();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::new(paths))
        .invoke_handler(tauri::generate_handler![commands::get_environment])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
