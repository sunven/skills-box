pub mod agents;
pub mod attach;
pub mod attachment_target;
pub mod backup;
pub mod baseline;
pub mod cli;
pub mod commands;
pub mod content;
pub mod detach;
pub mod detail;
pub mod env;
pub mod error;
pub mod git_tree;
pub mod interrupted;
pub mod inventory;
pub mod lock;
pub mod logs;
pub mod paths;
pub mod process;
pub mod project_lock;
pub mod projects;
pub mod read_model;
pub mod scope;
pub mod scope_layout;
pub mod security;
pub mod tasks;
pub mod timeouts;
pub mod update;
pub mod update_target;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::Manager;

use cli::CliRuntimeProvider;
use env::EnvironmentService;
use paths::AppPaths;
use process::tokio_executor::TokioExecutor;
use projects::ProjectRegistry;
use read_model::ReadModel;
use tasks::{InventoryConflictProvider, TaskManager, TaskManagerConfig};
use update::UpdateService;

pub struct AppState {
    pub paths: AppPaths,
    pub environment: Arc<EnvironmentService>,
    pub read_model: Arc<ReadModel>,
    pub projects: Arc<ProjectRegistry>,
    pub tasks: TaskManager,
    pub updates: UpdateService,
    /// Set by `quit_app` so window/exit events stop preventing the exit.
    pub exiting: AtomicBool,
}

impl AppState {
    pub fn new(paths: AppPaths) -> Self {
        let environment = Arc::new(EnvironmentService::from_process(paths.clone()));
        let runtime: Arc<dyn CliRuntimeProvider> = environment.clone();
        let tasks = TaskManager::new(
            runtime,
            Arc::new(TokioExecutor),
            Arc::new(InventoryConflictProvider::new(paths.clone())),
            paths.clone(),
            TaskManagerConfig::default(),
        );
        let projects = Arc::new(ProjectRegistry::new(paths.clone()));
        Self {
            read_model: Arc::new(ReadModel::with_projects(
                paths.clone(),
                Arc::clone(&projects),
            )),
            projects,
            updates: UpdateService::new(paths.clone()),
            paths,
            environment,
            tasks,
            exiting: AtomicBool::new(false),
        }
    }
}

/// Second launch: bring the existing main window forward instead of starting a
/// second process (two instances would each run their own single-writer queue
/// and could interleave CLI writes on the same lock).
fn focus_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let paths = AppPaths::discover();
    let app = tauri::Builder::default()
        // Must be registered first (plugin docs): a second instance only
        // forwards its argv and focuses the existing window.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            focus_main_window(app);
        }))
        .plugin(tauri_plugin_opener::init())
        // Folder picker for registering project roots. Only `dialog:allow-open`
        // is exposed to the renderer; no save/message/ask permissions.
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new(paths))
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<AppState>();
                if state.exiting.load(Ordering::SeqCst) {
                    return;
                }
                if let Some(active) = state.tasks.active_mutation() {
                    // Protect a half-written skill store: the window stays
                    // open while the frontend confirms the risky exit.
                    api.prevent_close();
                    use tauri::Emitter;
                    let _ = window.emit("exit-requested", active);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_environment,
            commands::list_projects,
            commands::add_project,
            commands::relocate_project,
            commands::forget_project,
            commands::set_active_scope,
            commands::get_inventory,
            commands::get_skill_detail,
            commands::list_skill_dir,
            commands::read_skill_file,
            commands::open_skill_folder,
            commands::open_skill_file,
            commands::open_external_url,
            commands::start_preview,
            commands::start_install,
            commands::start_update,
            commands::detach_agent,
            commands::attach_agent,
            commands::add_extra_agent,
            commands::remove_extra_agent,
            commands::start_uninstall,
            commands::check_updates,
            commands::get_update_check_state,
            commands::cancel_task,
            commands::get_active_mutation,
            commands::dismiss_interrupted_mutation,
            commands::quit_app,
            commands::reveal_app_data
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            let state = app_handle.state::<AppState>();
            if state.exiting.load(Ordering::SeqCst) {
                return;
            }
            if let Some(active) = state.tasks.active_mutation() {
                // Cmd+Q / OS quit: prevent the exit and let the frontend
                // confirm. A second quit request emits again.
                api.prevent_exit();
                use tauri::Emitter;
                let _ = app_handle.emit("exit-requested", active);
            }
        }
    });
}
