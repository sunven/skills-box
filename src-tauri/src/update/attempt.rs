//! Source budgeting, scheduling and report assembly for one Check attempt.

use futures::StreamExt;

use super::{UpdateCheckReport, UpdateSkillResult, UpdateSourceResult, UpdateSourceStatus};

pub(super) struct GroupOutcome {
    pub source: UpdateSourceResult,
    pub skills: Vec<UpdateSkillResult>,
    pub warnings: Vec<String>,
}

pub(super) enum PreparedSource<G> {
    Ready(Box<GroupOutcome>),
    Pending(G),
}

pub(super) struct PreparedCheck<G> {
    pub checked_at_epoch_ms: u64,
    pub skills: Vec<UpdateSkillResult>,
    /// Unique groups in source-identity/ref order, established by the caller.
    pub sources: Vec<PreparedSource<G>>,
}

pub(super) struct SchedulingLimits {
    pub max_sources: usize,
    pub concurrency: usize,
    pub deadline_ms: u64,
}

/// Owns the acquisition resources for this attempt. Scope-specific mappings
/// preserve cache history and fingerprint semantics behind this seam.
#[async_trait::async_trait]
pub(super) trait GroupExecutor: Send + Sync {
    type Group: Send + Sync;

    async fn check(&self, group: &Self::Group) -> GroupOutcome;
    fn skipped(&self, group: &Self::Group, max_sources: usize) -> GroupOutcome;
    fn timed_out(&self, group: &Self::Group) -> GroupOutcome;
}

/// Git and marketplace checks share these slots within one scope's attempt.
pub(super) struct SharedSlots<E> {
    pub executor: E,
    pub slots: std::sync::Arc<tokio::sync::Semaphore>,
}
#[async_trait::async_trait]
impl<E: GroupExecutor> GroupExecutor for SharedSlots<E> {
    type Group = E::Group;
    async fn check(&self, group: &Self::Group) -> GroupOutcome {
        let _permit = self
            .slots
            .acquire()
            .await
            .expect("attempt slots remain open");
        self.executor.check(group).await
    }
    fn skipped(&self, group: &Self::Group, max_sources: usize) -> GroupOutcome {
        self.executor.skipped(group, max_sources)
    }
    fn timed_out(&self, group: &Self::Group) -> GroupOutcome {
        self.executor.timed_out(group)
    }
}

pub(super) async fn run<E: GroupExecutor>(
    prepared: PreparedCheck<E::Group>,
    limits: SchedulingLimits,
    executor: E,
) -> UpdateCheckReport {
    let mut outcomes = Vec::new();
    let mut pending = Vec::new();
    let mut skipped = Vec::new();
    for source in prepared.sources {
        match source {
            PreparedSource::Ready(outcome) => outcomes.push(*outcome),
            PreparedSource::Pending(group) if pending.len() < limits.max_sources => {
                pending.push(group);
            }
            PreparedSource::Pending(group) => skipped.push(group),
        }
    }
    for group in skipped {
        outcomes.push(executor.skipped(&group, limits.max_sources));
    }

    if !pending.is_empty() {
        let mut completed = vec![false; pending.len()];
        let deadline = tokio::time::Instant::now()
            + std::time::Duration::from_millis(limits.deadline_ms.max(1));
        let checks: Vec<_> = pending
            .iter()
            .enumerate()
            .map(|(index, group)| {
                let executor = &executor;
                async move { (index, executor.check(group).await) }
            })
            .collect();
        let mut stream = futures::stream::iter(checks).buffer_unordered(limits.concurrency.max(1));

        while let Ok(Some((index, outcome))) =
            tokio::time::timeout_at(deadline, stream.next()).await
        {
            completed[index] = true;
            outcomes.push(outcome);
        }
        drop(stream);
        for (group, completed) in pending.iter().zip(completed) {
            if !completed {
                outcomes.push(executor.timed_out(group));
            }
        }
    }
    // SourceArchives itself retains shared acquisition futures. Dropping only
    // the stream would leave those alive until the caller released its owner.
    drop(executor);

    let mut skills = prepared.skills;
    let mut sources = Vec::new();
    let mut warnings = Vec::new();
    for outcome in outcomes {
        skills.extend(outcome.skills);
        sources.push(outcome.source);
        warnings.extend(outcome.warnings);
    }
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    sources.sort_by(|left, right| {
        left.source
            .to_ascii_lowercase()
            .cmp(&right.source.to_ascii_lowercase())
            .then_with(|| left.reference.cmp(&right.reference))
    });

    let skipped_source_count = sources
        .iter()
        .filter(|source| source.status == UpdateSourceStatus::Skipped)
        .count();
    if skipped_source_count > 0 {
        warnings.push(format!(
            "本次检查达到来源上限（{} 个），有 {skipped_source_count} 个来源尚未检查。",
            limits.max_sources
        ));
    }
    let cached_source_count = sources
        .iter()
        .filter(|source| source.status == UpdateSourceStatus::Cached)
        .count();
    let last_success_at_epoch_ms = sources
        .iter()
        .filter_map(|source| source.last_success_at_epoch_ms)
        .max();
    warnings.dedup();

    UpdateCheckReport {
        skills,
        sources,
        checked_at_epoch_ms: prepared.checked_at_epoch_ms,
        last_success_at_epoch_ms,
        skipped_source_count,
        cached_source_count,
        warnings,
    }
}

#[cfg(test)]
mod tests;
