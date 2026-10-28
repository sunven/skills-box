//! Process execution seam.
//!
//! All external commands (Node/npx/git probes today, the skills CLI later) go
//! through [`ProcessExecutor`]. Production uses [`tokio_executor::TokioExecutor`];
//! unit tests use [`fake::ScriptedExecutor`]. Nothing here depends on Tauri.

pub mod fake;
pub mod tokio_executor;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub timeout: Duration,
}

impl CommandSpec {
    pub fn new(
        program: impl Into<PathBuf>,
        args: Vec<String>,
        cwd: impl Into<PathBuf>,
        env: BTreeMap<String, String>,
        timeout: Duration,
    ) -> Self {
        Self {
            program: program.into(),
            args,
            cwd: cwd.into(),
            env,
            timeout,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOutput {
    /// Exit code, or `None` when terminated by a signal.
    pub code: Option<i32>,
    pub signal: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration: Duration,
}

impl ExecOutput {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }

    pub fn stdout_trimmed(&self) -> &str {
        self.stdout.trim()
    }

    pub fn stderr_trimmed(&self) -> &str {
        self.stderr.trim()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExecError {
    #[error("无法启动 {program}: {message}")]
    Spawn { program: String, message: String },
    #[error("进程 IO 错误: {0}")]
    Io(String),
    #[error("进程执行超时（{0:?}）")]
    Timeout(Duration),
}

#[async_trait::async_trait]
pub trait ProcessExecutor: Send + Sync {
    async fn run(&self, spec: CommandSpec) -> Result<ExecOutput, ExecError>;
}
