//! Execution of confirmed Batch targets, with action state retained for one batch.
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::error::{AppError, ErrorCode};
use crate::paths::AppPaths;
use crate::process::CancelToken;
use crate::projects::ProjectRegistry;
use crate::scope_layout::ScopeLayout;

use super::batch::{self, BatchAction, BatchPlan, BatchRow, BatchStatus, PlannedTarget};
use super::manager::{
    EventSink, FinishOutcome, TaskAction, TaskEnvelope, TaskEvent, TaskOverall, TaskPhase,
    TaskSkillStatus,
};
use super::session::{self, MutationAction};
use super::{uninstall, update};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Continuation {
    Continue,
    Stop,
}

pub(super) enum TargetCompletion {
    /// A failed prerequisite consumes this target's progress slot, without
    /// attempting a write, emitting progress or contributing a task-skill row.
    DependencyBlocked,
    Attempted {
        skill_status: TaskSkillStatus,
        warnings: Vec<AppError>,
        continuation: Continuation,
    },
}

pub(super) struct TargetOutcome {
    pub row: BatchRow,
    pub completion: TargetCompletion,
}

/// Production and scripted tests cross the same internal execution seam.
/// Calls are serial, in confirmed order; a stop never interrupts an active call.
#[async_trait::async_trait]
pub(super) trait TargetExecutor: Send {
    async fn execute(
        &mut self,
        target: &PlannedTarget,
        completed: u32,
        total: u32,
    ) -> TargetOutcome;
}

pub(super) struct BatchExecution {
    paths: AppPaths,
    projects: Arc<ProjectRegistry>,
    layout: ScopeLayout,
    ctx: session::SessionContext,
    install_timeout: Duration,
    plan: BatchPlan,
    copied: HashSet<String>,
    copy_failed: HashSet<String>,
}

impl BatchExecution {
    pub(super) fn new(
        paths: AppPaths,
        projects: Arc<ProjectRegistry>,
        layout: ScopeLayout,
        ctx: session::SessionContext,
        install_timeout: Duration,
        plan: BatchPlan,
    ) -> Self {
        Self {
            paths,
            projects,
            layout,
            ctx,
            install_timeout,
            plan,
            copied: HashSet::new(),
            copy_failed: HashSet::new(),
        }
    }

    async fn check_target(&self, target: &PlannedTarget) -> Result<(), AppError> {
        if self.plan.copy.is_some() {
            return Ok(());
        }
        let paths = self.paths.clone();
        let projects = self.projects.clone();
        let layout = self.layout.clone();
        let plan = self.plan.clone();
        let target = target.clone();
        tokio::task::spawn_blocking(move || {
            let current = ScopeLayout::resolve_detected(&paths, &projects, &layout.scope)?;
            current.ensure_mutation_ready()?;
            if current.root != plan.root
                || current.project.as_ref().map(|p| &p.check_revision)
                    != plan.project_revision.as_ref()
            {
                return Err(AppError::project_root_unavailable(
                    "项目",
                    "项目根目录已变化。",
                ));
            }
            batch::verify_target(&paths, &current, &plan, &target)
        })
        .await
        .map_err(|e| AppError::task_internal("批量校验", e))?
    }
}

#[async_trait::async_trait]
impl TargetExecutor for BatchExecution {
    async fn execute(
        &mut self,
        target: &PlannedTarget,
        completed: u32,
        total: u32,
    ) -> TargetOutcome {
        if self.plan.copy.is_some()
            && target.target.agent_id.is_some()
            && self.copy_failed.contains(&target.target.name)
        {
            let mut blocked = target.target.clone();
            blocked.reason = Some("该技能复制未成功，挂载未执行。".into());
            return TargetOutcome {
                row: BatchRow {
                    target: blocked,
                    status: BatchStatus::NotExecuted,
                    error: None,
                    backup_path: None,
                },
                completion: TargetCompletion::DependencyBlocked,
            };
        }
        let layout = &self.layout;
        let ctx = &self.ctx;
        let plan = &self.plan;
        let mut warnings = Vec::new();
        let mut continuation = Continuation::Continue;
        let label = target.target.agent_name.as_ref().map_or_else(
            || target.target.name.clone(),
            |agent| format!("{} · {agent}", target.target.name),
        );
        (ctx.sink)(TaskEvent::progress(
            &ctx.task_id,
            TaskPhase::Preparing,
            format!("正在{} {label}…", plan.preview.action.label()),
            completed,
            total,
        ));
        let observed = Arc::new(Mutex::new(None));
        let snapshots = Arc::new(Mutex::new(Vec::<String>::new()));
        let snapshot_copy = snapshots.clone();
        let record_snapshot = ctx.record_snapshot.clone();
        let paths = self.paths.clone();
        let paths_for_snapshot = paths.clone();
        let record = Arc::new(move |path: &std::path::Path| {
            record_snapshot(path);
            snapshot_copy
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(paths_for_snapshot.display(path));
        });
        let outer_sink = ctx.sink.clone();
        let skill_label = label.clone();
        let inner_sink: EventSink = Arc::new(move |mut event| {
            event.completed = Some(completed);
            event.total = Some(total);
            event.message = format!("{skill_label} · {}", event.message);
            outer_sink(event);
        });
        let inner_ctx = session::SessionContext {
            task_id: ctx.task_id.clone(),
            sink: inner_sink,
            // Stopping the batch cannot abort a target's write/verification.
            cancel: CancelToken::new(),
            envelope: TaskEnvelope {
                action: if plan.preview.action == BatchAction::Restore {
                    TaskAction::Install
                } else if plan.preview.action == BatchAction::Update {
                    TaskAction::Update
                } else {
                    TaskAction::Uninstall
                },
                ..ctx.envelope.clone()
            },
            runtime: ctx.runtime.clone(),
            executor: ctx.executor.clone(),
            record_snapshot: record.clone(),
            set_cancellable: Arc::new(|_| {}),
            expected_commands: target.target.command.clone().map(|command| vec![command]),
        };
        let checked = self.check_target(target).await;
        let outcome = match checked {
            Err(error) => Err(error),
            Ok(()) => match plan.preview.action {
                BatchAction::Update => {
                    let action = CheckedAction {
                        inner: update::UpdateAction::new(
                            paths.clone(),
                            layout.clone(),
                            self.install_timeout,
                        ),
                        paths: paths.clone(),
                        layout: layout.clone(),
                        plan: plan.clone(),
                        target: target.clone(),
                    };
                    let plan_paths = paths.clone();
                    let plan_layout = layout.clone();
                    let name = target.target.name.clone();
                    let prepared = tokio::task::spawn_blocking(move || {
                        update::plan_updates(&plan_paths, &plan_layout, &[name])
                    })
                    .await
                    .map_err(|e| AppError::task_internal("批量更新计划", e))
                    .and_then(|r| r);
                    match prepared {
                        Ok(prepared) => unit_outcome(
                            session::run(inner_ctx, action, prepared).await,
                            &mut warnings,
                        ),
                        Err(e) => Err(e),
                    }
                }
                BatchAction::Uninstall => {
                    let action = CheckedAction {
                        inner: uninstall::UninstallAction::new(
                            paths.clone(),
                            layout.clone(),
                            self.install_timeout,
                        ),
                        paths: paths.clone(),
                        layout: layout.clone(),
                        plan: plan.clone(),
                        target: target.clone(),
                    };
                    let plan_paths = paths.clone();
                    let plan_layout = layout.clone();
                    let skill_id = target.target.skill_id.clone();
                    let prepared = tokio::task::spawn_blocking(move || {
                        uninstall::plan_uninstall(&plan_paths, &plan_layout, &skill_id)
                    })
                    .await
                    .map_err(|e| AppError::task_internal("批量卸载计划", e))
                    .and_then(|r| r);
                    match prepared {
                        Ok(prepared) => unit_outcome(
                            session::run(inner_ctx, action, prepared).await,
                            &mut warnings,
                        ),
                        Err(e) => Err(e),
                    }
                }
                BatchAction::UninstallAgents => {
                    let layout = layout.clone();
                    let plan = plan.clone();
                    let target = target.clone();
                    tokio::task::spawn_blocking(move || {
                        crate::tasks::uninstall_agents::execute(
                            &paths,
                            &layout,
                            &target,
                            record.as_ref(),
                            &|| batch::verify_target(&paths, &layout, &plan, &target),
                        )
                    })
                    .await
                    .map_err(|e| AppError::task_internal("按 Agent 移除", e))
                    .and_then(|r| r)
                }
                BatchAction::Attach | BatchAction::Detach => {
                    let layout = layout.clone();
                    let plan = plan.clone();
                    let target = target.clone();
                    tokio::task::spawn_blocking(move || {
                        batch::verify_target(&paths, &layout, &plan, &target)?;
                        let agent = target.target.agent_id.as_deref().expect("agent target");
                        if plan.preview.action == BatchAction::Attach {
                            crate::attach::attach_agent_checked(
                                &paths,
                                &layout,
                                &target.target.skill_id,
                                agent,
                                &|| batch::verify_target(&paths, &layout, &plan, &target),
                            )?;
                        } else {
                            crate::detach::detach_agent_checked(
                                &paths,
                                &layout,
                                &target.target.skill_id,
                                agent,
                                record.as_ref(),
                                &|| batch::verify_target(&paths, &layout, &plan, &target),
                            )?;
                        }
                        Ok(())
                    })
                    .await
                    .map_err(|e| AppError::task_internal("批量挂载", e))
                    .and_then(|r| r)
                }
                BatchAction::CleanRecords => {
                    let write_layout = layout.clone();
                    let write_plan = plan.clone();
                    let write_target = target.clone();
                    let result = tokio::task::spawn_blocking(move || {
                        crate::tasks::stale_records::clean(
                            &paths,
                            &write_layout,
                            &write_plan,
                            &write_target,
                            record.as_ref(),
                            &|| {
                                batch::verify_target(
                                    &paths,
                                    &write_layout,
                                    &write_plan,
                                    &write_target,
                                )
                            },
                        )
                    })
                    .await
                    .map_err(|e| AppError::task_internal("清理记录", e))
                    .and_then(|r| r);
                    result.map(|w| warnings.extend(w))
                }
                BatchAction::Restore => {
                    let install =
                        plan.stale.as_ref().unwrap().installs[&target.target.name].clone();
                    let action = crate::tasks::stale_records::RestoreAction {
                        inner: crate::tasks::install::InstallAction::new(
                            paths.clone(),
                            layout.clone(),
                            self.install_timeout,
                            ctx.task_id.to_string(),
                        ),
                        paths,
                        layout: layout.clone(),
                        plan: plan.clone(),
                        target: target.clone(),
                        observed: observed.clone(),
                    };
                    unit_outcome(
                        session::run(inner_ctx, action, install).await,
                        &mut warnings,
                    )
                }
                BatchAction::CopyToProject => {
                    let plan = plan.clone();
                    let target = target.clone();
                    let published = self.copied.contains(&target.target.name);
                    tokio::task::spawn_blocking(move || {
                        crate::tasks::copy_to_project::execute(
                            &paths,
                            &plan,
                            &target,
                            published,
                            record.as_ref(),
                        )
                    })
                    .await
                    .map_err(|e| AppError::task_internal("复制到项目", e))
                    .and_then(|r| r)
                }
            },
        };
        let backup = snapshots
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .last()
            .cloned();
        let (mut status, mut error) = match outcome {
            Ok(()) => (BatchStatus::Success, None),
            Err(mut error) => {
                if plan.preview.action == BatchAction::Restore {
                    error.detail = Some(format!(
                        "{}\n{}",
                        error.detail.unwrap_or_default(),
                        crate::tasks::stale_records::observed_state(layout, &target.target.name)
                    ));
                }
                continuation = if batch::stops_batch(&error) {
                    Continuation::Stop
                } else {
                    Continuation::Continue
                };
                let status = if error.code == ErrorCode::BatchTargetChanged {
                    BatchStatus::Skipped
                } else {
                    BatchStatus::Failed
                };
                (status, Some(error))
            }
        };
        if plan.stale.is_some() {
            let observed = observed.lock().unwrap_or_else(|p| p.into_inner()).clone();
            match crate::tasks::stale_records::advance(
                layout,
                plan,
                &target.target.name,
                status == BatchStatus::Success,
                observed,
            ) {
                Ok(bytes) => {
                    if let Some(stale) = self.plan.stale.as_mut() {
                        stale.expected = bytes;
                    }
                }
                Err(e) => {
                    continuation = Continuation::Stop;
                    status = BatchStatus::Failed;
                    error = Some(e);
                }
            }
        }
        if self.plan.copy.is_some() && target.target.agent_id.is_none() {
            if status == BatchStatus::Success {
                self.copied.insert(target.target.name.clone());
            } else {
                self.copy_failed.insert(target.target.name.clone());
            }
        }
        let skill_status = if status == BatchStatus::Success {
            match self.plan.preview.action {
                BatchAction::Update => TaskSkillStatus::Updated,
                BatchAction::CleanRecords
                | BatchAction::Uninstall
                | BatchAction::UninstallAgents
                | BatchAction::Detach => TaskSkillStatus::Removed,
                BatchAction::Restore | BatchAction::Attach | BatchAction::CopyToProject => {
                    TaskSkillStatus::Installed
                }
            }
        } else if status == BatchStatus::Skipped {
            TaskSkillStatus::Skipped
        } else {
            TaskSkillStatus::Failed
        };

        TargetOutcome {
            row: BatchRow {
                target: target.target.clone(),
                status,
                error,
                backup_path: backup,
            },
            completion: TargetCompletion::Attempted {
                skill_status,
                warnings,
                continuation,
            },
        }
    }
}

fn unit_outcome(outcome: FinishOutcome, warnings: &mut Vec<AppError>) -> Result<(), AppError> {
    match outcome {
        FinishOutcome::Completed(result, _) => {
            warnings.extend(result.warnings);
            if result.overall == TaskOverall::Success {
                Ok(())
            } else {
                Err(result
                    .skills
                    .into_iter()
                    .find_map(|r| r.error)
                    .unwrap_or_else(|| AppError::verify_failed("目标未完成。")))
            }
        }
        FinishOutcome::Failed(error) => Err(error),
        FinishOutcome::Cancelled => Err(AppError::task_cancelled()),
    }
}

/// The session resolves the runtime before preflight. Check again there so
/// an edit made while runtime resolution was pending cannot be overwritten.
struct CheckedAction<A> {
    inner: A,
    paths: crate::paths::AppPaths,
    layout: ScopeLayout,
    plan: BatchPlan,
    target: PlannedTarget,
}
impl<A: MutationAction> MutationAction for CheckedAction<A> {
    type Plan = A::Plan;
    type Unit = A::Unit;
    fn profile(&self, p: &Self::Plan) -> session::SessionProfile {
        self.inner.profile(p)
    }
    fn uses_cli(&self, p: &Self::Plan) -> bool {
        self.inner.uses_cli(p)
    }
    fn execute_native(
        &self,
        u: &Self::Unit,
        p: &session::Preflight,
        r: &session::Reporter,
    ) -> Result<session::UnitOutcome, AppError> {
        self.inner.execute_native(u, p, r)
    }
    fn begin(&self, p: &Self::Plan) -> Result<session::StartOutcome<Self::Unit>, AppError> {
        self.inner.begin(p)
    }
    fn preflight(
        &self,
        u: &Self::Unit,
        r: &session::Reporter,
    ) -> Result<session::Preflight, AppError> {
        batch::verify_target(&self.paths, &self.layout, &self.plan, &self.target)?;
        self.inner.preflight(u, r)
    }
    fn before_write(&self, u: &Self::Unit) -> Result<(), AppError> {
        batch::verify_target(&self.paths, &self.layout, &self.plan, &self.target)?;
        self.inner.before_write(u)
    }
    fn command(
        &self,
        u: &Self::Unit,
        rt: &crate::cli::CliRuntime,
        r: &session::Reporter,
    ) -> crate::process::CommandSpec {
        self.inner.command(u, rt, r)
    }
    fn complete(
        &self,
        u: &Self::Unit,
        o: crate::process::ExecOutput,
        p: &session::Preflight,
        r: &session::Reporter,
    ) -> Result<session::UnitOutcome, AppError> {
        self.inner.complete(u, o, p, r)
    }
    fn on_infrastructure_failure(
        &self,
        u: &Self::Unit,
        e: AppError,
        p: Option<&session::Preflight>,
        r: &session::Reporter,
    ) -> session::FailureDisposition {
        self.inner.on_infrastructure_failure(u, e, p, r)
    }
    fn on_cancelled(&self, u: &Self::Unit, p: &session::Preflight, e: Option<AppError>) {
        self.inner.on_cancelled(u, p, e)
    }
}
