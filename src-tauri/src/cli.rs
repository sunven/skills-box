//! The skills CLI runtime shared by mutation/preview tasks.
//!
//! The frontend never supplies an executable, argument list, environment or
//! working directory. Tasks receive this value from the environment service
//! and only choose validated semantic arguments (source, skill names, agent
//! ids) from typed inputs.

use std::collections::BTreeMap;
use std::path::PathBuf;

use async_trait::async_trait;

use crate::error::AppError;

pub mod commands;
pub mod preview;
mod scoped;

pub use scoped::ScopedCli;

/// npm package selector shared by environment probes, execution and previews.
pub const CLI_VERSION: &str = "latest";

/// Directory written by `skills add`, distinct from the original lock key and metadata
/// name passed to `-s`. Discovery and install collision checks must agree.
pub(crate) fn install_directory_name(name: &str) -> String {
    let mut value = String::new();
    let mut replacing = false;
    for c in name.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_') {
            value.push(c);
            replacing = false;
        } else if !replacing {
            value.push('-');
            replacing = true;
        }
    }
    let name = value.trim_matches(['.', '-']);
    if name.is_empty() {
        "unnamed-skill".into()
    } else {
        name.chars().take(255).collect()
    }
}

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
    /// Requested npm version or tag, rather than the resolved CLI version.
    pub pinned_version: String,
}

#[async_trait]
pub trait CliRuntimeProvider: Send + Sync {
    async fn cli_runtime(&self) -> Result<CliRuntime, AppError>;
}
