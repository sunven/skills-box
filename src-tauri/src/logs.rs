//! Append-only JSONL operation log.
//!
//! Every update/uninstall/detach attempt appends one line under app data
//! `logs/operations.jsonl` with the time, action, skill, affected agents,
//! result, backup path and error. Logging is best effort: a log write failure
//! must never block or fail the operation, so callers ignore the returned
//! error.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::error::AppError;
use crate::paths::AppPaths;

pub const OPERATIONS_LOG_FILE: &str = "operations.jsonl";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationEntry {
    pub action: String,
    pub skill: String,
    pub agents: Vec<String>,
    pub result: String,
    pub backup_path: Option<String>,
    pub error: Option<AppError>,
}

impl OperationEntry {
    pub fn new(
        action: impl Into<String>,
        skill: impl Into<String>,
        agents: Vec<String>,
        result: impl Into<String>,
        backup_path: Option<String>,
        error: Option<AppError>,
    ) -> Self {
        Self {
            action: action.into(),
            skill: skill.into(),
            agents,
            result: result.into(),
            backup_path,
            error,
        }
    }
}

pub struct OperationLog {
    path: PathBuf,
    write_lock: Mutex<()>,
}

impl std::fmt::Debug for OperationLog {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OperationLog")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl OperationLog {
    pub fn new(paths: &AppPaths) -> Self {
        Self::from_path(paths.logs_dir().join(OPERATIONS_LOG_FILE))
    }

    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            write_lock: Mutex::new(()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one JSONL line. Failures are returned for callers/tests but must
    /// not be treated as operation failures.
    pub fn append(&self, entry: &OperationEntry) -> Result<(), AppError> {
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| log_write_failed(&self.path, error))?;
        }
        let now = now_ms();
        let line = LogLine {
            time: format_iso8601(now),
            time_epoch_ms: now,
            action: &entry.action,
            skill: &entry.skill,
            agents: &entry.agents,
            result: &entry.result,
            backup_path: entry.backup_path.as_deref(),
            error: entry.error.as_ref(),
        };
        let serialized =
            serde_json::to_string(&line).map_err(|error| log_write_failed(&self.path, error.into()))?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|error| log_write_failed(&self.path, error))?;
        file.write_all(serialized.as_bytes())
            .and_then(|_| file.write_all(b"\n"))
            .and_then(|_| file.flush())
            .map_err(|error| log_write_failed(&self.path, error))
    }
}

fn log_write_failed(path: &Path, error: std::io::Error) -> AppError {
    AppError::new("log.write_failed", "无法写入操作日志。")
        .detail(format!("{}：{error}", path.display()))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LogLine<'a> {
    time: String,
    time_epoch_ms: u64,
    action: &'a str,
    skill: &'a str,
    agents: &'a [String],
    result: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    backup_path: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a AppError>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

/// `YYYY-MM-DDTHH:MM:SS.mmmZ` without pulling in a date-time dependency.
pub fn format_iso8601(epoch_ms: u64) -> String {
    let seconds = (epoch_ms / 1000) as i64;
    let millis = epoch_ms % 1000;
    let days = seconds.div_euclid(86_400);
    let second_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        second_of_day / 3600,
        (second_of_day % 3600) / 60,
        second_of_day % 60,
    )
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    (year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::codes;

    fn entry(result: &str, error: Option<AppError>) -> OperationEntry {
        OperationEntry::new(
            "update",
            "alpha",
            vec!["qoder".to_string(), "universal".to_string()],
            result,
            Some("/tmp/backups/1-update-alpha".to_string()),
            error,
        )
    }

    #[test]
    fn appends_one_json_line_per_entry_with_the_required_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let log = OperationLog::from_path(tmp.path().join("logs/operations.jsonl"));
        log.append(&entry("updated", None)).unwrap();
        log.append(&entry(
            "failed",
            Some(AppError::cli_failed("boom").retryable(true)),
        ))
        .unwrap();

        let raw = fs::read_to_string(log.path()).unwrap();
        let lines: Vec<&str> = raw.lines().collect();
        assert_eq!(lines.len(), 2);

        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["action"], "update");
        assert_eq!(first["skill"], "alpha");
        assert_eq!(first["agents"], serde_json::json!(["qoder", "universal"]));
        assert_eq!(first["result"], "updated");
        assert_eq!(first["backupPath"], "/tmp/backups/1-update-alpha");
        assert_eq!(first["timeEpochMs"].as_u64().unwrap() > 0, true);
        let time = first["time"].as_str().unwrap();
        assert!(time.ends_with('Z'));
        assert_eq!(&time[4..5], "-");
        assert_eq!(&time[10..11], "T");
        assert!(first.get("error").is_none());

        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second["result"], "failed");
        assert_eq!(second["error"]["code"], codes::CLI_FAILED);
        assert_eq!(second["error"]["message"], "skills CLI 执行失败。");
        assert_eq!(second["error"]["retryable"], true);
    }

    #[test]
    fn log_write_failures_are_returned_but_never_panic() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("logs/operations.jsonl");
        fs::create_dir_all(&path).unwrap();
        let log = OperationLog::from_path(path);
        let error = log.append(&entry("updated", None)).unwrap_err();
        assert_eq!(error.code, "log.write_failed");
    }

    #[test]
    fn iso_time_formats_a_known_epoch() {
        assert_eq!(format_iso8601(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            format_iso8601(1_793_163_988_000),
            "2026-10-28T05:06:28.000Z"
        );
    }
}
