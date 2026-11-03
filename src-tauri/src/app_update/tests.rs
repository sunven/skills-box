use super::*;
use crate::cli::{CliRuntime, CliRuntimeProvider};
use crate::error::ErrorCode;
use crate::paths::AppPaths;
use crate::process::fake::ScriptedExecutor;
use crate::tasks::{InventoryConflictProvider, TaskManagerConfig};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::{Notify, Semaphore};

struct Runtime;
#[async_trait]
impl CliRuntimeProvider for Runtime {
    async fn cli_runtime(&self) -> Result<CliRuntime, AppError> {
        std::future::pending().await
    }
}

struct TestRelease {
    downloads: AtomicUsize,
    installs: AtomicUsize,
    download_gate: Semaphore,
    install_gate: (Mutex<bool>, std::sync::Condvar),
    signature_failure: AtomicBool,
    install_failure: AtomicBool,
    entered_install: Notify,
}
impl TestRelease {
    fn new() -> Self {
        Self {
            downloads: AtomicUsize::new(0),
            installs: AtomicUsize::new(0),
            download_gate: Semaphore::new(0),
            install_gate: (Mutex::new(false), std::sync::Condvar::new()),
            signature_failure: AtomicBool::new(false),
            install_failure: AtomicBool::new(false),
            entered_install: Notify::new(),
        }
    }
    fn finish_install(&self) {
        *self.install_gate.0.lock().unwrap() = true;
        self.install_gate.1.notify_all();
    }
}
#[async_trait]
impl Release for TestRelease {
    fn candidate(&self) -> Candidate {
        Candidate {
            version: "0.2.0".into(),
            notes: Some("Release notes".into()),
        }
    }
    async fn download(&self, progress: Progress) -> Result<Vec<u8>, AppError> {
        self.downloads.fetch_add(1, Ordering::SeqCst);
        progress(4, None, false);
        progress(0, None, true);
        self.download_gate.acquire().await.unwrap().forget();
        if self.signature_failure.load(Ordering::SeqCst) {
            return Err(AppError::app_update_signature_failed());
        }
        Ok(b"verified".to_vec())
    }
    fn install(&self, bytes: &[u8]) -> Result<(), AppError> {
        assert_eq!(bytes, b"verified");
        self.installs.fetch_add(1, Ordering::SeqCst);
        self.entered_install.notify_one();
        let mut done = self.install_gate.0.lock().unwrap();
        while !*done {
            done = self.install_gate.1.wait(done).unwrap();
        }
        if self.install_failure.load(Ordering::SeqCst) {
            return Err(AppError::app_update_install_failed("disk full"));
        }
        Ok(())
    }
}

struct TestDriver {
    release: Arc<TestRelease>,
    checks: AtomicUsize,
    check_gate: Semaphore,
    restarts: AtomicUsize,
    shutdown_failure: AtomicBool,
    no_update: AtomicBool,
    check_failure: AtomicBool,
}
#[async_trait]
impl Driver for TestDriver {
    async fn check(&self) -> Result<Option<Arc<dyn Release>>, AppError> {
        self.checks.fetch_add(1, Ordering::SeqCst);
        self.check_gate.acquire().await.unwrap().forget();
        if self.check_failure.load(Ordering::SeqCst) {
            return Err(AppError::app_update_check_failed("404"));
        }
        Ok((!self.no_update.load(Ordering::SeqCst))
            .then(|| self.release.clone() as Arc<dyn Release>))
    }
    async fn prepare_restart(&self) -> Result<(), AppError> {
        if self.shutdown_failure.load(Ordering::SeqCst) {
            Err(AppError::app_update_install_failed(
                "background not stopped",
            ))
        } else {
            Ok(())
        }
    }
    fn restart(&self) {
        self.restarts.fetch_add(1, Ordering::SeqCst);
    }
}

struct Fixture {
    _temp: tempfile::TempDir,
    tasks: TaskManager,
    service: AppUpdateService,
    driver: Arc<TestDriver>,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(temp.path().join("home"), temp.path().join("data"));
        paths.ensure_dirs().unwrap();
        let tasks = TaskManager::new(
            Arc::new(Runtime),
            Arc::new(ScriptedExecutor::new()),
            Arc::new(InventoryConflictProvider::new(paths.clone())),
            paths,
            TaskManagerConfig::default(),
        );
        let driver = Arc::new(TestDriver {
            release: Arc::new(TestRelease::new()),
            checks: AtomicUsize::new(0),
            check_gate: Semaphore::new(0),
            restarts: AtomicUsize::new(0),
            shutdown_failure: AtomicBool::new(false),
            no_update: AtomicBool::new(false),
            check_failure: AtomicBool::new(false),
        });
        let service = AppUpdateService::with_driver(tasks.clone(), driver.clone(), true);
        Self {
            _temp: temp,
            tasks,
            service,
            driver,
        }
    }
    async fn candidate(&self) {
        self.driver.check_gate.add_permits(1);
        assert_eq!(self.service.check().unwrap().phase, Phase::Checking);
        wait_for(&self.service, Phase::Available).await;
    }
}
async fn wait_for(service: &AppUpdateService, phase: Phase) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while service.snapshot().phase != phase {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("expected {phase:?}, found {:?}", service.snapshot()));
}

#[tokio::test]
async fn checks_distinguish_failure_current_and_candidate_and_are_single_flight() {
    let f = Fixture::new();
    f.driver.check_failure.store(true, Ordering::SeqCst);
    f.service.check().unwrap();
    assert_eq!(
        f.service.check().unwrap_err().code,
        ErrorCode::AppUpdateBusy
    );
    f.driver.check_gate.add_permits(1);
    wait_for(&f.service, Phase::Error).await;
    assert!(f.service.snapshot().last_attempt_at.is_some());
    assert_eq!(
        f.service.snapshot().error.unwrap().code,
        ErrorCode::AppUpdateCheckFailed
    );
    f.driver.check_failure.store(false, Ordering::SeqCst);
    f.driver.no_update.store(true, Ordering::SeqCst);
    f.driver.check_gate.add_permits(1);
    f.service.check().unwrap();
    wait_for(&f.service, Phase::Current).await;
    assert!(f.service.snapshot().candidate.is_none());
    f.driver.no_update.store(false, Ordering::SeqCst);
    f.candidate().await;
    assert_eq!(f.service.snapshot().candidate.unwrap().version, "0.2.0");
}

#[tokio::test]
async fn verified_download_waits_for_unregistered_writer_and_retry_reuses_bytes() {
    let f = Fixture::new();
    f.candidate().await;
    let writer = f.tasks.try_acquire_mutation().unwrap();
    assert!(f.tasks.active_mutation().is_none());
    f.service.update().unwrap();
    wait_for(&f.service, Phase::Verifying).await;
    assert_eq!(f.service.snapshot().total_bytes, None);
    assert!(f.service.configuration_write().is_ok());
    assert_eq!(f.driver.release.installs.load(Ordering::SeqCst), 0);
    f.driver.release.download_gate.add_permits(1);
    wait_for(&f.service, Phase::Ready).await;
    assert!(f.service.snapshot().has_verified_download);
    assert!(f.service.snapshot().error.is_some());
    assert_eq!(
        f.service.check().unwrap_err().code,
        ErrorCode::AppUpdateBusy
    );
    drop(writer);
    f.service.update().unwrap();
    f.driver.release.entered_install.notified().await;
    assert!(f.tasks.try_acquire_mutation().is_err());
    assert!(f.service.configuration_write().is_err());
    assert!(f.service.exit_guard().is_err());
    assert_eq!(f.driver.release.downloads.load(Ordering::SeqCst), 1);
    f.driver.release.finish_install();
    wait_for(&f.service, Phase::Restarting).await;
    assert!(f.tasks.try_acquire_mutation().is_err());
    assert!(f.service.exit_guard().is_err());
}

#[tokio::test]
async fn signature_failure_never_installs_even_after_download_finished_callback() {
    let f = Fixture::new();
    f.candidate().await;
    f.driver
        .release
        .signature_failure
        .store(true, Ordering::SeqCst);
    f.service.update().unwrap();
    wait_for(&f.service, Phase::Verifying).await;
    assert!(f.tasks.try_acquire_mutation().is_ok());
    f.driver.release.download_gate.add_permits(1);
    wait_for(&f.service, Phase::Error).await;
    assert_eq!(f.driver.release.installs.load(Ordering::SeqCst), 0);
    assert_eq!(
        f.service.snapshot().error.unwrap().code,
        ErrorCode::AppUpdateSignatureFailed
    );
}

#[tokio::test]
async fn installed_restart_retry_does_not_download_or_install_again() {
    let f = Fixture::new();
    f.candidate().await;
    f.driver.shutdown_failure.store(true, Ordering::SeqCst);
    f.driver.release.finish_install();
    f.driver.release.download_gate.add_permits(1);
    f.service.update().unwrap();
    wait_for(&f.service, Phase::Installed).await;
    // Installed is published before prepare_restart; wait until its error is published.
    while f.service.snapshot().error.is_none() {
        tokio::task::yield_now().await;
    }
    assert!(f.tasks.try_acquire_mutation().is_err());
    f.driver.shutdown_failure.store(false, Ordering::SeqCst);
    f.service.update().unwrap();
    wait_for(&f.service, Phase::Restarting).await;
    assert_eq!(f.driver.release.downloads.load(Ordering::SeqCst), 1);
    assert_eq!(f.driver.release.installs.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn configuration_write_and_ordinary_exit_cannot_race_installation() {
    let f = Fixture::new();
    f.candidate().await;
    let configuration = f.service.configuration_write().unwrap();
    f.driver.release.download_gate.add_permits(1);
    f.service.update().unwrap();
    wait_for(&f.service, Phase::Ready).await;
    assert_eq!(f.driver.release.installs.load(Ordering::SeqCst), 0);
    drop(configuration);
    let exit = f.service.exit_guard().unwrap();
    f.service.begin_shutdown();
    assert_eq!(
        f.service.update().unwrap_err().code,
        ErrorCode::TaskShuttingDown
    );
    drop(exit);
}

#[tokio::test]
async fn dropping_request_does_not_release_live_installation_and_failure_releases_it() {
    let f = Fixture::new();
    f.candidate().await;
    f.driver
        .release
        .install_failure
        .store(true, Ordering::SeqCst);
    f.driver.release.download_gate.add_permits(1);
    let requester = {
        let service = f.service.clone();
        tokio::spawn(async move { service.update() })
    };
    requester.await.unwrap().unwrap();
    f.driver.release.entered_install.notified().await;
    assert!(f.tasks.try_acquire_mutation().is_err());
    f.driver.release.finish_install();
    wait_for(&f.service, Phase::Error).await;
    assert_eq!(
        f.service.snapshot().error.unwrap().code,
        ErrorCode::AppUpdateInstallFailed
    );
    assert!(f.service.snapshot().has_verified_download);
    assert!(f.tasks.try_acquire_mutation().is_ok());
    assert!(f.service.exit_guard().is_ok());
}

#[tokio::test]
async fn disabled_driver_never_starts_a_real_check() {
    let f = Fixture::new();
    f.service.configure(f.driver.clone(), false);
    assert_eq!(
        f.service.check().unwrap_err().code,
        ErrorCode::AppUpdateUnavailable
    );
    assert_eq!(f.driver.checks.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn synchronous_ipc_threads_can_start_check_and_update_without_a_tokio_context() {
    let f = Fixture::new();
    let service = f.service.clone();
    std::thread::spawn(move || service.check())
        .join()
        .expect("check must not panic outside a Tokio runtime")
        .unwrap();
    f.driver.check_gate.add_permits(1);
    wait_for(&f.service, Phase::Available).await;
    let writer = f.tasks.try_acquire_mutation().unwrap();
    let service = f.service.clone();
    std::thread::spawn(move || service.update())
        .join()
        .expect("update must not panic outside a Tokio runtime")
        .unwrap();
    f.driver.release.download_gate.add_permits(1);
    wait_for(&f.service, Phase::Ready).await;
    assert_eq!(f.driver.release.installs.load(Ordering::SeqCst), 0);
    drop(writer);
}
