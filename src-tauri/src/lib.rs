pub mod agents;
mod app_data;
pub mod app_update;
pub mod attach;
pub mod attachment_target;
pub mod backup;
pub mod baseline;
pub mod cli;
pub mod commands;
pub mod content;
pub mod detach;
pub mod detail;
pub mod discovery;
pub mod env;
pub mod error;
pub mod extra_agent_guard;
mod fsutil;
pub mod git_tree;
pub mod interrupted;
pub mod inventory;
pub mod lock;
pub mod logs;
pub mod marketplace;
pub mod paths;
pub mod process;
pub mod project_lock;
mod project_path;
pub mod projects;
pub mod read_model;
pub mod scope;
pub mod scope_layout;
pub mod security;
pub mod settings;
pub mod tasks;
mod time;
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
use update::project_checks::ProjectUpdateChecks;
use update::state::UpdateCheckStateStore;

pub struct AppState {
    pub paths: AppPaths,
    pub app_data: std::sync::Arc<app_data::DataService>,
    pub environment: Arc<EnvironmentService>,
    pub read_model: Arc<ReadModel>,
    pub projects: Arc<ProjectRegistry>,
    pub tasks: TaskManager,
    pub app_updates: app_update::AppUpdateService,
    pub updates: UpdateService,
    pub project_updates: ProjectUpdateChecks,
    pub settings: Arc<settings::SettingsService>,
    pub github_api: update::api::GitHubApiClient,
    pub discovery: discovery::DiscoveryService,
    pub cli_commands: Arc<cli::commands::CliCommandStore>,
    /// Set by `quit_app` so window/exit events stop preventing the exit.
    pub exiting: AtomicBool,
}

impl AppState {
    pub fn new(paths: AppPaths) -> Self {
        let cli_commands = Arc::new(cli::commands::CliCommandStore::default());
        let executor: Arc<dyn process::ProcessExecutor> =
            Arc::new(cli::commands::ObservedExecutor {
                executor: Arc::new(TokioExecutor),
                commands: cli_commands.clone(),
            });
        let environment = Arc::new(EnvironmentService::new(env::EnvResolver::new(
            paths.clone(),
            executor.clone(),
            env::EnvResolverConfig::from_process(&paths),
        )));
        let runtime: Arc<dyn CliRuntimeProvider> = environment.clone();
        let projects = Arc::new(ProjectRegistry::new(paths.clone()));
        let check_states = Arc::new(UpdateCheckStateStore::new(&paths));
        let settings = Arc::new(settings::SettingsService::from_process(paths.clone()));
        let github_api = update::api::GitHubApiClient::new(Arc::clone(&settings));
        let tasks = TaskManager::with_projects(
            runtime.clone(),
            executor.clone(),
            Arc::new(InventoryConflictProvider::new(paths.clone())),
            paths.clone(),
            Arc::clone(&projects),
            TaskManagerConfig::default(),
        );
        let read_model = Arc::new(ReadModel::with_projects(
            paths.clone(),
            Arc::clone(&projects),
        ));
        let discovery = discovery::DiscoveryService::new(
            paths.clone(),
            Arc::clone(&projects),
            Arc::clone(&read_model),
            runtime,
            tasks.clone(),
            github_api.clone(),
            executor,
        );
        Self {
            app_updates: app_update::AppUpdateService::new(tasks.clone()),
            app_data: std::sync::Arc::new(app_data::DataService::default()),
            read_model,
            discovery,
            cli_commands,
            projects: Arc::clone(&projects),
            updates: UpdateService::new_with_api(
                paths.clone(),
                Arc::clone(&check_states),
                github_api.clone(),
            ),
            project_updates: ProjectUpdateChecks::with_api(
                paths.clone(),
                Arc::clone(&projects),
                check_states,
                github_api.clone(),
            ),
            settings,
            github_api,
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

/// Stop accepting writes and discovery, then exit once both have drained.
fn spawn_graceful_shutdown(
    app: tauri::AppHandle,
    state: &AppState,
    exit_guard: tokio::sync::OwnedRwLockReadGuard<()>,
) {
    state.exiting.store(true, Ordering::SeqCst);
    state.app_updates.begin_shutdown();
    let discovery = state.discovery.clone();
    let tasks = state.tasks.clone();
    tauri::async_runtime::spawn(async move {
        let _exit_guard = exit_guard;
        let _ = tasks.shutdown_mutations().await;
        discovery.shutdown_and_wait().await;
        app.exit(0);
    });
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
        .setup(|app| {
            use tauri::Emitter;
            let handle = app.handle().clone();
            app.state::<AppState>()
                .cli_commands
                .listen(Arc::new(move |record| {
                    let _ = handle.emit("cli-command", record);
                }));
            #[cfg(target_os = "macos")]
            {
                app.handle()
                    .plugin(tauri_plugin_updater::Builder::new().build())?;
                app.state::<AppState>().app_updates.configure(
                    Arc::new(app_update::native::NativeDriver(app.handle().clone())),
                    !cfg!(debug_assertions),
                );
            }
            Ok(())
        })
        .manage(AppState::new(paths))
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<AppState>();
                if state.exiting.load(Ordering::SeqCst) {
                    return;
                }
                let Ok(exit_guard) = state.app_updates.exit_guard() else {
                    api.prevent_close();
                    return;
                };
                if let Some(active) = state.tasks.active_mutation() {
                    // Protect a half-written skill store: the window stays
                    // open while the frontend confirms the risky exit.
                    api.prevent_close();
                    use tauri::Emitter;
                    let _ = window.emit("exit-requested", active);
                } else {
                    // Closing the main window ends discovery as well; a macOS
                    // window lifecycle must not leave an invisible scan running.
                    api.prevent_close();
                    spawn_graceful_shutdown(window.app_handle().clone(), state.inner(), exit_guard);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_data,
            commands::save_data_preferences,
            commands::preview_app_data_cleanup,
            commands::clean_app_data,
            commands::reveal_backup,
            commands::get_app_update_state,
            commands::check_app_update,
            commands::start_app_update,
            commands::get_environment,
            commands::get_cli_commands,
            commands::preview_cli_commands,
            commands::list_marketplace_skills,
            commands::get_marketplace_skill,
            commands::get_marketplace_cli,
            commands::prepare_marketplace_install,
            commands::start_marketplace_install,
            commands::get_discovery_state,
            commands::start_discovery_scan,
            commands::acknowledge_discoveries,
            commands::set_discovery_ignored,
            commands::invalidate_discovery_source,
            commands::get_github_settings,
            commands::save_github_token,
            commands::clear_github_token,
            commands::test_github_auth,
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
            commands::list_stale_records,
            commands::preview_batch,
            commands::refresh_batch_preview,
            commands::start_batch,
            commands::detach_agent,
            commands::attach_agent,
            commands::add_extra_agent,
            commands::update_extra_agent,
            commands::remove_extra_agent,
            commands::start_uninstall,
            commands::check_updates,
            commands::get_update_check_state,
            commands::get_update_check_states,
            commands::cancel_task,
            commands::get_active_mutation,
            commands::dismiss_interrupted_mutation,
            commands::quit_app,
            commands::reveal_app_data
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            app_handle.state::<AppState>().discovery.shutdown();
        }
        if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
            // Tauri restart cannot be vetoed. Our native updater has already
            // retained the installation permit and completed background cleanup.
            if code == Some(tauri::RESTART_EXIT_CODE) {
                return;
            }
            let state = app_handle.state::<AppState>();
            if state.exiting.load(Ordering::SeqCst) {
                return;
            }
            let Ok(exit_guard) = state.app_updates.exit_guard() else {
                api.prevent_exit();
                return;
            };
            if let Some(active) = state.tasks.active_mutation() {
                // Cmd+Q / OS quit: prevent the exit and let the frontend
                // confirm. A second quit request emits again.
                api.prevent_exit();
                use tauri::Emitter;
                let _ = app_handle.emit("exit-requested", active);
            } else {
                api.prevent_exit();
                spawn_graceful_shutdown(app_handle.clone(), state.inner(), exit_guard);
            }
        }
    });
}
