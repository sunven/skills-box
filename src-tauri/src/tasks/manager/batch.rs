//! One registered task owns every target, including native attachment writes.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::{
    EventSink, FinishOutcome, TaskAction, TaskEnvelope, TaskEvent, TaskId, TaskManager,
    TaskOverall, TaskPhase, TaskResult, TaskSkillResult, TaskSkillStatus,
};
use crate::error::{AppError, ErrorCode};
use crate::interrupted::MutationKind;
use crate::logs::OperationLog;
use crate::process::CancelToken;
use crate::scope::ScopeId;
use crate::scope_layout::ScopeLayout;
use crate::tasks::batch::{
    self, BatchAction, BatchConfirmation, BatchPlan, BatchPreview, BatchRequest, BatchResult,
    BatchRow, BatchStatus, PlannedTarget,
};
use crate::tasks::session::{self, MutationAction};
use crate::tasks::{uninstall, update};
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
        let mut plan =
            tokio::task::spawn_blocking(move || {
                let plan = batch::plan(&paths, &layout, request, id, updates)?;
                if let Some(records) = checked_records {
                    if plan.targets.iter().any(|target| {
                        records.get(&target.target.name) != target.lock_entry.as_ref()
                    }) {
                        return Err(AppError::batch_preview_expired());
                    }
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
        if previews.len() >= 32 {
            if let Some(oldest) = previews
                .iter()
                .min_by_key(|(_, p)| p.created_at)
                .map(|(id, _)| id.clone())
            {
                previews.remove(&oldest);
            }
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
    mut plan: BatchPlan,
) -> FinishOutcome {
    let total = plan.targets.iter().filter(|t| t.target.eligible).count() as u32;
    let mut completed = 0;
    let mut stopped = false;
    let mut rows = Vec::new();
    let mut warnings = Vec::new();
    let mut skill_results = Vec::new();
    let mut copied = std::collections::HashSet::new();
    let mut copy_failed = std::collections::HashSet::new();
    for target in &plan.targets.clone() {
        if !target.target.eligible {
            rows.push(row(target, BatchStatus::Skipped, None, None));
            continue;
        }
        if stopped || ctx.cancel.is_cancelled() {
            rows.push(row(target, BatchStatus::NotExecuted, None, None));
            continue;
        }
        if plan.copy.is_some()
            && target.target.agent_id.is_some()
            && copy_failed.contains(&target.target.name)
        {
            let mut skipped = row(target, BatchStatus::NotExecuted, None, None);
            skipped.target.reason = Some("该技能复制未成功，挂载未执行。".into());
            rows.push(skipped);
            completed += 1;
            continue;
        }
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
        let paths = manager.inner.paths.clone();
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
        let checked = check_target(&manager, &layout, &plan, target).await;
        let outcome = match checked {
            Err(error) => Err(error),
            Ok(()) => match plan.preview.action {
                BatchAction::Update => {
                    let action = CheckedAction {
                        inner: update::UpdateAction::new(
                            paths.clone(),
                            layout.clone(),
                            manager.inner.config.install_timeout,
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
                            manager.inner.config.install_timeout,
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
                            &|| verify_target(&paths, &layout, &plan, &target),
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
                        verify_target(&paths, &layout, &plan, &target)?;
                        let agent = target.target.agent_id.as_deref().expect("agent target");
                        if plan.preview.action == BatchAction::Attach {
                            crate::attach::attach_agent_checked(
                                &paths,
                                &layout,
                                &target.target.skill_id,
                                agent,
                                &|| verify_target(&paths, &layout, &plan, &target),
                            )?;
                        } else {
                            crate::detach::detach_agent_checked(
                                &paths,
                                &layout,
                                &target.target.skill_id,
                                agent,
                                record.as_ref(),
                                &|| verify_target(&paths, &layout, &plan, &target),
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
                            &|| verify_target(&paths, &write_layout, &write_plan, &write_target),
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
                            manager.inner.config.install_timeout,
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
                    let published = copied.contains(&target.target.name);
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
                        crate::tasks::stale_records::observed_state(&layout, &target.target.name)
                    ));
                }
                stopped = batch::stops_batch(&error);
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
                &layout,
                &plan,
                &target.target.name,
                status == BatchStatus::Success,
                observed,
            ) {
                Ok(bytes) => plan.stale.as_mut().unwrap().expected = bytes,
                Err(e) => {
                    stopped = true;
                    status = BatchStatus::Failed;
                    error = Some(e);
                }
            }
        }
        if plan.copy.is_some() && target.target.agent_id.is_none() {
            if status == BatchStatus::Success {
                copied.insert(target.target.name.clone());
            } else {
                copy_failed.insert(target.target.name.clone());
            }
        }
        let skill_status = if status == BatchStatus::Success {
            match plan.preview.action {
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
        skill_results.push(TaskSkillResult {
            name: target.target.name.clone(),
            status: skill_status,
            error: error.clone(),
            backup_path: backup.clone(),
        });
        rows.push(row(target, status, error, backup));
        completed += 1;
        (ctx.sink)(TaskEvent::progress(
            &ctx.task_id,
            TaskPhase::Verifying,
            format!("已处理 {completed} / {total} 个目标"),
            completed,
            total,
        ));
    }
    let succeeded = rows
        .iter()
        .filter(|r| r.status == BatchStatus::Success)
        .count();
    let overall = if ctx.cancel.is_cancelled() {
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
        scope: ctx.envelope.scope.clone(),
        overall,
        skills: skill_results,
        warnings,
        backup_path,
        log_path: Some(
            manager
                .inner
                .paths
                .display(OperationLog::for_scope(&manager.inner.paths, &layout.scope).path()),
        ),
        started_at: ctx.envelope.started_at,
        finished_at_epoch_ms: crate::time::now_ms(),
        preview: None,
        batch: Some(BatchResult {
            action: plan.preview.action,
            rows,
        }),
    };
    FinishOutcome::Completed(
        result,
        format!(
            "批量{}已结束，{succeeded} 个目标成功。",
            plan.preview.action.label()
        ),
    )
}

async fn check_target(
    manager: &TaskManager,
    layout: &ScopeLayout,
    plan: &BatchPlan,
    target: &PlannedTarget,
) -> Result<(), AppError> {
    if plan.copy.is_some() {
        return Ok(());
    }
    let manager = manager.clone();
    let layout = layout.clone();
    let plan = plan.clone();
    let target = target.clone();
    tokio::task::spawn_blocking(move || {
        let current = manager.scope_layout(&layout.scope)?;
        if current.root != plan.root
            || current.project.as_ref().map(|p| &p.check_revision) != plan.project_revision.as_ref()
        {
            return Err(AppError::project_root_unavailable(
                "项目",
                "项目根目录已变化。",
            ));
        }
        verify_target(&manager.inner.paths, &current, &plan, &target)
    })
    .await
    .map_err(|e| AppError::task_internal("批量校验", e))?
}

pub(crate) fn verify_target(
    paths: &crate::paths::AppPaths,
    layout: &ScopeLayout,
    plan: &BatchPlan,
    target: &PlannedTarget,
) -> Result<(), AppError> {
    // Re-detect directories/configuration even after backup. The original
    // layout alone would miss an Agent that appeared while a writer prepared.
    let registry = crate::projects::ProjectRegistry::new(paths.clone());
    let current = ScopeLayout::resolve_detected(paths, &registry, &layout.scope)?;
    if current.root != plan.root
        || current.project.as_ref().map(|p| &p.check_revision) != plan.project_revision.as_ref()
    {
        return Err(AppError::project_root_unavailable(
            "项目",
            "项目根目录已变化。",
        ));
    }
    let layout = &current;
    layout.ensure_mutation_ready()?;
    if plan.stale.is_some() {
        return crate::tasks::stale_records::verify(paths, layout, plan, target);
    }
    if plan.preview.action == BatchAction::UninstallAgents {
        return crate::tasks::uninstall_agents::verify(paths, layout, target);
    }
    let fresh = batch::inspect(
        paths,
        layout,
        plan.preview.action,
        &target.target.skill_id,
        &target.target.name,
        target.target.agent_id.as_deref(),
        &plan.updates,
    )?;
    if !fresh.target.eligible || fresh.fingerprint != target.fingerprint {
        return Err(AppError::batch_target_changed(
            fresh
                .target
                .reason
                .unwrap_or_else(|| "内容或挂载范围与确认时不同。".into()),
        ));
    }
    Ok(())
}

fn row(
    target: &PlannedTarget,
    status: BatchStatus,
    error: Option<AppError>,
    backup_path: Option<String>,
) -> BatchRow {
    BatchRow {
        target: target.target.clone(),
        status,
        error,
        backup_path,
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
        verify_target(&self.paths, &self.layout, &self.plan, &self.target)?;
        self.inner.preflight(u, r)
    }
    fn before_write(&self, u: &Self::Unit) -> Result<(), AppError> {
        verify_target(&self.paths, &self.layout, &self.plan, &self.target)?;
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

#[cfg(test)]
mod tests;
