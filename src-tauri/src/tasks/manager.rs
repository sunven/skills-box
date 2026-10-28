//! Mutation task foundation: task ids, per-task channels, phases, progress,
//! single-writer mutation permits, and cancellable preview tasks.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::sync::Semaphore;

use crate::cli::{CliRuntimeProvider, preview_command};
use crate::error::AppError;
use crate::lock::LockFile;
use crate::paths::AppPaths;
use crate::process::{CancelToken, ExecError, ProcessExecutor};

use super::conflicts::{ConflictInfo, ConflictProvider};
use super::install::{
    self, INSTALL_TIMEOUT, InstallOutput, InstallPlan, InstallRequest,
    install_command as build_install_command, parse_install_output, snapshot_lock, summary_message,
    validate_request, verify_install,
};
use super::parser::{ParsedSkill, PreviewParseOutcome, parse_preview_output};
use super::source::{SourceSpec, validate_source};

pub type TaskId = String;
pub type EventSink = Arc<dyn Fn(TaskEvent) + Send + Sync>;

pub const PREVIEW_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPhase {
    Preparing,
    /// Preview clones the repository before parsing its SKILL.md files.
    Cloning,
    RunningCli,
    Verifying,
    Completed,
    Failed,
    Cancelled,
}

impl TaskPhase {
    pub fn is_final(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskOverall {
    Success,
    Partial,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskSkillStatus {
    Installed,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSkillResult {
    pub name: String,
    pub status: TaskSkillStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AppError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskResult {
    pub overall: TaskOverall,
    pub skills: Vec<TaskSkillResult>,
    pub warnings: Vec<AppError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_path: Option<String>,
    pub finished_at_epoch_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<PreviewSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSkill {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflict: Option<ConflictInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSummary {
    /// Normalized `owner/repo`.
    pub source: String,
    pub skills: Vec<PreviewSkill>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_output: Option<String>,
    pub parse_failed: bool,
    pub no_skills: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskEvent {
    pub task_id: String,
    pub phase: TaskPhase,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<TaskResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AppError>,
}

impl TaskEvent {
    fn progress(
        task_id: &str,
        phase: TaskPhase,
        message: impl Into<String>,
        completed: u32,
        total: u32,
    ) -> Self {
        Self {
            task_id: task_id.to_string(),
            phase,
            message: message.into(),
            skill: None,
            completed: Some(completed),
            total: Some(total),
            result: None,
            error: None,
        }
    }

    fn finished(
        task_id: &str,
        phase: TaskPhase,
        message: impl Into<String>,
        completed: u32,
        total: u32,
        result: TaskResult,
        error: Option<AppError>,
    ) -> Self {
        Self {
            task_id: task_id.to_string(),
            phase,
            message: message.into(),
            skill: None,
            completed: Some(completed),
            total: Some(total),
            result: Some(result),
            error,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelAck {
    pub task_id: String,
    pub accepted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TaskManagerConfig {
    pub preview_timeout: Duration,
    pub install_timeout: Duration,
}

impl Default for TaskManagerConfig {
    fn default() -> Self {
        Self {
            preview_timeout: PREVIEW_TIMEOUT,
            install_timeout: INSTALL_TIMEOUT,
        }
    }
}

pub struct TaskManager {
    inner: Arc<Inner>,
}

struct Inner {
    runtime: Arc<dyn CliRuntimeProvider>,
    executor: Arc<dyn ProcessExecutor>,
    conflicts: Arc<dyn ConflictProvider>,
    paths: AppPaths,
    tasks: Mutex<HashMap<TaskId, TaskEntry>>,
    next_id: AtomicU64,
    mutation_slots: Arc<Semaphore>,
    config: TaskManagerConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskState {
    Running,
    Finished,
}

struct TaskEntry {
    state: TaskState,
    cancel: CancelToken,
    /// Install tasks become non-cancellable immediately before the CLI spawns.
    cancellable: bool,
}

#[derive(Debug)]
enum FinishOutcome {
    Completed(TaskResult, String),
    Failed(AppError),
    Cancelled,
}

/// Held for the duration of a mutation. `try_acquire_mutation` backs the
/// serialized install/update/uninstall queue; preview never takes this permit.
pub struct MutationPermit {
    _permit: tokio::sync::OwnedSemaphorePermit,
}

impl std::fmt::Debug for MutationPermit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MutationPermit")
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for TaskManager {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = self
            .inner
            .tasks
            .lock()
            .map(|tasks| tasks.len())
            .unwrap_or_default();
        formatter
            .debug_struct("TaskManager")
            .field("tasks", &count)
            .finish_non_exhaustive()
    }
}

impl TaskManager {
    pub fn new(
        runtime: Arc<dyn CliRuntimeProvider>,
        executor: Arc<dyn ProcessExecutor>,
        conflicts: Arc<dyn ConflictProvider>,
        paths: AppPaths,
        config: TaskManagerConfig,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                runtime,
                executor,
                conflicts,
                paths,
                tasks: Mutex::new(HashMap::new()),
                next_id: AtomicU64::new(1),
                mutation_slots: Arc::new(Semaphore::new(1)),
                config,
            }),
        }
    }

    /// Try to become the single active mutation. Concurrent callers get
    /// `task.busy` immediately (no queueing) so the UI can report it.
    pub fn try_acquire_mutation(&self) -> Result<MutationPermit, AppError> {
        Arc::clone(&self.inner.mutation_slots)
            .try_acquire_owned()
            .map(|permit| MutationPermit { _permit: permit })
            .map_err(|_| AppError::task_busy())
    }

    /// Validate the source synchronously, then run a read-only preview task.
    /// Preview does not acquire the mutation semaphore.
    pub async fn start_preview(
        &self,
        source: &str,
        on_event: EventSink,
    ) -> Result<TaskId, AppError> {
        let spec = validate_source(source)?;
        let task_id = format!("task-{}", self.inner.next_id.fetch_add(1, Ordering::SeqCst));
        let cancel = CancelToken::new();
        {
            let mut tasks = self.lock_tasks();
            tasks.insert(
                task_id.clone(),
                TaskEntry {
                    state: TaskState::Running,
                    cancel: cancel.clone(),
                    cancellable: true,
                },
            );
        }

        let manager = self.clone();
        let spawned_id = task_id.clone();
        tokio::spawn(async move {
            manager
                .run_preview(spawned_id, spec, cancel, on_event)
                .await;
        });
        Ok(task_id)
    }

    /// Validate a mutation request synchronously, acquire the single-writer
    /// permit, and run the install task. Same-name conflicts are rejected
    /// before a task exists.
    pub async fn start_install(
        &self,
        request: InstallRequest,
        on_event: EventSink,
    ) -> Result<TaskId, AppError> {
        let paths = self.inner.paths.clone();
        let plan = tokio::task::spawn_blocking(move || {
            let plan = validate_request(&paths, &request)?;
            if let Some(conflict) = install::conflicts_for(&paths, &plan.skills)? {
                return Err(conflict);
            }
            Ok::<InstallPlan, AppError>(plan)
        })
        .await
        .map_err(|error| AppError::cli_failed(format!("校验安装请求失败：{error}")))??;

        // The permit is taken after validation so invalid requests do not
        // briefly occupy the write slot, and held for the whole task.
        let permit = self.try_acquire_mutation()?;
        let task_id = format!("task-{}", self.inner.next_id.fetch_add(1, Ordering::SeqCst));
        let cancel = CancelToken::new();
        {
            let mut tasks = self.lock_tasks();
            tasks.insert(
                task_id.clone(),
                TaskEntry {
                    state: TaskState::Running,
                    cancel: cancel.clone(),
                    cancellable: true,
                },
            );
        }

        let manager = self.clone();
        let spawned_id = task_id.clone();
        tokio::spawn(async move {
            manager
                .run_install(spawned_id, plan, permit, cancel, on_event)
                .await;
        });
        Ok(task_id)
    }

    pub fn cancel_task(&self, task_id: &str) -> Result<CancelAck, AppError> {
        let tasks = self.lock_tasks();
        let Some(entry) = tasks.get(task_id) else {
            return Err(AppError::task_not_found());
        };
        if entry.state == TaskState::Finished || !entry.cancellable {
            return Err(AppError::task_cannot_cancel());
        }
        entry.cancel.cancel();
        Ok(CancelAck {
            task_id: task_id.to_string(),
            accepted: true,
            message: Some("已请求取消".to_string()),
        })
    }

    /// Seal a task against cancellation and report whether cancellation was
    /// requested before the seal. Used immediately before spawning a mutation.
    fn seal_task(&self, task_id: &str) -> bool {
        let mut tasks = self.lock_tasks();
        match tasks.get_mut(task_id) {
            Some(entry) => {
                entry.cancellable = false;
                entry.cancel.is_cancelled()
            }
            None => true,
        }
    }

    async fn run_preview(
        &self,
        task_id: TaskId,
        source: SourceSpec,
        cancel: CancelToken,
        sink: EventSink,
    ) {
        emit(
            &sink,
            TaskEvent::progress(&task_id, TaskPhase::Preparing, "正在准备预览…", 0, 1),
        );
        if cancel.is_cancelled() {
            self.finish(&task_id, &sink, FinishOutcome::Cancelled);
            return;
        }

        let runtime = tokio::select! {
            result = self.inner.runtime.cli_runtime() => match result {
                Ok(runtime) => runtime,
                Err(error) => {
                    self.finish(&task_id, &sink, FinishOutcome::Failed(error));
                    return;
                }
            },
            _ = cancel.cancelled() => {
                self.finish(&task_id, &sink, FinishOutcome::Cancelled);
                return;
            }
        };
        if cancel.is_cancelled() {
            self.finish(&task_id, &sink, FinishOutcome::Cancelled);
            return;
        }

        emit(
            &sink,
            TaskEvent::progress(
                &task_id,
                TaskPhase::Cloning,
                "正在克隆仓库并读取 skill 列表…",
                0,
                1,
            ),
        );
        let spec = preview_command(
            &runtime,
            &source.normalized,
            self.inner.config.preview_timeout,
        );
        let output = match self.inner.executor.run(spec, cancel.clone()).await {
            Ok(output) => output,
            Err(ExecError::Cancelled) => {
                self.finish(&task_id, &sink, FinishOutcome::Cancelled);
                return;
            }
            Err(ExecError::Timeout(timeout)) => {
                self.finish(
                    &task_id,
                    &sink,
                    FinishOutcome::Failed(AppError::network_fetch_failed(format!(
                        "预览超时（{} 秒），请检查网络后重试。",
                        timeout.as_secs()
                    ))),
                );
                return;
            }
            Err(ExecError::Spawn { program, message }) => {
                self.finish(
                    &task_id,
                    &sink,
                    FinishOutcome::Failed(AppError::env_cli_missing(format!(
                        "无法启动 {program}: {message}"
                    ))),
                );
                return;
            }
            Err(ExecError::Io(message)) => {
                self.finish(
                    &task_id,
                    &sink,
                    FinishOutcome::Failed(AppError::cli_failed(message)),
                );
                return;
            }
        };
        if cancel.is_cancelled() {
            self.finish(&task_id, &sink, FinishOutcome::Cancelled);
            return;
        }

        match parse_preview_output(&output.stdout) {
            PreviewParseOutcome::Skills(skills) => {
                let total = skills.len().max(1) as u32;
                emit(
                    &sink,
                    TaskEvent::progress(
                        &task_id,
                        TaskPhase::Verifying,
                        "正在解析预览结果…",
                        0,
                        total,
                    ),
                );
                let conflicts = self.lookup_conflicts(&skills).await;
                let preview_skills: Vec<PreviewSkill> = skills
                    .iter()
                    .map(|skill| PreviewSkill {
                        name: skill.name.clone(),
                        description: skill.description.clone(),
                        group: skill.group.clone(),
                        conflict: conflicts.get(&skill.name).cloned(),
                    })
                    .collect();
                let count = preview_skills.len() as u32;
                let result = TaskResult {
                    overall: TaskOverall::Success,
                    skills: Vec::new(),
                    warnings: Vec::new(),
                    backup_path: None,
                    log_path: None,
                    finished_at_epoch_ms: now_ms(),
                    preview: Some(PreviewSummary {
                        source: source.normalized.clone(),
                        skills: preview_skills,
                        raw_output: None,
                        parse_failed: false,
                        no_skills: false,
                    }),
                };
                self.finish(
                    &task_id,
                    &sink,
                    FinishOutcome::Completed(result, format!("预览完成，共 {count} 个 skill。")),
                );
            }
            PreviewParseOutcome::NoSkills => {
                emit(
                    &sink,
                    TaskEvent::progress(&task_id, TaskPhase::Verifying, "正在确认预览结果…", 0, 1),
                );
                let result = TaskResult {
                    overall: TaskOverall::Success,
                    skills: Vec::new(),
                    warnings: Vec::new(),
                    backup_path: None,
                    log_path: None,
                    finished_at_epoch_ms: now_ms(),
                    preview: Some(PreviewSummary {
                        source: source.normalized.clone(),
                        skills: Vec::new(),
                        raw_output: None,
                        parse_failed: false,
                        no_skills: true,
                    }),
                };
                self.finish(
                    &task_id,
                    &sink,
                    FinishOutcome::Completed(
                        result,
                        "仓库中没有可安装的 skill（缺少 SKILL.md）。".to_string(),
                    ),
                );
            }
            PreviewParseOutcome::Failed { raw } => {
                if output.success() {
                    emit(
                        &sink,
                        TaskEvent::progress(
                            &task_id,
                            TaskPhase::Verifying,
                            "预览输出无法解析，已保留原始文本…",
                            0,
                            1,
                        ),
                    );
                    let warning = AppError::source_preview_failed(
                        "预览输出无法解析，已展示原始文本；可手动输入 skill 名。",
                    );
                    let result = TaskResult {
                        overall: TaskOverall::Success,
                        skills: Vec::new(),
                        warnings: vec![warning],
                        backup_path: None,
                        log_path: None,
                        finished_at_epoch_ms: now_ms(),
                        preview: Some(PreviewSummary {
                            source: source.normalized.clone(),
                            skills: Vec::new(),
                            raw_output: Some(raw),
                            parse_failed: true,
                            no_skills: false,
                        }),
                    };
                    self.finish(
                        &task_id,
                        &sink,
                        FinishOutcome::Completed(
                            result,
                            "预览输出无法解析，已保留原始文本。".to_string(),
                        ),
                    );
                } else {
                    self.finish(
                        &task_id,
                        &sink,
                        FinishOutcome::Failed(classify_cli_failure(&output.stdout, &output.stderr)),
                    );
                }
            }
        }
    }

    async fn run_install(
        &self,
        task_id: TaskId,
        plan: InstallPlan,
        permit: MutationPermit,
        cancel: CancelToken,
        sink: EventSink,
    ) {
        let _permit = permit;
        let total = plan.total();
        emit(
            &sink,
            TaskEvent::progress(&task_id, TaskPhase::Preparing, "正在准备安装…", 0, total),
        );
        if cancel.is_cancelled() {
            self.finish(&task_id, &sink, FinishOutcome::Cancelled);
            return;
        }

        let runtime = tokio::select! {
            result = self.inner.runtime.cli_runtime() => match result {
                Ok(runtime) => runtime,
                Err(error) => {
                    self.finish(&task_id, &sink, FinishOutcome::Failed(error));
                    return;
                }
            },
            _ = cancel.cancelled() => {
                self.finish(&task_id, &sink, FinishOutcome::Cancelled);
                return;
            }
        };
        if cancel.is_cancelled() {
            self.finish(&task_id, &sink, FinishOutcome::Cancelled);
            return;
        }

        // Strict lock read, final conflict re-check, then the pre-spawn
        // snapshot. The lock is never written by the app.
        let paths = self.inner.paths.clone();
        let snapshot_plan = plan.clone();
        let snapshot_task = task_id.clone();
        let snapshot = tokio::select! {
            result = tokio::task::spawn_blocking(move || -> Result<PathBuf, AppError> {
                LockFile::read(&paths.lock_path())?;
                if let Some(conflict) = install::conflicts_for(&paths, &snapshot_plan.skills)? {
                    return Err(conflict);
                }
                snapshot_lock(&paths, &snapshot_task)
            }) => match result {
                Ok(Ok(path)) => path,
                Ok(Err(error)) => {
                    self.finish(&task_id, &sink, FinishOutcome::Failed(error));
                    return;
                }
                Err(error) => {
                    self.finish(
                        &task_id,
                        &sink,
                        FinishOutcome::Failed(AppError::backup_failed(error.to_string())),
                    );
                    return;
                }
            },
            _ = cancel.cancelled() => {
                self.finish(&task_id, &sink, FinishOutcome::Cancelled);
                return;
            }
        };
        if cancel.is_cancelled() {
            self.finish(&task_id, &sink, FinishOutcome::Cancelled);
            return;
        }

        emit(
            &sink,
            TaskEvent::progress(
                &task_id,
                TaskPhase::Preparing,
                format!("已快照 lock：{}", self.inner.paths.display(&snapshot)),
                0,
                total,
            ),
        );

        // From here on the CLI may write: cancellation is rejected.
        if self.seal_task(&task_id) {
            self.finish(&task_id, &sink, FinishOutcome::Cancelled);
            return;
        }

        emit(
            &sink,
            TaskEvent::progress(
                &task_id,
                TaskPhase::RunningCli,
                "正在执行 skills add…",
                0,
                total,
            ),
        );
        let spec = build_install_command(&runtime, &plan, self.inner.config.install_timeout);
        let output = match self.inner.executor.run(spec, cancel.clone()).await {
            Ok(output) => output,
            Err(ExecError::Cancelled) => {
                self.finish(&task_id, &sink, FinishOutcome::Cancelled);
                return;
            }
            Err(ExecError::Timeout(timeout)) => {
                self.finish(
                    &task_id,
                    &sink,
                    FinishOutcome::Failed(AppError::cli_failed(format!(
                        "安装超时（{} 秒）。",
                        timeout.as_secs()
                    ))),
                );
                return;
            }
            Err(ExecError::Spawn { program, message }) => {
                self.finish(
                    &task_id,
                    &sink,
                    FinishOutcome::Failed(AppError::env_cli_missing(format!(
                        "无法启动 {program}: {message}"
                    ))),
                );
                return;
            }
            Err(ExecError::Io(message)) => {
                self.finish(
                    &task_id,
                    &sink,
                    FinishOutcome::Failed(AppError::cli_failed(message)),
                );
                return;
            }
        };

        emit(
            &sink,
            TaskEvent::progress(
                &task_id,
                TaskPhase::Verifying,
                "正在校验 canonical、agent 挂载与 lock…",
                0,
                total,
            ),
        );

        // The per-skill JSON status is authoritative for failures, but an
        // installed entry still has to pass the filesystem verification below.
        let cli_results = match parse_install_output(&output.stdout) {
            InstallOutput::Results(results) => results,
            InstallOutput::Fatal(detail) => {
                let error = classify_install_failure(&detail, &output.stderr);
                self.finish(&task_id, &sink, FinishOutcome::Failed(error));
                return;
            }
            InstallOutput::Unrecognized(detail) => {
                let error = classify_install_failure(&detail, &output.stderr);
                self.finish(&task_id, &sink, FinishOutcome::Failed(error));
                return;
            }
        };

        let verification_paths = self.inner.paths.clone();
        let verification_plan = plan.clone();
        let verification = match tokio::task::spawn_blocking(move || {
            verify_install(&verification_paths, &verification_plan, &cli_results)
        })
        .await
        {
            Ok(verification) => verification,
            Err(error) => {
                self.finish(
                    &task_id,
                    &sink,
                    FinishOutcome::Failed(AppError::verify_failed(error.to_string())),
                );
                return;
            }
        };

        for (index, result) in verification.results.iter().enumerate() {
            let mut event = TaskEvent::progress(
                &task_id,
                TaskPhase::Verifying,
                format!("已校验 {}", result.name),
                index as u32 + 1,
                total,
            );
            event.skill = Some(result.name.clone());
            emit(&sink, event);
        }

        let overall = install::overall_for(&verification.results);
        let message = summary_message(overall, &verification.results);
        let result = TaskResult {
            overall,
            skills: verification.results,
            warnings: verification.warnings,
            backup_path: Some(self.inner.paths.display(&snapshot)),
            log_path: None,
            finished_at_epoch_ms: now_ms(),
            preview: None,
        };
        self.finish(&task_id, &sink, FinishOutcome::Completed(result, message));
    }

    async fn lookup_conflicts(&self, skills: &[ParsedSkill]) -> HashMap<String, ConflictInfo> {
        let names: Vec<String> = skills.iter().map(|skill| skill.name.clone()).collect();
        if names.is_empty() {
            return HashMap::new();
        }
        let provider = Arc::clone(&self.inner.conflicts);
        tokio::task::spawn_blocking(move || provider.lookup(&names))
            .await
            .unwrap_or_default()
    }

    fn finish(&self, task_id: &str, sink: &EventSink, outcome: FinishOutcome) {
        let cancel_requested = {
            let mut tasks = self.lock_tasks();
            let Some(entry) = tasks.get_mut(task_id) else {
                return;
            };
            if entry.state == TaskState::Finished {
                return;
            }
            entry.state = TaskState::Finished;
            entry.cancel.is_cancelled()
        };
        let outcome = if cancel_requested {
            FinishOutcome::Cancelled
        } else {
            outcome
        };
        let event = match outcome {
            FinishOutcome::Completed(result, message) => {
                let total = result
                    .preview
                    .as_ref()
                    .map(|preview| preview.skills.len().max(1) as u32)
                    .or_else(|| (!result.skills.is_empty()).then_some(result.skills.len() as u32))
                    .unwrap_or(1);
                TaskEvent::finished(
                    task_id,
                    TaskPhase::Completed,
                    message,
                    total,
                    total,
                    result,
                    None,
                )
            }
            FinishOutcome::Failed(error) => TaskEvent::finished(
                task_id,
                TaskPhase::Failed,
                error.message.clone(),
                0,
                1,
                TaskResult {
                    overall: TaskOverall::Failed,
                    skills: Vec::new(),
                    warnings: Vec::new(),
                    backup_path: None,
                    log_path: None,
                    finished_at_epoch_ms: now_ms(),
                    preview: None,
                },
                Some(error),
            ),
            FinishOutcome::Cancelled => TaskEvent::finished(
                task_id,
                TaskPhase::Cancelled,
                "预览已取消。",
                0,
                1,
                TaskResult {
                    overall: TaskOverall::Cancelled,
                    skills: Vec::new(),
                    warnings: Vec::new(),
                    backup_path: None,
                    log_path: None,
                    finished_at_epoch_ms: now_ms(),
                    preview: None,
                },
                None,
            ),
        };
        emit(sink, event);
    }

    fn lock_tasks(&self) -> std::sync::MutexGuard<'_, HashMap<TaskId, TaskEntry>> {
        self.inner
            .tasks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn classify_install_failure(detail: &str, stderr: &str) -> AppError {
    let combined = if stderr.trim().is_empty() {
        detail.trim().to_string()
    } else if detail.trim().is_empty() {
        stderr.trim().to_string()
    } else {
        format!("{}\n{}", detail.trim(), stderr.trim())
    };
    let detail = super::parser::truncate_chars(&combined, 16 * 1024);
    let lower = detail.to_lowercase();
    if lower.contains("failed to clone")
        || lower.contains("unable to access")
        || lower.contains("couldn't connect")
        || lower.contains("failed to connect")
        || lower.contains("could not resolve")
        || lower.contains("network is unreachable")
    {
        AppError::network_fetch_failed(detail)
    } else {
        AppError::cli_failed(detail)
    }
}

fn classify_cli_failure(stdout: &str, stderr: &str) -> AppError {
    let detail = if stderr.trim().is_empty() {
        stdout.trim().to_string()
    } else if stdout.trim().is_empty() {
        stderr.trim().to_string()
    } else {
        format!("{}\n{}", stdout.trim(), stderr.trim())
    };
    let detail = super::parser::truncate_chars(&detail, 16 * 1024);
    let lower = detail.to_lowercase();
    if lower.contains("failed to clone")
        || lower.contains("unable to access")
        || lower.contains("couldn't connect")
        || lower.contains("failed to connect")
        || lower.contains("could not resolve")
        || lower.contains("network is unreachable")
    {
        AppError::network_fetch_failed(detail)
    } else if lower.contains("authentication failed") {
        AppError::source_preview_failed(format!("仓库不存在或无法公开访问：{detail}"))
    } else if detail.is_empty() {
        AppError::source_preview_failed("skills CLI 未返回预览输出。")
    } else {
        AppError::source_preview_failed(detail)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn emit(sink: &EventSink, event: TaskEvent) {
    sink(event);
}

impl Clone for TaskManager {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::CliRuntime;
    use crate::error::codes;
    use crate::process::fake::{ScriptedExecutor, ScriptedResponse};
    use crate::process::{CommandSpec, ExecOutput};
    use crate::tasks::conflicts::ConflictProvider;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

    const SINGLE_CI: &str = include_str!("../../tests/fixtures/cli/single-skill-ci.txt");
    const NO_SKILLS: &str = include_str!("../../tests/fixtures/cli/no-skills.txt");
    const NETWORK: &str = include_str!("../../tests/fixtures/cli/network-failure.txt");

    struct FakeRuntime {
        result: Result<CliRuntime, AppError>,
    }

    impl FakeRuntime {
        fn ok() -> Self {
            Self {
                result: Ok(CliRuntime {
                    npx_path: PathBuf::from("/node/bin/npx"),
                    node_dir: PathBuf::from("/node/bin"),
                    git_dir: Some(PathBuf::from("/usr/bin")),
                    cwd: PathBuf::from("/tmp/skills-box-cwd"),
                    home: PathBuf::from("/tmp/skills-box-home"),
                    env: BTreeMap::from([
                        ("CI".to_string(), "true".to_string()),
                        ("DISABLE_TELEMETRY".to_string(), "1".to_string()),
                    ]),
                    pinned_version: "1.7.4".to_string(),
                }),
            }
        }

        fn err(error: AppError) -> Self {
            Self { result: Err(error) }
        }
    }

    #[async_trait::async_trait]
    impl CliRuntimeProvider for FakeRuntime {
        async fn cli_runtime(&self) -> Result<CliRuntime, AppError> {
            self.result.clone()
        }
    }

    struct HangingRuntime;

    #[async_trait::async_trait]
    impl CliRuntimeProvider for HangingRuntime {
        async fn cli_runtime(&self) -> Result<CliRuntime, AppError> {
            std::future::pending().await
        }
    }

    #[derive(Default)]
    struct FakeConflicts {
        map: HashMap<String, ConflictInfo>,
    }

    impl ConflictProvider for FakeConflicts {
        fn lookup(&self, names: &[String]) -> HashMap<String, ConflictInfo> {
            self.map
                .iter()
                .filter(|(name, _)| names.contains(name))
                .map(|(name, info)| (name.clone(), info.clone()))
                .collect()
        }
    }

    fn sink() -> (EventSink, UnboundedReceiver<TaskEvent>) {
        let (sender, receiver) = unbounded_channel();
        let sink: EventSink = Arc::new(move |event| {
            let _ = sender.send(event);
        });
        (sink, receiver)
    }

    fn manager(
        executor: ScriptedExecutor,
        conflicts: HashMap<String, ConflictInfo>,
        preview_timeout: Duration,
    ) -> (TaskManager, Arc<ScriptedExecutor>) {
        manager_with_runtime(executor, conflicts, preview_timeout, FakeRuntime::ok())
    }

    fn manager_with_runtime(
        executor: ScriptedExecutor,
        conflicts: HashMap<String, ConflictInfo>,
        preview_timeout: Duration,
        runtime: FakeRuntime,
    ) -> (TaskManager, Arc<ScriptedExecutor>) {
        let executor = Arc::new(executor);
        let manager = TaskManager::new(
            Arc::new(runtime),
            executor.clone(),
            Arc::new(FakeConflicts { map: conflicts }),
            AppPaths::new("/tmp/skills-box-test-home", "/tmp/skills-box-test-data"),
            TaskManagerConfig {
                preview_timeout,
                install_timeout: INSTALL_TIMEOUT,
            },
        );
        (manager, executor)
    }

    async fn final_event(receiver: &mut UnboundedReceiver<TaskEvent>) -> TaskEvent {
        loop {
            let event = receiver.recv().await.expect("final event");
            if event.phase.is_final() {
                return event;
            }
        }
    }

    #[test]
    fn task_event_json_matches_the_frontend_contract() {
        let event = TaskEvent::progress("task-7", TaskPhase::Cloning, "正在克隆", 0, 3);
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["taskId"], "task-7");
        assert_eq!(value["phase"], "cloning");
        assert_eq!(value["message"], "正在克隆");
        assert_eq!(value["completed"], 0);
        assert_eq!(value["total"], 3);
        assert!(value.get("result").is_none());
        assert!(value.get("error").is_none());
    }

    #[tokio::test]
    async fn preview_success_streams_phases_progress_and_conflicts() {
        let (manager, executor) = manager(
            ScriptedExecutor::new().expect(
                "npx",
                &["skills@1.7.4", "add", "owner/repo", "-l", "-g"],
                ScriptedResponse::ok(SINGLE_CI),
            ),
            HashMap::from([(
                "find-skills".to_string(),
                ConflictInfo {
                    management: crate::inventory::ManagementState::Managed,
                    source: Some("acme/skills".to_string()),
                },
            )]),
            Duration::from_secs(60),
        );
        let (sink, mut receiver) = sink();
        let task_id = manager
            .start_preview("https://github.com/owner/repo", sink)
            .await
            .unwrap();
        assert!(task_id.starts_with("task-"));

        let mut phases = Vec::new();
        let final_event = loop {
            let event = receiver.recv().await.unwrap();
            phases.push(event.phase);
            if event.phase.is_final() {
                break event;
            }
        };
        assert_eq!(
            phases,
            vec![
                TaskPhase::Preparing,
                TaskPhase::Cloning,
                TaskPhase::Verifying,
                TaskPhase::Completed
            ]
        );
        assert_eq!(final_event.completed, Some(1));
        assert_eq!(final_event.total, Some(1));
        let result = final_event.result.unwrap();
        assert_eq!(result.overall, TaskOverall::Success);
        let preview = result.preview.unwrap();
        assert_eq!(preview.source, "owner/repo");
        assert_eq!(preview.skills.len(), 1);
        assert_eq!(preview.skills[0].name, "find-skills");
        assert_eq!(
            preview.skills[0]
                .conflict
                .as_ref()
                .unwrap()
                .source
                .as_deref(),
            Some("acme/skills")
        );

        let calls = executor.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].program, PathBuf::from("/node/bin/npx"));
        assert_eq!(
            calls[0].args,
            vec!["-y", "skills@1.7.4", "add", "owner/repo", "-l", "-g"]
        );
        assert_eq!(calls[0].timeout, Duration::from_secs(60));
        assert_eq!(calls[0].env["CI"], "true");
    }

    #[tokio::test]
    async fn unparseable_output_falls_back_to_raw_text() {
        let (manager, _) = manager(
            ScriptedExecutor::new().expect_any(ScriptedResponse::ok("garbage output")),
            HashMap::new(),
            Duration::from_secs(60),
        );
        let (sink, mut receiver) = sink();
        manager.start_preview("owner/repo", sink).await.unwrap();
        let event = final_event(&mut receiver).await;

        assert_eq!(event.phase, TaskPhase::Completed);
        let result = event.result.unwrap();
        assert_eq!(result.overall, TaskOverall::Success);
        assert_eq!(result.warnings[0].code, codes::SOURCE_PREVIEW_FAILED);
        let preview = result.preview.unwrap();
        assert!(preview.parse_failed);
        assert!(preview.raw_output.unwrap().contains("garbage output"));
    }

    #[tokio::test]
    async fn no_skills_is_a_completed_empty_state() {
        let (manager, _) = manager(
            ScriptedExecutor::new().expect_any(ScriptedResponse::Output {
                code: 1,
                stdout: NO_SKILLS.to_string(),
                stderr: String::new(),
            }),
            HashMap::new(),
            Duration::from_secs(60),
        );
        let (sink, mut receiver) = sink();
        manager.start_preview("owner/repo", sink).await.unwrap();
        let event = final_event(&mut receiver).await;

        assert_eq!(event.phase, TaskPhase::Completed);
        let preview = event.result.unwrap().preview.unwrap();
        assert!(preview.no_skills);
        assert!(preview.skills.is_empty());
    }

    #[tokio::test]
    async fn network_failure_maps_to_network_error() {
        let (manager, _) = manager(
            ScriptedExecutor::new().expect_any(ScriptedResponse::Output {
                code: 1,
                stdout: NETWORK.to_string(),
                stderr: String::new(),
            }),
            HashMap::new(),
            Duration::from_secs(60),
        );
        let (sink, mut receiver) = sink();
        manager.start_preview("owner/repo", sink).await.unwrap();
        let event = final_event(&mut receiver).await;

        assert_eq!(event.phase, TaskPhase::Failed);
        let error = event.error.unwrap();
        assert_eq!(error.code, codes::NETWORK_FETCH_FAILED);
        assert!(error.retryable);
        assert!(error.message.contains("网络"));
    }

    #[tokio::test]
    async fn preview_timeout_fails_the_task() {
        let (manager, _) = manager(
            ScriptedExecutor::new().expect_any(ScriptedResponse::hang()),
            HashMap::new(),
            Duration::from_millis(30),
        );
        let (sink, mut receiver) = sink();
        manager.start_preview("owner/repo", sink).await.unwrap();
        let event = tokio::time::timeout(Duration::from_secs(2), final_event(&mut receiver))
            .await
            .expect("timeout should finish the task");

        assert_eq!(event.phase, TaskPhase::Failed);
        assert_eq!(event.error.unwrap().code, codes::NETWORK_FETCH_FAILED);
    }

    #[tokio::test]
    async fn preview_can_be_cancelled_while_running() {
        let (manager, executor) = manager(
            ScriptedExecutor::new().expect_any(ScriptedResponse::hang()),
            HashMap::new(),
            Duration::from_secs(60),
        );
        let (sink, mut receiver) = sink();
        let task_id = manager.start_preview("owner/repo", sink).await.unwrap();

        loop {
            let event = receiver.recv().await.unwrap();
            if event.phase == TaskPhase::Cloning {
                break;
            }
        }
        let ack = manager.cancel_task(&task_id).unwrap();
        assert!(ack.accepted);
        assert_eq!(ack.task_id, task_id);

        let event = tokio::time::timeout(Duration::from_secs(2), final_event(&mut receiver))
            .await
            .expect("cancellation should finish the task");
        assert_eq!(event.phase, TaskPhase::Cancelled);
        assert_eq!(event.result.unwrap().overall, TaskOverall::Cancelled);
        assert_eq!(executor.call_count(), 1);
    }

    #[tokio::test]
    async fn preview_can_be_cancelled_while_preparing_the_environment() {
        let manager = TaskManager::new(
            Arc::new(HangingRuntime),
            Arc::new(ScriptedExecutor::new()),
            Arc::new(FakeConflicts::default()),
            AppPaths::new("/tmp/skills-box-test-home", "/tmp/skills-box-test-data"),
            TaskManagerConfig {
                preview_timeout: Duration::from_secs(60),
                install_timeout: INSTALL_TIMEOUT,
            },
        );
        let (sink, mut receiver) = sink();
        let task_id = manager.start_preview("owner/repo", sink).await.unwrap();
        let event = receiver.recv().await.unwrap();
        assert_eq!(event.phase, TaskPhase::Preparing);

        manager.cancel_task(&task_id).unwrap();
        let event = tokio::time::timeout(Duration::from_secs(2), final_event(&mut receiver))
            .await
            .expect("cancellation during environment preparation should finish");
        assert_eq!(event.phase, TaskPhase::Cancelled);
    }

    #[tokio::test]
    async fn cancel_after_completion_is_rejected() {
        let (manager, _) = manager(
            ScriptedExecutor::new().expect_any(ScriptedResponse::ok(SINGLE_CI)),
            HashMap::new(),
            Duration::from_secs(60),
        );
        let (sink, mut receiver) = sink();
        let task_id = manager.start_preview("owner/repo", sink).await.unwrap();
        let event = final_event(&mut receiver).await;
        assert_eq!(event.phase, TaskPhase::Completed);

        let error = manager.cancel_task(&task_id).unwrap_err();
        assert_eq!(error.code, codes::TASK_CANNOT_CANCEL);
        let missing = manager.cancel_task("task-does-not-exist").unwrap_err();
        assert_eq!(missing.code, codes::TASK_NOT_FOUND);
    }

    #[tokio::test]
    async fn mutation_slot_is_single_writer_and_preview_does_not_take_it() {
        let (manager, _) = manager(
            ScriptedExecutor::new().expect_any(ScriptedResponse::ok(SINGLE_CI)),
            HashMap::new(),
            Duration::from_secs(60),
        );
        let permit = manager.try_acquire_mutation().unwrap();
        let busy = manager.try_acquire_mutation().unwrap_err();
        assert_eq!(busy.code, codes::TASK_BUSY);

        let (sink, mut receiver) = sink();
        manager.start_preview("owner/repo", sink).await.unwrap();
        let event = tokio::time::timeout(Duration::from_secs(2), final_event(&mut receiver))
            .await
            .expect("preview must not wait for the write lock");
        assert_eq!(event.phase, TaskPhase::Completed);

        drop(permit);
        assert!(manager.try_acquire_mutation().is_ok());
    }

    #[tokio::test]
    async fn invalid_source_is_rejected_before_spawning() {
        let (manager, executor) = manager(
            ScriptedExecutor::new(),
            HashMap::new(),
            Duration::from_secs(60),
        );
        let (sink, mut receiver) = sink();
        let error = manager
            .start_preview("owner/repo/sub", sink)
            .await
            .unwrap_err();
        assert_eq!(error.code, codes::SOURCE_UNSUPPORTED_SUBDIR);
        assert_eq!(executor.call_count(), 0);
        assert!(receiver.try_recv().is_err(), "no task should be created");
    }

    #[tokio::test]
    async fn runtime_error_fails_the_task_without_spawning() {
        let (manager, executor) = manager_with_runtime(
            ScriptedExecutor::new(),
            HashMap::new(),
            Duration::from_secs(60),
            FakeRuntime::err(AppError::env_node_missing("no node")),
        );
        let (sink, mut receiver) = sink();
        manager.start_preview("owner/repo", sink).await.unwrap();
        let event = final_event(&mut receiver).await;

        assert_eq!(event.phase, TaskPhase::Failed);
        assert_eq!(event.error.unwrap().code, codes::ENV_NODE_MISSING);
        assert_eq!(executor.call_count(), 0);
    }

    // ------------------------------------------------------------------
    // Install execution (ticket 05)
    // ------------------------------------------------------------------

    const INSTALL_SUCCESS: &str = include_str!("../../tests/fixtures/cli/install-success.txt");
    const INSTALL_PARTIAL: &str = include_str!("../../tests/fixtures/cli/install-partial.txt");
    const INSTALL_COPY: &str = include_str!("../../tests/fixtures/cli/install-copy.txt");
    const INSTALL_FATAL_CLONE: &str =
        include_str!("../../tests/fixtures/cli/install-fatal-clone.txt");

    struct InstallResponse {
        code: i32,
        stdout: String,
        stderr: String,
        delay: Duration,
        post: Option<Box<dyn FnOnce() + Send>>,
    }

    impl InstallResponse {
        fn success(stdout: &str, post: impl FnOnce() + Send + 'static) -> Self {
            Self {
                code: 0,
                stdout: stdout.to_string(),
                stderr: String::new(),
                delay: Duration::ZERO,
                post: Some(Box::new(post)),
            }
        }

        fn output(code: i32, stdout: &str, stderr: &str) -> Self {
            Self {
                code,
                stdout: stdout.to_string(),
                stderr: stderr.to_string(),
                delay: Duration::ZERO,
                post: None,
            }
        }

        fn delayed(mut self, delay: Duration) -> Self {
            self.delay = delay;
            self
        }
    }

    struct InstallExecutor {
        response: Mutex<Option<InstallResponse>>,
        calls: Mutex<Vec<CommandSpec>>,
    }

    impl InstallExecutor {
        fn new(response: InstallResponse) -> Self {
            Self {
                response: Mutex::new(Some(response)),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<CommandSpec> {
            self.calls.lock().expect("calls mutex").clone()
        }

        fn call_count(&self) -> usize {
            self.calls.lock().expect("calls mutex").len()
        }
    }

    #[async_trait::async_trait]
    impl ProcessExecutor for InstallExecutor {
        async fn run(
            &self,
            spec: CommandSpec,
            _cancel: CancelToken,
        ) -> Result<ExecOutput, ExecError> {
            self.calls.lock().expect("calls mutex").push(spec);
            let response = self
                .response
                .lock()
                .expect("response mutex")
                .take()
                .expect("one scripted install response");
            if !response.delay.is_zero() {
                tokio::time::sleep(response.delay).await;
            }
            if let Some(post) = response.post {
                post();
            }
            Ok(ExecOutput {
                code: Some(response.code),
                signal: None,
                stdout: response.stdout,
                stderr: response.stderr,
                duration: Duration::ZERO,
            })
        }
    }

    struct InstallFixture {
        _tmp: tempfile::TempDir,
        home: PathBuf,
        paths: AppPaths,
        executor: Arc<InstallExecutor>,
        manager: TaskManager,
    }

    fn install_fixture(response: InstallResponse) -> InstallFixture {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(home.join(".qoder")).unwrap();
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        paths.ensure_dirs().unwrap();
        let executor = Arc::new(InstallExecutor::new(response));
        let manager = TaskManager::new(
            Arc::new(FakeRuntime::ok()),
            executor.clone(),
            Arc::new(FakeConflicts::default()),
            paths.clone(),
            TaskManagerConfig {
                preview_timeout: Duration::from_secs(60),
                install_timeout: Duration::from_secs(5),
            },
        );
        InstallFixture {
            _tmp: tmp,
            home,
            paths,
            executor,
            manager,
        }
    }

    fn install_request(skills: &[&str], agents: &[&str]) -> InstallRequest {
        InstallRequest {
            source: "vercel-labs/skills".to_string(),
            skills: skills.iter().map(|value| value.to_string()).collect(),
            agents: agents.iter().map(|value| value.to_string()).collect(),
        }
    }

    fn lock_json(names: &[String]) -> String {
        let mut skills = serde_json::Map::new();
        for name in names {
            skills.insert(
                name.clone(),
                serde_json::json!({
                    "source": "vercel-labs/skills",
                    "sourceType": "github",
                    "sourceUrl": "https://github.com/vercel-labs/skills.git",
                    "skillPath": format!("skills/{name}/SKILL.md"),
                    "skillFolderHash": "0123456789abcdef0123456789abcdef01234567",
                    "installedAt": "2026-01-01T00:00:00.000Z",
                    "updatedAt": "2026-01-01T00:00:00.000Z"
                }),
            );
        }
        serde_json::json!({ "version": 3, "skills": skills }).to_string()
    }

    fn write_pre_lock(paths: &AppPaths, names: &[&str]) {
        let names: Vec<String> = names.iter().map(|value| value.to_string()).collect();
        fs::create_dir_all(paths.agents_root()).unwrap();
        fs::write(paths.lock_path(), lock_json(&names)).unwrap();
    }

    struct PostSkill {
        name: &'static str,
        canonical: bool,
        agent_dir: Option<&'static str>,
        copy: bool,
        lock: bool,
    }

    impl PostSkill {
        fn installed(name: &'static str, agent_dir: &'static str) -> Self {
            Self {
                name,
                canonical: true,
                agent_dir: Some(agent_dir),
                copy: false,
                lock: true,
            }
        }

        fn copy(name: &'static str, agent_dir: &'static str) -> Self {
            Self {
                copy: true,
                ..Self::installed(name, agent_dir)
            }
        }

        fn lock_only(name: &'static str) -> Self {
            Self {
                name,
                canonical: false,
                agent_dir: None,
                copy: false,
                lock: true,
            }
        }
    }

    fn post_state(home: PathBuf, skills: Vec<PostSkill>) -> impl FnOnce() + Send + 'static {
        move || {
            let agents_root = home.join(".agents");
            let canonical_root = agents_root.join("skills");
            let mut lock_names = Vec::new();
            for skill in &skills {
                if skill.canonical {
                    let canonical = canonical_root.join(skill.name);
                    fs::create_dir_all(&canonical).unwrap();
                    fs::write(canonical.join("SKILL.md"), format!("# {}\n", skill.name)).unwrap();
                }
                if let Some(relative) = skill.agent_dir {
                    let agent_dir = home.join(relative);
                    fs::create_dir_all(&agent_dir).unwrap();
                    let attachment = agent_dir.join(skill.name);
                    if skill.copy {
                        fs::create_dir_all(&attachment).unwrap();
                    } else {
                        #[cfg(unix)]
                        std::os::unix::fs::symlink(canonical_root.join(skill.name), &attachment)
                            .unwrap();
                    }
                }
                if skill.lock {
                    lock_names.push(skill.name.to_string());
                }
            }
            fs::create_dir_all(&agents_root).unwrap();
            fs::write(agents_root.join(".skill-lock.json"), lock_json(&lock_names)).unwrap();
        }
    }

    async fn run_install_task(
        manager: &TaskManager,
        request: InstallRequest,
    ) -> (Vec<TaskPhase>, TaskEvent) {
        let (sink, mut receiver) = sink();
        manager.start_install(request, sink).await.unwrap();
        let mut phases = Vec::new();
        let final_event = loop {
            let event = receiver.recv().await.expect("install event");
            phases.push(event.phase);
            if event.phase.is_final() {
                break event;
            }
        };
        (phases, final_event)
    }

    #[tokio::test]
    async fn install_success_streams_phases_verifies_and_snapshots_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(home.join(".qoder")).unwrap();
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        paths.ensure_dirs().unwrap();
        let executor = Arc::new(InstallExecutor::new(InstallResponse::success(
            INSTALL_SUCCESS,
            post_state(
                home.clone(),
                vec![PostSkill::installed("find-skills", ".qoder/skills")],
            ),
        )));
        let manager = TaskManager::new(
            Arc::new(FakeRuntime::ok()),
            executor.clone(),
            Arc::new(FakeConflicts::default()),
            paths.clone(),
            TaskManagerConfig {
                preview_timeout: Duration::from_secs(60),
                install_timeout: Duration::from_secs(5),
            },
        );
        write_pre_lock(&paths, &["existing"]);

        let (phases, final_event) =
            run_install_task(&manager, install_request(&["find-skills"], &["qoder"])).await;

        assert_eq!(final_event.phase, TaskPhase::Completed);
        assert_eq!(phases.first(), Some(&TaskPhase::Preparing));
        assert!(phases.contains(&TaskPhase::RunningCli));
        assert_eq!(phases.last(), Some(&TaskPhase::Completed));
        assert_eq!(final_event.completed, Some(1));
        assert_eq!(final_event.total, Some(1));

        let result = final_event.result.expect("install result");
        assert_eq!(result.overall, TaskOverall::Success);
        assert_eq!(result.skills.len(), 1);
        assert_eq!(result.skills[0].name, "find-skills");
        assert_eq!(result.skills[0].status, TaskSkillStatus::Installed);
        assert!(result.warnings.is_empty());
        let backup = result.backup_path.expect("lock snapshot path");
        let snapshot = PathBuf::from(&backup).join("skill-lock.json");
        assert!(snapshot.is_file(), "snapshot at {snapshot:?}");
        let snapshot_json = fs::read_to_string(&snapshot).unwrap();
        assert!(snapshot_json.contains("existing"));
        assert!(home.join(".agents/skills/find-skills/SKILL.md").is_file());
        assert!(home.join(".qoder/skills/find-skills").is_symlink());

        let calls = executor.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].program, PathBuf::from("/node/bin/npx"));
        assert_eq!(
            calls[0].args,
            vec![
                "-y",
                "skills@1.7.4",
                "add",
                "vercel-labs/skills",
                "-g",
                "-s",
                "find-skills",
                "-a",
                "qoder",
                "-a",
                "universal",
                "-y",
                "--json"
            ]
        );
        assert_eq!(calls[0].timeout, Duration::from_secs(5));
        for forbidden in ["--all", "--copy", "*", "update", "check", "remove"] {
            assert!(!calls[0].args.iter().any(|arg| arg == forbidden));
        }
    }

    #[tokio::test]
    async fn install_partial_returns_per_skill_results_without_task_error() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(home.join(".qoder")).unwrap();
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        paths.ensure_dirs().unwrap();
        let executor = Arc::new(InstallExecutor::new(InstallResponse::success(
            INSTALL_PARTIAL,
            post_state(
                home.clone(),
                vec![PostSkill::installed("find-skills", ".qoder/skills")],
            ),
        )));
        let manager = TaskManager::new(
            Arc::new(FakeRuntime::ok()),
            executor,
            Arc::new(FakeConflicts::default()),
            paths,
            TaskManagerConfig {
                preview_timeout: Duration::from_secs(60),
                install_timeout: Duration::from_secs(5),
            },
        );

        let (_phases, final_event) = run_install_task(
            &manager,
            install_request(&["find-skills", "no-such-skill"], &["qoder"]),
        )
        .await;

        assert_eq!(final_event.phase, TaskPhase::Completed);
        assert!(final_event.error.is_none());
        assert_eq!(final_event.completed, Some(2));
        assert_eq!(final_event.total, Some(2));
        let result = final_event.result.unwrap();
        assert_eq!(result.overall, TaskOverall::Partial);
        assert_eq!(result.skills[0].status, TaskSkillStatus::Installed);
        assert_eq!(result.skills[1].status, TaskSkillStatus::Skipped);
        assert_eq!(
            result.skills[1].error.as_ref().unwrap().code,
            codes::SKILL_NOT_IN_SOURCE
        );
    }

    #[tokio::test]
    async fn install_detects_silent_copy_fallback_as_a_warning() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(home.join(".qoder")).unwrap();
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        paths.ensure_dirs().unwrap();
        let executor = Arc::new(InstallExecutor::new(InstallResponse::success(
            INSTALL_COPY,
            post_state(
                home.clone(),
                vec![PostSkill::copy("find-skills", ".qoder/skills")],
            ),
        )));
        let manager = TaskManager::new(
            Arc::new(FakeRuntime::ok()),
            executor,
            Arc::new(FakeConflicts::default()),
            paths,
            TaskManagerConfig {
                preview_timeout: Duration::from_secs(60),
                install_timeout: Duration::from_secs(5),
            },
        );

        let (_phases, final_event) =
            run_install_task(&manager, install_request(&["find-skills"], &["qoder"])).await;
        let result = final_event.result.unwrap();
        assert_eq!(result.overall, TaskOverall::Success);
        assert_eq!(result.skills[0].status, TaskSkillStatus::Installed);
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(result.warnings[0].code, codes::VERIFY_COPY_FALLBACK);
    }

    #[tokio::test]
    async fn cli_exit_zero_without_canonical_is_a_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(home.join(".qoder")).unwrap();
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        paths.ensure_dirs().unwrap();
        let executor = Arc::new(InstallExecutor::new(InstallResponse::success(
            INSTALL_SUCCESS,
            post_state(home.clone(), vec![PostSkill::lock_only("find-skills")]),
        )));
        let manager = TaskManager::new(
            Arc::new(FakeRuntime::ok()),
            executor,
            Arc::new(FakeConflicts::default()),
            paths,
            TaskManagerConfig {
                preview_timeout: Duration::from_secs(60),
                install_timeout: Duration::from_secs(5),
            },
        );

        let (_phases, final_event) =
            run_install_task(&manager, install_request(&["find-skills"], &["qoder"])).await;
        assert_eq!(final_event.phase, TaskPhase::Completed);
        let result = final_event.result.unwrap();
        assert_eq!(result.overall, TaskOverall::Failed);
        assert_eq!(result.skills[0].status, TaskSkillStatus::Failed);
        assert_eq!(
            result.skills[0].error.as_ref().unwrap().code,
            codes::VERIFY_CANONICAL_MISSING
        );
    }

    #[tokio::test]
    async fn unparseable_json_is_fatal_even_with_exit_zero() {
        let fixture = install_fixture(InstallResponse::output(0, "garbage", ""));
        let (_phases, final_event) = run_install_task(
            &fixture.manager,
            install_request(&["find-skills"], &["qoder"]),
        )
        .await;
        assert_eq!(final_event.phase, TaskPhase::Failed);
        assert_eq!(final_event.error.unwrap().code, codes::CLI_FAILED);
        assert_eq!(fixture.executor.call_count(), 1);
    }

    #[tokio::test]
    async fn fatal_clone_failure_maps_to_network_error() {
        let fixture = install_fixture(InstallResponse::output(1, INSTALL_FATAL_CLONE, ""));
        let (_phases, final_event) = run_install_task(
            &fixture.manager,
            install_request(&["find-skills"], &["qoder"]),
        )
        .await;
        assert_eq!(final_event.phase, TaskPhase::Failed);
        assert_eq!(final_event.error.unwrap().code, codes::NETWORK_FETCH_FAILED);
    }

    #[tokio::test]
    async fn same_name_conflict_is_rejected_before_spawning() {
        let fixture = install_fixture(InstallResponse::output(0, INSTALL_SUCCESS, ""));
        let taken = fixture.home.join(".agents/skills/find-skills");
        fs::create_dir_all(&taken).unwrap();
        fs::write(taken.join("SKILL.md"), "# find-skills\n").unwrap();

        let (sink, mut receiver) = sink();
        let error = fixture
            .manager
            .start_install(install_request(&["find-skills"], &["qoder"]), sink)
            .await
            .unwrap_err();
        assert_eq!(error.code, codes::SKILL_CONFLICT);
        assert_eq!(fixture.executor.call_count(), 0);
        assert!(receiver.try_recv().is_err());
        assert!(taken.join("SKILL.md").is_file(), "no overwrite");
    }

    #[tokio::test]
    async fn corrupt_lock_blocks_install_before_spawning() {
        let fixture = install_fixture(InstallResponse::output(0, INSTALL_SUCCESS, ""));
        fs::create_dir_all(fixture.paths.agents_root()).unwrap();
        fs::write(fixture.paths.lock_path(), "{not json").unwrap();

        let (sink, _receiver) = sink();
        let error = fixture
            .manager
            .start_install(install_request(&["find-skills"], &["qoder"]), sink)
            .await
            .unwrap_err();
        assert_eq!(error.code, codes::LOCK_CORRUPT);
        assert_eq!(fixture.executor.call_count(), 0);
    }

    #[tokio::test]
    async fn install_can_be_cancelled_before_spawn_without_touching_the_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(home.join(".qoder")).unwrap();
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        paths.ensure_dirs().unwrap();
        write_pre_lock(&paths, &["existing"]);
        let before = fs::read_to_string(paths.lock_path()).unwrap();
        let executor = Arc::new(InstallExecutor::new(InstallResponse::output(
            0,
            INSTALL_SUCCESS,
            "",
        )));
        let manager = TaskManager::new(
            Arc::new(HangingRuntime),
            executor.clone(),
            Arc::new(FakeConflicts::default()),
            paths.clone(),
            TaskManagerConfig {
                preview_timeout: Duration::from_secs(60),
                install_timeout: Duration::from_secs(5),
            },
        );

        let (sink, mut receiver) = sink();
        let task_id = manager
            .start_install(install_request(&["find-skills"], &["qoder"]), sink)
            .await
            .unwrap();
        loop {
            let event = receiver.recv().await.unwrap();
            if event.phase == TaskPhase::Preparing {
                break;
            }
        }
        let ack = manager.cancel_task(&task_id).unwrap();
        assert!(ack.accepted);
        let final_event = tokio::time::timeout(Duration::from_secs(2), final_event(&mut receiver))
            .await
            .expect("cancel should finish the task");
        assert_eq!(final_event.phase, TaskPhase::Cancelled);
        assert_eq!(final_event.result.unwrap().overall, TaskOverall::Cancelled);
        assert_eq!(executor.call_count(), 0);
        assert_eq!(fs::read_to_string(paths.lock_path()).unwrap(), before);
    }

    #[tokio::test]
    async fn install_cannot_be_cancelled_after_spawn() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(home.join(".qoder")).unwrap();
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        paths.ensure_dirs().unwrap();
        let executor = Arc::new(InstallExecutor::new(
            InstallResponse::success(
                INSTALL_SUCCESS,
                post_state(
                    home.clone(),
                    vec![PostSkill::installed("find-skills", ".qoder/skills")],
                ),
            )
            .delayed(Duration::from_millis(120)),
        ));
        let manager = TaskManager::new(
            Arc::new(FakeRuntime::ok()),
            executor,
            Arc::new(FakeConflicts::default()),
            paths,
            TaskManagerConfig {
                preview_timeout: Duration::from_secs(60),
                install_timeout: Duration::from_secs(5),
            },
        );

        let (sink, mut receiver) = sink();
        let task_id = manager
            .start_install(install_request(&["find-skills"], &["qoder"]), sink)
            .await
            .unwrap();
        loop {
            let event = receiver.recv().await.unwrap();
            if event.phase == TaskPhase::RunningCli {
                break;
            }
        }
        let error = manager.cancel_task(&task_id).unwrap_err();
        assert_eq!(error.code, codes::TASK_CANNOT_CANCEL);

        let final_event = tokio::time::timeout(Duration::from_secs(2), final_event(&mut receiver))
            .await
            .expect("install should finish");
        assert_eq!(final_event.phase, TaskPhase::Completed);
    }

    #[tokio::test]
    async fn second_install_is_rejected_with_task_busy() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(home.join(".qoder")).unwrap();
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        paths.ensure_dirs().unwrap();
        let executor = Arc::new(InstallExecutor::new(
            InstallResponse::success(
                INSTALL_SUCCESS,
                post_state(
                    home.clone(),
                    vec![PostSkill::installed("find-skills", ".qoder/skills")],
                ),
            )
            .delayed(Duration::from_millis(150)),
        ));
        let manager = TaskManager::new(
            Arc::new(FakeRuntime::ok()),
            executor,
            Arc::new(FakeConflicts::default()),
            paths,
            TaskManagerConfig {
                preview_timeout: Duration::from_secs(60),
                install_timeout: Duration::from_secs(5),
            },
        );

        let (event_sink, mut receiver) = sink();
        manager
            .start_install(install_request(&["find-skills"], &["qoder"]), event_sink)
            .await
            .unwrap();
        let (second_sink, _second_receiver) = sink();
        let error = manager
            .start_install(install_request(&["other-skill"], &["qoder"]), second_sink)
            .await
            .unwrap_err();
        assert_eq!(error.code, codes::TASK_BUSY);

        let final_event = tokio::time::timeout(Duration::from_secs(2), final_event(&mut receiver))
            .await
            .expect("first install should finish");
        assert_eq!(final_event.phase, TaskPhase::Completed);
    }
}
