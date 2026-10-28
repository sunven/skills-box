//! Production executor built on `tokio::process::Command`.

use std::process::Stdio;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

use super::{CancelToken, CommandSpec, ExecError, ExecOutput, ProcessExecutor};

#[derive(Debug, Default, Clone, Copy)]
pub struct TokioExecutor;

enum WaitOutcome {
    Exited(std::process::ExitStatus),
    TimedOut,
    Cancelled,
    Io(std::io::Error),
}

#[async_trait]
impl ProcessExecutor for TokioExecutor {
    async fn run(&self, spec: CommandSpec, cancel: CancelToken) -> Result<ExecOutput, ExecError> {
        if cancel.is_cancelled() {
            return Err(ExecError::Cancelled);
        }
        let started = Instant::now();
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .current_dir(&spec.cwd)
            .env_clear()
            .envs(&spec.env)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .process_group(0);

        let mut child = command.spawn().map_err(|error| ExecError::Spawn {
            program: spec.program.display().to_string(),
            message: error.to_string(),
        })?;
        let pid = child.id();
        let mut stdout_pipe = child
            .stdout
            .take()
            .expect("stdout is piped when spawning with Stdio::piped");
        let mut stderr_pipe = child
            .stderr
            .take()
            .expect("stderr is piped when spawning with Stdio::piped");

        let stdout_task = tokio::spawn(async move {
            let mut buffer = Vec::new();
            let _ = stdout_pipe.read_to_end(&mut buffer).await;
            buffer
        });
        let stderr_task = tokio::spawn(async move {
            let mut buffer = Vec::new();
            let _ = stderr_pipe.read_to_end(&mut buffer).await;
            buffer
        });

        let outcome = tokio::select! {
            biased;
            _ = cancel.cancelled() => WaitOutcome::Cancelled,
            result = tokio::time::timeout(spec.timeout, child.wait()) => match result {
                Ok(Ok(status)) => WaitOutcome::Exited(status),
                Ok(Err(error)) => WaitOutcome::Io(error),
                Err(_) => WaitOutcome::TimedOut,
            },
        };

        let status = match outcome {
            WaitOutcome::Exited(status) => status,
            WaitOutcome::Cancelled => {
                stop_child(&mut child, pid).await;
                collect_pipes(stdout_task, stderr_task).await;
                return Err(ExecError::Cancelled);
            }
            WaitOutcome::TimedOut => {
                stop_child(&mut child, pid).await;
                collect_pipes(stdout_task, stderr_task).await;
                return Err(ExecError::Timeout(spec.timeout));
            }
            WaitOutcome::Io(error) => {
                stop_child(&mut child, pid).await;
                collect_pipes(stdout_task, stderr_task).await;
                return Err(ExecError::Io(error.to_string()));
            }
        };

        let stdout = stdout_task.await.unwrap_or_default();
        let stderr = stderr_task.await.unwrap_or_default();
        let signal = {
            use std::os::unix::process::ExitStatusExt;
            status.signal()
        };

        Ok(ExecOutput {
            code: status.code(),
            signal,
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
            duration: started.elapsed(),
        })
    }
}

/// Graceful stop: SIGTERM the whole process group, wait up to 3 seconds, then
/// SIGKILL. `process_group(0)` makes the direct child the group leader, so
/// npx → npm → skills → git descendants are all covered.
async fn stop_child(child: &mut Child, pid: Option<u32>) {
    terminate_group(pid, libc::SIGTERM);
    if tokio::time::timeout(Duration::from_secs(3), child.wait())
        .await
        .is_err()
    {
        terminate_group(pid, libc::SIGKILL);
        let _ = child.wait().await;
    }
}

async fn collect_pipes(
    stdout_task: tokio::task::JoinHandle<Vec<u8>>,
    stderr_task: tokio::task::JoinHandle<Vec<u8>>,
) {
    let _ = tokio::time::timeout(Duration::from_secs(1), stdout_task).await;
    let _ = tokio::time::timeout(Duration::from_secs(1), stderr_task).await;
}

fn terminate_group(pid: Option<u32>, signal: i32) {
    if let Some(pid) = pid {
        // Safe: killpg only signals the child's own process group (set above
        // with process_group(0)); an error means the group already exited.
        unsafe {
            libc::killpg(pid as libc::pid_t, signal);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn spec(program: &str, args: &[&str], env: &[(&str, &str)], timeout_ms: u64) -> CommandSpec {
        CommandSpec::new(
            program,
            args.iter().map(|arg| arg.to_string()).collect(),
            "/tmp",
            env.iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect::<BTreeMap<_, _>>(),
            Duration::from_millis(timeout_ms),
        )
    }

    #[tokio::test]
    async fn captures_stdout_and_exit_code() {
        let output = TokioExecutor
            .run(
                spec("/bin/echo", &["hello"], &[], 5_000),
                CancelToken::new(),
            )
            .await
            .unwrap();
        assert!(output.success());
        assert_eq!(output.stdout_trimmed(), "hello");
        assert_eq!(output.code, Some(0));
    }

    #[tokio::test]
    async fn reports_non_zero_exit_as_output_not_error() {
        let output = TokioExecutor
            .run(
                spec("/bin/sh", &["-c", "echo boom >&2; exit 7"], &[], 5_000),
                CancelToken::new(),
            )
            .await
            .unwrap();
        assert!(!output.success());
        assert_eq!(output.code, Some(7));
        assert_eq!(output.stderr_trimmed(), "boom");
    }

    #[tokio::test]
    async fn env_is_cleared_and_only_spec_env_is_visible() {
        let output = TokioExecutor
            .run(
                spec(
                    "/bin/sh",
                    &["-c", "printf '%s|%s|%s' \"$PATH\" \"$HOME\" \"$FOO\""],
                    &[("PATH", "/bin"), ("FOO", "bar")],
                    5_000,
                ),
                CancelToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(output.stdout, "/bin||bar");
    }

    #[tokio::test]
    async fn times_out_and_kills_the_process_group() {
        let started = std::time::Instant::now();
        let error = TokioExecutor
            .run(
                spec("/bin/sh", &["-c", "sleep 10"], &[], 150),
                CancelToken::new(),
            )
            .await
            .unwrap_err();
        assert_eq!(error, ExecError::Timeout(Duration::from_millis(150)));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "timeout should not wait for the child"
        );
    }

    #[tokio::test]
    async fn cancellation_kills_the_process_group_without_residue() {
        let tmp = tempfile::tempdir().unwrap();
        let pid_file = tmp.path().join("child.pid");
        let script = format!("echo $$ > '{}'; sleep 30", pid_file.display());
        let cancel = CancelToken::new();
        let task = tokio::spawn({
            let cancel = cancel.clone();
            async move {
                TokioExecutor
                    .run(
                        CommandSpec::new(
                            "/bin/sh",
                            vec!["-c".to_string(), script],
                            tmp.path(),
                            BTreeMap::new(),
                            Duration::from_secs(60),
                        ),
                        cancel,
                    )
                    .await
            }
        });

        let mut pid: Option<i32> = None;
        for _ in 0..100 {
            if let Ok(text) = std::fs::read_to_string(&pid_file)
                && let Ok(value) = text.trim().parse::<i32>()
            {
                pid = Some(value);
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let pid = pid.expect("child pid file should be written");
        cancel.cancel();

        let error = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("cancellation should finish promptly")
            .expect("task should not panic")
            .unwrap_err();
        assert_eq!(error, ExecError::Cancelled);

        // The direct child was reaped; its whole process group must be gone.
        let group_alive = unsafe { libc::killpg(pid, 0) };
        assert_eq!(
            group_alive, -1,
            "process group {pid} should no longer exist"
        );
    }

    #[tokio::test]
    async fn reports_spawn_errors() {
        let error = TokioExecutor
            .run(
                spec("/definitely/not/a/program", &[], &[], 1_000),
                CancelToken::new(),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, ExecError::Spawn { .. }));
    }
}
