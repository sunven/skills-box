use super::*;
use crate::error::AppError;
use crate::inventory::UpdateState;
use crate::update::UpdateUnknownReason;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

struct Started {
    name: &'static str,
    finish: oneshot::Sender<GroupOutcome>,
}

struct ControlledExecutor {
    started: mpsc::UnboundedSender<Started>,
    // Represents resources retained by the executor independently of its work.
    _resource: oneshot::Receiver<()>,
}

#[async_trait::async_trait]
impl GroupExecutor for ControlledExecutor {
    type Group = &'static str;

    async fn check(&self, name: &Self::Group) -> GroupOutcome {
        let (finish, result) = oneshot::channel();
        self.started.send(Started { name, finish }).unwrap();
        result.await.unwrap()
    }

    fn skipped(&self, name: &Self::Group, max_sources: usize) -> GroupOutcome {
        let mut outcome = outcome(name, UpdateSourceStatus::Skipped);
        outcome
            .warnings
            .push(format!("skipped:{name}:{max_sources}"));
        outcome
    }

    fn timed_out(&self, name: &Self::Group) -> GroupOutcome {
        let mut outcome = outcome(name, UpdateSourceStatus::Error);
        outcome.warnings.push(format!("timeout:{name}"));
        outcome
    }
}

fn executor() -> (
    ControlledExecutor,
    mpsc::UnboundedReceiver<Started>,
    oneshot::Sender<()>,
) {
    let (started, requests) = mpsc::unbounded_channel();
    let (resource, owned_resource) = oneshot::channel();
    (
        ControlledExecutor {
            started,
            _resource: owned_resource,
        },
        requests,
        resource,
    )
}

fn outcome(name: &str, status: UpdateSourceStatus) -> GroupOutcome {
    let (state, unknown_reason, timestamp, error) = match status {
        UpdateSourceStatus::Ok | UpdateSourceStatus::Cached => {
            (UpdateState::Current, None, Some(40), None)
        }
        UpdateSourceStatus::Skipped => (
            UpdateState::Unknown,
            Some(UpdateUnknownReason::NotChecked),
            None,
            None,
        ),
        UpdateSourceStatus::Error => (
            UpdateState::Error,
            None,
            None,
            Some(AppError::network_error("test failure")),
        ),
    };
    let cached = status == UpdateSourceStatus::Cached;
    GroupOutcome {
        source: UpdateSourceResult {
            source: name.into(),
            reference: None,
            status,
            skill_count: 1,
            cached,
            truncated: false,
            last_success_at_epoch_ms: timestamp,
            error: error.clone(),
            note: None,
        },
        skills: vec![UpdateSkillResult {
            name: name.into(),
            source: Some(name.into()),
            reference: None,
            lock_hash: None,
            remote_hash: None,
            state,
            unknown_reason,
            cached,
            last_success_at_epoch_ms: timestamp,
            previous_state: None,
            error,
            note: None,
        }],
        warnings: Vec::new(),
    }
}

fn prepared(sources: Vec<PreparedSource<&'static str>>) -> PreparedCheck<&'static str> {
    PreparedCheck {
        checked_at_epoch_ms: 100,
        skills: Vec::new(),
        sources,
    }
}

fn limits() -> SchedulingLimits {
    SchedulingLimits {
        max_sources: 20,
        concurrency: 2,
        deadline_ms: 10_000,
    }
}

fn complete(started: Started, status: UpdateSourceStatus) {
    let mut outcome = outcome(started.name, status);
    outcome.warnings.push(format!("completed:{}", started.name));
    assert!(started.finish.send(outcome).is_ok());
}

#[tokio::test]
async fn ready_sources_do_not_displace_pending_sources_and_report_phases_are_preserved() {
    let (executor, mut requests, resource) = executor();
    let mut first = outcome("b-cached", UpdateSourceStatus::Cached);
    first.warnings = vec!["repeat".into(), "repeat".into()];
    let mut second = outcome("d-cached", UpdateSourceStatus::Cached);
    second.warnings = vec!["other".into(), "repeat".into()];
    let mut input = prepared(vec![
        PreparedSource::Pending("a-network"),
        PreparedSource::Ready(Box::new(first)),
        PreparedSource::Pending("c-skipped"),
        PreparedSource::Ready(Box::new(second)),
    ]);
    input.skills = outcome("uncheckable", UpdateSourceStatus::Skipped).skills;
    let mut check = Box::pin(run(
        input,
        SchedulingLimits {
            max_sources: 1,
            ..limits()
        },
        executor,
    ));
    assert!(futures::poll!(&mut check).is_pending());
    let started = requests.try_recv().unwrap();
    assert_eq!(started.name, "a-network");
    assert!(requests.try_recv().is_err());
    complete(started, UpdateSourceStatus::Ok);
    let report = check.await;

    assert!(
        resource.is_closed(),
        "resources must be released before publication"
    );
    assert_eq!(report.cached_source_count, 2);
    assert_eq!(report.skipped_source_count, 1);
    assert_eq!(report.checked_at_epoch_ms, 100);
    assert_eq!(report.last_success_at_epoch_ms, Some(40));
    assert_eq!(
        report
            .skills
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        [
            "a-network",
            "b-cached",
            "c-skipped",
            "d-cached",
            "uncheckable"
        ]
    );
    assert_eq!(
        report.warnings,
        [
            "repeat",
            "other",
            "repeat",
            "skipped:c-skipped:1",
            "completed:a-network",
            "本次检查达到来源上限（1 个），有 1 个来源尚未检查。",
        ]
    );
}

#[tokio::test]
async fn bounded_work_completes_out_of_order_and_source_failures_are_isolated() {
    let (executor, mut requests, resource) = executor();
    let mut check = Box::pin(run(
        prepared(
            ["a", "b", "c"]
                .into_iter()
                .map(PreparedSource::Pending)
                .collect(),
        ),
        limits(),
        executor,
    ));
    assert!(futures::poll!(&mut check).is_pending());
    let a = requests.try_recv().unwrap();
    let b = requests.try_recv().unwrap();
    assert_eq!((a.name, b.name), ("a", "b"));
    assert!(requests.try_recv().is_err(), "only two groups may start");

    complete(b, UpdateSourceStatus::Error);
    assert!(futures::poll!(&mut check).is_pending());
    let c = requests.try_recv().unwrap();
    assert_eq!(c.name, "c");
    complete(c, UpdateSourceStatus::Ok);
    assert!(futures::poll!(&mut check).is_pending());
    complete(a, UpdateSourceStatus::Ok);
    let report = check.await;

    assert!(resource.is_closed());
    assert_eq!(
        report
            .sources
            .iter()
            .map(|s| (s.source.as_str(), s.status))
            .collect::<Vec<_>>(),
        [
            ("a", UpdateSourceStatus::Ok),
            ("b", UpdateSourceStatus::Error),
            ("c", UpdateSourceStatus::Ok),
        ]
    );
    assert_eq!(
        report.warnings,
        ["completed:b", "completed:c", "completed:a"]
    );
}

#[tokio::test]
async fn one_deadline_covers_active_and_queued_work_and_releases_resources() {
    let (executor, mut requests, resource) = executor();
    let mut check = Box::pin(run(
        prepared(
            ["a", "b", "c", "d"]
                .into_iter()
                .map(PreparedSource::Pending)
                .collect(),
        ),
        SchedulingLimits {
            max_sources: 3,
            concurrency: 1,
            deadline_ms: 400,
        },
        executor,
    ));
    assert!(futures::poll!(&mut check).is_pending());
    let a = requests.try_recv().unwrap();
    assert_eq!(a.name, "a");
    // The second group must inherit the remaining budget, not get another 400 ms.
    tokio::time::sleep(Duration::from_millis(250)).await;
    complete(a, UpdateSourceStatus::Ok);
    assert!(futures::poll!(&mut check).is_pending());
    let b = requests.try_recv().unwrap();
    assert_eq!(b.name, "b");
    let report = tokio::time::timeout(Duration::from_millis(300), check)
        .await
        .expect("completion must not reset the attempt deadline");

    assert!(b.finish.is_closed());
    assert!(resource.is_closed());
    assert!(
        requests.try_recv().is_err(),
        "queued and skipped groups must not start"
    );
    assert_eq!(
        report
            .sources
            .iter()
            .map(|s| (s.source.as_str(), s.status))
            .collect::<Vec<_>>(),
        [
            ("a", UpdateSourceStatus::Ok),
            ("b", UpdateSourceStatus::Error),
            ("c", UpdateSourceStatus::Error),
            ("d", UpdateSourceStatus::Skipped),
        ]
    );
    assert_eq!(
        report.skills.iter().map(|s| s.state).collect::<Vec<_>>(),
        [
            UpdateState::Current,
            UpdateState::Error,
            UpdateState::Error,
            UpdateState::Unknown,
        ]
    );
    assert_eq!(
        report.warnings,
        [
            "skipped:d:3",
            "completed:a",
            "timeout:b",
            "timeout:c",
            "本次检查达到来源上限（3 个），有 1 个来源尚未检查。",
        ]
    );
}

#[tokio::test]
async fn dropping_an_attempt_releases_active_work_and_its_executor() {
    let (executor, mut requests, resource) = executor();
    let mut check = Box::pin(run(
        prepared(
            ["a", "b", "c"]
                .into_iter()
                .map(PreparedSource::Pending)
                .collect(),
        ),
        limits(),
        executor,
    ));
    assert!(futures::poll!(&mut check).is_pending());
    let a = requests.try_recv().unwrap();
    let b = requests.try_recv().unwrap();
    assert!(!resource.is_closed());
    drop(check);
    assert!(a.finish.is_closed());
    assert!(b.finish.is_closed());
    assert!(resource.is_closed());
    assert!(requests.try_recv().is_err());
}

#[tokio::test]
async fn zero_concurrency_and_deadline_still_start_one_group_and_finish() {
    let (executor, mut requests, resource) = executor();
    let mut check = Box::pin(run(
        prepared(
            ["a", "b"]
                .into_iter()
                .map(PreparedSource::Pending)
                .collect(),
        ),
        SchedulingLimits {
            concurrency: 0,
            deadline_ms: 0,
            ..limits()
        },
        executor,
    ));
    assert!(futures::poll!(&mut check).is_pending());
    let a = requests.try_recv().unwrap();
    assert_eq!(a.name, "a");
    assert!(requests.try_recv().is_err());
    let report = tokio::time::timeout(Duration::from_secs(1), check)
        .await
        .unwrap();
    assert!(
        report
            .sources
            .iter()
            .all(|s| s.status == UpdateSourceStatus::Error)
    );
    assert!(a.finish.is_closed());
    assert!(resource.is_closed());
}

#[test]
fn zero_budget_preserves_ready_results_without_starting_work() {
    let (executor, mut requests, resource) = executor();
    let report = futures::executor::block_on(run(
        prepared(vec![
            PreparedSource::Pending("a"),
            PreparedSource::Ready(Box::new(outcome("b", UpdateSourceStatus::Cached))),
        ]),
        SchedulingLimits {
            max_sources: 0,
            ..limits()
        },
        executor,
    ));
    assert_eq!(report.cached_source_count, 1);
    assert_eq!(report.skipped_source_count, 1);
    assert_eq!(report.sources[0].status, UpdateSourceStatus::Skipped);
    assert_eq!(report.sources[1].status, UpdateSourceStatus::Cached);
    assert!(requests.try_recv().is_err());
    assert!(resource.is_closed());
}

#[test]
fn empty_or_uncheckable_only_attempts_need_no_runtime() {
    for with_uncheckable in [false, true] {
        let (executor, mut requests, resource) = executor();
        let mut input = prepared(Vec::new());
        if with_uncheckable {
            input.skills = outcome("unsupported", UpdateSourceStatus::Skipped).skills;
        }
        let report = futures::executor::block_on(run(input, limits(), executor));
        assert_eq!(report.skills.len(), usize::from(with_uncheckable));
        assert!(report.sources.is_empty());
        assert!(report.warnings.is_empty());
        assert_eq!(report.checked_at_epoch_ms, 100);
        assert_eq!(report.last_success_at_epoch_ms, None);
        assert_eq!(report.cached_source_count, 0);
        assert_eq!(report.skipped_source_count, 0);
        assert!(requests.try_recv().is_err());
        assert!(resource.is_closed());
    }
}

#[test]
fn report_sorts_sources_by_case_insensitive_name_then_ref_and_keeps_success_time() {
    let (executor, _, resource) = executor();
    let mut first = outcome("Z/source", UpdateSourceStatus::Cached);
    first.source.last_success_at_epoch_ms = Some(12);
    let mut main = outcome("a/source", UpdateSourceStatus::Cached);
    main.source.reference = Some("main".into());
    main.source.last_success_at_epoch_ms = Some(50);
    let mut dev = outcome("A/source", UpdateSourceStatus::Cached);
    dev.source.reference = Some("dev".into());
    let mut equal = outcome("a/SOURCE", UpdateSourceStatus::Cached);
    equal.source.reference = Some("dev".into());
    let report = futures::executor::block_on(run(
        prepared(
            vec![first, main, dev, equal]
                .into_iter()
                .map(|outcome| PreparedSource::Ready(Box::new(outcome)))
                .collect(),
        ),
        limits(),
        executor,
    ));
    assert_eq!(
        report
            .sources
            .iter()
            .map(|s| (s.source.as_str(), s.reference.as_deref()))
            .collect::<Vec<_>>(),
        [
            ("A/source", Some("dev")),
            ("a/SOURCE", Some("dev")),
            ("a/source", Some("main")),
            ("Z/source", None),
        ]
    );
    assert_eq!(
        report
            .skills
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        ["A/source", "Z/source", "a/SOURCE", "a/source"]
    );
    assert_eq!(report.last_success_at_epoch_ms, Some(50));
    assert_eq!(report.checked_at_epoch_ms, 100);
    assert_eq!(report.cached_source_count, 4);
    assert!(resource.is_closed());
}
