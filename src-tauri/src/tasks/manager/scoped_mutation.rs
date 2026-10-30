//! Scope preparation and write-slot ownership inside TaskManager.
//!
//! A layout is acquired under the permit. Blocking workers own both while
//! running; callers only receive a plan that can start a mutation session.

use crate::error::AppError;
use crate::interrupted::MutationKind;
use crate::paths::AppPaths;
use crate::scope::ScopeId;
use crate::scope_layout::ScopeLayout;

use super::{EventSink, MutationPermit, TaskAction, TaskId, TaskManager, session};

struct ScopeMutation {
    permit: MutationPermit,
    layout: ScopeLayout,
}

impl ScopeMutation {
    fn acquire(manager: &TaskManager, scope: &ScopeId) -> Result<Self, AppError> {
        let permit = manager.try_acquire_mutation()?;
        let layout = manager.scope_layout(scope)?;
        Ok(Self { permit, layout })
    }
}

pub(super) struct PlannedMutation<P> {
    manager: TaskManager,
    kind: MutationKind,
    scope: ScopeMutation,
    plan: P,
}

impl<P> PlannedMutation<P> {
    pub(super) fn plan(&self) -> &P {
        &self.plan
    }

    /// Registration and execution consume the owned scope together. No caller
    /// can detach its layout from the permit or release it before finish.
    pub(super) fn start<A, F>(
        self,
        skills: Vec<String>,
        on_event: EventSink,
        build_action: F,
    ) -> Result<TaskId, AppError>
    where
        P: Send + Sync + Clone + 'static,
        A: session::MutationAction<Plan = P>,
        F: FnOnce(ScopeLayout, TaskId) -> A,
    {
        let manager = &self.manager;
        let action_kind = match self.kind {
            MutationKind::Install => TaskAction::Install,
            MutationKind::Update => TaskAction::Update,
            MutationKind::Uninstall => TaskAction::Uninstall,
        };
        let registered = manager.register_mutation(
            action_kind,
            self.kind,
            skills,
            self.scope.layout.scope.clone(),
            self.scope.layout.lock_path.to_string_lossy().into_owned(),
        )?;
        let action = build_action(self.scope.layout, registered.task_id.clone());
        let ctx = manager.session_context(&registered, on_event.clone());
        let manager = self.manager;
        let spawned_id = registered.task_id.clone();
        tokio::spawn(async move {
            let _permit = self.scope.permit;
            let outcome = session::run(ctx, action, self.plan).await;
            manager.finish(&spawned_id, &on_event, outcome);
        });
        Ok(registered.task_id)
    }
}

#[cfg(test)]
mod tests;

impl TaskManager {
    pub(super) async fn plan_scoped_mutation<P, F>(
        &self,
        scope: ScopeId,
        kind: MutationKind,
        plan: F,
    ) -> Result<PlannedMutation<P>, AppError>
    where
        P: Send + 'static,
        F: FnOnce(&AppPaths, &ScopeLayout) -> Result<P, AppError> + Send + 'static,
    {
        let scope = ScopeMutation::acquire(self, &scope)?;
        let paths = self.inner.paths.clone();
        let manager = self.clone();
        // Dropping the requesting future cannot abort a running blocking
        // worker. Move the permit into it, then back with the completed plan.
        tokio::task::spawn_blocking(move || {
            let plan = plan(&paths, &scope.layout)?;
            Ok(PlannedMutation {
                manager,
                kind,
                scope,
                plan,
            })
        })
        .await
        .map_err(|error| AppError::task_internal(kind.label(), error))?
    }

    pub(super) async fn run_scoped_native<R, F, E>(
        &self,
        scope: ScopeId,
        run: F,
        join_error: E,
    ) -> Result<R, AppError>
    where
        R: Send + 'static,
        F: FnOnce(&AppPaths, &ScopeLayout) -> Result<R, AppError> + Send + 'static,
        E: FnOnce(tokio::task::JoinError) -> AppError,
    {
        let scope = ScopeMutation::acquire(self, &scope)?;
        let paths = self.inner.paths.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = scope.permit;
            run(&paths, &scope.layout)
        })
        .await
        .map_err(join_error)?
    }
}
