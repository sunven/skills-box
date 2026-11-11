//! Production executor built on `tokio::process::Command`.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

use super::{CancelToken, CommandSpec, ExecError, ExecOutput, ProcessExecutor};

/// Debug-only fault injection hook. Release builds ignore
/// `SKILLS_BOX_FAKE_CLI` because [`apply_fake_cli`] receives
/// `cfg!(debug_assertions)` and returns the spec untouched when it is false.
///
/// Accepted values: `slow` (sleeps 30s), `slow:<seconds>`, `fail` (exit 1),
/// `invalid-json` (non-JSON stdout) and `cancel` (hangs until cancelled).
/// Only commands that actually invoke the skills CLI are rewritten, so
/// the fake never affects environment probes (`node --version`, `git`, ...).
pub fn apply_fake_cli(spec: CommandSpec, raw: Option<&str>, debug: bool) -> CommandSpec {
    let Some((program, args)) = fake_cli_override(&spec, raw, debug) else {
        return spec;
    };
    CommandSpec {
        program,
        args,
        ..spec
    }
}

fn fake_cli_override(
    spec: &CommandSpec,
    raw: Option<&str>,
    debug: bool,
) -> Option<(PathBuf, Vec<String>)> {
    if !debug {
        return None;
    }
    let mode = raw?.trim();
    if mode.is_empty() || !invokes_skills_cli(spec) {
        return None;
    }
    let script = match mode {
        "slow" => "sleep 30; printf 'fake skills CLI finished slowly\\n'; exit 0".to_string(),
        "fail" => "echo 'fake skills CLI injected failure' >&2; exit 1".to_string(),
        "invalid-json" => "printf 'fake skills output that is not json\\n'; exit 0".to_string(),
        "cancel" => "while true; do sleep 1; done".to_string(),
        other => {
            let seconds = other.strip_prefix("slow:")?;
            let seconds: u64 = seconds.parse().ok()?;
            format!("sleep {seconds}; printf 'fake skills CLI finished slowly\\n'; exit 0")
        }
    };
    Some((PathBuf::from("/bin/sh"), vec!["-c".to_string(), script]))
}

fn invokes_skills_cli(spec: &CommandSpec) -> bool {
    let program = spec
        .program
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    program == "npx"
        || program == "skills"
        || spec
            .args
            .iter()
            .any(|arg| arg.starts_with("skills@") || arg == "skills")
}

fn inject_debug_fake_cli(spec: CommandSpec) -> CommandSpec {
    let raw = std::env::var("SKILLS_BOX_FAKE_CLI").ok();
    apply_fake_cli(spec, raw.as_deref(), cfg!(debug_assertions))
}

#[derive(Debug, Default, Clone, Copy)]
pub struct TokioExecutor;

enum WaitOutcome {
    Exited(std::process::ExitStatus, Vec<u8>, Vec<u8>),
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
        let spec = inject_debug_fake_cli(spec);
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

        // The deadline covers the leader and both pipes. Descendants can keep
        // a pipe open after the leader exits; cancellation must still kill
        // their process group, and dropping this future stops the reads.
        let outcome = tokio::select! {
            biased;
            _ = cancel.cancelled() => WaitOutcome::Cancelled,
            result = tokio::time::timeout(spec.timeout, async {
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                let (status, _, _) = tokio::try_join!(
                    child.wait(),
                    stdout_pipe.read_to_end(&mut stdout),
                    stderr_pipe.read_to_end(&mut stderr),
                )?;
                Ok::<_, std::io::Error>((status, stdout, stderr))
            }) => match result {
                Ok(Ok((status, stdout, stderr))) => WaitOutcome::Exited(status, stdout, stderr),
                Ok(Err(error)) => WaitOutcome::Io(error),
                Err(_) => WaitOutcome::TimedOut,
            },
        };

        let (status, stdout, stderr) = match outcome {
            WaitOutcome::Exited(status, stdout, stderr) => (status, stdout, stderr),
            WaitOutcome::Cancelled => {
                stop_child(&mut child, pid).await;
                return Err(ExecError::Cancelled);
            }
            WaitOutcome::TimedOut => {
                stop_child(&mut child, pid).await;
                return Err(ExecError::Timeout(spec.timeout));
            }
            WaitOutcome::Io(error) => {
                stop_child(&mut child, pid).await;
                return Err(ExecError::Io(error.to_string()));
            }
        };

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

/// Graceful stop: SIGTERM the whole process group, wait up to 3 seconds for
/// the whole group to drain, then SIGKILL. `process_group(0)` makes the direct
/// child the group leader, so npx → npm → skills → git descendants are all
/// covered. The leader may exit on SIGTERM while a descendant ignores it, so
/// the grace period probes the group itself instead of only `child.wait()`.
async fn stop_child(child: &mut Child, pid: Option<u32>) {
    terminate_group(pid, libc::SIGTERM);
    let Some(pid) = pid else {
        let _ = child.wait().await;
        return;
    };

    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        // Reap the leader first: an unreaped zombie leader keeps the group
        // visible to `killpg` and would otherwise force a needless SIGKILL.
        let _ = child.try_wait();
        if !group_alive(pid) || tokio::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    if group_alive(pid) {
        terminate_group(Some(pid), libc::SIGKILL);
        // SIGKILL is immediate, but give the kernel a moment to tear down the
        // group before reporting the task as cancelled.
        for _ in 0..40 {
            if !group_alive(pid) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
    let _ = child.wait().await;
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

/// True while at least one process in the group still exists.
fn group_alive(pid: u32) -> bool {
    // Safe: signal 0 only performs existence/permission checks.
    unsafe { libc::killpg(pid as libc::pid_t, 0) == 0 }
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
    async fn regression_timeout_covers_output_after_the_leader_exits() {
        let error = TokioExecutor
            .run(
                spec("/bin/sh", &["-c", "/bin/sleep 2 & exit 0"], &[], 100),
                CancelToken::new(),
            )
            .await
            .unwrap_err();
        assert_eq!(error, ExecError::Timeout(Duration::from_millis(100)));
    }

    #[tokio::test]
    async fn regression_cancellation_covers_output_after_the_leader_exits() {
        let tmp = tempfile::tempdir().unwrap();
        let pid_file = tmp.path().join("leader.pid");
        let script = format!("/bin/sleep 2 & echo $$ > '{}'; exit 0", pid_file.display());
        let cancel = CancelToken::new();
        let task = tokio::spawn({
            let cancel = cancel.clone();
            async move {
                TokioExecutor
                    .run(spec("/bin/sh", &["-c", &script], &[], 10_000), cancel)
                    .await
            }
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Ok(raw) = std::fs::read_to_string(&pid_file)
                    && let Ok(pid) = raw.trim().parse::<i32>()
                    && unsafe { libc::kill(pid, 0) } == -1
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("leader should have exited and been reaped");
        cancel.cancel();
        assert_eq!(task.await.unwrap().unwrap_err(), ExecError::Cancelled);
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
    async fn cancellation_escalates_to_sigkill_when_descendants_ignore_sigterm() {
        let tmp = tempfile::tempdir().unwrap();
        let pid_file = tmp.path().join("child.pid");
        // The leader exits on SIGTERM, but the inner shell ignores it and keeps
        // the process group alive; cancellation must escalate to SIGKILL.
        let script = format!(
            "echo $$ > '{}'; sh -c 'trap \"\" TERM; while true; do sleep 1; done' & wait",
            pid_file.display()
        );
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

        let error = tokio::time::timeout(Duration::from_secs(8), task)
            .await
            .expect("cancellation should escalate and finish")
            .expect("task should not panic")
            .unwrap_err();
        assert_eq!(error, ExecError::Cancelled);
        assert!(
            !group_alive(pid as u32),
            "process group {pid} should be gone after SIGKILL escalation"
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

    fn npx_spec(timeout_ms: u64) -> CommandSpec {
        spec(
            "/fake/bin/npx",
            &["-y", "skills@latest", "add", "owner/repo", "-l", "-g"],
            &[],
            timeout_ms,
        )
    }

    struct FakeCliEnv(Option<std::ffi::OsString>);

    /// Process-global environment variables cannot be scoped per test; hold
    /// this lock whenever a test sets `SKILLS_BOX_FAKE_CLI`.
    async fn fake_env_lock() -> tokio::sync::MutexGuard<'static, ()> {
        static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        LOCK.lock().await
    }

    impl FakeCliEnv {
        fn set(value: &str) -> Self {
            let previous = std::env::var_os("SKILLS_BOX_FAKE_CLI");
            // Safety: tests that invoke the real executor with a fake CLI are
            // the only writers of this variable in the test process.
            unsafe { std::env::set_var("SKILLS_BOX_FAKE_CLI", value) };
            Self(previous)
        }
    }

    impl Drop for FakeCliEnv {
        fn drop(&mut self) {
            // Safety: see `FakeCliEnv::set`.
            unsafe {
                match self.0.take() {
                    Some(value) => std::env::set_var("SKILLS_BOX_FAKE_CLI", value),
                    None => std::env::remove_var("SKILLS_BOX_FAKE_CLI"),
                }
            }
        }
    }

    #[test]
    fn fake_cli_only_rewrites_the_skills_cli_and_only_in_debug() {
        let rewritten = apply_fake_cli(npx_spec(5_000), Some("fail"), true);
        assert_eq!(rewritten.program, PathBuf::from("/bin/sh"));
        assert_eq!(rewritten.args[0], "-c");
        assert!(rewritten.args[1].contains("exit 1"));
        assert_eq!(rewritten.timeout, Duration::from_secs(5));

        // Release builds and non-CLI probes are never rewritten.
        let original = npx_spec(5_000);
        assert_eq!(
            apply_fake_cli(original.clone(), Some("fail"), false),
            original
        );
        let probe = spec("/fake/bin/node", &["--version"], &[], 5_000);
        assert_eq!(apply_fake_cli(probe.clone(), Some("fail"), true), probe);

        for (value, needle) in [
            ("slow", "sleep 30"),
            ("slow:2", "sleep 2"),
            ("cancel", "while true"),
            ("invalid-json", "not json"),
        ] {
            let rewritten = apply_fake_cli(npx_spec(5_000), Some(value), true);
            assert!(
                rewritten
                    .args
                    .get(1)
                    .is_some_and(|arg| arg.contains(needle)),
                "{value} should inject {needle}"
            );
        }
        let untouched = npx_spec(5_000);
        assert_eq!(
            apply_fake_cli(untouched.clone(), Some("nonsense"), true),
            untouched
        );
    }

    #[tokio::test]
    async fn debug_fake_cli_fail_runs_through_the_real_executor() {
        let _lock = fake_env_lock().await;
        let _guard = FakeCliEnv::set("fail");
        let output = TokioExecutor
            .run(npx_spec(5_000), CancelToken::new())
            .await
            .unwrap();
        assert_eq!(output.code, Some(1));
        assert!(output.stderr_trimmed().contains("injected failure"));
    }

    #[tokio::test]
    async fn debug_fake_cli_slow_times_out_through_the_real_executor() {
        let _lock = fake_env_lock().await;
        let _guard = FakeCliEnv::set("slow:5");
        let started = std::time::Instant::now();
        let error = TokioExecutor
            .run(npx_spec(150), CancelToken::new())
            .await
            .unwrap_err();
        assert_eq!(error, ExecError::Timeout(Duration::from_millis(150)));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the injected slow command must be killed by the executor timeout"
        );
    }

    #[tokio::test]
    async fn debug_fake_cli_invalid_json_runs_through_the_real_executor() {
        let _lock = fake_env_lock().await;
        let _guard = FakeCliEnv::set("invalid-json");
        let output = TokioExecutor
            .run(npx_spec(5_000), CancelToken::new())
            .await
            .unwrap();
        assert!(output.success());
        assert!(output.stdout_trimmed().contains("not json"));
    }

    #[tokio::test]
    async fn debug_fake_cli_cancel_hangs_until_the_process_group_is_killed() {
        let _lock = fake_env_lock().await;
        let _guard = FakeCliEnv::set("cancel");
        let cancel = CancelToken::new();
        let task_cancel = cancel.clone();
        let task =
            tokio::spawn(async move { TokioExecutor.run(npx_spec(60_000), task_cancel).await });
        tokio::time::sleep(Duration::from_millis(120)).await;
        cancel.cancel();
        let error = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("fake cancel should terminate promptly")
            .expect("task should not panic")
            .unwrap_err();
        assert_eq!(error, ExecError::Cancelled);
    }
}
