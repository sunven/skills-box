//! Read-only health check for one Scope's Lock entry.
//!
//! This is not the inventory lock model (a later ticket owns it); it only
//! tells the environment banner or the startup interruption report whether the
//! lock is missing (normal on a fresh machine), valid for its schema, corrupt,
//! or an unsupported version. Global scopes use the v3 lock; Project roots use
//! the v1/v2 `skills-lock.json`.

use std::path::Path;

use serde::Serialize;

use crate::error::AppError;
use crate::paths::AppPaths;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LockStatus {
    Ok,
    Missing,
    Corrupt,
    Unsupported,
    Unreadable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockHealth {
    pub path: String,
    pub exists: bool,
    pub status: LockStatus,
    pub version: Option<u64>,
    pub skill_count: Option<usize>,
    pub error: Option<AppError>,
}

impl LockHealth {
    pub fn healthy(&self) -> bool {
        matches!(self.status, LockStatus::Ok | LockStatus::Missing)
    }
}

/// Lock schema owned by one Scope. The startup interruption report selects it
/// from the marker's scope; the environment banner always inspects Global.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockSchema {
    GlobalV3,
    Project,
}

impl LockSchema {
    fn supports_version(self, version: u64) -> bool {
        match self {
            Self::GlobalV3 => version == 3,
            Self::Project => crate::project_lock::PROJECT_LOCK_VERSIONS.contains(&version),
        }
    }

    fn corrupt(self, detail: impl Into<String>) -> AppError {
        match self {
            Self::GlobalV3 => AppError::lock_corrupt(detail),
            Self::Project => AppError::project_lock_corrupt(detail),
        }
    }

    fn unsupported(self, version: u64) -> AppError {
        match self {
            Self::GlobalV3 => AppError::lock_unsupported_version(version),
            Self::Project => AppError::project_lock_unsupported_version(version),
        }
    }

    fn read_failed(self, detail: impl Into<String>) -> AppError {
        match self {
            Self::GlobalV3 => AppError::lock_read_failed(detail),
            Self::Project => AppError::project_lock_read_failed(detail),
        }
    }
}

pub fn inspect(paths: &AppPaths) -> LockHealth {
    let path = paths.lock_path();
    inspect_path(&path, &paths.display(&path), LockSchema::GlobalV3)
}

/// Same strict rules as [`inspect`] for callers that hold an explicit lock
/// path and schema (startup interruption verification, tests).
pub fn inspect_path(path: &Path, display: &str, schema: LockSchema) -> LockHealth {
    let display = display.to_string();
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return LockHealth {
                path: display,
                exists: false,
                status: LockStatus::Missing,
                version: None,
                skill_count: None,
                error: None,
            };
        }
        Err(error) => {
            return LockHealth {
                path: display,
                exists: true,
                status: LockStatus::Unreadable,
                version: None,
                skill_count: None,
                error: Some(schema.read_failed(error.to_string())),
            };
        }
    };

    let value: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(error) => {
            return LockHealth {
                path: display,
                exists: true,
                status: LockStatus::Corrupt,
                version: None,
                skill_count: None,
                error: Some(schema.corrupt(error.to_string())),
            };
        }
    };

    let version = value.get("version").and_then(serde_json::Value::as_u64);
    if !version.is_some_and(|version| schema.supports_version(version)) {
        let status = if version.is_some() {
            LockStatus::Unsupported
        } else {
            LockStatus::Corrupt
        };
        let error = match version {
            Some(version) => schema.unsupported(version),
            None => schema.corrupt("缺少 version 字段或类型错误"),
        };
        return LockHealth {
            path: display,
            exists: true,
            status,
            version,
            skill_count: None,
            error: Some(error),
        };
    }

    let Some(skills) = value.get("skills").and_then(serde_json::Value::as_object) else {
        return LockHealth {
            path: display,
            exists: true,
            status: LockStatus::Corrupt,
            version,
            skill_count: None,
            error: Some(schema.corrupt("skills 字段缺失或不是对象")),
        };
    };

    LockHealth {
        path: display,
        exists: true,
        status: LockStatus::Ok,
        version,
        skill_count: Some(skills.len()),
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    fn temp_paths() -> (tempfile::TempDir, AppPaths) {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(tmp.path().join("home"), tmp.path().join("app-data"));
        (tmp, paths)
    }

    #[test]
    fn missing_lock_is_healthy() {
        let (_tmp, paths) = temp_paths();
        let health = inspect(&paths);
        assert_eq!(health.status, LockStatus::Missing);
        assert!(!health.exists);
        assert!(health.healthy());
        assert!(health.error.is_none());
        assert_eq!(health.path, "~/.agents/.skill-lock.json");
    }

    #[test]
    fn valid_v3_lock_reports_entries() {
        let (_tmp, paths) = temp_paths();
        std::fs::create_dir_all(paths.agents_root()).unwrap();
        std::fs::write(
            paths.lock_path(),
            r#"{"version":3,"skills":{"alpha":{"source":"a/b"},"beta":{"source":"c/d"}}}"#,
        )
        .unwrap();
        let health = inspect(&paths);
        assert_eq!(health.status, LockStatus::Ok);
        assert_eq!(health.version, Some(3));
        assert_eq!(health.skill_count, Some(2));
        assert!(health.healthy());
        assert!(health.error.is_none());
    }

    #[test]
    fn corrupt_json_is_reported_with_lock_code() {
        let (_tmp, paths) = temp_paths();
        std::fs::create_dir_all(paths.agents_root()).unwrap();
        std::fs::write(paths.lock_path(), "{not json").unwrap();
        let health = inspect(&paths);
        assert_eq!(health.status, LockStatus::Corrupt);
        assert!(health.exists);
        assert!(!health.healthy());
        assert_eq!(health.error.as_ref().unwrap().code, ErrorCode::LockCorrupt);
    }

    #[test]
    fn non_v3_lock_is_unsupported_not_silently_empty() {
        let (_tmp, paths) = temp_paths();
        std::fs::create_dir_all(paths.agents_root()).unwrap();
        std::fs::write(paths.lock_path(), r#"{"version":2,"skills":{}}"#).unwrap();
        let health = inspect(&paths);
        assert_eq!(health.status, LockStatus::Unsupported);
        assert_eq!(health.version, Some(2));
        assert_eq!(
            health.error.as_ref().unwrap().code,
            ErrorCode::LockUnsupportedVersion
        );
        assert!(!health.healthy());
    }

    #[test]
    fn v3_without_skills_object_is_corrupt() {
        let (_tmp, paths) = temp_paths();
        std::fs::create_dir_all(paths.agents_root()).unwrap();
        std::fs::write(paths.lock_path(), r#"{"version":3}"#).unwrap();
        let health = inspect(&paths);
        assert_eq!(health.status, LockStatus::Corrupt);
        assert_eq!(health.error.as_ref().unwrap().code, ErrorCode::LockCorrupt);
    }

    #[test]
    fn valid_project_v1_and_v2_locks_are_healthy() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("skills-lock.json");
        for version in [1, 2] {
            std::fs::write(
                &path,
                serde_json::json!({"version":version,"skills":{"alpha":{"source":"a/b"}}})
                    .to_string(),
            )
            .unwrap();
            let health = inspect_path(&path, "project-lock", LockSchema::Project);
            assert_eq!(health.status, LockStatus::Ok, "v{version}");
            assert_eq!(health.version, Some(version));
            assert_eq!(health.skill_count, Some(1));
            assert!(health.healthy());
        }
    }

    #[test]
    fn global_v3_lock_is_unsupported_under_the_project_schema() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("skills-lock.json");
        std::fs::write(&path, r#"{"version":3,"skills":{}}"#).unwrap();
        let health = inspect_path(&path, "project-lock", LockSchema::Project);
        assert_eq!(health.status, LockStatus::Unsupported);
        assert!(!health.healthy());
        assert_eq!(
            health.error.as_ref().unwrap().code,
            ErrorCode::ProjectLockUnsupported
        );
    }

    #[test]
    fn project_corrupt_json_uses_the_project_message() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("skills-lock.json");
        std::fs::write(&path, b"{not json").unwrap();
        let health = inspect_path(&path, "project-lock", LockSchema::Project);
        assert_eq!(health.status, LockStatus::Corrupt);
        assert_eq!(health.error.as_ref().unwrap().code, ErrorCode::LockCorrupt);
        assert!(health.error.as_ref().unwrap().message.contains("项目"));
    }
}
