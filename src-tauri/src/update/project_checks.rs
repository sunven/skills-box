//! Project Update check lifecycle: capture, compute, validate, publish.
//!
//! The short coordination lock covers registry changes and state reads/writes,
//! never the network. Callers do not assemble invalidation and submission.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde::Serialize;

use super::UpdateCheckResponse;
use super::project::ProjectUpdateService;
use super::state::{UpdateCheckState, UpdateCheckStateStore};
use crate::error::AppError;
use crate::paths::AppPaths;
use crate::projects::{ProjectEntry, ProjectRegistry};
use crate::scope::ScopeId;
use crate::scope_layout::ScopeLayout;

#[derive(Debug, Clone, Serialize)]
pub struct RelocateProjectResult {
    pub project: ProjectEntry,
    pub warnings: Vec<AppError>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForgetProjectResult {
    pub scope: ScopeId,
    pub warnings: Vec<AppError>,
}

pub struct ProjectUpdateChecks {
    paths: AppPaths,
    projects: Arc<ProjectRegistry>,
    checker: ProjectUpdateService,
    state: Arc<UpdateCheckStateStore>,
    coordination: Mutex<()>,
}

impl ProjectUpdateChecks {
    pub fn new(
        paths: AppPaths,
        projects: Arc<ProjectRegistry>,
        state: Arc<UpdateCheckStateStore>,
    ) -> Self {
        Self::with_api(
            paths,
            projects,
            state,
            super::api::GitHubApiClient::anonymous(),
        )
    }

    pub fn with_api(
        paths: AppPaths,
        projects: Arc<ProjectRegistry>,
        state: Arc<UpdateCheckStateStore>,
        api: super::api::GitHubApiClient,
    ) -> Self {
        let checker = ProjectUpdateService::with_api(paths.clone(), api);
        Self::with_checker(paths, projects, state, checker)
    }

    pub fn with_checker(
        paths: AppPaths,
        projects: Arc<ProjectRegistry>,
        state: Arc<UpdateCheckStateStore>,
        checker: ProjectUpdateService,
    ) -> Self {
        Self {
            paths,
            projects,
            checker,
            state,
            coordination: Mutex::new(()),
        }
    }

    pub async fn check(
        &self,
        scope: ScopeId,
        skill_ids: Option<Vec<String>>,
        expected_revision: Option<String>,
    ) -> Result<UpdateCheckResponse, AppError> {
        let (layout, revision) = {
            let _guard = self.lock();
            if let Some(expected) = &expected_revision {
                if !self.is_current(&scope, expected) {
                    return Ok(Self::invalidated(scope, expected.clone()));
                }
            }
            let layout = ScopeLayout::resolve(&self.paths, &self.projects, &scope, &[])?;
            let revision = layout
                .project
                .as_ref()
                .ok_or_else(|| AppError::project_not_found(&scope.to_string()))?
                .check_revision
                .clone();
            (layout, revision)
        };
        let full = skill_ids.is_none();
        let outcome = self.checker.check(&layout, skill_ids).await;
        let _guard = self.lock();
        // Validate errors as well: an old root's read failure must not become
        // a failure in the relocated Project's UI.
        if !self.is_current(&scope, &revision) {
            return Ok(Self::invalidated(scope, revision));
        }
        let mut report = outcome?;
        let state = if full {
            let mut state = UpdateCheckState::from_report(&report);
            state.check_revision = Some(revision.clone());
            if let Err(error) = self.state.store(&scope, &state) {
                report
                    .warnings
                    .push(format!("无法写入更新检查状态：{error}"));
            }
            Some(state)
        } else {
            None
        };
        Ok(UpdateCheckResponse::Applied {
            scope,
            check_revision: Some(revision),
            report,
            state,
        })
    }

    /// The mutation permit is held by the command, as required by ADR 0013.
    /// Registry save persists the new revision before best-effort cleanup.
    pub fn relocate(&self, id: &str, root: &str) -> Result<RelocateProjectResult, AppError> {
        let _guard = self.lock();
        let project = self.projects.relocate(id, root)?;
        let warnings = self.cleanup(&project.project.scope());
        Ok(RelocateProjectResult { project, warnings })
    }

    pub fn forget(&self, id: &str) -> Result<ForgetProjectResult, AppError> {
        let _guard = self.lock();
        let scope = self.projects.forget(id)?;
        let warnings = self.cleanup(&scope);
        Ok(ForgetProjectResult { scope, warnings })
    }

    pub fn read_state(&self, scope: &ScopeId) -> Option<UpdateCheckState> {
        let _guard = self.lock();
        self.state
            .load(scope)
            .filter(|state| self.valid_state(scope, state))
    }

    pub fn read_states(&self) -> BTreeMap<String, UpdateCheckState> {
        let _guard = self.lock();
        self.state
            .load_all()
            .into_iter()
            .filter(|(id, state)| {
                id.parse::<ScopeId>()
                    .is_ok_and(|scope| self.valid_state(&scope, state))
            })
            .collect()
    }

    fn valid_state(&self, scope: &ScopeId, state: &UpdateCheckState) -> bool {
        matches!(scope, ScopeId::Global)
            || state
                .check_revision
                .as_ref()
                .is_some_and(|revision| self.is_current(scope, revision))
    }

    fn is_current(&self, scope: &ScopeId, revision: &str) -> bool {
        scope
            .project_id()
            .and_then(|id| self.projects.lookup(id).ok())
            .is_some_and(|entry| entry.project.check_revision == revision)
    }

    fn cleanup(&self, scope: &ScopeId) -> Vec<AppError> {
        self.state
            .forget_scope(scope)
            .err()
            .map(|error| AppError::update_state_cleanup_failed(error.to_string()))
            .into_iter()
            .collect()
    }

    fn invalidated(scope: ScopeId, check_revision: String) -> UpdateCheckResponse {
        UpdateCheckResponse::Invalidated {
            invalidated: true,
            scope,
            check_revision,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ()> {
        self.coordination
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}
