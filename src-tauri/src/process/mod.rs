//! Process execution seam.
//!
//! All external commands (Node/npx/git probes today, the skills CLI later) go
//! through [`ProcessExecutor`]. Production uses [`tokio_executor::TokioExecutor`];
//! unit tests use [`fake::ScriptedExecutor`]. Nothing here depends on Tauri.

pub mod fake;
pub mod tokio_executor;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::sync::Notify;

/// Cooperative cancellation shared between the task manager and the executor.
///
/// The token is race-free: cancelling before or while [`CancelToken::cancelled`]
/// is awaited always wakes the waiter.
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    inner: Arc<CancelInner>,
}

#[derive(Debug, Default)]
struct CancelInner {
    cancelled: AtomicBool,
    notify: Notify,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark the token as cancelled. Repeated calls are harmless.
    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::SeqCst);
        self.inner.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        let notified = self.inner.notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }
}

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
    #[error("进程已取消")]
    Cancelled,
}

#[async_trait::async_trait]
pub trait ProcessExecutor: Send + Sync {
    async fn run(&self, spec: CommandSpec, cancel: CancelToken) -> Result<ExecOutput, ExecError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancel_token_wakes_waiters_and_is_idempotent() {
        let token = CancelToken::new();
        assert!(!token.is_cancelled());
        let waiter = tokio::spawn({
            let token = token.clone();
            async move {
                token.cancelled().await;
            }
        });
        token.cancel();
        token.cancel();
        tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .expect("waiter should wake")
            .expect("waiter should not panic");
        assert!(token.is_cancelled());
    }

    #[tokio::test]
    async fn already_cancelled_token_returns_immediately() {
        let token = CancelToken::new();
        token.cancel();
        tokio::time::timeout(Duration::from_millis(50), token.cancelled())
            .await
            .expect("cancelled token must not wait");
    }
}
