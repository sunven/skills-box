//! The pinned skills CLI runtime shared by mutation/preview tasks.
//!
//! The frontend never supplies an executable, argument list, environment or
//! working directory. Tasks receive this value from the environment service
//! and only choose validated semantic arguments (source, skill names, agent
//! ids) from typed inputs.

use std::collections::BTreeMap;
use std::path::PathBuf;

use async_trait::async_trait;

use crate::error::AppError;

mod scoped;

pub use scoped::ScopedCli;

pub const PINNED_CLI_VERSION: &str = "1.7.4";

#[derive(Debug, Clone)]
pub struct CliRuntime {
    /// Absolute `<node_dir>/npx` path.
    pub npx_path: PathBuf,
    pub node_dir: PathBuf,
    pub git_dir: Option<PathBuf>,
    /// Empty app-data working directory handed to the CLI.
    pub cwd: PathBuf,
    pub home: PathBuf,
    /// Minimal child environment (env_clear allowlist + CI/telemetry flags).
    pub env: BTreeMap<String, String>,
    pub pinned_version: String,
}

#[async_trait]
pub trait CliRuntimeProvider: Send + Sync {
    async fn cli_runtime(&self) -> Result<CliRuntime, AppError>;
}
