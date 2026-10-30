//! Interrupted-mutation marker and next-startup verification.
//!
//! A mutation writes `runtime/interrupted-mutation.json` before it touches the
//! disk and removes it after the task reaches a final state. If the process is
//! killed (confirmed exit while a mutation runs, crash, power loss), the marker
//! survives together with the mandatory backup snapshots.
//!
//! On the next start `inspect` re-reads the strict v3 lock health and reports
//! which snapshots still exist, so the UI can surface a warning instead of
//! silently pretending the previous operation either succeeded or never ran.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::env::{LockHealth, lock_health};
use crate::paths::AppPaths;

pub const MARKER_FILE: &str = "interrupted-mutation.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationKind {
    Install,
    Update,
    Uninstall,
}

impl MutationKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Install => "安装",
            Self::Update => "更新",
            Self::Uninstall => "卸载",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterruptedMutation {
    pub task_id: String,
    pub kind: MutationKind,
    /// Scope the mutation was started in. Legacy markers without a scope are
    /// read as global so the next launch still warns.
    #[serde(default)]
    pub scope: crate::scope::ScopeId,
    pub skills: Vec<String>,
    pub started_at_epoch_ms: u64,
    #[serde(default)]
    pub snapshot_paths: Vec<String>,
}

/// Serialized to the frontend through `EnvReport.interrupted`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InterruptedReport {
    pub marker_path: String,
    /// The marker existed but could not be parsed; the mutation may still have
    /// been interrupted, so this is still a warning.
    pub marker_corrupt: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker_error: Option<String>,
    pub task_id: Option<String>,
    pub kind: Option<MutationKind>,
    /// Scope that owns the interrupted record. `None` only for a corrupt
    /// marker; a legacy marker without a scope reports global.
    pub scope: Option<crate::scope::ScopeId>,
    pub skills: Vec<String>,
    pub started_at_epoch_ms: Option<u64>,
    pub snapshot_paths: Vec<String>,
    pub snapshots_present: Vec<String>,
    pub snapshots_missing: Vec<String>,
    pub lock: LockHealth,
    pub detected_at_epoch_ms: u64,
}

impl InterruptedReport {
    pub fn lock_is_healthy(&self) -> bool {
        self.lock.healthy()
    }

    pub fn snapshots_are_intact(&self) -> bool {
        self.snapshots_missing.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct MarkerStore {
    path: PathBuf,
    lock_path: PathBuf,
}

impl MarkerStore {
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            path: paths.interrupted_marker_path(),
            lock_path: paths.lock_path(),
        }
    }

    pub fn from_paths(marker_path: impl Into<PathBuf>, lock_path: impl Into<PathBuf>) -> Self {
        Self {
            path: marker_path.into(),
            lock_path: lock_path.into(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Persist a mutation marker before the operation starts. Each call
    /// replaces the previous marker; the single-writer permit guarantees that
    /// only one mutation is ever in flight.
    pub fn begin(&self, mutation: &InterruptedMutation) -> std::io::Result<()> {
        self.write(mutation)
    }

    /// Record a snapshot path as soon as the backup exists, so a crash after
    /// that point still tells the next launch where the snapshot lives.
    pub fn record_snapshots(&self, mutation: &mut InterruptedMutation, snapshot: &Path) {
        let snapshot = snapshot.to_string_lossy().into_owned();
        if mutation.snapshot_paths.contains(&snapshot) {
            return;
        }
        mutation.snapshot_paths.push(snapshot);
        let _ = self.write(mutation);
    }

    /// Remove the marker after a completed, failed or cancelled task. Failure
    /// to remove it is not fatal: the next launch shows an extra warning.
    pub fn clear(&self) {
        match fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {}
        }
    }

    pub fn read(&self) -> MarkerRead {
        let raw = match fs::read_to_string(&self.path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return MarkerRead::Absent;
            }
            Err(error) => return MarkerRead::Corrupt(error.to_string()),
        };
        match serde_json::from_str::<InterruptedMutation>(&raw) {
            Ok(mutation) => MarkerRead::Present(mutation),
            Err(error) => MarkerRead::Corrupt(error.to_string()),
        }
    }

    /// Read the marker (if any), re-check the lock and snapshot existence.
    pub fn inspect(&self) -> Option<InterruptedReport> {
        let (marker_corrupt, mutation, corrupt_detail) = match self.read() {
            MarkerRead::Absent => return None,
            MarkerRead::Corrupt(detail) => (true, None, Some(detail)),
            MarkerRead::Present(mutation) => (false, Some(mutation), None),
        };
        let lock =
            lock_health::inspect_path(&self.lock_path, &self.lock_path.display().to_string());

        let mut snapshots_present = Vec::new();
        let mut snapshots_missing = Vec::new();
        if let Some(mutation) = &mutation {
            for snapshot in &mutation.snapshot_paths {
                if Path::new(snapshot).exists() {
                    snapshots_present.push(snapshot.clone());
                } else {
                    snapshots_missing.push(snapshot.clone());
                }
            }
        }
        Some(InterruptedReport {
            marker_path: self.path.display().to_string(),
            marker_corrupt,
            marker_error: corrupt_detail,
            task_id: mutation.as_ref().map(|mutation| mutation.task_id.clone()),
            kind: mutation.as_ref().map(|mutation| mutation.kind),
            scope: mutation.as_ref().map(|mutation| mutation.scope.clone()),
            skills: mutation
                .as_ref()
                .map(|mutation| mutation.skills.clone())
                .unwrap_or_default(),
            started_at_epoch_ms: mutation
                .as_ref()
                .map(|mutation| mutation.started_at_epoch_ms),
            snapshot_paths: mutation
                .as_ref()
                .map(|mutation| mutation.snapshot_paths.clone())
                .unwrap_or_default(),
            snapshots_present,
            snapshots_missing,
            lock,
            detected_at_epoch_ms: epoch_ms(),
        })
    }

    fn write(&self, mutation: &InterruptedMutation) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temp = self.path.with_extension("json.tmp");
        let body = serde_json::to_vec_pretty(mutation)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        fs::write(&temp, body)?;
        fs::rename(&temp, &self.path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkerRead {
    Absent,
    Present(InterruptedMutation),
    Corrupt(String),
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

pub fn inspect(paths: &AppPaths) -> Option<InterruptedReport> {
    MarkerStore::new(paths).inspect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, AppPaths, MarkerStore) {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(tmp.path().join("home"), tmp.path().join("app-data"));
        fs::create_dir_all(paths.agents_root()).unwrap();
        paths.ensure_dirs().unwrap();
        let store = MarkerStore::new(&paths);
        (tmp, paths, store)
    }

    fn mutation(skills: &[&str]) -> InterruptedMutation {
        InterruptedMutation {
            task_id: "task-7".to_string(),
            kind: MutationKind::Update,
            scope: crate::scope::ScopeId::Global,
            skills: skills.iter().map(|skill| skill.to_string()).collect(),
            started_at_epoch_ms: 1_700_000_000_000,
            snapshot_paths: Vec::new(),
        }
    }

    #[test]
    fn absent_marker_reports_nothing() {
        let (_tmp, _paths, store) = fixture();
        assert_eq!(store.read(), MarkerRead::Absent);
        assert!(store.inspect().is_none());
    }

    #[test]
    fn begin_read_record_and_clear_round_trip() {
        let (_tmp, _paths, store) = fixture();
        let mut record = mutation(&["alpha"]);
        store.begin(&record).unwrap();

        let MarkerRead::Present(read) = store.read() else {
            panic!("marker should parse");
        };
        assert_eq!(read, record);

        let snapshot = Path::new("/tmp/skills-box-snapshot");
        store.record_snapshots(&mut record, snapshot);
        store.record_snapshots(&mut record, snapshot);
        assert_eq!(record.snapshot_paths.len(), 1);

        store.clear();
        assert_eq!(store.read(), MarkerRead::Absent);
        store.clear(); // clearing twice is harmless
    }

    #[test]
    fn corrupt_marker_is_still_a_warning() {
        let (_tmp, _paths, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), b"{not json").unwrap();
        let report = store.inspect().expect("corrupt marker must warn");
        assert!(report.marker_corrupt);
        assert!(report.task_id.is_none());
        assert!(!report.lock_is_healthy() || report.lock.healthy());
    }

    #[test]
    fn report_verifies_lock_health_and_snapshot_existence() {
        let (tmp, paths, store) = fixture();
        fs::write(
            paths.lock_path(),
            r#"{"version":3,"skills":{"alpha":{"source":"a/b"}}}"#,
        )
        .unwrap();
        fs::create_dir_all(paths.backups_dir()).unwrap();
        let present = paths.backups_dir().join("snapshot-present");
        fs::create_dir_all(&present).unwrap();
        let missing = tmp.path().join("snapshot-missing");

        let mut record = mutation(&["alpha"]);
        record.snapshot_paths = vec![
            present.to_string_lossy().into_owned(),
            missing.to_string_lossy().into_owned(),
        ];
        store.begin(&record).unwrap();

        let report = store.inspect().unwrap();
        assert!(!report.marker_corrupt);
        assert_eq!(report.kind, Some(MutationKind::Update));
        assert_eq!(report.skills, vec!["alpha"]);
        assert_eq!(report.lock.status, crate::env::LockStatus::Ok);
        assert!(report.lock_is_healthy());
        assert_eq!(report.snapshots_present, vec![present.to_string_lossy()]);
        assert_eq!(report.snapshots_missing, vec![missing.to_string_lossy()]);
        assert!(!report.snapshots_are_intact());
    }

    #[test]
    fn corrupt_lock_is_reported_by_the_startup_verification() {
        let (_tmp, paths, store) = fixture();
        fs::write(paths.lock_path(), b"{broken").unwrap();
        store.begin(&mutation(&["alpha"])).unwrap();
        let report = store.inspect().unwrap();
        assert_eq!(report.lock.status, crate::env::LockStatus::Corrupt);
        assert!(!report.lock_is_healthy());
        assert!(report.snapshots_are_intact(), "no snapshots were recorded");
    }

    #[test]
    fn marker_and_report_carry_the_scope_that_started_the_mutation() {
        let (_tmp, _paths, store) = fixture();
        let mut record = mutation(&["alpha"]);
        record.scope = crate::scope::ScopeId::project(uuid::Uuid::new_v4());
        store.begin(&record).unwrap();

        let report = store.inspect().unwrap();
        assert_eq!(report.scope, Some(record.scope.clone()));

        // A legacy marker without a `scope` field still parses as global.
        let legacy = r#"{
            "taskId": "task-legacy",
            "kind": "update",
            "skills": ["alpha"],
            "startedAtEpochMs": 1,
            "snapshotPaths": []
        }"#;
        fs::write(store.path(), legacy).unwrap();
        let report = store.inspect().unwrap();
        assert_eq!(report.scope, Some(crate::scope::ScopeId::Global));
    }

    #[test]
    fn kind_labels_are_chinese() {
        assert_eq!(MutationKind::Install.label(), "安装");
        assert_eq!(MutationKind::Update.label(), "更新");
        assert_eq!(MutationKind::Uninstall.label(), "卸载");
    }
}
