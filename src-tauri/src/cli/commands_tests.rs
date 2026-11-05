#[cfg(test)]
use super::*;
use crate::process::fake::{ScriptedExecutor, ScriptedResponse};
use std::{collections::BTreeMap, time::Duration};

fn spec() -> CommandSpec {
    let mut spec = CommandSpec::new(
        "/Node tools/npx",
        [
            "-y",
            "skills@latest",
            "add",
            "owner/repo#release/v2",
            "",
            "$(touch nope)",
            "O'Brien",
        ]
        .map(String::from)
        .to_vec(),
        "/tmp/O'Brien skills",
        BTreeMap::new(),
        Duration::from_secs(5),
    );
    spec.cli_context = Some(CliCommandContext::task("task-1", ScopeId::Global));
    spec
}

#[test]
fn quoted_command_round_trips_through_a_shell_without_expanding_arguments() {
    let tmp = tempfile::tempdir().unwrap();
    let mut spec = spec();
    spec.cwd = tmp.path().to_path_buf();
    spec.program = "/usr/bin/printf".into();
    spec.args.insert(0, "%s\\0".into());
    let output = std::process::Command::new("/bin/sh")
        .args(["-c", &display_command(&spec)])
        .output()
        .unwrap();
    assert!(output.status.success());
    let values: Vec<_> = output
        .stdout
        .split(|b| *b == 0)
        .filter(|v| !v.is_empty())
        .map(|v| String::from_utf8_lossy(v).into_owned())
        .collect();
    assert_eq!(
        values,
        spec.args[1..]
            .iter()
            .filter(|v| !v.is_empty())
            .cloned()
            .collect::<Vec<_>>()
    );
    assert!(!tmp.path().join("nope").exists());
    let command = display_command(&super::tests::spec());
    assert!(command.starts_with("cd '/tmp/O'\\''Brien skills' && '/Node tools/npx'"));
    assert!(command.contains("'owner/repo#release/v2' '' '$(touch nope)' 'O'\\''Brien'"));
}

#[tokio::test]
async fn invocation_is_published_before_execution_and_settled_for_each_outcome() {
    for (response, status) in [
        (ScriptedResponse::ok("ok"), "succeeded"),
        (ScriptedResponse::fail(1, "failed"), "failed"),
        (ScriptedResponse::timeout(), "failed"),
        (ScriptedResponse::spawn_error("missing"), "failed"),
    ] {
        let executor = Arc::new(ScriptedExecutor::new().expect_any(response));
        let commands = Arc::new(CliCommandStore::default());
        let events = Arc::new(Mutex::new(Vec::new()));
        commands.listen(Arc::new({
            let executor = executor.clone();
            let events = events.clone();
            move |record| {
                if record.status == "running" {
                    assert_eq!(executor.call_count(), 0);
                }
                events.lock().unwrap().push(record);
            }
        }));
        let observed = ObservedExecutor {
            executor: executor.clone(),
            commands: commands.clone(),
        };
        let _ = observed.run(spec(), CancelToken::new()).await;
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].status, "running");
        assert_eq!(events[1].status, status);
        assert_eq!(events[0].id, events[1].id);
        assert_eq!(events[1].command, display_command(&executor.calls()[0]));
        assert_eq!(commands.records(), vec![events[1].clone()]);
    }
}

#[tokio::test]
async fn cancellation_before_invocation_and_native_calls_produce_no_cli_record() {
    let executor = Arc::new(ScriptedExecutor::new().expect_any(ScriptedResponse::ok("native")));
    let commands = Arc::new(CliCommandStore::default());
    let observed = ObservedExecutor {
        executor: executor.clone(),
        commands: commands.clone(),
    };
    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(
        observed.run(spec(), cancel).await.unwrap_err(),
        ExecError::Cancelled
    );
    assert_eq!(executor.call_count(), 0);
    let mut native = spec();
    native.cli_context = None;
    observed.run(native, CancelToken::new()).await.unwrap();
    assert_eq!(executor.call_count(), 1);
    assert!(commands.records().is_empty());
}

#[tokio::test]
async fn in_flight_cancellation_settles_the_record() {
    let executor = Arc::new(ScriptedExecutor::new().expect_any(ScriptedResponse::hang()));
    let commands = Arc::new(CliCommandStore::default());
    let observed = Arc::new(ObservedExecutor {
        executor,
        commands: commands.clone(),
    });
    let cancel = CancelToken::new();
    let run = tokio::spawn({
        let cancel = cancel.clone();
        async move { observed.run(spec(), cancel).await }
    });
    while commands.records().is_empty() {
        tokio::task::yield_now().await;
    }
    cancel.cancel();
    assert_eq!(run.await.unwrap().unwrap_err(), ExecError::Cancelled);
    assert_eq!(commands.records()[0].status, "cancelled");
}

#[test]
fn latest_background_invocation_survives_delayed_completion_and_task_records_accumulate() {
    let commands = CliCommandStore::default();
    let record = |id: &str, started_at, status: &str| CliCommandRecord {
        id: id.into(),
        started_at,
        status: status.into(),
        command: id.into(),
        context: CliCommandContext::background("discovery", Some("owner/repo".into())),
    };
    commands.publish(record("old", 1, "running"));
    commands.publish(record("new", 2, "running"));
    commands.publish(record("old", 1, "succeeded"));
    commands.publish(record("new", 2, "succeeded"));
    commands.publish(record("new", 2, "running"));
    assert_eq!(commands.records(), vec![record("new", 2, "succeeded")]);
    for id in ["task-a", "task-b"] {
        let mut task = record(id, 3, "succeeded");
        task.context = CliCommandContext::task("same-task", ScopeId::Global);
        commands.publish(task);
    }
    assert_eq!(commands.records().len(), 3);
}
