use super::{provider, records::Records};
use crate::{
    error::AppError,
    inventory::UpdateState,
    update::{
        UpdateCheckReport, UpdateSkillResult, UpdateSourceResult, UpdateSourceStatus,
        UpdateUnknownReason,
    },
};
use futures::{StreamExt, stream};
use std::collections::BTreeSet;

pub async fn check(
    records: Records,
    selected: Option<&BTreeSet<String>>,
    budget: usize,
    concurrency: usize,
    deadline_ms: u64,
    slots: &tokio::sync::Semaphore,
) -> Vec<UpdateSkillResult> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(deadline_ms);
    let entries = records
        .skills
        .into_values()
        .filter(|r| selected.is_none_or(|s| s.contains(&r.name)))
        .enumerate()
        .collect::<Vec<_>>();
    stream::iter(entries)
        .map(|(index, record)| async move {
            if index >= budget {
                return row(&record, None, None, true);
            }
            let result = tokio::time::timeout_at(deadline, async {
                let _permit = slots.acquire().await.expect("attempt slots remain open");
                provider::latest(&record.coordinate).await
            })
            .await
            .unwrap_or_else(|_| Err(AppError::marketplace_unavailable("检查更新超时")));
            match result {
                Ok(version) => row(&record, Some(version), None, false),
                Err(e) => row(&record, None, Some(e), false),
            }
        })
        .buffer_unordered(concurrency.max(1))
        .collect()
        .await
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
    #[test]
    fn marketplace_checks_compare_platform_labels_and_never_substitute_package_hashes() {
        let installed = super::super::records::Installation {
            name: "original".into(),
            coordinate: super::super::Coordinate::from_canonical("@publisher/slug").unwrap(),
            version: "release-2026.11".into(),
            content_hash: "local-content-hash".into(),
            installed_at: "now".into(),
            updated_at: "now".into(),
        };
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
