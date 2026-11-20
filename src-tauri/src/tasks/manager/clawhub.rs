use super::{EventSink, TaskId, TaskManager};
use crate::{
    cli::preview::CommandPlan,
    error::AppError,
    interrupted::MutationKind,
    marketplace::Coordinate,
    scope::ScopeId,
    tasks::marketplace::clawhub::{Action, Preview, stage_for_start},
};

impl TaskManager {
    pub async fn prepare_clawhub(
        &self,
        scope: ScopeId,
        coordinate: Coordinate,
        agents: Vec<String>,
    ) -> Result<Preview, AppError> {
        let layout = self.scope_layout(&scope)?;
        let plan = crate::tasks::marketplace::clawhub::prepare_plan(
            &self.inner.paths,
            layout,
            coordinate,
            agents,
        )
        .await?;
        let commands = plan.command_preview(&plan.publication.layout, None);
        let preview = Preview {
            coordinate: plan.publication.coordinate.clone(),
            name: plan.publication.name.clone(),
            version: plan.version.clone(),
            reference_label: plan.reference_label(),
            canonical_url: plan.canonical_url.clone(),
            commands: commands.clone(),
        };
        let mut cache = self
            .inner
            .clawhub_previews
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        cache.retain(|_, plan| crate::time::now_ms().saturating_sub(plan.created_at) < 900_000);
        if cache.len() >= 32 {
            cache.clear();
        }
        cache.insert(commands.id, plan);
        Ok(preview)
    }

    pub async fn start_clawhub(
        &self,
        scope: ScopeId,
        preview_id: String,
        sink: EventSink,
    ) -> Result<TaskId, AppError> {
        let plan = self
            .inner
            .clawhub_previews
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&preview_id)
            .cloned()
            .ok_or_else(AppError::cli_command_changed)?;
        if scope != plan.publication.layout.scope
            || crate::time::now_ms().saturating_sub(plan.created_at) > 900_000
        {
            return Err(AppError::cli_command_changed());
        }
        // The confirmed reference is authoritative; never resolve a different
        // archive version or commit after confirmation.
        let staged = stage_for_start(&plan).await?;
        if staged.name != plan.publication.name {
            return Err(AppError::cli_command_changed());
        }
        let mut plan = plan;
        plan.attach_staged(staged);
        let name = plan.name().to_string();
        let planned = self
            .plan_scoped_mutation(scope, MutationKind::Install, move |paths, layout| {
                plan.check(paths, layout)?;
                Ok(plan)
            })
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
                .clawhub_previews
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(&preview_id);
        }
        result
    }
}
