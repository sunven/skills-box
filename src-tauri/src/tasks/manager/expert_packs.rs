use super::{EventSink, TaskId, TaskManager};
use crate::{
    error::AppError,
    expert_packs::{self, Member, Snapshot, records::Records},
    marketplace::{provider, runtime},
    scope::ScopeId,
    tasks::{
        expert_packs::{Plan, Preview},
        marketplace::{Plan as Download, StagingDirectory},
    },
};

impl TaskManager {
    pub async fn local_packs(
        &self,
        scope: ScopeId,
    ) -> Result<Vec<expert_packs::state::LocalPack>, AppError> {
        let layout = self.scope_layout(&scope)?;
        let paths = self.inner.paths.clone();
        tokio::task::spawn_blocking(move || expert_packs::state::list(&paths, &layout))
            .await
            .map_err(|e| AppError::task_internal("读取专家包", e))?
    }
    pub async fn forget_pack(&self, scope: ScopeId, slug: String) -> Result<(), AppError> {
        expert_packs::validate_slug(&slug)?;
        self.run_scoped_native(
            scope,
            move |paths, layout| {
                layout.ensure_mutation_ready()?;
                let mut records = Records::read(&layout.lock_path)?;
                let Some(snapshot) = records.packs.get(&slug) else {
                    return Ok(());
                };
                crate::backup::BackupStore::for_layout(paths, layout).create(
                    crate::backup::BackupAction::CleanRecords,
                    &snapshot.workflow_name,
                    None,
                    &[],
                    &[],
                )?;
                layout.ensure_mutation_ready()?;
                if Records::read(&layout.lock_path)? != records {
                    return Err(AppError::batch_target_changed("专家包记录已变化，请重试。"));
                }
                records.packs.remove(&slug);
                records.save(&layout.lock_path)
            },
            |e| AppError::task_internal("忘记专家包", e),
        )
        .await
    }
    pub async fn prepare_pack(
        &self,
        scope: ScopeId,
        slug: String,
        agents: Vec<String>,
    ) -> Result<Preview, AppError> {
        use futures::{StreamExt, TryStreamExt, stream};
        expert_packs::validate_slug(&slug)?;
        let layout = self.scope_layout(&scope)?;
        layout.ensure_mutation_ready()?;
        let previous = Records::read(&layout.lock_path)?.packs.get(&slug).cloned();
        let market = crate::marketplace::records::Records::read(&layout.lock_path)?;
        let snapshot = if let Some(snapshot) = &previous {
            snapshot.clone()
        } else {
            let definition = expert_packs::provider::detail(&slug).await?;
            if let Some(e) = definition.install_error {
                return Err(e);
            }
            let members: Vec<Member> = stream::iter(definition.members)
                .map(|reference| {
                    let market = &market;
                    let layout = &layout;
                    async move {
                        let coordinate = reference.coordinate.ok_or_else(|| {
                            reference.error.unwrap_or_else(|| {
                                AppError::marketplace_unavailable("成员缺少完整来源")
                            })
                        })?;
                        let (name, version, markdown) = if let Some(installed) =
                            market.skills.values().find(|r| r.coordinate == coordinate)
                        {
                            expert_packs::state::clean_content(
                                layout,
                                &installed.name,
                                &installed.content_hash,
                            )?;
                            (
                                installed.name.clone(),
                                installed.version.clone(),
                                std::fs::read_to_string(
                                    layout.canonical_dir.join(&installed.name).join("SKILL.md"),
                                )
                                .map_err(AppError::marketplace_unavailable)?,
                            )
                        } else {
                            let detail = provider::detail(&coordinate).await?;
                            if let Some(e) = detail.install_error {
                                return Err(e);
                            }
                            (
                                detail.original_name.ok_or_else(|| {
                                    AppError::marketplace_unavailable("成员缺少原始 name")
                                })?,
                                detail.summary.version,
                                detail.markdown,
                            )
                        };
                        Ok::<_, AppError>(Member {
                            reference: reference.slug,
                            coordinate,
                            name,
                            version,
                            markdown,
                        })
                    }
                })
                .buffered(4)
                .try_collect()
                .await?;
            let (workflow_name, installed_workflow) = expert_packs::workflow::adapt(
                &slug,
                &definition.summary.display_name,
                &definition.summary.summary,
                &definition.workflow,
                &members,
            )?;
            Snapshot {
                slug: slug.clone(),
                display_name: definition.summary.display_name,
                summary: definition.summary.summary,
                original_workflow: definition.workflow,
                workflow_name,
                installed_workflow,
                members,
                agents,
                installed_at: crate::time::now_ms(),
            }
        };
        snapshot.validate()?;
        let missing: Vec<_> = snapshot
            .members
            .iter()
            .filter(|m| !market.skills.contains_key(&m.name))
            .cloned()
            .collect();
        // Supplement pins missing releases to the original snapshot, even when upstream changed.
        if previous.is_some() {
            stream::iter(missing.clone())
                .map(|m| async move {
                    if provider::release_markdown(&m.coordinate, &m.version).await? != m.markdown {
                        return Err(AppError::marketplace_unavailable(format!(
                            "{} 原版本内容已变化，无法补齐。",
                            m.name
                        )));
                    }
                    Ok::<_, AppError>(())
                })
                .buffered(4)
                .try_collect::<Vec<_>>()
                .await?;
        }
        let runtime = if missing.is_empty() {
            None
        } else {
            let (status, runtime) =
                runtime::detect(&self.inner.paths, self.inner.executor.as_ref()).await;
            Some(runtime.ok_or_else(|| status.error.unwrap())?)
        };
        let paths = self.inner.paths.clone();
        let plan = tokio::task::spawn_blocking(move || {
            let (_, targets) =
                crate::tasks::install::validate_selected_agents(&paths, &layout, &snapshot.agents)?;
            let downloads = missing
                .into_iter()
                .map(|member| {
                    Ok(Download {
                        detail: member.detail(),
                        runtime: runtime.clone().unwrap(),
                        stage: StagingDirectory::new()?,
                        layout: layout.clone(),
                        targets: targets.clone(),
                        previous: None,
                        previous_paths: None,
                        previous_fingerprint: None,
                        created_at: crate::time::now_ms(),
                    })
                })
                .collect::<Result<Vec<_>, AppError>>()?;
            Plan::build(&paths, layout, snapshot, previous, downloads)
        })
        .await
        .map_err(|e| AppError::task_internal("专家包预览", e))??;
        let preview = plan.preview();
        let mut cache = self
            .inner
            .pack_previews
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        cache.retain(|_, p| crate::time::now_ms().saturating_sub(p.created_at) < 900_000);
        if cache.len() >= 32 {
            cache.clear();
        }
        cache.insert(preview.commands.id.clone(), plan);
        Ok(preview)
    }
    pub async fn start_pack(
        &self,
        scope: ScopeId,
        preview_id: String,
        sink: EventSink,
    ) -> Result<TaskId, AppError> {
        let plan = self
            .inner
            .pack_previews
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&preview_id)
            .cloned()
            .ok_or_else(AppError::cli_command_changed)?;
        if plan.layout.scope != scope
            || crate::time::now_ms().saturating_sub(plan.created_at) > 900_000
        {
            return Err(AppError::cli_command_changed());
        }
        if let Some(first) = plan.units.iter().find_map(|u| u.download.as_ref()) {
            let (status, runtime) =
                runtime::detect(&self.inner.paths, self.inner.executor.as_ref()).await;
            if runtime.ok_or_else(|| status.error.unwrap())?.program != first.runtime.program {
                return Err(AppError::cli_command_changed());
            }
        }
        let prepared = self
            .plan_scoped_mutation(
                scope,
                crate::interrupted::MutationKind::Install,
                move |paths, layout| {
                    plan.check(paths, layout)?;
                    Ok(plan)
                },
            )
            .await?
            .with_command_preview(Some(preview_id.clone()))
            .await?;
        let result = prepared.start_pack(sink);
        if result.is_ok() {
            self.inner
                .pack_previews
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&preview_id);
        }
        result
    }
}
