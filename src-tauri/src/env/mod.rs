//! Environment detection: PATH discovery, tool probes, agent detection, lock
//! health, and the cached `EnvReport` consumed by the header.

pub mod child_env;
pub mod lock_health;
mod resolver;

use serde::Serialize;

use crate::agents::DetectedAgent;
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
    pub issues: Vec<AppError>,
    pub warnings: Vec<String>,
    pub checked_at_epoch_ms: u64,
    pub pinned_cli_version: String,
}

/// Session cache around `EnvResolver`. A refresh explicitly re-runs all probes;
/// otherwise the first report is reused for the lifetime of the app session.
pub struct EnvironmentService {
    resolver: EnvResolver,
    cache: tokio::sync::Mutex<Option<EnvReport>>,
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

    pub async fn get(&self, refresh: bool) -> Result<EnvReport, AppError> {
        let mut cache = self.cache.lock().await;
        if !refresh && let Some(report) = cache.as_ref() {
            return Ok(report.clone());
        }
        let report = self.resolver.resolve().await?;
        *cache = Some(report.clone());
        Ok(report)
    }
}
