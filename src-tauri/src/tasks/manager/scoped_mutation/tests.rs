use super::*;
use crate::cli::{CliRuntime, CliRuntimeProvider};
use crate::error::ErrorCode;
use crate::process::fake::ScriptedExecutor;
use crate::projects::ProjectRegistry;
use crate::tasks::install::{InstallAction, InstallRequest, validate_request};
use crate::tasks::{
    InventoryConflictProvider, RegistryChangeGuard, TaskEvent, TaskManagerConfig, TaskPhase,
};
use std::fs;
use std::sync::{Arc, mpsc};
use std::time::Duration;
use tokio::sync::{mpsc::UnboundedReceiver, oneshot};

struct PendingRuntime;

#[async_trait::async_trait]
impl CliRuntimeProvider for PendingRuntime {
    async fn cli_runtime(&self) -> Result<CliRuntime, AppError> {
        std::future::pending().await
    }
}

struct Fixture {
    _tmp: tempfile::TempDir,
    manager: TaskManager,
    executor: Arc<ScriptedExecutor>,
    scope: ScopeId,
}

impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(tmp.path().join("home"), tmp.path().join("data"));
        paths.ensure_dirs().unwrap();
        let root = tmp.path().join("project");
        fs::create_dir_all(&root).unwrap();
        let projects = Arc::new(ProjectRegistry::new(paths.clone()));
        let scope = projects
            .add(root.to_str().unwrap())
            .unwrap()
            .project
            .scope();
        let executor = Arc::new(ScriptedExecutor::new());
        let manager = TaskManager::with_projects(
            Arc::new(PendingRuntime),
            executor.clone(),
            Arc::new(InventoryConflictProvider::new(paths.clone())),
            paths,
            projects,
            TaskManagerConfig::default(),
        );
        Self {
            _tmp: tmp,
            manager,
            executor,
            scope,
        }
    }

    fn request() -> InstallRequest {
        InstallRequest {
            source: "acme/skills".into(),
            skills: vec!["alpha".into()],
            agents: vec![],
            retry: false,
        }
    }

    fn sink() -> (EventSink, UnboundedReceiver<TaskEvent>) {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        (
            Arc::new(move |event| {
                let _ = sender.send(event);
            }),
            receiver,
        )
    }
}

struct WorkerPause {
    started: oneshot::Sender<()>,
    resume: mpsc::Receiver<()>,
}

impl WorkerPause {
    fn new() -> (Self, oneshot::Receiver<()>, mpsc::Sender<()>) {
        let (started, waiting) = oneshot::channel();
        let (resume, gate) = mpsc::channel();
        (
            Self {
                started,
                resume: gate,
            },
            waiting,
            resume,
        )
    }

    fn wait(self) {
        self.started.send(()).unwrap();
        self.resume.recv().expect("test releases the worker");
    }
}

async fn started(waiting: oneshot::Receiver<()>) {
    tokio::time::timeout(Duration::from_secs(2), waiting)
        .await
        .unwrap()
        .unwrap();
}

async fn available(manager: &TaskManager) -> RegistryChangeGuard {
    // Observe the public refusal until the detached worker actually finishes.
    // No sleep or knowledge of its internal drop order is needed.
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match manager.begin_registry_change() {
                Ok(guard) => break guard,
                Err(error) if error.code == ErrorCode::TaskBusy => tokio::task::yield_now().await,
                Err(error) => panic!("unexpected refusal: {error}"),
            }
        }
    })
    .await
    .expect("completed worker releases the write slot")
}

#[tokio::test]
async fn registry_guard_blocks_all_five_paths_before_resolving_scope() {
    let fixture = Fixture::new();
    let missing = ScopeId::project(uuid::Uuid::new_v4());
    let manager = &fixture.manager;
    let mut guard = Some(manager.begin_registry_change().unwrap());
    for expected in [ErrorCode::TaskBusy, ErrorCode::ProjectNotFound] {
        let (sink, _) = Fixture::sink();
        assert_eq!(
            manager
                .start_install(missing.clone(), Fixture::request(), sink)
                .await
                .unwrap_err()
                .code,
            expected
        );
        let (sink, _) = Fixture::sink();
        assert_eq!(
            manager
                .start_update(missing.clone(), vec!["alpha".into()], sink)
                .await
                .unwrap_err()
                .code,
            expected
        );
        let (sink, _) = Fixture::sink();
        assert_eq!(
            manager
                .start_uninstall(missing.clone(), "alpha".into(), sink)
                .await
                .unwrap_err()
                .code,
            expected
        );
        assert_eq!(
            manager
                .attach_agent(missing.clone(), "alpha".into(), "qoder".into())
                .await
                .unwrap_err()
                .code,
            expected
        );
        assert_eq!(
            manager
                .detach_agent(missing.clone(), "alpha".into(), "qoder".into())
                .await
                .unwrap_err()
                .code,
            expected
        );
        drop(guard.take());
    }
    drop(manager.begin_registry_change().unwrap());
    assert_eq!(fixture.executor.call_count(), 0);
}

#[tokio::test]
async fn request_error_precedence_is_preserved() {
    let fixture = Fixture::new();
    let manager = &fixture.manager;
    let guard = manager.begin_registry_change().unwrap();
    let mut invalid_install = Fixture::request();
    invalid_install.skills = vec!["../invalid".into()];
    let (sink, _) = Fixture::sink();
    assert_eq!(
        manager
            .start_install(fixture.scope.clone(), invalid_install, sink)
            .await
            .unwrap_err()
            .code,
        ErrorCode::TaskBusy,
    );
    let (sink, _) = Fixture::sink();
    assert_eq!(
        manager
            .start_update(fixture.scope.clone(), vec!["../invalid".into()], sink)
            .await
            .unwrap_err()
            .code,
        ErrorCode::SkillInvalidName,
    );
    drop(guard);
    manager.shutdown_mutations().await;
    let (sink, _) = Fixture::sink();
    assert_eq!(
        manager
            .start_update(fixture.scope.clone(), vec!["../invalid".into()], sink)
            .await
            .unwrap_err()
            .code,
        ErrorCode::TaskShuttingDown,
    );
}

#[tokio::test]
async fn abandoned_planning_keeps_the_write_slot_until_the_worker_finishes() {
    let fixture = Fixture::new();
    let manager = fixture.manager.clone();
    let scope = fixture.scope.clone();
    let (pause, waiting, resume) = WorkerPause::new();
    let requesting = tokio::spawn(async move {
        manager
            .plan_scoped_mutation(scope, MutationKind::Install, move |paths, layout| {
                pause.wait();
                validate_request(paths, layout, &Fixture::request())
            })
            .await
    });
    started(waiting).await;
    assert!(fixture.manager.active_mutation().is_none());
    requesting.abort();
    assert!(requesting.await.err().unwrap().is_cancelled());
    assert_eq!(
        fixture.manager.begin_registry_change().unwrap_err().code,
        ErrorCode::TaskBusy
    );
    resume.send(()).unwrap();
    drop(available(&fixture.manager).await);
    assert!(fixture.manager.interrupted_report().is_none());
    assert_eq!(fixture.executor.call_count(), 0);
}

#[tokio::test]
async fn abandoned_native_work_keeps_the_write_slot_until_completion() {
    let fixture = Fixture::new();
    let manager = fixture.manager.clone();
    let scope = fixture.scope.clone();
    let (pause, waiting, resume) = WorkerPause::new();
    let requesting = tokio::spawn(async move {
        manager
            .run_scoped_native(
                scope,
                move |_, _| {
                    pause.wait();
                    Ok(())
                },
                |error| AppError::agent_detach_failed(error.to_string()),
            )
            .await
    });
    started(waiting).await;
    requesting.abort();
    assert!(requesting.await.unwrap_err().is_cancelled());
    assert_eq!(
        fixture.manager.begin_registry_change().unwrap_err().code,
        ErrorCode::TaskBusy
    );
    resume.send(()).unwrap();
    drop(available(&fixture.manager).await);
    assert!(fixture.manager.active_mutation().is_none());
    assert_eq!(fixture.executor.call_count(), 0);
}

#[tokio::test]
async fn planning_errors_and_panics_release_the_scope_without_registering() {
    for panic in [false, true] {
        let fixture = Fixture::new();
        let result = fixture
            .manager
            .plan_scoped_mutation(fixture.scope.clone(), MutationKind::Install, move |_, _| {
                if panic {
                    panic!("planning fixture panic");
                }
                Err::<(), _>(AppError::skill_invalid_name("planning fixture error"))
            })
            .await;
        let error = result.err().expect("planning fails");
        assert_eq!(
            error.code,
            if panic {
                ErrorCode::TaskInternal
            } else {
                ErrorCode::SkillInvalidName
            }
        );
        drop(fixture.manager.begin_registry_change().unwrap());
        assert!(fixture.manager.active_mutation().is_none());
        assert!(fixture.manager.interrupted_report().is_none());
        assert_eq!(fixture.executor.call_count(), 0);
    }
}

#[tokio::test]
async fn shutdown_during_planning_prevents_registration_and_cli_execution() {
    let fixture = Fixture::new();
    let manager = fixture.manager.clone();
    let scope = fixture.scope.clone();
    let (pause, waiting, resume) = WorkerPause::new();
    let requesting = tokio::spawn(async move {
        let planned = manager
            .plan_scoped_mutation(scope, MutationKind::Install, move |paths, layout| {
                pause.wait();
                validate_request(paths, layout, &Fixture::request())
            })
            .await?;
        let skills = planned.plan().skills.clone();
        let (sink, _) = Fixture::sink();
        planned.start(skills, sink, |layout, id| {
            InstallAction::new(
                manager.inner.paths.clone(),
                layout,
                manager.inner.config.install_timeout,
                id,
            )
        })
    });
    started(waiting).await;
    let report = fixture.manager.shutdown_mutations().await;
    assert!(report.active.is_none());
    assert_eq!(report.cancelled_tasks, 0);
    resume.send(()).unwrap();
    assert_eq!(
        requesting.await.unwrap().unwrap_err().code,
        ErrorCode::TaskShuttingDown
    );
    assert!(fixture.manager.active_mutation().is_none());
    assert!(fixture.manager.interrupted_report().is_none());
    assert_eq!(fixture.executor.call_count(), 0);
}

#[tokio::test]
async fn the_owned_scope_covers_execution_and_releases_after_cancellation() {
    let fixture = Fixture::new();
    let manager = &fixture.manager;
    let planned = manager
        .plan_scoped_mutation(
            fixture.scope.clone(),
            MutationKind::Install,
            |paths, layout| validate_request(paths, layout, &Fixture::request()),
        )
        .await
        .unwrap();
    let skills = planned.plan().skills.clone();
    let (sink, mut receiver) = Fixture::sink();
    let id = planned
        .start(skills, sink, |layout, id| {
            InstallAction::new(
                manager.inner.paths.clone(),
                layout,
                manager.inner.config.install_timeout,
                id,
            )
        })
        .unwrap();
    let event = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(event.phase, TaskPhase::Preparing);
    assert_eq!(manager.active_mutation().unwrap().scope, fixture.scope);
    assert_eq!(
        manager.begin_registry_change().unwrap_err().code,
        ErrorCode::TaskBusy
    );
    manager.cancel_task(&id).unwrap();
    let event = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(event.phase, TaskPhase::Cancelled);
    assert_eq!(event.result.unwrap().scope, fixture.scope);
    drop(available(manager).await);
    assert!(manager.active_mutation().is_none());
    assert!(manager.interrupted_report().is_none());
    assert_eq!(fixture.executor.call_count(), 0);
}
