use super::{MarketplaceSource, provider, records::Records};
use crate::{
    error::AppError,
    inventory::UpdateState,
    update::{
        UpdateCheckReport, UpdateSkillResult, UpdateSourceResult, UpdateSourceStatus,
        UpdateUnknownReason,
    },
};
use futures::{StreamExt, future::BoxFuture, stream};
use std::{collections::BTreeSet, sync::Arc};

type LatestCheck =
    Arc<dyn Fn(super::Coordinate) -> BoxFuture<'static, Result<String, AppError>> + Send + Sync>;

pub async fn check(
    records: Records,
    selected: Option<&BTreeSet<String>>,
    budget: usize,
    concurrency: usize,
    deadline_ms: u64,
    slots: &tokio::sync::Semaphore,
) -> Vec<UpdateSkillResult> {
    let latest: LatestCheck =
        Arc::new(|coordinate| Box::pin(async move { provider::latest(&coordinate).await }));
    check_with(
        records,
        selected,
        budget,
        concurrency,
        deadline_ms,
        slots,
        latest,
    )
    .await
}

/// Fetch uncheckable rows from their record only and never from a provider of
/// another source. Only SkillHub rows consume the shared source budget.
async fn check_with(
    records: Records,
    selected: Option<&BTreeSet<String>>,
    budget: usize,
    concurrency: usize,
    deadline_ms: u64,
    slots: &tokio::sync::Semaphore,
    latest: LatestCheck,
) -> Vec<UpdateSkillResult> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(deadline_ms);
    let mut skillhub_index = 0usize;
    let entries = records
        .skills
        .into_values()
        .filter(|r| selected.is_none_or(|s| s.contains(&r.name)))
        .map(|record| {
            let index = (record.coordinate.source == MarketplaceSource::Skillhub).then(|| {
                let index = skillhub_index;
                skillhub_index += 1;
                index
            });
            (record, index)
        })
        .collect::<Vec<_>>();
    stream::iter(entries)
        .map(|(record, index)| {
            let latest = Arc::clone(&latest);
            async move {
                let Some(index) = index else {
                    return unavailable_row(&record);
                };
                if index >= budget {
                    return row(&record, None, None, true);
                }
                let result = tokio::time::timeout_at(deadline, async {
                    let _permit = slots.acquire().await.expect("attempt slots remain open");
                    latest(record.coordinate.clone()).await
                })
                .await
                .unwrap_or_else(|_| Err(AppError::marketplace_unavailable("检查更新超时")));
                match result {
                    Ok(version) => row(&record, Some(version), None, false),
                    Err(e) => row(&record, None, Some(e), false),
                }
            }
        })
        .buffer_unordered(concurrency.max(1))
        .collect()
        .await
}

/// ClawHub installation is paused (ticket 02 gate), so an update check must
/// report an explicit unknown state instead of fabricating a candidate or
/// querying SkillHub.
fn unavailable_row(record: &super::records::Installation) -> UpdateSkillResult {
    UpdateSkillResult {
        name: record.name.clone(),
        source: Some(record.coordinate.canonical()),
        reference: None,
        lock_hash: Some(record.version.clone()),
        remote_hash: None,
        state: UpdateState::Unknown,
        unknown_reason: Some(UpdateUnknownReason::Uncheckable),
        cached: false,
        last_success_at_epoch_ms: None,
        previous_state: None,
        error: None,
        note: Some("ClawHub 安装通道暂停，暂不支持更新检查；本地管理不受影响。".into()),
    }
}
fn row(
    record: &super::records::Installation,
    latest: Option<String>,
    error: Option<AppError>,
    skipped: bool,
) -> UpdateSkillResult {
    let state = if skipped {
        UpdateState::Unknown
    } else if error.is_some() {
        UpdateState::Error
    } else if latest.as_deref() == Some(&record.version) {
        UpdateState::Current
    } else {
        UpdateState::Update
    };
    UpdateSkillResult {
        name: record.name.clone(),
        source: Some(record.coordinate.canonical()),
        reference: None,
        lock_hash: Some(record.version.clone()),
        remote_hash: latest,
        state,
        unknown_reason: skipped.then_some(UpdateUnknownReason::NotChecked),
        cached: false,
        last_success_at_epoch_ms: (!skipped && error.is_none()).then(crate::time::now_ms),
        previous_state: None,
        error,
        note: Some(
            if skipped {
                "本次检查达到来源上限。"
            } else {
                "SkillHub 平台版本；同版本内容重新发布不触发更新。"
            }
            .into(),
        ),
    }
}
pub fn merge(report: &mut UpdateCheckReport, results: Vec<UpdateSkillResult>) {
    for result in results {
        if result.unknown_reason == Some(UpdateUnknownReason::NotChecked) {
            report.skipped_source_count += 1;
        }
        if result.last_success_at_epoch_ms > report.last_success_at_epoch_ms {
            report.last_success_at_epoch_ms = result.last_success_at_epoch_ms;
        }
        report.sources.push(UpdateSourceResult {
            source: result.source.clone().unwrap_or_default(),
            reference: None,
            status: if result.unknown_reason.is_some() {
                UpdateSourceStatus::Skipped
            } else if result.error.is_some() {
                UpdateSourceStatus::Error
            } else {
                UpdateSourceStatus::Ok
            },
            skill_count: 1,
            cached: false,
            truncated: false,
            last_success_at_epoch_ms: result.last_success_at_epoch_ms,
            error: result.error.clone(),
            note: result.note.clone(),
        });
        report.skills.push(result);
    }
    report.skills.sort_by(|a, b| a.name.cmp(&b.name));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::marketplace::{Coordinate, records::Installation};
    use std::sync::Mutex;

    fn installation(
        name: &str,
        coordinate: Coordinate,
        version: &str,
        content_hash: &str,
    ) -> Installation {
        Installation {
            name: name.into(),
            coordinate,
            version: version.into(),
            content_hash: content_hash.into(),
            installed_at: "now".into(),
            updated_at: "now".into(),
            diagnostics: None,
        }
    }

    fn records() -> Records {
        let mut records = Records::default();
        let skillhub = installation(
            "alpha",
            Coordinate::from_canonical("@publisher/alpha").unwrap(),
            "1.0.0",
            "hash-alpha",
        );
        let clawhub = installation(
            "beta",
            Coordinate::from_canonical_for(MarketplaceSource::Clawhub, "@aws/agents-build")
                .unwrap(),
            "b4416dddac9b6a0cc5412136e6dbf8f07ffdb31c",
            "hash-beta",
        );
        records.skills.insert(skillhub.name.clone(), skillhub);
        records.skills.insert(clawhub.name.clone(), clawhub);
        records
    }

    #[tokio::test]
    async fn clawhub_records_never_reach_the_skillhub_provider_and_report_unknown() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let latest: LatestCheck = {
            let seen = Arc::clone(&seen);
            Arc::new(move |coordinate| {
                seen.lock().unwrap().push(coordinate);
                Box::pin(async { Ok("2.0.0".to_string()) })
            })
        };
        let rows = check_with(
            records(),
            None,
            8,
            2,
            5_000,
            &tokio::sync::Semaphore::new(2),
            latest,
        )
        .await;
        let mut seen = seen.lock().unwrap().clone();
        seen.sort_by(|a, b| a.canonical().cmp(&b.canonical()));
        assert_eq!(seen.len(), 1, "only SkillHub rows query a provider");
        assert_eq!(seen[0].source, MarketplaceSource::Skillhub);
        assert_eq!(seen[0].canonical(), "@publisher/alpha");
        let row = |name: &str| rows.iter().find(|row| row.name == name).unwrap();
        assert_eq!(row("alpha").state, UpdateState::Update);
        assert_eq!(row("alpha").remote_hash.as_deref(), Some("2.0.0"));
        let clawhub = row("beta");
        assert_eq!(clawhub.state, UpdateState::Unknown);
        assert_eq!(
            clawhub.unknown_reason,
            Some(UpdateUnknownReason::Uncheckable)
        );
        assert!(clawhub.remote_hash.is_none());
        assert_eq!(
            clawhub.lock_hash.as_deref(),
            Some("b4416dddac9b6a0cc5412136e6dbf8f07ffdb31c")
        );
        assert!(clawhub.note.as_deref().unwrap().contains("ClawHub"));
    }

    #[tokio::test]
    async fn clawhub_rows_do_not_consume_the_skillhub_source_budget() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let latest: LatestCheck = {
            let seen = Arc::clone(&seen);
            Arc::new(move |coordinate| {
                seen.lock().unwrap().push(coordinate);
                Box::pin(async { Ok("2.0.0".to_string()) })
            })
        };
        let rows = check_with(
            records(),
            None,
            0,
            2,
            5_000,
            &tokio::sync::Semaphore::new(2),
            latest,
        )
        .await;
        assert!(seen.lock().unwrap().is_empty());
        let row = |name: &str| rows.iter().find(|row| row.name == name).unwrap();
        assert_eq!(
            row("alpha").unknown_reason,
            Some(UpdateUnknownReason::NotChecked)
        );
        assert_eq!(
            row("beta").unknown_reason,
            Some(UpdateUnknownReason::Uncheckable)
        );
    }

    #[test]
    fn marketplace_checks_compare_platform_labels_and_never_substitute_package_hashes() {
        let installed = installation(
            "original",
            Coordinate::from_canonical("@publisher/slug").unwrap(),
            "release-2026.11",
            "local-content-hash",
        );
        let current = row(&installed, Some(installed.version.clone()), None, false);
        assert_eq!(current.state, UpdateState::Current);
        assert_eq!(current.lock_hash.as_deref(), Some("release-2026.11"));
        assert_eq!(
            row(&installed, Some("release-2026.12".into()), None, false).state,
            UpdateState::Update
        );
        let failed = row(
            &installed,
            None,
            Some(AppError::marketplace_unavailable("offline")),
            false,
        );
        assert_eq!(failed.state, UpdateState::Error);
        assert!(failed.remote_hash.is_none());
        assert_eq!(
            row(&installed, None, None, true).unknown_reason,
            Some(UpdateUnknownReason::NotChecked)
        );
    }
}
