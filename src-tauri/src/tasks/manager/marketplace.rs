use super::{EventSink, TaskId, TaskManager};
use crate::{
    cli::preview::{CliCommandPreview, CommandPlan},
    error::AppError,
    marketplace::{Coordinate, provider, records::Records, runtime},
    scope::ScopeId,
    tasks::{
        marketplace::{Action, Plan, Preview, StagingDirectory},
        update::UpdatePlan,
    },
};

impl TaskManager {
    pub async fn marketplace_cli(&self) -> runtime::Status {
        runtime::detect(&self.inner.paths, self.inner.executor.as_ref())
            .await
            .0
    }

    pub async fn prepare_marketplace(
        &self,
        scope: ScopeId,
        coordinate: Coordinate,
        agents: Vec<String>,
    ) -> Result<Preview, AppError> {
        let layout = self.scope_layout(&scope)?;
        let (detail, detection) = tokio::join!(
            provider::detail(&coordinate),
            runtime::detect(&self.inner.paths, self.inner.executor.as_ref())
        );
        let detail = detail?;
        if let Some(error) = &detail.install_error {
            return Err(error.clone());
        }
        let runtime = detection.1.ok_or_else(|| detection.0.error.unwrap())?;
        let paths = self.inner.paths.clone();
        let plan = tokio::task::spawn_blocking(move || {
            let (_, targets) =
                crate::tasks::install::validate_selected_agents(&paths, &layout, &agents)?;
            let stage = StagingDirectory::new()?;
            let plan = Plan {
                detail,
                runtime,
                stage,
                layout,
                targets,
                previous: None,
                previous_paths: None,
                previous_fingerprint: None,
                created_at: crate::time::now_ms(),
            };
            plan.check(&paths, &plan.layout)?;
            Ok::<_, AppError>(plan)
        })
        .await
        .map_err(|e| AppError::task_internal("市场安装预览", e))??;
        let commands = plan.command_preview(&plan.layout, None);
        let preview = Preview {
            detail: plan.detail.clone(),
            commands: commands.clone(),
        };
        let mut cache = self
            .inner
            .marketplace_previews
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        cache.retain(|_, p| crate::time::now_ms().saturating_sub(p.created_at) < 900_000);
        if cache.len() >= 32 {
            cache.clear();
        }
        cache.insert(commands.id, plan);
        Ok(preview)
    }

    pub async fn start_marketplace(
        &self,
        scope: ScopeId,
        preview_id: String,
        sink: EventSink,
    ) -> Result<TaskId, AppError> {
        let plan = self
            .inner
            .marketplace_previews
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&preview_id)
            .cloned()
            .ok_or_else(AppError::cli_command_changed)?;
        if scope != plan.layout.scope
            || crate::time::now_ms().saturating_sub(plan.created_at) > 900_000
        {
            return Err(AppError::cli_command_changed());
        }
        let detection = runtime::detect(&self.inner.paths, self.inner.executor.as_ref()).await;
        let current_runtime = detection.1.ok_or_else(|| detection.0.error.unwrap())?;
        if current_runtime.program != plan.runtime.program {
            return Err(AppError::cli_command_changed());
        }
        // Recheck exact release availability, never resolve latest after confirmation.
        if provider::release_markdown(
            &plan.detail.summary.coordinate,
            &plan.detail.summary.version,
        )
        .await?
            != plan.detail.markdown
        {
            return Err(AppError::cli_command_changed());
        }
        let name = plan.name().to_string();
        let planned = self
            .plan_scoped_mutation(
                scope,
                crate::interrupted::MutationKind::Install,
                move |paths, layout| {
                    plan.check(paths, layout)?;
                    Ok(plan)
                },
            )
            .await?;
        let planned = planned
            .with_command_preview(Some(preview_id.clone()))
            .await?;
        let result = planned.start(vec![name], sink, |layout, _| Action {
            paths: self.inner.paths.clone(),
            layout,
        });
        if result.is_ok() {
            self.inner
                .marketplace_previews
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&preview_id);
        }
        result
    }

    pub(super) async fn prepare_marketplace_updates(
        &self,
        layout: &crate::scope_layout::ScopeLayout,
        plan: &mut UpdatePlan,
    ) -> Result<(), AppError> {
        if !plan.items.iter().any(|i| i.marketplace_record.is_some()) {
            return Ok(());
        }
        let single = plan.total() == 1;
        let detection = runtime::detect(&self.inner.paths, self.inner.executor.as_ref()).await;
        let runtime = detection.1.ok_or_else(|| detection.0.error.unwrap());
        for mut item in std::mem::take(&mut plan.items) {
            let Some(record) = &item.marketplace_record else {
                plan.items.push(item);
                continue;
            };
            let prepared = async {
                let runtime = runtime.clone()?;
                let detail = provider::detail(&record.coordinate).await?;
                if detail.original_name.as_deref() != Some(&item.name) {
                    return Err(AppError::marketplace_unavailable(format!(
                        "新版本的原始技能名已变化（原名 {}），请卸载后重新安装。",
                        item.name
                    )));
                }
                if let Some(error) = &detail.install_error {
                    return Err(error.clone());
                }
                let paths = self.inner.paths.clone();
                let layout = layout.clone();
                let item = item.clone();
                let record = record.clone();
                tokio::task::spawn_blocking(move || {
                    let previous_paths = crate::tasks::uninstall::plan_uninstall(
                        &paths,
                        &layout,
                        &layout.scope.skill_id(&item.name),
                    )?;
                    let previous_fingerprint = crate::baseline::fingerprint_folder(&item.canonical)
                        .map_err(AppError::skill_fingerprint_failed)?
                        .content_hash;
                    let prepared = Plan {
                        detail,
                        runtime,
                        stage: StagingDirectory::new()?,
                        layout: layout.clone(),
                        targets: item.targets.clone(),
                        previous: Some(record.clone()),
                        previous_paths: Some(previous_paths),
                        previous_fingerprint: Some(previous_fingerprint),
                        created_at: crate::time::now_ms(),
                    };
                    prepared.check(&paths, &layout)?;
                    Ok::<_, AppError>(prepared)
                })
                .await
                .map_err(|e| AppError::task_internal("市场更新预览", e))?
            }
            .await;
            match prepared {
                Ok(prepared) => {
                    item.marketplace_plan = Some(prepared);
                    plan.items.push(item);
                }
                Err(error) if single => return Err(error),
                Err(error) => plan.skipped.push(crate::tasks::update::UpdateSkip {
                    name: item.name,
                    error,
                }),
            }
        }
        Ok(())
    }

    pub(super) async fn preview_update_commands(
        &self,
        scope: ScopeId,
        skills: Vec<String>,
    ) -> Result<CliCommandPreview, AppError> {
        let layout = self.scope_layout(&scope)?;
        let names = crate::tasks::install::validate_skill_names(&skills)?;
        let paths = self.inner.paths.clone();
        let worker_layout = layout.clone();
        let mut plan = tokio::task::spawn_blocking(move || {
            crate::tasks::update::plan_updates(&paths, &worker_layout, &names)
        })
        .await
        .map_err(|e| AppError::task_internal("更新预览", e))??;
        self.prepare_marketplace_updates(&layout, &mut plan).await?;
        let runtime = if plan.needs_cli() {
            match self.inner.runtime.cli_runtime().await {
                Ok(runtime) => Some(runtime),
                Err(error) if plan.items.iter().any(|i| i.marketplace_plan.is_some()) => {
                    let items = std::mem::take(&mut plan.items);
                    for item in items {
                        if item.marketplace_record.is_some() {
                            plan.items.push(item);
                        } else {
                            plan.skipped.push(crate::tasks::update::UpdateSkip {
                                name: item.name,
                                error: error.clone(),
                            });
                        }
                    }
                    None
                }
                Err(error) => return Err(error),
            }
        } else {
            None
        };
        let preview = plan.command_preview(&layout, runtime.as_ref());
        {
            let mut cache = self
                .inner
                .update_previews
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if cache.len() >= 32 {
                cache.clear();
            }
            cache.insert(preview.id.clone(), (scope, plan));
        }
        Ok(preview)
    }

    pub(super) fn pinned_updates(
        &self,
        preview_id: &Option<String>,
    ) -> Option<(ScopeId, UpdatePlan)> {
        preview_id.as_ref().and_then(|id| {
            self.inner
                .update_previews
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(id)
                .cloned()
        })
    }

    pub(super) async fn validate_pinned_updates(
        &self,
        pinned: &Option<(ScopeId, UpdatePlan)>,
    ) -> Result<(), AppError> {
        if let Some((_, plan)) = pinned
            .as_ref()
            .filter(|(_, p)| p.items.iter().any(|i| i.marketplace_plan.is_some()))
        {
            let detection = runtime::detect(&self.inner.paths, self.inner.executor.as_ref()).await;
            let runtime = detection.1.ok_or_else(|| detection.0.error.unwrap())?;
            for item in &plan.items {
                if let Some(plan) = &item.marketplace_plan {
                    if crate::time::now_ms().saturating_sub(plan.created_at) > 900_000
                        || runtime.program != plan.runtime.program
                    {
                        return Err(AppError::cli_command_changed());
                    }
                    if provider::release_markdown(
                        &plan.detail.summary.coordinate,
                        &plan.detail.summary.version,
                    )
                    .await?
                        != plan.detail.markdown
                    {
                        return Err(AppError::cli_command_changed());
                    }
                }
            }
        }
        Ok(())
    }
}

pub(super) fn apply_pinned_updates(
    paths: &crate::paths::AppPaths,
    layout: &crate::scope_layout::ScopeLayout,
    plan: &mut UpdatePlan,
    pinned: Option<(ScopeId, UpdatePlan)>,
) -> Result<(), AppError> {
    if let Some((scope, previous)) = &pinned {
        if scope != &layout.scope {
            return Err(AppError::cli_command_changed());
        }
        for item in std::mem::take(&mut plan.items) {
            if let Some(skip) = previous.skipped.iter().find(|s| s.name == item.name) {
                plan.skipped.push(skip.clone());
            } else {
                plan.items.push(item);
            }
        }
    }
    for item in &mut plan.items {
        if item.marketplace_record.is_none() {
            continue;
        }
        let previous = pinned
            .as_ref()
            .filter(|(scope, _)| scope == &layout.scope)
            .and_then(|(_, p)| p.items.iter().find(|i| i.name == item.name))
            .and_then(|i| i.marketplace_plan.clone())
            .ok_or_else(AppError::cli_command_changed)?;
        previous.check(paths, layout)?;
        if item.marketplace_record != previous.previous
            || Records::read(&layout.lock_path)?.skills.get(&item.name)
                != previous.previous.as_ref()
        {
            return Err(AppError::cli_command_changed());
        }
        item.marketplace_plan = Some(previous);
    }
    Ok(())
}
