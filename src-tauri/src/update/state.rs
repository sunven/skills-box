//! Persisted summary of the last full update check, per scope.
//!
//! The file lives in app data (not `cache/`) because the header and sidebar
//! badges show it across restarts. It is version 2:
//! `{ "version": 2, "scopes": { "<scopeId>": state } }`. A version-1 file
//! (single global state) is read as the global scope and migrated on the next
//! write. Only full checks write it; scoped per-skill checks never advance a
//! scope's last-check time. Writes are atomic and best-effort: a failure is
//! surfaced as a report warning, never as a failed check.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{UpdateCheckReport, UpdateSourceStatus};
use crate::paths::AppPaths;
use crate::scope::ScopeId;

/// File schema version. Version 1 stored one global state at the top level.
pub const UPDATE_CHECK_STATE_VERSION: u32 = 2;
/// Per-scope entry schema; unchanged shape from the v1 state object.
pub const UPDATE_CHECK_STATE_ENTRY_VERSION: u32 = 1;

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
    /// The per-scope entry version, not the file version.
    pub version: u32,
    pub last_attempt_at: u64,
    pub last_success_at: Option<u64>,
    pub last_outcome: UpdateCheckOutcome,
    pub failed_source_count: usize,
    pub cached_source_count: usize,
    pub skipped_source_count: usize,
    /// Skills whose remote fingerprint differs from the lock. Sidebar badges
    /// read this instead of re-deriving it from a report they may not have.
    pub candidate_count: usize,
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
            version: UPDATE_CHECK_STATE_ENTRY_VERSION,
            last_attempt_at: report.checked_at_epoch_ms,
            last_success_at: report.last_success_at_epoch_ms,
            last_outcome,
            failed_source_count,
            cached_source_count: report.cached_source_count,
            skipped_source_count: report.skipped_source_count,
            candidate_count: report.candidate_count(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateCheckStateFile {
    version: u32,
    #[serde(default)]
    scopes: BTreeMap<String, UpdateCheckState>,
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

    /// One scope's state. A corrupt file, an unknown schema version or a read
    /// error is a miss: the app behaves as if no check has run yet. A version-1
    /// file only ever described the global scope.
    pub fn load(&self, scope: &ScopeId) -> Option<UpdateCheckState> {
        let raw = fs::read_to_string(&self.path).ok()?;
        let scopes = parse_scopes(&raw)?;
        scopes
            .get(&scope.as_string())
            .filter(|state| state.version == UPDATE_CHECK_STATE_ENTRY_VERSION)
            .cloned()
    }

    /// Every readable scope entry, keyed by scope string. Used by tests and by
    /// callers that need to know whether a scope was ever checked.
    pub fn load_all(&self) -> BTreeMap<String, UpdateCheckState> {
        fs::read_to_string(&self.path)
            .ok()
            .and_then(|raw| parse_scopes(&raw))
            .map(|scopes| {
                scopes
                    .into_iter()
                    .filter(|(_, state)| state.version == UPDATE_CHECK_STATE_ENTRY_VERSION)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Persist one scope. Reading a version-1 file migrates its global state
    /// into the version-2 map, so no other scope's history is lost.
    pub fn store(&self, scope: &ScopeId, state: &UpdateCheckState) -> io::Result<()> {
        let mut scopes = self.read_migratable_scopes();
        scopes.insert(scope.as_string(), state.clone());
        let file = UpdateCheckStateFile {
            version: UPDATE_CHECK_STATE_VERSION,
            scopes,
        };
        let serialized = serde_json::to_vec(&file)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        write_atomic(&self.path, &serialized)
    }

    /// Drop one scope's persisted state. Version 1 only ever stored the global
    /// check, so forgetting a project leaves it untouched; version 2 removes
    /// the scope key. A missing or corrupt file is left as-is.
    pub fn forget_scope(&self, scope: &ScopeId) -> io::Result<()> {
        let raw = match fs::read_to_string(&self.path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&raw) else {
            return Ok(());
        };
        if value.get("version").and_then(serde_json::Value::as_u64)
            != Some(UPDATE_CHECK_STATE_VERSION as u64)
        {
            return Ok(());
        }
        let Some(scopes) = value
            .get_mut("scopes")
            .and_then(serde_json::Value::as_object_mut)
        else {
            return Ok(());
        };
        if scopes.remove(&scope.as_string()).is_none() {
            return Ok(());
        }
        let serialized = serde_json::to_vec(&value)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        write_atomic(&self.path, &serialized)
    }

    fn read_migratable_scopes(&self) -> BTreeMap<String, UpdateCheckState> {
        let Ok(raw) = fs::read_to_string(&self.path) else {
            return BTreeMap::new();
        };
        parse_scopes(&raw).unwrap_or_default()
    }
}

/// Parse either schema into a scope map. Version 1 maps to `global`.
fn parse_scopes(raw: &str) -> Option<BTreeMap<String, UpdateCheckState>> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let version = value.get("version")?.as_u64()?;
    if version == UPDATE_CHECK_STATE_VERSION as u64 {
        let file: UpdateCheckStateFile = serde_json::from_value(value).ok()?;
        if file.version != UPDATE_CHECK_STATE_VERSION {
            return None;
        }
        return Some(file.scopes);
    }
    if version == UPDATE_CHECK_STATE_ENTRY_VERSION as u64 {
        let state: UpdateCheckState = serde_json::from_value(value).ok()?;
        if state.version != UPDATE_CHECK_STATE_ENTRY_VERSION {
            return None;
        }
        let mut scopes = BTreeMap::new();
        scopes.insert(ScopeId::Global.as_string(), state);
        return Some(scopes);
    }
    None
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_file_name(format!(
        ".tmp-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    fs::write(&temp, bytes)?;
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::UpdateState;
    use crate::update::{UpdateSkillResult, UpdateSourceResult};

    fn project() -> ScopeId {
        ScopeId::project(uuid::Uuid::parse_str("5a7b6c1d-2e3f-4a5b-8c9d-0e1f2a3b4c5d").unwrap())
    }

    fn other_project() -> ScopeId {
        ScopeId::project(uuid::Uuid::parse_str("0f1e2d3c-4b5a-4697-8879-6a5b4c3d2e1f").unwrap())
    }

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

    fn skill(name: &str, state: UpdateState) -> UpdateSkillResult {
        UpdateSkillResult {
            name: name.to_string(),
            source: None,
            reference: None,
            lock_hash: None,
            remote_hash: None,
            state,
            cached: false,
            last_success_at_epoch_ms: None,
            previous_state: None,
            error: None,
            note: None,
        }
    }

    fn report(
        states: &[UpdateState],
        statuses: &[UpdateSourceStatus],
        last_success_at: Option<u64>,
    ) -> UpdateCheckReport {
        let sources: Vec<_> = statuses.iter().copied().map(source).collect();
        UpdateCheckReport {
            skills: states
                .iter()
                .enumerate()
                .map(|(index, state)| skill(&format!("skill-{index}"), *state))
                .collect(),
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

    fn state(last_attempt_at: u64) -> UpdateCheckState {
        UpdateCheckState {
            version: UPDATE_CHECK_STATE_ENTRY_VERSION,
            last_attempt_at,
            last_success_at: None,
            last_outcome: UpdateCheckOutcome::Ok,
            failed_source_count: 0,
            cached_source_count: 0,
            skipped_source_count: 0,
            candidate_count: 0,
        }
    }

    #[test]
    fn derives_outcomes_and_candidate_count_from_a_report() {
        let ok = UpdateCheckState::from_report(&report(
            &[UpdateState::Current, UpdateState::Update, UpdateState::Update],
            &[UpdateSourceStatus::Ok, UpdateSourceStatus::Cached],
            Some(900),
        ));
        assert_eq!(ok.last_outcome, UpdateCheckOutcome::Ok);
        assert_eq!(ok.last_attempt_at, 1_000);
        assert_eq!(ok.last_success_at, Some(900));
        assert_eq!(ok.cached_source_count, 1);
        assert_eq!(ok.candidate_count, 2);
        assert_eq!(ok.version, UPDATE_CHECK_STATE_ENTRY_VERSION);

        let partial = UpdateCheckState::from_report(&report(
            &[UpdateState::Current],
            &[UpdateSourceStatus::Ok, UpdateSourceStatus::Error],
            Some(900),
        ));
        assert_eq!(partial.last_outcome, UpdateCheckOutcome::Partial);
        assert_eq!(partial.failed_source_count, 1);

        let skipped = UpdateCheckState::from_report(&report(
            &[UpdateState::Current],
            &[UpdateSourceStatus::Ok, UpdateSourceStatus::Skipped],
            Some(900),
        ));
        assert_eq!(skipped.last_outcome, UpdateCheckOutcome::Partial);
        assert_eq!(skipped.skipped_source_count, 1);

        let failed = UpdateCheckState::from_report(&report(
            &[UpdateState::Error],
            &[UpdateSourceStatus::Error, UpdateSourceStatus::Skipped],
            None,
        ));
        assert_eq!(failed.last_outcome, UpdateCheckOutcome::Failed);
        assert_eq!(failed.last_success_at, None);
        assert_eq!(failed.failed_source_count, 1);
        assert_eq!(failed.skipped_source_count, 1);
        assert_eq!(failed.candidate_count, 0);
    }

    #[test]
    fn writes_v2_and_round_trips_each_scope_independently() {
        let tmp = tempfile::tempdir().unwrap();
        let store = UpdateCheckStateStore::from_path(tmp.path().join("app-data/update-check.json"));

        store.store(&ScopeId::Global, &state(1)).unwrap();
        store.store(&project(), &state(2)).unwrap();

        assert_eq!(store.load(&ScopeId::Global), Some(state(1)));
        assert_eq!(store.load(&project()), Some(state(2)));
        assert_eq!(store.load(&other_project()), None);

        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(store.path()).unwrap()).unwrap();
        assert_eq!(value["version"], 2);
        assert_eq!(value["scopes"]["global"]["lastAttemptAt"], 1);
        assert_eq!(value["scopes"][project().as_string()]["lastAttemptAt"], 2);
    }

    #[test]
    fn reads_a_v1_file_as_global_and_migrates_it_on_write() {
        let tmp = tempfile::tempdir().unwrap();
        let store = UpdateCheckStateStore::from_path(tmp.path().join("app-data/update-check.json"));
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(
            store.path(),
            serde_json::json!({
                "version": 1,
                "lastAttemptAt": 1_000,
                "lastSuccessAt": null,
                "lastOutcome": "ok",
                "failedSourceCount": 0,
                "cachedSourceCount": 0,
                "skippedSourceCount": 0,
                "candidateCount": 3
            })
            .to_string(),
        )
        .unwrap();

        let loaded = store.load(&ScopeId::Global).unwrap();
        assert_eq!(loaded.last_attempt_at, 1_000);
        assert_eq!(loaded.candidate_count, 3);
        assert_eq!(store.load(&project()), None);

        store.store(&project(), &state(2)).unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(store.path()).unwrap()).unwrap();
        assert_eq!(value["version"], 2);
        assert_eq!(value["scopes"]["global"]["lastAttemptAt"], 1_000);
        assert_eq!(value["scopes"][project().as_string()]["lastAttemptAt"], 2);
        assert_eq!(store.load(&ScopeId::Global), Some(loaded));
    }

    #[test]
    fn corrupt_or_foreign_versions_are_misses() {
        let tmp = tempfile::tempdir().unwrap();
        let store = UpdateCheckStateStore::from_path(tmp.path().join("app-data/update-check.json"));
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();

        fs::write(store.path(), "{not json").unwrap();
        assert_eq!(store.load(&ScopeId::Global), None);

        fs::write(
            store.path(),
            serde_json::json!({ "version": 3, "scopes": {} }).to_string(),
        )
        .unwrap();
        assert_eq!(store.load(&ScopeId::Global), None);

        fs::write(
            store.path(),
            serde_json::json!({
                "version": 2,
                "scopes": { "global": { "version": 99, "lastAttemptAt": 1 } }
            })
            .to_string(),
        )
        .unwrap();
        assert_eq!(store.load(&ScopeId::Global), None);
    }

    #[test]
    fn serializes_the_frontend_contract() {
        let mut value = state(1_000);
        value.last_outcome = UpdateCheckOutcome::Failed;
        value.failed_source_count = 2;
        value.candidate_count = 4;
        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(json["version"], 1);
        assert_eq!(json["lastAttemptAt"], 1_000);
        assert_eq!(json["lastOutcome"], "failed");
        assert_eq!(json["failedSourceCount"], 2);
        assert_eq!(json["candidateCount"], 4);
    }

    #[test]
    fn uses_the_app_data_path() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(tmp.path().join("home"), tmp.path().join("app-data"));
        let store = UpdateCheckStateStore::new(&paths);
        assert_eq!(store.path(), paths.update_check_state_path());
    }

    #[test]
    fn forget_scope_removes_only_that_scope_from_a_v2_file() {
        let tmp = tempfile::tempdir().unwrap();
        let store = UpdateCheckStateStore::from_path(tmp.path().join("app-data/update-check.json"));
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(
            store.path(),
            serde_json::json!({
                "version": 2,
                "scopes": {
                    "global": { "version": 1, "lastAttemptAt": 1 },
                    project().as_string(): { "version": 1, "lastAttemptAt": 2 },
                    other_project().as_string(): { "version": 1, "lastAttemptAt": 3 }
                }
            })
            .to_string(),
        )
        .unwrap();

        store.forget_scope(&project()).unwrap();

        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(store.path()).unwrap()).unwrap();
        assert_eq!(value["version"], 2);
        assert!(value["scopes"].get("global").is_some());
        assert!(value["scopes"].get(other_project().as_string()).is_some());
        assert!(value["scopes"].get(project().as_string()).is_none());
    }

    #[test]
    fn forget_scope_leaves_v1_missing_and_corrupt_files_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let store = UpdateCheckStateStore::from_path(tmp.path().join("app-data/update-check.json"));

        store.forget_scope(&project()).unwrap();
        assert!(!store.path().exists());

        let v1 = serde_json::json!({
            "version": 1,
            "lastAttemptAt": 1,
            "lastSuccessAt": null,
            "lastOutcome": "ok",
            "failedSourceCount": 0,
            "cachedSourceCount": 0,
            "skippedSourceCount": 0
        })
        .to_string();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), &v1).unwrap();
        store.forget_scope(&project()).unwrap();
        assert_eq!(fs::read_to_string(store.path()).unwrap(), v1);

        let corrupt = "{not json";
        fs::write(store.path(), corrupt).unwrap();
        store.forget_scope(&project()).unwrap();
        assert_eq!(fs::read_to_string(store.path()).unwrap(), corrupt);
    }
}
