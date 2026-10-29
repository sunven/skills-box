//! Mutation task foundation: task ids, per-task channels, phases, progress,
//! single-writer mutation permits, and cancellable preview tasks.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::sync::{Notify, Semaphore};

use crate::cli::{CliRuntimeProvider, preview_command};
use crate::error::AppError;
use crate::error::cli::{self, CliAction};
use crate::interrupted::{InterruptedMutation, MarkerStore, MutationKind};
use crate::paths::AppPaths;
use crate::process::{CancelToken, ExecError, ProcessExecutor};

use super::conflicts::{ConflictInfo, ConflictProvider};
use super::install::{self, INSTALL_TIMEOUT, InstallPlan, InstallRequest, validate_request};
use super::parser::{ParsedSkill, PreviewParseOutcome, parse_preview_output};
use super::session;
use super::source::{SourceSpec, validate_source};
use super::uninstall as uninstall_task;
use super::update as update_task;

pub type TaskId = String;
pub type EventSink = Arc<dyn Fn(TaskEvent) + Send + Sync>;

pub const PREVIEW_TIMEOUT: Duration = crate::timeouts::PREVIEW_TIMEOUT;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPhase {
    Preparing,
    /// Preview clones the repository before parsing its SKILL.md files.
    Cloning,
    /// Update writes the mandatory pre-operation backup before the CLI runs.
    Backup,
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
pub enum TaskAction {
    Preview,
    Install,
    Update,
    Uninstall,
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
    Updated,
    Removed,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskResult {
    /// Which mutation produced this result (`preview` for read-only tasks).
    pub action: TaskAction,
    pub overall: TaskOverall,
    pub skills: Vec<TaskSkillResult>,
    pub warnings: Vec<AppError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_path: Option<String>,
    /// Epoch milliseconds; set when the task was registered.
    pub started_at: u64,
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
    pub(super) fn progress(
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

/// Snapshot of the one in-flight mutation, returned to the exit dialog and
/// attached to the `exit-requested` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveMutationInfo {
    pub task_id: String,
    pub kind: MutationKind,
    pub skills: Vec<String>,
    pub started_at_epoch_ms: u64,
    pub snapshot_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShutdownReport {
    /// The mutation that was active when shutdown started, if any.
    pub active: Option<ActiveMutationInfo>,
    pub cancelled_tasks: usize,
    /// True when a mutation did not reach a final state within the grace
    /// period; the process group may still have been killed by the executor.
    pub timed_out: bool,
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
    marker: MarkerStore,
    tasks: Mutex<HashMap<TaskId, TaskEntry>>,
    next_id: AtomicU64,
    mutation_slots: Arc<Semaphore>,
    config: TaskManagerConfig,
    shutting_down: AtomicBool,
    finished_notify: Notify,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskState {
    Running,
    Finished,
}

#[derive(Debug, Clone)]
struct MutationState {
    kind: MutationKind,
    skills: Vec<String>,
    started_at_epoch_ms: u64,
    snapshot_paths: Vec<String>,
}

struct TaskEntry {
    state: TaskState,
    cancel: CancelToken,
    /// Install tasks become non-cancellable immediately before the CLI spawns.
    cancellable: bool,
    /// Present for install/update/uninstall (never for preview).
    mutation: Option<MutationState>,
    action: TaskAction,
    started_at_epoch_ms: u64,
}

#[derive(Debug)]
pub(super) enum FinishOutcome {
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
                marker: MarkerStore::new(&paths),
                paths,
                tasks: Mutex::new(HashMap::new()),
                next_id: AtomicU64::new(1),
                mutation_slots: Arc::new(Semaphore::new(1)),
                config,
                shutting_down: AtomicBool::new(false),
                finished_notify: Notify::new(),
            }),
        }
    }

    /// Try to become the single active mutation. Concurrent callers get
    /// `task.busy` immediately (no queueing) so the UI can report it.
    pub fn try_acquire_mutation(&self) -> Result<MutationPermit, AppError> {
        self.ensure_not_shutting_down()?;
        Arc::clone(&self.inner.mutation_slots)
            .try_acquire_owned()
            .map(|permit| MutationPermit { _permit: permit })
            .map_err(|_| AppError::task_busy())
    }

    fn ensure_not_shutting_down(&self) -> Result<(), AppError> {
        if self.inner.shutting_down.load(Ordering::SeqCst) {
            return Err(AppError::task_shutting_down());
        }
        Ok(())
    }

    /// The single in-flight install/update/uninstall, if any. Preview tasks are
    /// read-only and intentionally excluded: closing the window during a
    /// preview does not risk a half-written skill store.
    pub fn active_mutation(&self) -> Option<ActiveMutationInfo> {
        let tasks = self.lock_tasks();
        tasks.iter().find_map(|(task_id, entry)| {
            if entry.state != TaskState::Running {
                return None;
            }
            entry.mutation.as_ref().map(|mutation| ActiveMutationInfo {
                task_id: task_id.clone(),
                kind: mutation.kind,
                skills: mutation.skills.clone(),
                started_at_epoch_ms: mutation.started_at_epoch_ms,
                snapshot_paths: mutation.snapshot_paths.clone(),
            })
        })
    }

    pub fn has_running_mutation(&self) -> bool {
        self.active_mutation().is_some()
    }

    /// Write the interruption marker before the first disk write. Failures are
    /// non-fatal: the operation still runs, only crash forensics degrade.
    fn begin_marker(&self, task_id: &str) {
        let record = {
            let tasks = self.lock_tasks();
            let Some(mutation) = tasks.get(task_id).and_then(|entry| entry.mutation.as_ref())
            else {
                return;
            };
            InterruptedMutation {
                task_id: task_id.to_string(),
                kind: mutation.kind,
                skills: mutation.skills.clone(),
                started_at_epoch_ms: mutation.started_at_epoch_ms,
                snapshot_paths: mutation.snapshot_paths.clone(),
            }
        };
        let _ = self.inner.marker.begin(&record);
    }

    /// Record a mandatory backup snapshot as soon as it exists so the next
    /// startup report can point at it.
    fn record_snapshot(&self, task_id: &str, snapshot: &Path) {
        let record = {
            let mut tasks = self.lock_tasks();
            let Some(entry) = tasks.get_mut(task_id) else {
                return;
            };
            let Some(mutation) = entry.mutation.as_mut() else {
                return;
            };
            let snapshot = snapshot.to_string_lossy().into_owned();
            if !mutation.snapshot_paths.contains(&snapshot) {
                mutation.snapshot_paths.push(snapshot);
            }
            InterruptedMutation {
                task_id: task_id.to_string(),
                kind: mutation.kind,
                skills: mutation.skills.clone(),
                started_at_epoch_ms: mutation.started_at_epoch_ms,
                snapshot_paths: mutation.snapshot_paths.clone(),
            }
        };
        let _ = self.inner.marker.begin(&record);
    }

    /// Cancel every running task (preview included). Mutation tokens bypass the
    /// user-facing `cancellable` seal: a confirmed app exit must kill the
    /// process group even while a CLI is writing.
    pub fn cancel_all_tasks(&self) -> usize {
        let mut tasks = self.lock_tasks();
        let mut cancelled = 0;
        for entry in tasks.values_mut() {
            if entry.state == TaskState::Running {
                entry.cancel.cancel();
                cancelled += 1;
            }
        }
        cancelled
    }

    /// Stop for application exit: keep the marker (so the next launch warns),
    /// cancel all tasks and wait briefly for mutation process groups to be
    /// reaped. The executor escalates SIGTERM -> 3s -> SIGKILL, so six seconds
    /// covers the kill plus pipe cleanup.
    pub async fn shutdown_mutations(&self) -> ShutdownReport {
        self.inner.shutting_down.store(true, Ordering::SeqCst);
        let active = self.active_mutation();
        let cancelled_tasks = self.cancel_all_tasks();
        if active.is_none() {
            return ShutdownReport {
                active,
                cancelled_tasks,
                timed_out: false,
            };
        }

        let deadline = tokio::time::Instant::now() + Duration::from_secs(6);
        let timed_out = loop {
            if !self.has_running_mutation() {
                break false;
            }
            let notified = self.inner.finished_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if !self.has_running_mutation() {
                break false;
            }
            tokio::select! {
                _ = notified => {}
                _ = tokio::time::sleep_until(deadline) => break true,
            }
        };
        ShutdownReport {
            active,
            cancelled_tasks,
            timed_out,
        }
    }

    /// Remove the startup interruption marker after the user acknowledges it.
    pub fn dismiss_interrupted_marker(&self) {
        self.inner.marker.clear();
    }

    /// Read the startup interruption report for tests and diagnostics.
    pub fn interrupted_report(&self) -> Option<crate::interrupted::InterruptedReport> {
        self.inner.marker.inspect()
    }

    /// Validate the source synchronously, then run a read-only preview task.
    /// Preview does not acquire the mutation semaphore.
    pub async fn start_preview(
        &self,
        source: &str,
        on_event: EventSink,
    ) -> Result<TaskId, AppError> {
        self.ensure_not_shutting_down()?;
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
                    mutation: None,
                    action: TaskAction::Preview,
                    started_at_epoch_ms: now_ms(),
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

    /// Register one install/update/uninstall task and write the interruption
    /// marker. The task entry and its mutation share one clock read.
    fn register_mutation(
        &self,
        action: TaskAction,
        kind: MutationKind,
        skills: Vec<String>,
    ) -> (TaskId, CancelToken) {
        let task_id = format!("task-{}", self.inner.next_id.fetch_add(1, Ordering::SeqCst));
        let cancel = CancelToken::new();
        let started_at_epoch_ms = now_ms();
        {
            let mut tasks = self.lock_tasks();
            tasks.insert(
                task_id.clone(),
                TaskEntry {
                    state: TaskState::Running,
                    cancel: cancel.clone(),
                    cancellable: true,
                    mutation: Some(MutationState {
                        kind,
                        skills,
                        started_at_epoch_ms,
                        snapshot_paths: Vec::new(),
                    }),
                    action,
                    started_at_epoch_ms,
                },
            );
        }
        self.begin_marker(&task_id);
        (task_id, cancel)
    }

    /// Validate a mutation request synchronously, acquire the single-writer
    /// permit, and run the install task. Same-name conflicts are rejected
    /// before a task exists.
    pub async fn start_install(
        &self,
        request: InstallRequest,
        on_event: EventSink,
    ) -> Result<TaskId, AppError> {
        self.ensure_not_shutting_down()?;
        let paths = self.inner.paths.clone();
        let plan = tokio::task::spawn_blocking(move || {
            let plan = validate_request(&paths, &request)?;
            let retry_source = plan.retry.then_some(plan.source.as_str());
            if let Some(conflict) = install::conflicts_for(&paths, &plan.skills, retry_source)? {
                return Err(conflict);
            }
            Ok::<InstallPlan, AppError>(plan)
        })
        .await
        .map_err(|error| AppError::task_internal("安装", error))??;

        // The permit is taken after validation so invalid requests do not
        // briefly occupy the write slot, and held for the whole task.
        let permit = self.try_acquire_mutation()?;
        let (task_id, cancel) = self.register_mutation(
            TaskAction::Install,
            MutationKind::Install,
            plan.skills.clone(),
        );

        let action = install::InstallAction::new(
            self.inner.paths.clone(),
            self.inner.config.install_timeout,
            task_id.clone(),
        );
        let finish_sink = on_event.clone();
        let ctx = self.session_context(&task_id, on_event, cancel);
        let manager = self.clone();
        let spawned_id = task_id.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let outcome = session::run(ctx, action, plan).await;
            manager.finish(&spawned_id, &finish_sink, outcome);
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

    /// Run a serialized native detach. The single-writer permit is acquired
    /// before any filesystem scan, so detach cannot interleave with an
    /// install/update/uninstall. No CLI process is ever spawned here.
    pub async fn detach_agent(
        &self,
        skill_id: String,
        agent_id: String,
    ) -> Result<crate::detach::DetachResult, AppError> {
        self.ensure_not_shutting_down()?;
        let permit = self.try_acquire_mutation()?;
        let paths = self.inner.paths.clone();
        tokio::task::spawn_blocking(move || {
            // Keep the write slot for the whole scan + unlink/delete.
            let _permit = permit;
            let agents = crate::inventory::detected_agent_dirs(&paths);
            crate::detach::detach_agent(&paths, &agents, &skill_id, &agent_id)
        })
        .await
        .map_err(|error| AppError::agent_detach_failed(format!("断链任务失败：{error}")))?
    }

    /// Serialized native attach (the inverse of detach). Takes the same
    /// single-writer permit; no CLI process is spawned.
    pub async fn attach_agent(
        &self,
        skill_id: String,
        agent_id: String,
    ) -> Result<crate::attach::AttachResult, AppError> {
        self.ensure_not_shutting_down()?;
        let permit = self.try_acquire_mutation()?;
        let paths = self.inner.paths.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let agents = crate::inventory::detected_agent_dirs(&paths);
            crate::attach::attach_agent(&paths, &agents, &skill_id, &agent_id)
        })
        .await
        .map_err(|error| AppError::agent_attach_failed(format!("挂载任务失败：{error}")))?
    }

    /// Persist a user-defined extra agent. Serialized so an install never sees
    /// a half-written configuration file.
    pub async fn add_extra_agent(
        &self,
        entry: crate::agents::extras::UserExtraAgent,
    ) -> Result<crate::agents::extras::ExtraAgent, AppError> {
        self.ensure_not_shutting_down()?;
        let permit = self.try_acquire_mutation()?;
        let paths = self.inner.paths.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            crate::agents::extras::add_user_agent(&paths, &entry)
        })
        .await
        .map_err(|error| AppError::extra_agent_config(format!("保存额外 agent 失败：{error}")))?
    }

    /// Detach every attached skill, then remove one user-defined extra agent.
    /// A failed detach aborts before the definition is removed, so leftovers
    /// never become invisible orphans.
    pub async fn remove_extra_agent(
        &self,
        agent_id: String,
    ) -> Result<crate::agents::extras::RemoveExtraAgentResult, AppError> {
        self.ensure_not_shutting_down()?;
        let permit = self.try_acquire_mutation()?;
        let paths = self.inner.paths.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let extra = crate::agents::extras::find(&paths, &agent_id)
                .ok_or_else(|| AppError::extra_agent_not_found(&agent_id))?;
            if extra.builtin {
                return Err(AppError::extra_agent_builtin(&extra.display_name));
            }
            let agents = crate::inventory::detected_agent_dirs(&paths);
            let snapshot = crate::inventory::read_with_agents(&paths, &agents)?;
            let mut detached = Vec::new();
            for skill in &snapshot.skills {
                let attached = skill
                    .attachments
                    .iter()
                    .any(|attachment| attachment.agent_id == agent_id && attachment.detachable);
                if !attached {
                    continue;
                }
                let result = crate::detach::detach_agent(&paths, &agents, &skill.name, &agent_id)?;
                detached.push(result.skill_id);
            }
            crate::agents::extras::remove_user_agent(&paths, &agent_id)?;
            Ok(crate::agents::extras::RemoveExtraAgentResult {
                id: agent_id,
                display_name: extra.display_name,
                detached,
            })
        })
        .await
        .map_err(|error| AppError::extra_agent_config(format!("删除额外 agent 失败：{error}")))?
    }

    /// Plan and run a serialized update task. The selected ids are the
    /// frontend's update candidates; Rust independently re-checks management,
    /// canonical presence and lock source resolution. `skills update` and
    /// `skills check` are never spawned: every skill is rebuilt as `add`.
    pub async fn start_update(
        &self,
        skill_ids: Vec<String>,
        on_event: EventSink,
    ) -> Result<TaskId, AppError> {
        self.ensure_not_shutting_down()?;
        let names = install::validate_skill_names(&skill_ids)?;
        // Hold the write slot while planning so the lock/inventory snapshot
        // cannot be read in the middle of another mutation.
        let permit = self.try_acquire_mutation()?;
        let paths = self.inner.paths.clone();
        let plan = match tokio::task::spawn_blocking(move || {
            let agents = crate::inventory::detected_agent_dirs(&paths);
            update_task::plan_updates(&paths, &agents, &names)
        })
        .await
        {
            Ok(Ok(plan)) => plan,
            Ok(Err(error)) => return Err(error),
            Err(error) => {
                return Err(AppError::task_internal("更新", error));
            }
        };

        let (task_id, cancel) = self.register_mutation(
            TaskAction::Update,
            MutationKind::Update,
            plan.items.iter().map(|item| item.name.clone()).collect(),
        );

        let action = update_task::UpdateAction::new(
            self.inner.paths.clone(),
            self.inner.config.install_timeout,
        );
        let finish_sink = on_event.clone();
        let ctx = self.session_context(&task_id, on_event, cancel);
        let manager = self.clone();
        let spawned_id = task_id.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let outcome = session::run(ctx, action, plan).await;
            manager.finish(&spawned_id, &finish_sink, outcome);
        });
        Ok(task_id)
    }

    /// Validate an uninstall target synchronously, acquire the single-writer
    /// permit, and run the destructive task. Managed and copy-only skills are
    /// allowed; unmanaged skills and copy attachments referenced by another
    /// agent's symlink are rejected before a task exists.
    pub async fn start_uninstall(
        &self,
        skill_id: String,
        on_event: EventSink,
    ) -> Result<TaskId, AppError> {
        self.ensure_not_shutting_down()?;
        // Hold the write slot while planning so the lock/inventory snapshot
        // cannot be read in the middle of another mutation.
        let permit = self.try_acquire_mutation()?;
        let paths = self.inner.paths.clone();
        let plan = match tokio::task::spawn_blocking(move || {
            let agents = crate::inventory::detected_agent_dirs(&paths);
            uninstall_task::plan_uninstall(&paths, &agents, &skill_id)
        })
        .await
        {
            Ok(Ok(plan)) => plan,
            Ok(Err(error)) => return Err(error),
            Err(error) => {
                return Err(AppError::task_internal("卸载", error));
            }
        };

        let (task_id, cancel) = self.register_mutation(
            TaskAction::Uninstall,
            MutationKind::Uninstall,
            vec![plan.name.clone()],
        );

        let action = uninstall_task::UninstallAction::new(
            self.inner.paths.clone(),
            self.inner.config.install_timeout,
        );
        let finish_sink = on_event.clone();
        let ctx = self.session_context(&task_id, on_event, cancel);
        let manager = self.clone();
        let spawned_id = task_id.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let outcome = session::run(ctx, action, plan).await;
            manager.finish(&spawned_id, &finish_sink, outcome);
        });
        Ok(task_id)
    }

    /// Toggle cancellation acceptance for update tasks. Updates accept a
    /// cancel request between skills only, never while one skill is being
    /// backed up, written, or verified.
    fn set_task_cancellable(&self, task_id: &str, cancellable: bool) {
        let mut tasks = self.lock_tasks();
        if let Some(entry) = tasks.get_mut(task_id) {
            entry.cancellable = cancellable;
        }
    }

    /// Narrow session handle: the session never touches the task map or the
    /// marker directly.
    fn session_context(
        &self,
        task_id: &str,
        sink: EventSink,
        cancel: CancelToken,
    ) -> session::SessionContext {
        let snapshot_manager = self.clone();
        let snapshot_task = task_id.to_string();
        let record_snapshot = Arc::new(move |path: &Path| {
            snapshot_manager.record_snapshot(&snapshot_task, path);
        });
        let cancellable_manager = self.clone();
        let cancellable_task = task_id.to_string();
        let set_cancellable = Arc::new(move |value: bool| {
            cancellable_manager.set_task_cancellable(&cancellable_task, value);
        });
        session::SessionContext {
            task_id: task_id.to_string(),
            sink,
            cancel,
            runtime: Arc::clone(&self.inner.runtime),
            executor: Arc::clone(&self.inner.executor),
            record_snapshot,
            set_cancellable,
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
            Err(error) => {
                self.finish(
                    &task_id,
                    &sink,
                    FinishOutcome::Failed(cli::classify_exec_error(CliAction::Preview, error)),
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
                    action: TaskAction::Preview,
                    overall: TaskOverall::Success,
                    skills: Vec::new(),
                    warnings: Vec::new(),
                    backup_path: None,
                    log_path: None,
                    started_at: 0,
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
                    action: TaskAction::Preview,
                    overall: TaskOverall::Success,
                    skills: Vec::new(),
                    warnings: Vec::new(),
                    backup_path: None,
                    log_path: None,
                    started_at: 0,
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
                        action: TaskAction::Preview,
                        overall: TaskOverall::Success,
                        skills: Vec::new(),
                        warnings: vec![warning],
                        backup_path: None,
                        log_path: None,
                        started_at: 0,
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
                        FinishOutcome::Failed(
                            cli::classify_add_failure(&output.stdout, &output.stderr)
                                .to_app_error(CliAction::Preview),
                        ),
                    );
                }
            }
        }
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
        let (cancel_requested, clear_marker, action, started_at) = {
            let mut tasks = self.lock_tasks();
            let Some(entry) = tasks.get_mut(task_id) else {
                return;
            };
            if entry.state == TaskState::Finished {
                return;
            }
            entry.state = TaskState::Finished;
            (
                entry.cancel.is_cancelled(),
                // A confirmed application exit keeps the marker so the next
                // launch can warn and point at the retained snapshots.
                entry.mutation.is_some() && !self.inner.shutting_down.load(Ordering::SeqCst),
                entry.action,
                entry.started_at_epoch_ms,
            )
        };
        if clear_marker {
            self.inner.marker.clear();
        }
        self.inner.finished_notify.notify_waiters();
        let outcome = if cancel_requested {
            match outcome {
                // A cancellation that races with the final event keeps the
                // per-skill results instead of discarding what already ran.
                FinishOutcome::Completed(mut result, _) if !result.skills.is_empty() => {
                    result.overall = TaskOverall::Cancelled;
                    FinishOutcome::Completed(result, "操作已取消。".to_string())
                }
                _ => FinishOutcome::Cancelled,
            }
        } else {
            outcome
        };
        let event = match outcome {
            FinishOutcome::Completed(mut result, message) => {
                result.action = action;
                result.started_at = started_at;
                let phase = if result.overall == TaskOverall::Cancelled {
                    TaskPhase::Cancelled
                } else {
                    TaskPhase::Completed
                };
                let error =
                    (result.overall == TaskOverall::Cancelled).then(AppError::task_cancelled);
                let total = result
                    .preview
                    .as_ref()
                    .map(|preview| preview.skills.len().max(1) as u32)
                    .or_else(|| (!result.skills.is_empty()).then_some(result.skills.len() as u32))
                    .unwrap_or(1);
                TaskEvent::finished(task_id, phase, message, total, total, result, error)
            }
            FinishOutcome::Failed(error) => TaskEvent::finished(
                task_id,
                TaskPhase::Failed,
                error.message.clone(),
                0,
                1,
                TaskResult {
                    action,
                    overall: TaskOverall::Failed,
                    skills: Vec::new(),
                    warnings: Vec::new(),
                    backup_path: None,
                    log_path: None,
                    started_at,
                    finished_at_epoch_ms: now_ms(),
                    preview: None,
                },
                Some(error),
            ),
            FinishOutcome::Cancelled => TaskEvent::finished(
                task_id,
                TaskPhase::Cancelled,
                "任务已取消。",
                0,
                1,
                TaskResult {
                    action,
                    overall: TaskOverall::Cancelled,
                    skills: Vec::new(),
                    warnings: Vec::new(),
                    backup_path: None,
                    log_path: None,
                    started_at,
                    finished_at_epoch_ms: now_ms(),
                    preview: None,
                },
                Some(AppError::task_cancelled()),
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

pub(super) fn now_ms() -> u64 {
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
    use crate::baseline::BaselineStore;
    use crate::cli::CliRuntime;
    use crate::error::ErrorCode;
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
        assert_eq!(result.warnings[0].code, ErrorCode::SourcePreviewFailed);
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
        assert_eq!(error.code, ErrorCode::NetworkFetchFailed);
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
        assert_eq!(event.error.unwrap().code, ErrorCode::CliTimeout);
    }

    #[tokio::test]
    async fn preview_spawn_failure_maps_to_cli_spawn_failed() {
        let (manager, _) = manager(
            ScriptedExecutor::new().expect_any(ScriptedResponse::spawn_error("npx not found")),
            HashMap::new(),
            Duration::from_secs(60),
        );
        let (sink, mut receiver) = sink();
        manager.start_preview("owner/repo", sink).await.unwrap();
        let event = tokio::time::timeout(Duration::from_secs(2), final_event(&mut receiver))
            .await
            .expect("spawn failure should finish the task");

        assert_eq!(event.phase, TaskPhase::Failed);
        assert_eq!(event.error.unwrap().code, ErrorCode::CliSpawnFailed);
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
        assert_eq!(error.code, ErrorCode::TaskCannotCancel);
        let missing = manager.cancel_task("task-does-not-exist").unwrap_err();
        assert_eq!(missing.code, ErrorCode::TaskNotFound);
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
        assert_eq!(busy.code, ErrorCode::TaskBusy);

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
        assert_eq!(error.code, ErrorCode::SourceUnsupportedSubdir);
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
        assert_eq!(event.error.unwrap().code, ErrorCode::EnvNodeMissing);
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
            retry: false,
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
        assert_eq!(result.action, TaskAction::Install);
        assert!(result.started_at > 0, "startedAt is populated");
        assert_eq!(result.overall, TaskOverall::Success);
        assert_eq!(result.skills.len(), 1);
        assert_eq!(result.skills[0].name, "find-skills");
        assert_eq!(result.skills[0].status, TaskSkillStatus::Installed);
        assert!(result.warnings.is_empty());
        // The verified folder becomes the known-good baseline so later local
        // modifications are detected (spec §3.6).
        let baseline = BaselineStore::new(&paths)
            .load("find-skills")
            .expect("baseline recorded after a verified install");
        assert!(baseline.verified);
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
            ErrorCode::SkillNotInSource
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
        assert_eq!(result.warnings[0].code, ErrorCode::VerifyCopyFallback);
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
            ErrorCode::VerifyCanonicalMissing
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
        assert_eq!(final_event.error.unwrap().code, ErrorCode::CliInvalidJson);
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
        assert_eq!(
            final_event.error.unwrap().code,
            ErrorCode::NetworkFetchFailed
        );
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
        assert_eq!(error.code, ErrorCode::SkillConflict);
        assert_eq!(fixture.executor.call_count(), 0);
        assert!(receiver.try_recv().is_err());
        assert!(taken.join("SKILL.md").is_file(), "no overwrite");
    }

    #[tokio::test]
    async fn explicit_retry_may_overwrite_a_same_source_partial_install() {
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
        // A previous attempt partially wrote the same source: managed lock
        // entry plus canonical folder, but the agent attachment is missing.
        write_pre_lock(&paths, &["find-skills"]);
        let canonical = home.join(".agents/skills/find-skills");
        fs::create_dir_all(&canonical).unwrap();
        fs::write(canonical.join("SKILL.md"), "# old attempt\n").unwrap();

        let (sink, _receiver) = sink();
        let error = manager
            .start_install(install_request(&["find-skills"], &["qoder"]), sink)
            .await
            .unwrap_err();
        assert_eq!(
            error.code,
            ErrorCode::SkillConflict,
            "plain duplicate rejected"
        );
        assert_eq!(executor.call_count(), 0);

        let mut retry = install_request(&["find-skills"], &["qoder"]);
        retry.retry = true;
        let (_phases, final_event) = run_install_task(&manager, retry).await;
        assert_eq!(final_event.phase, TaskPhase::Completed);
        let result = final_event.result.expect("retry result");
        assert_eq!(result.overall, TaskOverall::Success);
        assert_eq!(result.skills[0].status, TaskSkillStatus::Installed);
        assert_eq!(executor.call_count(), 1);
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
        assert_eq!(error.code, ErrorCode::LockCorrupt);
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
        assert!(paths.interrupted_marker_path().exists());
        let final_event = tokio::time::timeout(Duration::from_secs(2), final_event(&mut receiver))
            .await
            .expect("cancel should finish the task");
        assert_eq!(final_event.phase, TaskPhase::Cancelled);
        assert_eq!(final_event.result.unwrap().overall, TaskOverall::Cancelled);
        assert_eq!(executor.call_count(), 0);
        assert_eq!(fs::read_to_string(paths.lock_path()).unwrap(), before);
        assert!(
            !paths.interrupted_marker_path().exists(),
            "a pre-spawn cancel clears the interruption marker"
        );
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
        assert_eq!(error.code, ErrorCode::TaskCannotCancel);

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
        assert_eq!(error.code, ErrorCode::TaskBusy);

        let final_event = tokio::time::timeout(Duration::from_secs(2), final_event(&mut receiver))
            .await
            .expect("first install should finish");
        assert_eq!(final_event.phase, TaskPhase::Completed);
    }

    #[tokio::test]
    async fn detach_is_serialized_and_never_spawns_a_cli() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(home.join(".qoder")).unwrap();
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        paths.ensure_dirs().unwrap();
        let canonical = paths.canonical_skills_dir().join("alpha");
        fs::create_dir_all(&canonical).unwrap();
        fs::write(canonical.join("SKILL.md"), "# alpha\n").unwrap();
        let skills_dir = home.join(".qoder/skills");
        fs::create_dir_all(&skills_dir).unwrap();
        std::os::unix::fs::symlink(&canonical, skills_dir.join("alpha")).unwrap();
        write_pre_lock(&paths, &["alpha"]);
        let lock_before = fs::read(paths.lock_path()).unwrap();

        // A hanging runtime proves detach never waits for Node/npx discovery.
        let executor = Arc::new(ScriptedExecutor::new());
        let manager = TaskManager::new(
            Arc::new(HangingRuntime),
            executor.clone(),
            Arc::new(FakeConflicts::default()),
            paths.clone(),
            TaskManagerConfig::default(),
        );

        let permit = manager.try_acquire_mutation().unwrap();
        let error = manager
            .detach_agent("alpha".to_string(), "qoder".to_string())
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::TaskBusy);
        assert!(skills_dir.join("alpha").is_symlink());
        drop(permit);

        let result = manager
            .detach_agent("alpha".to_string(), "qoder".to_string())
            .await
            .unwrap();
        assert_eq!(result.skill_id, "alpha");
        assert_eq!(result.agent_id, "qoder");
        assert_eq!(result.agent_name, "Qoder");
        assert_eq!(result.mode, crate::inventory::AttachmentMode::Link);
        assert!(fs::symlink_metadata(skills_dir.join("alpha")).is_err());
        assert!(canonical.join("SKILL.md").is_file());
        assert_eq!(fs::read(paths.lock_path()).unwrap(), lock_before);
        assert_eq!(executor.call_count(), 0, "detach must not spawn a CLI");
    }

    #[tokio::test]
    async fn detach_is_rejected_while_the_app_is_shutting_down() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(tmp.path().join("home"), tmp.path().join("app-data"));
        paths.ensure_dirs().unwrap();
        let manager = TaskManager::new(
            Arc::new(FakeRuntime::ok()),
            Arc::new(ScriptedExecutor::new()),
            Arc::new(FakeConflicts::default()),
            paths,
            TaskManagerConfig::default(),
        );
        manager.shutdown_mutations().await;
        let error = manager
            .detach_agent("alpha".to_string(), "qoder".to_string())
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::TaskShuttingDown);
    }

    // ------------------------------------------------------------------
    // Interrupted-mutation marker and exit protection (ticket 10)
    // ------------------------------------------------------------------

    fn hanging_install_fixture() -> (
        tempfile::TempDir,
        PathBuf,
        AppPaths,
        Arc<ScriptedExecutor>,
        TaskManager,
    ) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(home.join(".qoder")).unwrap();
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        paths.ensure_dirs().unwrap();
        let executor = Arc::new(ScriptedExecutor::new().expect_any(ScriptedResponse::hang()));
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
        (tmp, home, paths, executor, manager)
    }

    async fn wait_for_phase(
        receiver: &mut tokio::sync::mpsc::UnboundedReceiver<TaskEvent>,
        phase: TaskPhase,
    ) {
        loop {
            let event = receiver.recv().await.unwrap();
            if event.phase == phase {
                return;
            }
        }
    }

    #[tokio::test]
    async fn mutation_writes_the_interruption_marker_and_clears_it_on_completion() {
        let fixture = install_fixture(InstallResponse::success(INSTALL_SUCCESS, || {}));
        let marker = fixture.paths.interrupted_marker_path();
        let (sink, mut receiver) = sink();
        fixture
            .manager
            .start_install(install_request(&["find-skills"], &["qoder"]), sink)
            .await
            .unwrap();

        let record = crate::interrupted::MarkerStore::new(&fixture.paths).read();
        let crate::interrupted::MarkerRead::Present(record) = record else {
            panic!("mutation marker should exist while the task runs");
        };
        assert_eq!(record.kind, MutationKind::Install);
        assert_eq!(record.skills, vec!["find-skills"]);
        assert!(marker.exists());

        let final_event = tokio::time::timeout(Duration::from_secs(2), final_event(&mut receiver))
            .await
            .expect("install should finish");
        assert_eq!(final_event.phase, TaskPhase::Completed);
        assert!(!marker.exists(), "normal completion must clear the marker");
    }

    #[tokio::test]
    async fn active_mutation_reports_kind_skills_and_the_lock_snapshot() {
        let (_tmp, _home, paths, executor, manager) = hanging_install_fixture();
        let (event_sink, mut receiver) = sink();
        let task_id = manager
            .start_install(install_request(&["find-skills"], &["qoder"]), event_sink)
            .await
            .unwrap();
        wait_for_phase(&mut receiver, TaskPhase::RunningCli).await;

        let active = manager.active_mutation().expect("mutation is running");
        assert_eq!(active.task_id, task_id);
        assert_eq!(active.kind, MutationKind::Install);
        assert_eq!(active.skills, vec!["find-skills"]);
        assert_eq!(active.snapshot_paths.len(), 1);
        assert!(Path::new(&active.snapshot_paths[0]).exists());
        assert!(manager.has_running_mutation());
        assert_eq!(executor.call_count(), 1);

        manager.cancel_all_tasks();
        let final_event = tokio::time::timeout(Duration::from_secs(2), final_event(&mut receiver))
            .await
            .expect("cancelled install should finish");
        assert_eq!(final_event.phase, TaskPhase::Cancelled);
        assert!(!manager.has_running_mutation());
        assert!(!paths.interrupted_marker_path().exists());
    }

    #[tokio::test]
    async fn confirmed_exit_cancels_a_sealed_mutation_and_keeps_the_marker() {
        let (_tmp, _home, paths, _executor, manager) = hanging_install_fixture();
        let (event_sink, mut receiver) = sink();
        manager
            .start_install(install_request(&["find-skills"], &["qoder"]), event_sink)
            .await
            .unwrap();
        wait_for_phase(&mut receiver, TaskPhase::RunningCli).await;

        // The task is sealed: a user-facing cancel is rejected, but shutdown
        // must still cancel it.
        let error = manager.cancel_task("task-1").unwrap_err();
        assert_eq!(error.code, ErrorCode::TaskCannotCancel);

        let report = manager.shutdown_mutations().await;
        assert_eq!(report.cancelled_tasks, 1);
        assert!(!report.timed_out);
        let active = report.active.as_ref().expect("active mutation is reported");
        assert_eq!(active.kind, MutationKind::Install);
        assert_eq!(active.snapshot_paths.len(), 1);
        assert!(!manager.has_running_mutation());

        // The marker and the mandatory lock snapshot survive.
        let startup = manager
            .interrupted_report()
            .expect("next startup must report the interruption");
        assert_eq!(startup.kind, Some(MutationKind::Install));
        assert_eq!(startup.skills, vec!["find-skills"]);
        assert!(startup.lock_is_healthy());
        assert_eq!(startup.snapshots_present.len(), 1);
        assert!(startup.snapshots_are_intact());
        assert!(paths.interrupted_marker_path().exists());

        // No new task may start while the app is exiting.
        let (again_sink, _again_receiver) = sink();
        let error = manager
            .start_install(install_request(&["other"], &["qoder"]), again_sink)
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::TaskShuttingDown);

        // Dismissing the warning removes the marker but keeps the snapshot.
        manager.dismiss_interrupted_marker();
        assert!(manager.interrupted_report().is_none());
        assert!(Path::new(&startup.snapshots_present[0]).exists());
    }

    #[test]
    fn task_result_serializes_action_and_started_at_for_the_frontend() {
        let result = TaskResult {
            action: TaskAction::Uninstall,
            overall: TaskOverall::Success,
            skills: Vec::new(),
            warnings: Vec::new(),
            backup_path: None,
            log_path: None,
            started_at: 1_793_163_988_000,
            finished_at_epoch_ms: 1_793_163_989_000,
            preview: None,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["action"], "uninstall");
        assert_eq!(json["startedAt"], 1_793_163_988_000u64);
        assert_eq!(json["finishedAtEpochMs"], 1_793_163_989_000u64);
    }

    #[tokio::test]
    async fn cancelled_preview_never_leaves_an_interruption_marker() {
        let (manager, _executor) = manager(
            ScriptedExecutor::new().expect_any(ScriptedResponse::hang()),
            HashMap::new(),
            Duration::from_secs(60),
        );
        let (event_sink, mut receiver) = sink();
        let task_id = manager
            .start_preview("owner/repo", event_sink)
            .await
            .unwrap();
        wait_for_phase(&mut receiver, TaskPhase::Cloning).await;
        manager.cancel_task(&task_id).unwrap();
        let final_event = tokio::time::timeout(Duration::from_secs(2), final_event(&mut receiver))
            .await
            .unwrap();
        assert_eq!(final_event.phase, TaskPhase::Cancelled);
        assert!(manager.active_mutation().is_none());
    }
}
