use super::TaskManager;
use crate::cli::preview::{self, CliCommandPreview, CliCommandRequest, CommandPlan};
use crate::error::AppError;
use crate::scope::ScopeId;
use crate::tasks::{install, uninstall, update};

impl TaskManager {
    pub async fn preview_cli_commands(
        &self,
        scope: ScopeId,
        request: CliCommandRequest,
    ) -> Result<CliCommandPreview, AppError> {
        if let CliCommandRequest::Update { skills } = &request {
            return self.preview_update_commands(scope, skills.clone()).await;
        }
        let layout = self.scope_layout(&scope)?;
        if let CliCommandRequest::Preview { source } = &request {
            let source = crate::tasks::validate_source(source)?;
            let runtime = self.inner.runtime.cli_runtime().await?;
            return Ok(preview::source_preview(
                &self.inner.paths,
                &layout,
                &runtime,
                &source.normalized,
            ));
        }
        let paths = self.inner.paths.clone();
        let plan = tokio::task::spawn_blocking(
            move || -> Result<(_, Box<dyn CommandPlan + Send>), AppError> {
                let plan: Box<dyn CommandPlan + Send> = match request {
                    CliCommandRequest::Install { request } => {
                        let plan = install::validate_request(&paths, &layout, &request)?;
                        if let Some(conflict) = install::conflicts_for(
                            &paths,
                            &layout,
                            &plan.skills,
                            plan.retry.then_some(plan.source.as_str()),
                        )? {
                            return Err(conflict);
                        }
                        Box::new(plan)
                    }
                    CliCommandRequest::Update { skills } => Box::new(update::plan_updates(
                        &paths,
                        &layout,
                        &install::validate_skill_names(&skills)?,
                    )?),
                    CliCommandRequest::Uninstall { skill } => {
                        Box::new(uninstall::plan_uninstall(&paths, &layout, &skill)?)
                    }
                    CliCommandRequest::Preview { .. } => unreachable!(),
                };
                Ok((layout, plan))
            },
        )
        .await
        .map_err(|e| AppError::task_internal("命令预览", e))??;
        let (layout, plan) = plan;
        let runtime = if plan.needs_cli() {
            Some(self.inner.runtime.cli_runtime().await?)
        } else {
            None
        };
        Ok(plan.command_preview(&layout, runtime.as_ref()))
    }
}
