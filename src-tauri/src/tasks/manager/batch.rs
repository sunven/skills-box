//! One registered task owns every target, including native attachment writes.
use std::collections::HashMap;

use super::{
    EventSink, FinishOutcome, TaskAction, TaskEnvelope, TaskEvent, TaskId, TaskManager,
    TaskOverall, TaskPhase, TaskResult, TaskSkillResult,
};
use crate::error::AppError;
use crate::interrupted::MutationKind;
use crate::logs::OperationLog;
use crate::process::CancelToken;
use crate::scope::ScopeId;
use crate::scope_layout::ScopeLayout;
use crate::tasks::batch::{
    self, BatchAction, BatchConfirmation, BatchPlan, BatchPreview, BatchRequest, BatchResult,
    BatchRow, BatchStatus, PlannedTarget,
};
use crate::tasks::batch_execution::{
    BatchExecution, Continuation, TargetCompletion, TargetExecutor,
};
use crate::tasks::session;
use crate::tasks::update;
use crate::update::UpdateSkillResult;

impl TaskManager {
    async fn prepare_restoration(
        &self,
        layout: &ScopeLayout,
        plan: &mut BatchPlan,
    ) -> Result<(), AppError> {
        if plan.stale.as_ref().is_some_and(|s| s.source_checked) {
            return Ok(());
        }
        use crate::tasks::parser::PreviewParseOutcome;
        let mut sources = HashMap::new();
        let runtime = self.inner.runtime.cli_runtime().await;
        for target in &mut plan.targets {
            if !target.target.eligible {
                continue;
            }
            let install = &plan.stale.as_ref().unwrap().installs[&target.target.name];
            let result = if let Err(error) = &runtime {
                Err(error.clone())
            } else {
                if !sources.contains_key(&install.source) {
                    let result = crate::tasks::preview::source_preview(
                        self.inner.runtime.as_ref(),
                        self.inner.executor.as_ref(),
                        &self.inner.paths,
                        layout,
                        &install.source,
                        crate::timeouts::PREVIEW_TIMEOUT,
                        CancelToken::new(),
                        None,
                    )
                    .await
                    .and_then(|(output, parsed)| {
                        if !output.success() {
                            return Err(AppError::skill_install_failed("来源", output.stderr));
                        }
                        match parsed {
                            PreviewParseOutcome::Skills(skills) => Ok(skills),
                            PreviewParseOutcome::NoSkills => Ok(Vec::new()),
                            PreviewParseOutcome::Failed { raw } => {
                                Err(AppError::cli_invalid_json(raw))
                            }
                        }
                    });
                    sources.insert(install.source.clone(), result);
                }
                sources[&install.source].clone().and_then(|skills| {
                    let matches = skills
                        .iter()
                        .filter(|s| s.name.eq_ignore_ascii_case(&target.target.name))
                        .collect::<Vec<_>>();
                    if matches.len() != 1 || matches[0].name != target.target.name {
                        return Err(AppError::skill_not_in_source(
                            &target.target.name,
                            "原来源中找不到唯一的同名技能；可能已改名、删除或存在歧义。",
                        ));
                    }
                    Ok(())
                })
            };
            match result {
                Ok(()) => {
                    target.target.command = Some(crate::cli::commands::display_command(
                        &crate::cli::ScopedCli::new(runtime.as_ref().unwrap(), layout).install(
                            &install.source,
                            &install.skills,
                            &install.agent_ids,
                            self.inner.config.install_timeout,
                        ),
                    ))
                }
                Err(error) => {
                    target.target.eligible = false;
                    target.target.reason = Some(error.detail.unwrap_or(error.message));
                }
            }
        }
        plan.stale.as_mut().unwrap().source_checked = true;
        crate::tasks::stale_records::revision(plan);
        Ok(())
    }

    pub async fn stale_records(
        &self,
        scope: ScopeId,
    ) -> Result<Vec<crate::tasks::stale_records::StaleRecord>, AppError> {
        let layout = self.scope_layout(&scope)?;
        let paths = self.inner.paths.clone();
        tokio::task::spawn_blocking(move || crate::tasks::stale_records::list(&paths, &layout))
            .await
            .map_err(|e| AppError::task_internal("失效记录清单", e))?
    }

    pub async fn preview_batch(
        &self,
        scope: ScopeId,
        request: BatchRequest,
        updates: HashMap<String, UpdateSkillResult>,
    ) -> Result<BatchPreview, AppError> {
        self.preview_checked_batch(scope, request, updates, None)
            .await
    }

    pub(crate) async fn batch_update_records(
        &self,
        scope: ScopeId,
    ) -> Result<HashMap<String, crate::lock::LockEntry>, AppError> {
        let layout = self.scope_layout(&scope)?;
        tokio::task::spawn_blocking(move || {
            let lock = layout.lock()?;
            Ok(lock
                .entries()
                .iter()
                .map(|(name, entry)| (name.clone(), entry.clone()))
                .collect())
        })
        .await
        .map_err(|error| AppError::task_internal("更新记录校验", error))?
    }

    pub(crate) async fn preview_checked_batch(
        &self,
        scope: ScopeId,
        request: BatchRequest,
        updates: HashMap<String, UpdateSkillResult>,
        checked_records: Option<HashMap<String, crate::lock::LockEntry>>,
    ) -> Result<BatchPreview, AppError> {
        self.ensure_not_shutting_down()?;
        let layout = self.scope_layout(&scope)?;
        let command_layout = layout.clone();
        let paths = self.inner.paths.clone();
        let id = uuid::Uuid::new_v4().to_string();
        let mut plan = tokio::task::spawn_blocking(move || {
            let plan = batch::plan(&paths, &layout, request, id, updates)?;
            if let Some(records) = checked_records
                && plan
                    .targets
                    .iter()
                    .any(|target| records.get(&target.target.name) != target.lock_entry.as_ref())
            {
                return Err(AppError::batch_preview_expired());
            }
            Ok(plan)
        })
        .await
        .map_err(|e| AppError::task_internal("批量预览", e))??;
        self.fill_batch_commands(&command_layout, &mut plan).await?;
        let preview = plan.preview.clone();
        let mut previews = self
            .inner
            .batch_previews
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        previews.retain(|_, p| crate::time::now_ms().saturating_sub(p.created_at) < 30 * 60 * 1000);
        if previews.len() >= 32
            && let Some(oldest) = previews
                .iter()
                .min_by_key(|(_, p)| p.created_at)
                .map(|(id, _)| id.clone())
        {
            previews.remove(&oldest);
        }
        previews.insert(preview.id.clone(), plan);
        Ok(preview)
    }

    pub async fn refresh_batch_preview(&self, id: String) -> Result<BatchPreview, AppError> {
        let original = self.batch_plan(&id)?;
        let layout = self.scope_layout(&original.preview.scope)?;
        let command_layout = layout.clone();
        let paths = self.inner.paths.clone();
        let mut fresh = tokio::task::spawn_blocking(move || {
            if original.stale.is_some() {
                return crate::tasks::stale_records::replan(&paths, &layout, &original);
            }
            if original.copy.is_some() {
                return crate::tasks::copy_to_project::replan(&paths, &layout, &original);
            }
            let mut updates = original.updates;
            if original.preview.action == BatchAction::Update {
                for target in &original.targets {
                    let current = layout
                        .locate(&paths, &target.target.name)?
                        .and_then(|skill| skill.lock_entry);
                    if current != target.lock_entry {
                        updates.remove(&target.target.name);
                    }
                }
            }
            batch::plan(&paths, &layout, original.request, id, updates)
        })
        .await
        .map_err(|e| AppError::task_internal("批量预览", e))??;
        self.fill_batch_commands(&command_layout, &mut fresh)
            .await?;
        let preview = fresh.preview.clone();
        self.inner
            .batch_previews
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(preview.id.clone(), fresh);
        Ok(preview)
    }

    fn batch_plan(&self, id: &str) -> Result<BatchPlan, AppError> {
        self.inner
            .batch_previews
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(id)
            .filter(|p| crate::time::now_ms().saturating_sub(p.created_at) < 30 * 60 * 1000)
            .cloned()
            .ok_or_else(AppError::batch_preview_expired)
    }

    pub async fn start_batch(
        &self,
        scope: ScopeId,
        confirmation: BatchConfirmation,
        sink: EventSink,
    ) -> Result<TaskId, AppError> {
        let original = self.batch_plan(&confirmation.preview_id)?;
        if original.preview.scope != scope || original.preview.revision != confirmation.revision {
            return Err(AppError::batch_preview_expired());
        }
        if (matches!(
            original.preview.action,
            BatchAction::Uninstall | BatchAction::UninstallAgents
        ) && !confirmation.confirmed_delete)
            || (original
                .targets
                .iter()
                .any(|t| t.target.eligible && t.target.deletes_copy)
                && !confirmation.confirmed_copies)
        {
            return Err(AppError::batch_confirmation_needed());
        }
        let expected_revision = confirmation.revision;
        let expected_commands = original
            .preview
            .targets
            .iter()
            .filter_map(|t| t.command.clone())
            .collect();
        let planned = self
            .plan_scoped_mutation(scope, MutationKind::Batch, move |paths, layout| {
                let fresh = if original.stale.is_some() {
                    crate::tasks::stale_records::replan(paths, layout, &original)?
                } else if original.copy.is_some() {
                    crate::tasks::copy_to_project::replan(paths, layout, &original)?
                } else {
                    batch::plan(
                        paths,
                        layout,
                        original.request,
                        original.preview.id,
                        original.updates,
                    )?
                };
                if fresh.preview.revision != expected_revision {
                    return Err(AppError::batch_preview_expired());
                }
                if !fresh.targets.iter().any(|t| t.target.eligible) {
                    return Err(AppError::batch_confirmation_needed());
                }
                if let Some(copy) = &fresh.copy {
                    crate::projects::ProjectRegistry::new(paths.clone())
                        .register_preview(copy.layout.project.as_ref().expect("project"))?;
                }
                Ok(fresh)
            })
            .await?;
        let planned = planned.with_batch_commands(expected_commands).await?;
        let id = planned.start_batch(sink)?;
        self.inner
            .batch_previews
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&confirmation.preview_id);
        Ok(id)
    }

    /// Batch stop tokens are never passed into an in-flight writer. Shutdown
    /// waits for that target to finish, then later targets remain unexecuted.
    pub async fn shutdown_batch(&self) {
        self.inner
            .shutting_down
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.cancel_all_tasks();
        loop {
            let notified = self.inner.finished_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if !self.has_running_mutation() {
                break;
            }
            notified.await;
        }
        // Unlike forced standalone shutdown, the current batch writer and its
        // verification have settled. Do not report a graceful stop as a crash.
        self.inner.marker.clear();
    }
}

impl TaskManager {
    pub(super) async fn fill_batch_commands(
        &self,
        layout: &ScopeLayout,
        plan: &mut BatchPlan,
    ) -> Result<(), AppError> {
        if plan.preview.action == BatchAction::Restore {
            return self.prepare_restoration(layout, plan).await;
        }
        if !matches!(
            plan.preview.action,
            BatchAction::Update | BatchAction::Uninstall
        ) {
            return Ok(());
        }
        let needs_cli = plan.targets.iter().any(|t| {
            t.target.eligible
                && (plan.preview.action == BatchAction::Update || t.lock_entry.is_some())
        });
        let runtime = if needs_cli {
            Some(self.inner.runtime.cli_runtime().await?)
        } else {
            None
        };
        for target in &mut plan.targets {
            target.target.command = None;
            if !target.target.eligible {
                continue;
            }
            target.target.command = match plan.preview.action {
                BatchAction::Update => {
                    let update = update::plan_updates(
                        &self.inner.paths,
                        layout,
                        std::slice::from_ref(&target.target.name),
                    )?;
                    crate::cli::preview::update_preview(layout, runtime.as_ref(), &update)
                        .entries
                        .into_iter()
                        .next()
                        .and_then(|e| e.command)
                }
                BatchAction::Uninstall if target.lock_entry.is_some() => {
                    Some(crate::cli::commands::display_command(
                        &crate::cli::ScopedCli::new(
                            runtime.as_ref().expect("uninstall runtime"),
                            layout,
                        )
                        .uninstall(&target.target.name, crate::timeouts::WRITE_TIMEOUT),
                    ))
                }
                _ => None,
            };
        }
        plan.preview.targets = plan.targets.iter().map(|t| t.target.clone()).collect();
        Ok(())
    }
}

pub(super) async fn run(
    manager: TaskManager,
    layout: ScopeLayout,
    ctx: session::SessionContext,
    plan: BatchPlan,
) -> FinishOutcome {
    let targets = plan.targets.clone();
    let action = plan.preview.action;
    let log_path = manager
        .inner
        .paths
        .display(OperationLog::for_scope(&manager.inner.paths, &layout.scope).path());
    let mut execution = BatchExecution::new(
        manager.inner.paths.clone(),
        manager.inner.projects.clone(),
        layout,
        ctx.clone(),
        manager.inner.config.install_timeout,
        plan,
    );
    run_targets(
        BatchRun {
            task_id: ctx.task_id,
            sink: ctx.sink,
            cancel: ctx.cancel,
            envelope: ctx.envelope,
            action,
            targets,
            log_path,
        },
        &mut execution,
    )
    .await
}

struct BatchRun {
    task_id: String,
    sink: EventSink,
    cancel: CancelToken,
    envelope: TaskEnvelope,
    action: BatchAction,
    targets: Vec<PlannedTarget>,
    log_path: String,
}

async fn run_targets(run: BatchRun, execution: &mut impl TargetExecutor) -> FinishOutcome {
    let BatchRun {
        task_id,
        sink,
        cancel,
        envelope,
        action,
        targets,
        log_path,
    } = run;
    let total = targets.iter().filter(|t| t.target.eligible).count() as u32;
    let mut completed = 0;
    let mut stopped = false;
    let mut rows = Vec::new();
    let mut warnings = Vec::new();
    let mut skill_results = Vec::new();
    for target in &targets {
        let pending_status = if !target.target.eligible {
            Some(BatchStatus::Skipped)
        } else if stopped || cancel.is_cancelled() {
            Some(BatchStatus::NotExecuted)
        } else {
            None
        };
        if let Some(status) = pending_status {
            rows.push(BatchRow {
                target: target.target.clone(),
                status,
                error: None,
                backup_path: None,
            });
            continue;
        }
        let outcome = execution.execute(target, completed, total).await;
        completed += 1;
        let row = outcome.row;
        match outcome.completion {
            TargetCompletion::DependencyBlocked => rows.push(row),
            TargetCompletion::Attempted {
                skill_status,
                warnings: target_warnings,
                continuation,
            } => {
                stopped = continuation == Continuation::Stop;
                warnings.extend(target_warnings);
                skill_results.push(TaskSkillResult {
                    name: row.target.name.clone(),
                    status: skill_status,
                    error: row.error.clone(),
                    backup_path: row.backup_path.clone(),
                });
                rows.push(row);
                (sink)(TaskEvent::progress(
                    &task_id,
                    TaskPhase::Verifying,
                    format!("已处理 {completed} / {total} 个目标"),
                    completed,
                    total,
                ));
            }
        }
    }
    let succeeded = rows
        .iter()
        .filter(|r| r.status == BatchStatus::Success)
        .count();
    let overall = if cancel.is_cancelled() {
        TaskOverall::Cancelled
    } else if succeeded == rows.len() {
        TaskOverall::Success
    } else if succeeded > 0 {
        TaskOverall::Partial
    } else {
        TaskOverall::Failed
    };
    let backup_path = rows.iter().rev().find_map(|r| r.backup_path.clone());
    let result = TaskResult {
        action: TaskAction::Batch,
        scope: envelope.scope.clone(),
        overall,
        skills: skill_results,
        warnings,
        backup_path,
        log_path: Some(log_path),
        started_at: envelope.started_at,
        finished_at_epoch_ms: crate::time::now_ms(),
        preview: None,
        batch: Some(BatchResult { action, rows }),
    };
    FinishOutcome::Completed(
        result,
        format!("批量{}已结束，{succeeded} 个目标成功。", action.label()),
    )
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod scheduling_tests;
