use super::*;
use crate::tasks::TaskSkillStatus;
use crate::tasks::batch::BatchTarget;
use crate::tasks::batch_execution::TargetOutcome;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

struct ScriptedExecution {
    outcomes: VecDeque<TargetOutcome>,
    calls: Vec<(String, u32, u32)>,
    pause: Option<(oneshot::Sender<()>, oneshot::Receiver<()>)>,
}

impl ScriptedExecution {
    fn new(outcomes: Vec<TargetOutcome>) -> Self {
        Self {
            outcomes: outcomes.into(),
            calls: Vec::new(),
            pause: None,
        }
    }
}

#[async_trait::async_trait]
impl TargetExecutor for ScriptedExecution {
    async fn execute(
        &mut self,
        target: &PlannedTarget,
        completed: u32,
        total: u32,
    ) -> TargetOutcome {
        self.calls
            .push((target.target.name.clone(), completed, total));
        if let Some((entered, release)) = self.pause.take() {
            entered.send(()).unwrap();
            release.await.unwrap();
        }
        let result = self
            .outcomes
            .pop_front()
            .expect("unexpected target execution");
        assert_eq!(result.row.target.key, target.target.key);
        result
    }
}

fn target(name: &str, eligible: bool) -> PlannedTarget {
    let mut target = BatchTarget::new(name, name, None, None);
    target.eligible = eligible;
    PlannedTarget {
        target,
        fingerprint: String::new(),
        lock_entry: None,
    }
}

fn attempted(
    target: &PlannedTarget,
    status: BatchStatus,
    continuation: Continuation,
) -> TargetOutcome {
    let (skill_status, error) = match status {
        BatchStatus::Success => (TaskSkillStatus::Removed, None),
        BatchStatus::Skipped => (
            TaskSkillStatus::Skipped,
            Some(AppError::batch_target_changed("changed")),
        ),
        BatchStatus::Failed => (
            TaskSkillStatus::Failed,
            Some(AppError::verify_failed("failure")),
        ),
        BatchStatus::NotExecuted => panic!("not an attempt"),
    };
    TargetOutcome {
        row: BatchRow {
            target: target.target.clone(),
            status,
            error,
            backup_path: None,
        },
        completion: TargetCompletion::Attempted {
            skill_status,
            warnings: Vec::new(),
            continuation,
        },
    }
}

fn run(targets: Vec<PlannedTarget>) -> (BatchRun, Arc<Mutex<Vec<TaskEvent>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let recorded = events.clone();
    (
        BatchRun {
            task_id: "batch".into(),
            sink: Arc::new(move |event| recorded.lock().unwrap().push(event)),
            cancel: CancelToken::new(),
            envelope: TaskEnvelope {
                action: TaskAction::Batch,
                scope: ScopeId::Global,
                started_at: 42,
            },
            action: BatchAction::Uninstall,
            targets,
            log_path: "batch.log".into(),
        },
        events,
    )
}

fn result(outcome: FinishOutcome) -> TaskResult {
    match outcome {
        FinishOutcome::Completed(result, _) => result,
        other => panic!("expected retained batch result: {other:?}"),
    }
}

#[tokio::test]
async fn confirmed_order_retains_failures_skips_warnings_and_last_backup() {
    let targets = vec![
        target("excluded", false),
        target("failed", true),
        target("changed", true),
        target("success", true),
    ];
    let mut failure = attempted(&targets[1], BatchStatus::Failed, Continuation::Continue);
    failure.row.backup_path = Some("first-backup".into());
    let changed = attempted(&targets[2], BatchStatus::Skipped, Continuation::Continue);
    let mut success = attempted(&targets[3], BatchStatus::Success, Continuation::Continue);
    success.row.backup_path = Some("last-backup".into());
    let warning = AppError::verify_failed("baseline warning");
    if let TargetCompletion::Attempted { warnings, .. } = &mut success.completion {
        warnings.push(warning.clone());
    }
    let mut execution = ScriptedExecution::new(vec![failure, changed, success]);
    let (run, events) = run(targets);
    let result = result(run_targets(run, &mut execution).await);
    assert_eq!(
        execution.calls,
        [
            ("failed".into(), 0, 3),
            ("changed".into(), 1, 3),
            ("success".into(), 2, 3)
        ]
    );
    let rows = &result.batch.as_ref().unwrap().rows;
    assert_eq!(
        rows.iter().map(|r| r.status).collect::<Vec<_>>(),
        [
            BatchStatus::Skipped,
            BatchStatus::Failed,
            BatchStatus::Skipped,
            BatchStatus::Success
        ]
    );
    assert!(rows[0].error.is_none());
    assert!(rows[2].error.is_some());
    assert_eq!(
        result.skills.iter().map(|r| r.status).collect::<Vec<_>>(),
        [
            TaskSkillStatus::Failed,
            TaskSkillStatus::Skipped,
            TaskSkillStatus::Removed
        ]
    );
    assert_eq!(
        result.skills[0].backup_path.as_deref(),
        Some("first-backup")
    );
    assert_eq!(result.backup_path.as_deref(), Some("last-backup"));
    assert_eq!(result.warnings, [warning]);
    assert_eq!(result.overall, TaskOverall::Partial);
    assert_eq!(result.scope, ScopeId::Global);
    assert_eq!(result.started_at, 42);
    assert_eq!(result.log_path.as_deref(), Some("batch.log"));
    let progress = events.lock().unwrap();
    assert_eq!(
        progress
            .iter()
            .map(|e| (e.phase, e.completed, e.total))
            .collect::<Vec<_>>(),
        [
            (TaskPhase::Verifying, Some(1), Some(3)),
            (TaskPhase::Verifying, Some(2), Some(3)),
            (TaskPhase::Verifying, Some(3), Some(3)),
        ]
    );
}

#[tokio::test]
async fn dependency_blocked_targets_count_without_skill_rows_or_progress() {
    let targets = vec![
        target("copy", true),
        target("attachment", true),
        target("next", true),
    ];
    let failure = attempted(&targets[0], BatchStatus::Failed, Continuation::Continue);
    let mut blocked_target = targets[1].target.clone();
    blocked_target.reason = Some("prerequisite failed".into());
    let blocked = TargetOutcome {
        row: BatchRow {
            target: blocked_target,
            status: BatchStatus::NotExecuted,
            error: None,
            backup_path: None,
        },
        completion: TargetCompletion::DependencyBlocked,
    };
    let success = attempted(&targets[2], BatchStatus::Success, Continuation::Continue);
    let mut execution = ScriptedExecution::new(vec![failure, blocked, success]);
    let (run, events) = run(targets);
    let result = result(run_targets(run, &mut execution).await);
    assert_eq!(execution.calls[2], ("next".into(), 2, 3));
    assert_eq!(
        result
            .skills
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        ["copy", "next"]
    );
    let rows = &result.batch.as_ref().unwrap().rows;
    assert_eq!(rows[1].status, BatchStatus::NotExecuted);
    assert_eq!(
        rows[1].target.reason.as_deref(),
        Some("prerequisite failed")
    );
    assert_eq!(
        events
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.completed)
            .collect::<Vec<_>>(),
        [Some(1), Some(3)]
    );
}

#[tokio::test]
async fn stop_disposition_retains_results_and_preexisting_skips() {
    let targets = vec![
        target("success", true),
        target("fatal", true),
        target("later", true),
        target("excluded", false),
    ];
    let success = attempted(&targets[0], BatchStatus::Success, Continuation::Continue);
    let fatal = attempted(&targets[1], BatchStatus::Failed, Continuation::Stop);
    let mut execution = ScriptedExecution::new(vec![success, fatal]);
    let (run, events) = run(targets);
    let result = result(run_targets(run, &mut execution).await);
    assert_eq!(execution.calls.len(), 2);
    assert_eq!(
        result
            .batch
            .unwrap()
            .rows
            .iter()
            .map(|r| r.status)
            .collect::<Vec<_>>(),
        [
            BatchStatus::Success,
            BatchStatus::Failed,
            BatchStatus::NotExecuted,
            BatchStatus::Skipped
        ]
    );
    assert_eq!(result.overall, TaskOverall::Partial);
    assert_eq!(result.skills.len(), 2);
    assert_eq!(events.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn cancellation_waits_for_current_target_and_wins_over_its_stop_disposition() {
    for continuation in [Continuation::Continue, Continuation::Stop] {
        let targets = vec![target("active", true), target("later", true)];
        let status = if continuation == Continuation::Continue {
            BatchStatus::Success
        } else {
            BatchStatus::Failed
        };
        let mut execution =
            ScriptedExecution::new(vec![attempted(&targets[0], status, continuation)]);
        let (entered, ready) = oneshot::channel();
        let (release, resume) = oneshot::channel();
        execution.pause = Some((entered, resume));
        let (run, _) = run(targets);
        let cancel = run.cancel.clone();
        let worker = tokio::spawn(async move {
            let result = result(run_targets(run, &mut execution).await);
            (result, execution.calls)
        });
        ready.await.unwrap();
        cancel.cancel();
        tokio::task::yield_now().await;
        assert!(!worker.is_finished());
        release.send(()).unwrap();
        let (result, calls) = worker.await.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(result.overall, TaskOverall::Cancelled);
        assert_eq!(
            result
                .batch
                .unwrap()
                .rows
                .iter()
                .map(|r| r.status)
                .collect::<Vec<_>>(),
            [status, BatchStatus::NotExecuted]
        );
        assert_eq!(result.skills.len(), 1);
    }
}

#[tokio::test]
async fn cancellation_before_first_target_does_not_invoke_execution() {
    let (run, events) = run(vec![target("eligible", true), target("excluded", false)]);
    run.cancel.cancel();
    let mut execution = ScriptedExecution::new(Vec::new());
    let result = result(run_targets(run, &mut execution).await);
    assert!(execution.calls.is_empty());
    assert!(result.skills.is_empty());
    assert!(events.lock().unwrap().is_empty());
    assert_eq!(result.overall, TaskOverall::Cancelled);
    assert_eq!(
        result
            .batch
            .unwrap()
            .rows
            .iter()
            .map(|r| r.status)
            .collect::<Vec<_>>(),
        [BatchStatus::NotExecuted, BatchStatus::Skipped]
    );
}
