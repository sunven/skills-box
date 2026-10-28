//! The pinned skills CLI runtime shared by mutation/preview tasks.
//!
//! The frontend never supplies an executable, argument list, environment or
//! working directory. Tasks receive this value from the environment service
//! and only choose validated semantic arguments (source, skill names, agent
//! ids) from typed inputs.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;

use crate::error::AppError;
use crate::process::CommandSpec;

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

/// `npx -y skills@1.7.4 add <source> -l -g`.
///
/// `-l` is human-only and mutually exclusive with `--json`; `-y` is not needed
/// because list mode exits before any prompt. The source must already be
/// normalized to `owner/repo`.
pub fn preview_command(runtime: &CliRuntime, source: &str, timeout: Duration) -> CommandSpec {
    CommandSpec::new(
        &runtime.npx_path,
        vec![
            "-y".to_string(),
            format!("skills@{}", runtime.pinned_version),
            "add".to_string(),
            source.to_string(),
            "-l".to_string(),
            "-g".to_string(),
        ],
        &runtime.cwd,
        runtime.env.clone(),
        timeout,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime() -> CliRuntime {
        CliRuntime {
            npx_path: PathBuf::from("/node/bin/npx"),
            node_dir: PathBuf::from("/node/bin"),
            git_dir: Some(PathBuf::from("/usr/bin")),
            cwd: PathBuf::from("/app-data/cli-cwd"),
            home: PathBuf::from("/Users/tester"),
            env: BTreeMap::from([
                ("CI".to_string(), "true".to_string()),
                ("DISABLE_TELEMETRY".to_string(), "1".to_string()),
                ("PATH".to_string(), "/node/bin:/usr/bin:/bin".to_string()),
            ]),
            pinned_version: PINNED_CLI_VERSION.to_string(),
        }
    }

    #[test]
    fn preview_command_is_pinned_and_read_only() {
        let spec = preview_command(&runtime(), "owner/repo", Duration::from_secs(60));
        assert_eq!(spec.program, PathBuf::from("/node/bin/npx"));
        assert_eq!(
            spec.args,
            vec!["-y", "skills@1.7.4", "add", "owner/repo", "-l", "-g"]
        );
        assert_eq!(spec.cwd, PathBuf::from("/app-data/cli-cwd"));
        assert_eq!(spec.env["CI"], "true");
        assert_eq!(spec.env["DISABLE_TELEMETRY"], "1");
        assert_eq!(spec.timeout, Duration::from_secs(60));
        for forbidden in [
            "--all", "--copy", "--json", "--yes", "-a", "update", "check",
        ] {
            assert!(
                !spec.args.iter().any(|arg| arg == forbidden),
                "{forbidden} must never be part of preview"
            );
        }
    }
}
