//! Environment detection: PATH discovery, tool probes, agent detection, lock
//! health, and the cached `EnvReport` consumed by the header.

pub mod child_env;
pub mod lock_health;
mod resolver;

use serde::Serialize;

use crate::agents::DetectedAgent;
use crate::cli::{CliRuntime, CliRuntimeProvider};
use crate::error::AppError;
use crate::paths::AppPaths;

pub use lock_health::{LockHealth, LockStatus};
pub use resolver::{
    EnvResolver, EnvResolverConfig, SearchEnv, capture_process_env, node_override_from_env,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PathSource {
    Override,
    Volta,
    Fnm,
    Nvm,
    Homebrew,
    System,
    LoginShell,
    #[serde(rename = "none")]
    Unresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolState {
    Ok,
    Missing,
    TooOld,
    Error,
    NotChecked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub found: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub status: ToolState,
    pub detail: Option<String>,
}

impl ToolStatus {
    pub fn missing(detail: impl Into<String>) -> Self {
        Self {
            found: false,
            path: None,
            version: None,
            status: ToolState::Missing,
            detail: Some(detail.into()),
        }
    }

    pub fn not_checked(detail: impl Into<String>) -> Self {
        Self {
            found: false,
            path: None,
            version: None,
            status: ToolState::NotChecked,
            detail: Some(detail.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliStatus {
    pub available: bool,
    pub version: Option<String>,
    pub pinned_version: String,
    pub status: ToolState,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvReport {
    pub ready: bool,
    pub mutations_enabled: bool,
    pub path_source: PathSource,
    pub path_source_detail: Option<String>,
    pub node: ToolStatus,
    pub npx: ToolStatus,
    pub git: ToolStatus,
    pub cli: CliStatus,
    pub agents: Vec<DetectedAgent>,
    pub lock: LockHealth,
    /// Set when a previous mutation marker survived (crash or confirmed exit
    /// while a write was in flight). The UI surfaces it and offers dismissal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interrupted: Option<crate::interrupted::InterruptedReport>,
    pub issues: Vec<AppError>,
    pub warnings: Vec<String>,
    pub checked_at_epoch_ms: u64,
    pub pinned_cli_version: String,
}

/// Environment report plus the absolute CLI runtime captured during the same
/// probe. The runtime is intentionally not serialized: the frontend never sees
/// executables, argv, env or cwd.
#[derive(Debug, Clone)]
pub struct ResolvedEnvironment {
    pub report: EnvReport,
    pub cli_runtime: Option<CliRuntime>,
}

/// Session cache around `EnvResolver`. A refresh explicitly re-runs all probes;
/// otherwise the first resolution is reused for the lifetime of the app session.
pub struct EnvironmentService {
    resolver: EnvResolver,
    cache: tokio::sync::Mutex<Option<ResolvedEnvironment>>,
}

impl EnvironmentService {
    pub fn new(resolver: EnvResolver) -> Self {
        Self {
            resolver,
            cache: tokio::sync::Mutex::new(None),
        }
    }

    pub fn from_process(paths: AppPaths) -> Self {
        Self::new(EnvResolver::from_process(paths))
    }

    pub async fn resolved(&self, refresh: bool) -> Result<ResolvedEnvironment, AppError> {
        let mut cache = self.cache.lock().await;
        if !refresh && let Some(resolved) = cache.as_ref() {
            return Ok(resolved.clone());
        }
        let resolved = self.resolver.resolve_detailed().await?;
        *cache = Some(resolved.clone());
        Ok(resolved)
    }

    pub async fn get(&self, refresh: bool) -> Result<EnvReport, AppError> {
        self.resolved(refresh).await.map(|resolved| resolved.report)
    }

    pub async fn cli_runtime(&self) -> Result<CliRuntime, AppError> {
        let resolved = self.resolved(false).await?;
        if let Some(runtime) = resolved.cli_runtime {
            return Ok(runtime);
        }
        // Preview/install can tolerate a broken lock (it is read-only for the
        // lock snapshot), but not a missing toolchain.
        if let Some(issue) = resolved
            .report
            .issues
            .iter()
            .find(|issue| issue.code.starts_with("env."))
        {
            return Err(issue.clone());
        }
        Err(AppError::env_cli_missing(
            resolved
                .report
                .cli
                .detail
                .clone()
                .unwrap_or_else(|| "skills CLI 未就绪".to_string()),
        ))
    }
}

#[async_trait::async_trait]
impl CliRuntimeProvider for EnvironmentService {
    async fn cli_runtime(&self) -> Result<CliRuntime, AppError> {
        EnvironmentService::cli_runtime(self).await
    }
}
