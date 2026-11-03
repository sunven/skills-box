//! Application releases have their own lifecycle, outside the Skill task registry.
//! Only installation owns the Skill writer permit. Workers, not IPC futures,
//! own downloads, verified bytes and installation through restart handoff.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::Serialize;
use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock, Semaphore};

use crate::error::AppError;
use crate::tasks::{MutationPermit, TaskManager};

pub const RELEASE_URL: &str = "https://github.com/sunven/skills-box/releases/latest";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Idle,
    Checking,
    Current,
    Available,
    Downloading,
    Verifying,
    Ready,
    Installing,
    Installed,
    Restarting,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub version: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub revision: u64,
    pub enabled: bool,
    pub current_version: String,
    pub phase: Phase,
    pub candidate: Option<Candidate>,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub has_verified_download: bool,
    pub last_attempt_at: Option<u64>,
    pub error: Option<AppError>,
}

pub type Progress = Arc<dyn Fn(u64, Option<u64>, bool) + Send + Sync>;

#[async_trait]
pub trait Release: Send + Sync {
    fn candidate(&self) -> Candidate;
    /// Return only bytes whose signature has already been verified.
    async fn download(&self, progress: Progress) -> Result<Vec<u8>, AppError>;
    fn install(&self, bytes: &[u8]) -> Result<(), AppError>;
}

#[async_trait]
pub trait Driver: Send + Sync {
    async fn check(&self) -> Result<Option<Arc<dyn Release>>, AppError>;
    async fn prepare_restart(&self) -> Result<(), AppError>;
    fn restart(&self);
}

struct DisabledDriver;
#[async_trait]
impl Driver for DisabledDriver {
    async fn check(&self) -> Result<Option<Arc<dyn Release>>, AppError> {
        Err(AppError::app_update_unavailable())
    }
    async fn prepare_restart(&self) -> Result<(), AppError> {
        Err(AppError::app_update_unavailable())
    }
    fn restart(&self) {}
}

struct InstallationGuards {
    _configuration: OwnedRwLockWriteGuard<()>,
    _mutation: MutationPermit,
}

struct State {
    snapshot: Snapshot,
    release: Option<Arc<dyn Release>>,
    bytes: Option<Arc<Vec<u8>>>,
    // Retained even after restart() returns; Tauri posts a main-loop event.
    installed: Option<InstallationGuards>,
}

struct Inner {
    state: Mutex<State>,
    driver: Mutex<Arc<dyn Driver>>,
    operation: Arc<Semaphore>,
    configuration: Arc<RwLock<()>>,
    shutting_down: AtomicBool,
    tasks: TaskManager,
}

#[derive(Clone)]
pub struct AppUpdateService(Arc<Inner>);

impl AppUpdateService {
    pub fn new(tasks: TaskManager) -> Self {
        Self::with_driver(tasks, Arc::new(DisabledDriver), false)
    }

    pub fn with_driver(tasks: TaskManager, driver: Arc<dyn Driver>, enabled: bool) -> Self {
        Self(Arc::new(Inner {
            state: Mutex::new(State {
                snapshot: Snapshot {
                    revision: 0,
                    enabled,
                    current_version: env!("CARGO_PKG_VERSION").into(),
                    phase: Phase::Idle,
                    candidate: None,
                    downloaded_bytes: 0,
                    total_bytes: None,
                    has_verified_download: false,
                    last_attempt_at: None,
                    error: None,
                },
                release: None,
                bytes: None,
                installed: None,
            }),
            driver: Mutex::new(driver),
            operation: Arc::new(Semaphore::new(1)),
            configuration: Arc::new(RwLock::new(())),
            shutting_down: AtomicBool::new(false),
            tasks,
        }))
    }

    pub fn configure(&self, driver: Arc<dyn Driver>, enabled: bool) {
        *self.0.driver.lock().unwrap_or_else(|e| e.into_inner()) = driver;
        self.change(|state| state.snapshot.enabled = enabled);
    }

    pub fn snapshot(&self) -> Snapshot {
        self.0
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .snapshot
            .clone()
    }

    fn change(&self, change: impl FnOnce(&mut State)) -> Snapshot {
        let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        change(&mut state);
        state.snapshot.revision += 1;
        state.snapshot.clone()
    }

    fn driver(&self) -> Arc<dyn Driver> {
        self.0
            .driver
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Configuration writes do not serialize with ordinary Skill mutations,
    /// but cannot start or finish across application replacement.
    pub fn configuration_write(&self) -> Result<OwnedRwLockReadGuard<()>, AppError> {
        if self.0.shutting_down.load(Ordering::SeqCst) {
            return Err(AppError::task_shutting_down());
        }
        self.0
            .configuration
            .clone()
            .try_read_owned()
            .map_err(|_| AppError::app_update_busy())
    }

    /// The read guard closes the race between checking installation and quit.
    /// The caller holds it until ordinary shutdown has finished.
    pub fn exit_guard(&self) -> Result<OwnedRwLockReadGuard<()>, AppError> {
        self.0
            .configuration
            .clone()
            .try_read_owned()
            .map_err(|_| AppError::app_update_busy())
    }

    pub fn begin_shutdown(&self) {
        self.0.shutting_down.store(true, Ordering::SeqCst);
    }

    fn begin_operation(&self) -> Result<tokio::sync::OwnedSemaphorePermit, AppError> {
        if self.0.shutting_down.load(Ordering::SeqCst) {
            return Err(AppError::task_shutting_down());
        }
        if !self.snapshot().enabled {
            return Err(AppError::app_update_unavailable());
        }
        self.0
            .operation
            .clone()
            .try_acquire_owned()
            .map_err(|_| AppError::app_update_busy())
    }

    pub fn check(&self) -> Result<Snapshot, AppError> {
        let permit = self.begin_operation()?;
        {
            let state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.bytes.is_some() || state.installed.is_some() {
                return Err(AppError::app_update_busy());
            }
        }
        let snapshot = self.change(|state| {
            state.snapshot.phase = Phase::Checking;
            state.snapshot.error = None;
            state.snapshot.last_attempt_at = Some(crate::time::now_ms());
        });
        let service = self.clone();
        tauri::async_runtime::spawn(async move {
            let _permit = permit;
            let result = service.driver().check().await;
            service.change(|state| match result {
                Ok(release) => {
                    state.snapshot.candidate = release.as_ref().map(|r| r.candidate());
                    state.snapshot.phase = if release.is_some() {
                        Phase::Available
                    } else {
                        Phase::Current
                    };
                    state.release = release;
                    state.bytes = None;
                    state.snapshot.has_verified_download = false;
                    state.snapshot.downloaded_bytes = 0;
                    state.snapshot.total_bytes = None;
                }
                Err(error) => {
                    state.snapshot.phase = Phase::Error;
                    state.snapshot.error = Some(error);
                }
            });
        });
        Ok(snapshot)
    }

    /// Returns immediately. The detached worker owns the whole update run.
    pub fn update(&self) -> Result<Snapshot, AppError> {
        let permit = self.begin_operation()?;
        let (release, bytes, installed) = {
            let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
            let release = state
                .release
                .clone()
                .ok_or_else(AppError::app_update_unavailable)?;
            (release, state.bytes.clone(), state.installed.take())
        };
        let snapshot = self.change(|state| {
            state.snapshot.error = None;
            state.snapshot.phase = if installed.is_some() {
                Phase::Installed
            } else if bytes.is_some() {
                Phase::Ready
            } else {
                Phase::Downloading
            };
        });
        let service = self.clone();
        tauri::async_runtime::spawn(async move {
            let _permit = permit;
            if let Some(guards) = installed {
                service.finish_restart(guards).await;
                return;
            }
            let bytes = match bytes {
                Some(bytes) => bytes,
                None => {
                    let progress_service = service.clone();
                    let progress: Progress = Arc::new(move |chunk, total, finished| {
                        progress_service.change(|state| {
                            state.snapshot.total_bytes = total.or(state.snapshot.total_bytes);
                            state.snapshot.downloaded_bytes =
                                state.snapshot.downloaded_bytes.saturating_add(chunk);
                            if finished {
                                state.snapshot.phase = Phase::Verifying;
                            }
                        });
                    });
                    service.change(|state| {
                        state.snapshot.downloaded_bytes = 0;
                        state.snapshot.total_bytes = None;
                    });
                    match release.download(progress).await {
                        Ok(bytes) => {
                            let bytes = Arc::new(bytes);
                            service.change(|state| {
                                state.bytes = Some(bytes.clone());
                                state.snapshot.has_verified_download = true;
                            });
                            bytes
                        }
                        Err(error) => {
                            service.fail(error);
                            return;
                        }
                    }
                }
            };
            let guards = match service.installation_guards() {
                Ok(guards) => guards,
                Err(error) => {
                    service.change(|state| {
                        state.snapshot.phase = Phase::Ready;
                        state.snapshot.error = Some(error);
                    });
                    return;
                }
            };
            service.change(|state| state.snapshot.phase = Phase::Installing);
            // Guards live in the blocking worker, including if its JoinHandle
            // or any caller is dropped. On completion they return for restart.
            let result =
                tokio::task::spawn_blocking(move || (release.install(&bytes), guards)).await;
            match result {
                Ok((Ok(()), guards)) => {
                    service.change(|state| {
                        state.bytes = None;
                        state.snapshot.has_verified_download = false;
                        state.snapshot.phase = Phase::Installed;
                    });
                    service.finish_restart(guards).await;
                }
                Ok((Err(error), _guards)) => service.fail(error),
                Err(error) => service.fail(AppError::app_update_install_failed(error)),
            }
        });
        Ok(snapshot)
    }

    fn installation_guards(&self) -> Result<InstallationGuards, AppError> {
        let configuration = self
            .0
            .configuration
            .clone()
            .try_write_owned()
            .map_err(|_| AppError::app_update_busy())?;
        if self.0.shutting_down.load(Ordering::SeqCst) {
            return Err(AppError::task_shutting_down());
        }
        let mutation = self.0.tasks.try_acquire_mutation()?;
        Ok(InstallationGuards {
            _configuration: configuration,
            _mutation: mutation,
        })
    }

    async fn finish_restart(&self, guards: InstallationGuards) {
        let driver = self.driver();
        if let Err(error) = driver.prepare_restart().await {
            self.change(|state| {
                state.snapshot.phase = Phase::Installed;
                state.snapshot.error = Some(error);
                state.installed = Some(guards);
            });
            return;
        }
        self.change(|state| {
            state.snapshot.phase = Phase::Restarting;
            state.installed = Some(guards);
        });
        driver.restart();
    }

    fn fail(&self, error: AppError) {
        self.change(|state| {
            state.snapshot.phase = Phase::Error;
            state.snapshot.error = Some(error);
        });
    }
}

#[cfg(target_os = "macos")]
pub mod native;
#[cfg(test)]
mod tests;
