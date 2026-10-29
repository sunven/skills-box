//! One serialized mutation protocol shared by install, update and uninstall.
//!
//! The session owns the unit loop, runtime resolution, snapshot recording,
//! cancellation windows, executor invocation, `ExecError` mapping and the
//! final `TaskResult` envelope. Each action owns its plan, units, preflight
//! content, command, verification/bookkeeping, per-unit events and failure or
//! cancellation bookkeeping.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::cli::{CliRuntime, CliRuntimeProvider};
use crate::error::AppError;
use crate::error::cli::{self, CliAction};
use crate::process::{CancelToken, CommandSpec, ExecError, ExecOutput, ProcessExecutor};

use super::manager::{
    EventSink, FinishOutcome, TaskAction, TaskEvent, TaskOverall, TaskPhase, TaskResult,
    TaskSkillResult, TaskSkillStatus, now_ms,
};
use super::summary;

#[derive(Clone)]
pub struct SessionContext {
    pub task_id: String,
    pub sink: EventSink,
    pub cancel: CancelToken,
    pub runtime: Arc<dyn CliRuntimeProvider>,
    pub executor: Arc<dyn ProcessExecutor>,
    pub record_snapshot: Arc<dyn Fn(&Path) + Send + Sync>,
    pub set_cancellable: Arc<dyn Fn(bool) + Send + Sync>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancellationPolicy {
    /// Install/uninstall: after the single unit is sealed, cancellation stays
    /// rejected until the task finishes.
    SealOnly,
    /// Update: cancellation is accepted again between units.
    BetweenUnits,
}

#[derive(Debug, Clone)]
pub struct SessionProfile {
    pub action: TaskAction,
    pub initial_message: String,
    pub success_status: TaskSkillStatus,
    pub total: u32,
    pub cancellation: CancellationPolicy,
}

#[derive(Debug, Clone)]
pub struct StartOutcome<U> {
    pub units: Vec<U>,
    pub initial_rows: Vec<TaskSkillResult>,
    pub log_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProgressMessage {
    pub phase: TaskPhase,
    pub message: String,
    pub skill: Option<String>,
    pub completed: u32,
}

#[derive(Debug, Clone)]
pub struct Preflight {
    pub snapshot: PathBuf,
    /// Emitted by the session after the snapshot is recorded, before the seal.
    pub post_message: Option<ProgressMessage>,
    pub warnings: Vec<AppError>,
}

#[derive(Debug, Clone)]
pub struct FinalProgress {
    pub phase: TaskPhase,
    pub message: String,
    pub skill: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UnitOutcome {
    pub rows: Vec<TaskSkillResult>,
    pub warnings: Vec<AppError>,
    pub backup_path: String,
    /// Emitted after the unit's rows are appended, and after update re-enables
    /// cancellation, so a cancel requested on this event is accepted.
    pub final_progress: Option<FinalProgress>,
}

#[derive(Debug)]
pub enum FailureDisposition {
    TaskFailed(AppError),
    UnitFailed(UnitOutcome),
}

pub trait MutationAction: Send + Sync + 'static {
    type Plan: Send + Sync + Clone + 'static;
    type Unit: Send + Sync + Clone + 'static;

    fn profile(&self, plan: &Self::Plan) -> SessionProfile;

    fn begin(&self, plan: &Self::Plan) -> Result<StartOutcome<Self::Unit>, AppError>;

    fn preflight(&self, unit: &Self::Unit, reporter: &Reporter) -> Result<Preflight, AppError>;

    fn command(&self, unit: &Self::Unit, runtime: &CliRuntime, reporter: &Reporter) -> CommandSpec;

    fn complete(
        &self,
        unit: &Self::Unit,
        output: ExecOutput,
        preflight: &Preflight,
        reporter: &Reporter,
    ) -> Result<UnitOutcome, AppError>;

    fn on_infrastructure_failure(
        &self,
        unit: &Self::Unit,
        error: AppError,
        preflight: Option<&Preflight>,
        reporter: &Reporter,
    ) -> FailureDisposition;

    fn on_cancelled(&self, unit: &Self::Unit, preflight: &Preflight, error: Option<AppError>);
}

#[derive(Clone)]
pub struct Reporter {
    task_id: String,
    sink: EventSink,
    completed: u32,
    total: u32,
}

impl Reporter {
    pub fn progress(&self, phase: TaskPhase, message: impl Into<String>, skill: Option<&str>) {
        self.progress_at(phase, message, self.completed, skill);
    }

    pub fn progress_at(
        &self,
        phase: TaskPhase,
        message: impl Into<String>,
        completed: u32,
        skill: Option<&str>,
    ) {
        let mut event = TaskEvent::progress(&self.task_id, phase, message, completed, self.total);
        event.skill = skill.map(str::to_string);
        emit(&self.sink, event);
    }
}

pub async fn run<A: MutationAction>(
    ctx: SessionContext,
    action: A,
    plan: A::Plan,
) -> FinishOutcome {
    let action = Arc::new(action);
    let profile = action.profile(&plan);
    emit(
        &ctx.sink,
        TaskEvent::progress(
            &ctx.task_id,
            TaskPhase::Preparing,
            profile.initial_message.clone(),
            0,
            profile.total,
        ),
    );
    if ctx.cancel.is_cancelled() {
        return FinishOutcome::Cancelled;
    }

    let runtime = tokio::select! {
        result = ctx.runtime.cli_runtime() => match result {
            Ok(runtime) => runtime,
            Err(error) => return FinishOutcome::Failed(error),
        },
        _ = ctx.cancel.cancelled() => return FinishOutcome::Cancelled,
    };
    if ctx.cancel.is_cancelled() {
        return FinishOutcome::Cancelled;
    }

    let StartOutcome {
        units,
        initial_rows,
        log_path,
    } = match action.begin(&plan) {
        Ok(start) => start,
        Err(error) => return FinishOutcome::Failed(error),
    };
    let total = profile.total;
    let unit_count = units.len();
    let mut rows = initial_rows;
    let mut warnings: Vec<AppError> = Vec::new();
    let mut backup_paths: Vec<String> = Vec::new();

    for (index, unit) in units.iter().enumerate() {
        (ctx.set_cancellable)(true);
        if ctx.cancel.is_cancelled() {
            return cancelled_outcome(
                &profile,
                std::mem::take(&mut rows),
                std::mem::take(&mut warnings),
                &backup_paths,
                log_path.clone(),
            );
        }
        let reporter = Reporter {
            task_id: ctx.task_id.clone(),
            sink: ctx.sink.clone(),
            completed: rows.len() as u32,
            total,
        };

        let preflight = {
            let hook_action = Arc::clone(&action);
            let hook_unit = unit.clone();
            let hook_reporter = reporter.clone();
            let preflight = tokio::task::spawn_blocking(move || {
                hook_action.preflight(&hook_unit, &hook_reporter)
            })
            .await;
            match preflight {
                Ok(Ok(preflight)) => preflight,
                Ok(Err(error)) => return FinishOutcome::Failed(error),
                Err(join) => {
                    let error = AppError::backup_failed(join.to_string());
                    match action.on_infrastructure_failure(unit, error.clone(), None, &reporter) {
                        FailureDisposition::TaskFailed(error) => {
                            return FinishOutcome::Failed(error);
                        }
                        // A preflight failure has no completed unit to keep.
                        FailureDisposition::UnitFailed(_) => return FinishOutcome::Failed(error),
                    }
                }
            }
        };

        (ctx.record_snapshot)(&preflight.snapshot);
        if let Some(message) = &preflight.post_message {
            let mut event = TaskEvent::progress(
                &ctx.task_id,
                message.phase,
                message.message.clone(),
                message.completed,
                total,
            );
            event.skill = message.skill.clone();
            emit(&ctx.sink, event);
        }
        warnings.extend(preflight.warnings.iter().cloned());

        if ctx.cancel.is_cancelled() {
            action.on_cancelled(unit, &preflight, None);
            return cancelled_outcome(
                &profile,
                std::mem::take(&mut rows),
                std::mem::take(&mut warnings),
                &backup_paths,
                log_path.clone(),
            );
        }
        (ctx.set_cancellable)(false);
        if ctx.cancel.is_cancelled() {
            action.on_cancelled(unit, &preflight, None);
            return cancelled_outcome(
                &profile,
                std::mem::take(&mut rows),
                std::mem::take(&mut warnings),
                &backup_paths,
                log_path.clone(),
            );
        }

        let spec = action.command(unit, &runtime, &reporter);
        let output = match ctx.executor.run(spec, ctx.cancel.clone()).await {
            Ok(output) => output,
            Err(ExecError::Cancelled) => {
                action.on_cancelled(unit, &preflight, Some(AppError::cli_cancelled()));
                return cancelled_outcome(
                    &profile,
                    std::mem::take(&mut rows),
                    std::mem::take(&mut warnings),
                    &backup_paths,
                    log_path.clone(),
                );
            }
            Err(error) => {
                let error = cli::classify_exec_error(cli_action(profile.action), error);
                match action.on_infrastructure_failure(unit, error, Some(&preflight), &reporter) {
                    FailureDisposition::TaskFailed(error) => return FinishOutcome::Failed(error),
                    FailureDisposition::UnitFailed(outcome) => {
                        let final_progress = outcome.final_progress.clone();
                        backup_paths.push(outcome.backup_path.clone());
                        rows.extend(outcome.rows);
                        warnings.extend(outcome.warnings);
                        emit_final_progress(
                            &ctx,
                            final_progress.as_ref(),
                            rows.len() as u32,
                            total,
                        );
                        if index + 1 < unit_count {
                            tokio::task::yield_now().await;
                        }
                        continue;
                    }
                }
            }
        };

        let completed = {
            let hook_action = Arc::clone(&action);
            let hook_unit = unit.clone();
            let hook_reporter = reporter.clone();
            let hook_preflight = preflight.clone();
            tokio::task::spawn_blocking(move || {
                hook_action.complete(&hook_unit, output, &hook_preflight, &hook_reporter)
            })
            .await
        };
        let outcome = match completed {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(error)) => return FinishOutcome::Failed(error),
            Err(join) => {
                let error = AppError::verify_failed(join.to_string());
                match action.on_infrastructure_failure(unit, error, Some(&preflight), &reporter) {
                    FailureDisposition::TaskFailed(error) => return FinishOutcome::Failed(error),
                    FailureDisposition::UnitFailed(outcome) => {
                        let final_progress = outcome.final_progress.clone();
                        backup_paths.push(outcome.backup_path.clone());
                        rows.extend(outcome.rows);
                        warnings.extend(outcome.warnings);
                        emit_final_progress(
                            &ctx,
                            final_progress.as_ref(),
                            rows.len() as u32,
                            total,
                        );
                        if index + 1 < unit_count {
                            tokio::task::yield_now().await;
                        }
                        continue;
                    }
                }
            }
        };

        if matches!(profile.cancellation, CancellationPolicy::BetweenUnits) {
            (ctx.set_cancellable)(true);
        }
        let final_progress = outcome.final_progress.clone();
        backup_paths.push(outcome.backup_path.clone());
        rows.extend(outcome.rows);
        warnings.extend(outcome.warnings);
        emit_final_progress(&ctx, final_progress.as_ref(), rows.len() as u32, total);
        if index + 1 < unit_count {
            tokio::task::yield_now().await;
        }
    }

    let overall = summary::overall_for(&rows, profile.success_status);
    let message = summary::message(profile.action, overall, &rows);
    FinishOutcome::Completed(
        build_result(&profile, rows, warnings, &backup_paths, log_path, overall),
        message,
    )
}

fn cancelled_outcome(
    profile: &SessionProfile,
    rows: Vec<TaskSkillResult>,
    warnings: Vec<AppError>,
    backup_paths: &[String],
    log_path: Option<String>,
) -> FinishOutcome {
    if rows.is_empty() {
        return FinishOutcome::Cancelled;
    }
    let overall = TaskOverall::Cancelled;
    let message = summary::message(profile.action, overall, &rows);
    FinishOutcome::Completed(
        build_result(profile, rows, warnings, backup_paths, log_path, overall),
        message,
    )
}

fn build_result(
    profile: &SessionProfile,
    rows: Vec<TaskSkillResult>,
    warnings: Vec<AppError>,
    backup_paths: &[String],
    log_path: Option<String>,
    overall: TaskOverall,
) -> TaskResult {
    TaskResult {
        action: profile.action,
        overall,
        skills: rows,
        warnings,
        backup_path: (backup_paths.len() == 1).then(|| backup_paths[0].clone()),
        log_path,
        started_at: 0,
        finished_at_epoch_ms: now_ms(),
        preview: None,
    }
}

fn emit_final_progress(
    ctx: &SessionContext,
    message: Option<&FinalProgress>,
    completed: u32,
    total: u32,
) {
    let Some(message) = message else {
        return;
    };
    let mut event = TaskEvent::progress(
        &ctx.task_id,
        message.phase,
        message.message.clone(),
        completed,
        total,
    );
    event.skill = message.skill.clone();
    emit(&ctx.sink, event);
}

fn cli_action(action: TaskAction) -> CliAction {
    match action {
        TaskAction::Preview => CliAction::Preview,
        TaskAction::Install => CliAction::Install,
        TaskAction::Update => CliAction::Update,
        TaskAction::Uninstall => CliAction::Uninstall,
    }
}

fn emit(sink: &EventSink, event: TaskEvent) {
    sink(event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::CliRuntime;
    use crate::error::ErrorCode;
    use crate::process::fake::{ScriptedExecutor, ScriptedResponse};
    use std::collections::BTreeMap;
    use std::sync::Mutex;
    use std::time::Duration;

    struct FakeRuntime;

    #[async_trait::async_trait]
    impl CliRuntimeProvider for FakeRuntime {
        async fn cli_runtime(&self) -> Result<CliRuntime, AppError> {
            Ok(CliRuntime {
                npx_path: PathBuf::from("/node/bin/npx"),
                node_dir: PathBuf::from("/node/bin"),
                git_dir: Some(PathBuf::from("/usr/bin")),
                cwd: PathBuf::from("/tmp/skills-box-cwd"),
                home: PathBuf::from("/tmp/skills-box-home"),
                env: BTreeMap::new(),
                pinned_version: "1.7.4".to_string(),
            })
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum FailureMode {
        Task,
        Unit,
    }

    #[derive(Clone)]
    struct FakeAction {
        calls: Arc<Mutex<Vec<String>>>,
        profile: SessionProfile,
        units: Vec<String>,
        initial_rows: Vec<TaskSkillResult>,
        failure_mode: FailureMode,
        cancel: CancelToken,
        cancel_in_preflight: Option<String>,
        cancel_in_complete: Option<String>,
        panic_in_complete: bool,
        final_progress: bool,
    }

    impl FakeAction {
        fn new(
            calls: Arc<Mutex<Vec<String>>>,
            cancel: CancelToken,
            profile: SessionProfile,
            units: Vec<&str>,
        ) -> Self {
            Self {
                calls,
                profile,
                units: units.into_iter().map(str::to_string).collect(),
                initial_rows: Vec::new(),
                failure_mode: FailureMode::Unit,
                cancel,
                cancel_in_preflight: None,
                cancel_in_complete: None,
                panic_in_complete: false,
                final_progress: false,
            }
        }

        fn record(&self, call: impl Into<String>) {
            self.calls.lock().expect("calls").push(call.into());
        }

        fn row(&self, name: &str, status: TaskSkillStatus) -> TaskSkillResult {
            TaskSkillResult {
                name: name.to_string(),
                status,
                error: None,
                backup_path: Some(format!("/tmp/backup-{name}")),
            }
        }
    }

    impl MutationAction for FakeAction {
        type Plan = ();
        type Unit = String;

        fn profile(&self, _plan: &Self::Plan) -> SessionProfile {
            self.profile.clone()
        }

        fn begin(&self, _plan: &Self::Plan) -> Result<StartOutcome<Self::Unit>, AppError> {
            self.record("begin");
            Ok(StartOutcome {
                units: self.units.clone(),
                initial_rows: self.initial_rows.clone(),
                log_path: Some("/tmp/operations.jsonl".to_string()),
            })
        }

        fn preflight(
            &self,
            unit: &Self::Unit,
            _reporter: &Reporter,
        ) -> Result<Preflight, AppError> {
            self.record(format!("preflight:{unit}"));
            if self.cancel_in_preflight.as_deref() == Some(unit.as_str()) {
                self.cancel.cancel();
            }
            Ok(Preflight {
                snapshot: PathBuf::from(format!("/tmp/snap-{unit}")),
                post_message: None,
                warnings: Vec::new(),
            })
        }

        fn command(
            &self,
            unit: &Self::Unit,
            _runtime: &CliRuntime,
            _reporter: &Reporter,
        ) -> CommandSpec {
            self.record(format!("command:{unit}"));
            CommandSpec::new(
                "/bin/true",
                vec![unit.clone()],
                "/tmp",
                BTreeMap::new(),
                Duration::from_secs(1),
            )
        }

        fn complete(
            &self,
            unit: &Self::Unit,
            _output: ExecOutput,
            _preflight: &Preflight,
            _reporter: &Reporter,
        ) -> Result<UnitOutcome, AppError> {
            self.record(format!("complete:{unit}"));
            if self.panic_in_complete {
                panic!("fake complete panic");
            }
            if self.cancel_in_complete.as_deref() == Some(unit.as_str()) {
                self.cancel.cancel();
            }
            Ok(UnitOutcome {
                rows: vec![self.row(unit, self.profile.success_status)],
                warnings: Vec::new(),
                backup_path: format!("/tmp/backup-{unit}"),
                final_progress: self.final_progress.then(|| FinalProgress {
                    phase: TaskPhase::Verifying,
                    message: format!("processed {unit}"),
                    skill: Some(unit.clone()),
                }),
            })
        }

        fn on_infrastructure_failure(
            &self,
            unit: &Self::Unit,
            error: AppError,
            preflight: Option<&Preflight>,
            _reporter: &Reporter,
        ) -> FailureDisposition {
            self.record(format!("failure:{unit}"));
            match self.failure_mode {
                FailureMode::Task => FailureDisposition::TaskFailed(error),
                FailureMode::Unit => FailureDisposition::UnitFailed(UnitOutcome {
                    rows: vec![self.row(unit, TaskSkillStatus::Failed)],
                    warnings: Vec::new(),
                    backup_path: preflight
                        .map(|preflight| preflight.snapshot.display().to_string())
                        .unwrap_or_else(|| format!("/tmp/backup-{unit}")),
                    final_progress: self.final_progress.then(|| FinalProgress {
                        phase: TaskPhase::Verifying,
                        message: format!("processed {unit}"),
                        skill: Some(unit.clone()),
                    }),
                }),
            }
        }

        fn on_cancelled(
            &self,
            unit: &Self::Unit,
            _preflight: &Preflight,
            _error: Option<AppError>,
        ) {
            self.record(format!("cancelled:{unit}"));
        }
    }

    struct Harness {
        ctx: SessionContext,
        snapshots: Arc<Mutex<Vec<PathBuf>>>,
        cancellable: Arc<Mutex<Vec<bool>>>,
        calls: Arc<Mutex<Vec<String>>>,
    }

    fn harness(executor: Arc<dyn ProcessExecutor>, cancel: CancelToken) -> Harness {
        let (sender, _events) = tokio::sync::mpsc::unbounded_channel();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let sink_calls = Arc::clone(&calls);
        let sink: EventSink = Arc::new(move |event| {
            sink_calls
                .lock()
                .expect("calls")
                .push(format!("event:{:?}:{}", event.phase, event.message));
            let _ = sender.send(event);
        });
        let snapshots = Arc::new(Mutex::new(Vec::new()));
        let record_snapshots = Arc::clone(&snapshots);
        let snapshot_calls = Arc::clone(&calls);
        let record_snapshot = Arc::new(move |path: &Path| {
            record_snapshots
                .lock()
                .expect("snapshots")
                .push(path.to_path_buf());
            snapshot_calls
                .lock()
                .expect("calls")
                .push(format!("snapshot:{}", path.display()));
        });
        let cancellable = Arc::new(Mutex::new(Vec::new()));
        let record_cancellable = Arc::clone(&cancellable);
        let cancellable_calls = Arc::clone(&calls);
        let set_cancellable = Arc::new(move |value: bool| {
            record_cancellable.lock().expect("cancellable").push(value);
            cancellable_calls
                .lock()
                .expect("calls")
                .push(format!("cancellable:{value}"));
        });
        Harness {
            ctx: SessionContext {
                task_id: "task-1".to_string(),
                sink,
                cancel,
                runtime: Arc::new(FakeRuntime),
                executor,
                record_snapshot,
                set_cancellable,
            },
            snapshots,
            cancellable,
            calls,
        }
    }

    fn profile(
        action: TaskAction,
        success_status: TaskSkillStatus,
        total: u32,
        cancellation: CancellationPolicy,
    ) -> SessionProfile {
        SessionProfile {
            action,
            initial_message: "正在准备…".to_string(),
            success_status,
            total,
            cancellation,
        }
    }

    fn calls(harness: &Harness) -> Vec<String> {
        harness.calls.lock().expect("calls").clone()
    }

    #[tokio::test]
    async fn one_unit_runs_hooks_in_order_and_assembles_the_envelope() {
        let cancel = CancelToken::new();
        let executor = Arc::new(ScriptedExecutor::new().expect_any(ScriptedResponse::ok("{}")));
        let harness = harness(executor.clone(), cancel.clone());
        let action = FakeAction::new(
            Arc::clone(&harness.calls),
            cancel,
            profile(
                TaskAction::Install,
                TaskSkillStatus::Installed,
                1,
                CancellationPolicy::SealOnly,
            ),
            vec!["alpha"],
        );

        let outcome = run(harness.ctx.clone(), action, ()).await;
        let FinishOutcome::Completed(result, message) = outcome else {
            panic!("expected completed");
        };
        assert_eq!(result.action, TaskAction::Install);
        assert_eq!(result.overall, TaskOverall::Success);
        assert_eq!(result.skills.len(), 1);
        assert_eq!(result.skills[0].status, TaskSkillStatus::Installed);
        assert_eq!(result.backup_path.as_deref(), Some("/tmp/backup-alpha"));
        assert_eq!(result.log_path.as_deref(), Some("/tmp/operations.jsonl"));
        assert_eq!(message, "安装完成，已验证 1 个 skill。");
        assert_eq!(
            harness.snapshots.lock().expect("snapshots").as_slice(),
            &[PathBuf::from("/tmp/snap-alpha")]
        );

        let calls = calls(&harness);
        let begin = calls.iter().position(|call| call == "begin").unwrap();
        let preflight = calls
            .iter()
            .position(|call| call == "preflight:alpha")
            .unwrap();
        let snapshot = calls
            .iter()
            .position(|call| call == "snapshot:/tmp/snap-alpha")
            .unwrap();
        let command = calls
            .iter()
            .position(|call| call == "command:alpha")
            .unwrap();
        let complete = calls
            .iter()
            .position(|call| call == "complete:alpha")
            .unwrap();
        assert!(
            begin < preflight && preflight < snapshot && snapshot < command && command < complete
        );
        assert_eq!(
            harness.cancellable.lock().expect("cancellable").as_slice(),
            &[true, false]
        );
    }

    #[tokio::test]
    async fn unit_failure_continues_and_finishes_as_partial() {
        let cancel = CancelToken::new();
        let executor = Arc::new(
            ScriptedExecutor::new()
                .expect_any(ScriptedResponse::timeout())
                .expect_any(ScriptedResponse::ok("{}")),
        );
        let harness = harness(executor.clone(), cancel.clone());
        let mut action = FakeAction::new(
            Arc::clone(&harness.calls),
            cancel,
            profile(
                TaskAction::Update,
                TaskSkillStatus::Updated,
                2,
                CancellationPolicy::BetweenUnits,
            ),
            vec!["alpha", "beta"],
        );
        action.failure_mode = FailureMode::Unit;

        let outcome = run(harness.ctx.clone(), action, ()).await;
        let FinishOutcome::Completed(result, _) = outcome else {
            panic!("expected completed");
        };
        assert_eq!(result.overall, TaskOverall::Partial);
        assert_eq!(result.skills.len(), 2);
        assert_eq!(result.skills[0].status, TaskSkillStatus::Failed);
        assert_eq!(result.skills[1].status, TaskSkillStatus::Updated);
        assert_eq!(result.backup_path, None);
        assert!(calls(&harness).iter().any(|call| call == "failure:alpha"));
        assert!(calls(&harness).iter().any(|call| call == "complete:beta"));
    }

    #[tokio::test]
    async fn task_failure_aborts_without_rows() {
        let cancel = CancelToken::new();
        let executor = Arc::new(ScriptedExecutor::new().expect_any(ScriptedResponse::timeout()));
        let harness = harness(executor.clone(), cancel.clone());
        let mut action = FakeAction::new(
            Arc::clone(&harness.calls),
            cancel,
            profile(
                TaskAction::Install,
                TaskSkillStatus::Installed,
                1,
                CancellationPolicy::SealOnly,
            ),
            vec!["alpha"],
        );
        action.failure_mode = FailureMode::Task;

        let outcome = run(harness.ctx.clone(), action, ()).await;
        let FinishOutcome::Failed(error) = outcome else {
            panic!("expected failed");
        };
        assert_eq!(error.code, ErrorCode::CliTimeout);
        assert!(!calls(&harness).iter().any(|call| call == "complete:alpha"));
    }

    #[tokio::test]
    async fn cancel_after_preflight_logs_and_keeps_prior_rows() {
        let cancel = CancelToken::new();
        let executor = Arc::new(ScriptedExecutor::new().expect_any(ScriptedResponse::ok("{}")));
        let harness = harness(executor.clone(), cancel.clone());
        let mut action = FakeAction::new(
            Arc::clone(&harness.calls),
            cancel,
            profile(
                TaskAction::Update,
                TaskSkillStatus::Updated,
                2,
                CancellationPolicy::BetweenUnits,
            ),
            vec!["alpha", "beta"],
        );
        action.cancel_in_preflight = Some("beta".to_string());

        let outcome = run(harness.ctx.clone(), action, ()).await;
        let FinishOutcome::Completed(result, _) = outcome else {
            panic!("expected completed with prior rows");
        };
        assert_eq!(result.overall, TaskOverall::Cancelled);
        assert_eq!(result.skills.len(), 1);
        assert_eq!(result.skills[0].name, "alpha");
        assert!(calls(&harness).iter().any(|call| call == "cancelled:beta"));
        assert!(!calls(&harness).iter().any(|call| call == "command:beta"));
    }

    #[tokio::test]
    async fn cancellation_between_units_keeps_rows_without_a_cancel_hook() {
        let cancel = CancelToken::new();
        let executor = Arc::new(ScriptedExecutor::new().expect_any(ScriptedResponse::ok("{}")));
        let harness = harness(executor.clone(), cancel.clone());
        let mut action = FakeAction::new(
            Arc::clone(&harness.calls),
            cancel,
            profile(
                TaskAction::Update,
                TaskSkillStatus::Updated,
                2,
                CancellationPolicy::BetweenUnits,
            ),
            vec!["alpha", "beta"],
        );
        action.cancel_in_complete = Some("alpha".to_string());

        let outcome = run(harness.ctx.clone(), action, ()).await;
        let FinishOutcome::Completed(result, _) = outcome else {
            panic!("expected completed with prior rows");
        };
        assert_eq!(result.overall, TaskOverall::Cancelled);
        assert_eq!(result.skills.len(), 1);
        assert!(!calls(&harness).iter().any(|call| call == "cancelled:alpha"));
        assert!(!calls(&harness).iter().any(|call| call == "preflight:beta"));
    }

    #[tokio::test]
    async fn final_progress_is_emitted_after_cancellation_is_re_enabled() {
        let cancel = CancelToken::new();
        let executor = Arc::new(ScriptedExecutor::new().expect_any(ScriptedResponse::ok("{}")));
        let harness = harness(executor.clone(), cancel.clone());
        let mut action = FakeAction::new(
            Arc::clone(&harness.calls),
            cancel,
            profile(
                TaskAction::Update,
                TaskSkillStatus::Updated,
                1,
                CancellationPolicy::BetweenUnits,
            ),
            vec!["alpha"],
        );
        action.final_progress = true;

        let outcome = run(harness.ctx.clone(), action, ()).await;
        assert!(matches!(outcome, FinishOutcome::Completed(_, _)));
        let calls = calls(&harness);
        let complete = calls
            .iter()
            .position(|call| call == "complete:alpha")
            .expect("complete");
        let re_enabled = calls
            .iter()
            .rposition(|call| call == "cancellable:true")
            .expect("re-enable");
        let final_progress = calls
            .iter()
            .position(|call| call == "event:Verifying:processed alpha")
            .expect("final progress");
        assert!(complete < re_enabled && re_enabled < final_progress);
    }

    #[tokio::test]
    async fn complete_join_error_uses_the_action_failure_disposition() {
        let cancel = CancelToken::new();
        let executor = Arc::new(ScriptedExecutor::new().expect_any(ScriptedResponse::ok("{}")));
        let harness = harness(executor.clone(), cancel.clone());
        let mut action = FakeAction::new(
            Arc::clone(&harness.calls),
            cancel,
            profile(
                TaskAction::Update,
                TaskSkillStatus::Updated,
                1,
                CancellationPolicy::BetweenUnits,
            ),
            vec!["alpha"],
        );
        action.panic_in_complete = true;
        action.failure_mode = FailureMode::Unit;

        let outcome = run(harness.ctx.clone(), action, ()).await;
        let FinishOutcome::Completed(result, _) = outcome else {
            panic!("expected completed with a failed row");
        };
        assert_eq!(result.overall, TaskOverall::Failed);
        assert_eq!(result.skills[0].status, TaskSkillStatus::Failed);
        assert!(calls(&harness).iter().any(|call| call == "failure:alpha"));
    }
}
