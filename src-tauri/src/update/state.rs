//! Persisted summary of the last full update check.
//!
//! The file lives in app data (not `cache/`) because the header shows it across
//! restarts. Only full checks write it; scoped per-skill checks never advance
//! the global last-check time. Writes are atomic and best-effort: a failure is
//! surfaced as a report warning, never as a failed check.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{UpdateCheckReport, UpdateSourceStatus};
use crate::paths::AppPaths;

pub const UPDATE_CHECK_STATE_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateCheckOutcome {
    Ok,
    Partial,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckState {
    pub version: u32,
    pub last_attempt_at: u64,
    pub last_success_at: Option<u64>,
    pub last_outcome: UpdateCheckOutcome,
    pub failed_source_count: usize,
    pub cached_source_count: usize,
    pub skipped_source_count: usize,
}

impl UpdateCheckState {
    pub fn from_report(report: &UpdateCheckReport) -> Self {
        let failed_source_count = report
            .sources
            .iter()
            .filter(|source| source.status == UpdateSourceStatus::Error)
            .count();
        let successful_source_count = report
            .sources
            .iter()
            .filter(|source| {
                matches!(
                    source.status,
                    UpdateSourceStatus::Ok | UpdateSourceStatus::Cached
                )
            })
            .count();
        let last_outcome = if failed_source_count > 0 && successful_source_count == 0 {
            UpdateCheckOutcome::Failed
        } else if failed_source_count > 0 || report.skipped_source_count > 0 {
            UpdateCheckOutcome::Partial
        } else {
            UpdateCheckOutcome::Ok
        };
        Self {
            version: UPDATE_CHECK_STATE_VERSION,
            last_attempt_at: report.checked_at_epoch_ms,
            last_success_at: report.last_success_at_epoch_ms,
            last_outcome,
            failed_source_count,
            cached_source_count: report.cached_source_count,
            skipped_source_count: report.skipped_source_count,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpdateCheckStateStore {
    path: PathBuf,
}

impl UpdateCheckStateStore {
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            path: paths.update_check_state_path(),
        }
    }

    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A corrupt file, an unknown schema version or a read error is a miss: the
    /// app behaves as if no check has run yet.
    pub fn load(&self) -> Option<UpdateCheckState> {
        let raw = fs::read_to_string(&self.path).ok()?;
        let state: UpdateCheckState = serde_json::from_str(&raw).ok()?;
        (state.version == UPDATE_CHECK_STATE_VERSION).then_some(state)
    }

    pub fn store(&self, state: &UpdateCheckState) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let serialized = serde_json::to_string(state)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let temp = self.path.with_file_name(format!(
            ".tmp-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        ));
        fs::write(&temp, serialized)?;
        if let Err(error) = fs::rename(&temp, &self.path) {
            let _ = fs::remove_file(&temp);
            return Err(error);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::UpdateSourceResult;

    fn source(status: UpdateSourceStatus) -> UpdateSourceResult {
        UpdateSourceResult {
            source: format!("acme/{status:?}"),
            reference: None,
            status,
            skill_count: 1,
            cached: status == UpdateSourceStatus::Cached,
            truncated: false,
            last_success_at_epoch_ms: None,
            error: None,
            note: None,
        }
    }

    fn report(statuses: &[UpdateSourceStatus], last_success_at: Option<u64>) -> UpdateCheckReport {
        let sources: Vec<_> = statuses.iter().copied().map(source).collect();
        UpdateCheckReport {
            skills: Vec::new(),
            cached_source_count: sources
                .iter()
                .filter(|source| source.status == UpdateSourceStatus::Cached)
                .count(),
            skipped_source_count: sources
                .iter()
                .filter(|source| source.status == UpdateSourceStatus::Skipped)
                .count(),
            sources,
            checked_at_epoch_ms: 1_000,
            last_success_at_epoch_ms: last_success_at,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn derives_ok_partial_and_failed_outcomes_from_source_statuses() {
        let ok = UpdateCheckState::from_report(&report(
            &[UpdateSourceStatus::Ok, UpdateSourceStatus::Cached],
            Some(900),
        ));
        assert_eq!(ok.last_outcome, UpdateCheckOutcome::Ok);
        assert_eq!(ok.last_attempt_at, 1_000);
        assert_eq!(ok.last_success_at, Some(900));
        assert_eq!(ok.cached_source_count, 1);
        assert_eq!(ok.failed_source_count, 0);

        let partial = UpdateCheckState::from_report(&report(
            &[UpdateSourceStatus::Ok, UpdateSourceStatus::Error],
            Some(900),
        ));
        assert_eq!(partial.last_outcome, UpdateCheckOutcome::Partial);
        assert_eq!(partial.failed_source_count, 1);

        let skipped = UpdateCheckState::from_report(&report(
            &[UpdateSourceStatus::Ok, UpdateSourceStatus::Skipped],
            Some(900),
        ));
        assert_eq!(skipped.last_outcome, UpdateCheckOutcome::Partial);
        assert_eq!(skipped.skipped_source_count, 1);

        let failed = UpdateCheckState::from_report(&report(
            &[UpdateSourceStatus::Error, UpdateSourceStatus::Skipped],
            None,
        ));
        assert_eq!(failed.last_outcome, UpdateCheckOutcome::Failed);
        assert_eq!(failed.last_success_at, None);
        assert_eq!(failed.failed_source_count, 1);
        assert_eq!(failed.skipped_source_count, 1);
    }

    #[test]
    fn round_trips_and_rejects_corrupt_or_foreign_versions() {
        let tmp = tempfile::tempdir().unwrap();
        let store = UpdateCheckStateStore::from_path(tmp.path().join("app-data/update-check.json"));
        let state = UpdateCheckState {
            version: UPDATE_CHECK_STATE_VERSION,
            last_attempt_at: 1_000,
            last_success_at: Some(900),
            last_outcome: UpdateCheckOutcome::Partial,
            failed_source_count: 1,
            cached_source_count: 2,
            skipped_source_count: 3,
        };

        store.store(&state).unwrap();
        assert_eq!(store.load(), Some(state.clone()));

        fs::write(store.path(), "{not json").unwrap();
        assert_eq!(store.load(), None);

        fs::write(
            store.path(),
            serde_json::json!({
                "version": UPDATE_CHECK_STATE_VERSION + 1,
                "lastAttemptAt": 1_000,
                "lastSuccessAt": null,
                "lastOutcome": "ok",
                "failedSourceCount": 0,
                "cachedSourceCount": 0,
                "skippedSourceCount": 0
            })
            .to_string(),
        )
        .unwrap();
        assert_eq!(store.load(), None);
    }

    #[test]
    fn serializes_the_frontend_contract() {
        let state = UpdateCheckState {
            version: 1,
            last_attempt_at: 1_000,
            last_success_at: None,
            last_outcome: UpdateCheckOutcome::Failed,
            failed_source_count: 2,
            cached_source_count: 0,
            skipped_source_count: 1,
        };
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["version"], 1);
        assert_eq!(json["lastAttemptAt"], 1_000);
        assert!(json["lastSuccessAt"].is_null());
        assert_eq!(json["lastOutcome"], "failed");
        assert_eq!(json["failedSourceCount"], 2);
        assert_eq!(json["cachedSourceCount"], 0);
        assert_eq!(json["skippedSourceCount"], 1);
    }

    #[test]
    fn uses_the_app_data_path() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(tmp.path().join("home"), tmp.path().join("app-data"));
        let store = UpdateCheckStateStore::new(&paths);
        assert_eq!(store.path(), paths.update_check_state_path());
    }
}
