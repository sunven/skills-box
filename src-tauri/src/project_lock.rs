//! Strict read-only model for a project root's `skills-lock.json` (v1/v2).
//!
//! Project locks differ from the global v3 lock: they store a SHA-256
//! `computedHash` instead of a Git tree hash, and carry no install/update
//! timestamps or mount list. V1 uses the legacy locale-dependent hash; v2 uses
//! the portable hash introduced in skills@1.8.1. A broken lock must not become
//! an empty inventory. A missing lock is still normal.
//!
//! Entries are normalized into the shared `LockEntry` shape so inventory,
//! detail and update-target code do not branch on the lock version. Timestamps
//! stay `None` and the computed hash lands in `skill_folder_hash`.

use std::collections::BTreeMap;
use std::path::Path;

use crate::error::AppError;
use crate::lock::LockEntry;

pub const PROJECT_LOCK_VERSIONS: &[u64] = &[1, 2];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectLockFile {
    version: u64,
    entries: BTreeMap<String, LockEntry>,
}

impl Default for ProjectLockFile {
    fn default() -> Self {
        Self {
            version: 1,
            entries: BTreeMap::new(),
        }
    }
}

impl ProjectLockFile {
    pub fn read(path: &Path) -> Result<Self, AppError> {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => {
                return Err(AppError::project_lock_read_failed(error.to_string()));
            }
        };
        Self::parse(&raw)
    }

    pub fn parse(raw: &str) -> Result<Self, AppError> {
        let value: serde_json::Value = serde_json::from_str(raw)
            .map_err(|error| AppError::project_lock_corrupt(error.to_string()))?;

        let Some(version) = value.get("version") else {
            return Err(AppError::project_lock_corrupt("缺少 version 字段"));
        };
        let Some(version) = version.as_u64() else {
            return Err(AppError::project_lock_corrupt("version 字段不是整数"));
        };
        if !PROJECT_LOCK_VERSIONS.contains(&version) {
            return Err(AppError::project_lock_unsupported_version(version));
        }

        let Some(skills) = value.get("skills") else {
            return Err(AppError::project_lock_corrupt("缺少 skills 字段"));
        };
        let Some(skills) = skills.as_object() else {
            return Err(AppError::project_lock_corrupt("skills 字段不是对象"));
        };

        let mut entries = BTreeMap::new();
        for (name, raw_entry) in skills {
            entries.insert(name.clone(), parse_entry(name, raw_entry)?);
        }
        Ok(Self { version, entries })
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn get(&self, name: &str) -> Option<&LockEntry> {
        self.entries.get(name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &LockEntry> {
        self.entries.values()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &BTreeMap<String, LockEntry> {
        &self.entries
    }
}

fn parse_entry(name: &str, raw: &serde_json::Value) -> Result<LockEntry, AppError> {
    let Some(object) = raw.as_object() else {
        return Err(entry_error(name, "条目不是对象"));
    };

    let required = |field: &str| -> Result<String, AppError> {
        object
            .get(field)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| entry_error(name, format!("{field} 缺失或不是字符串")))
    };

    let optional = |field: &str| -> Result<Option<String>, AppError> {
        match object.get(field) {
            None | Some(serde_json::Value::Null) => Ok(None),
            Some(value) => value
                .as_str()
                .map(|value| Some(value.to_string()))
                .ok_or_else(|| entry_error(name, format!("{field} 不是字符串"))),
        }
    };

    Ok(LockEntry {
        name: name.to_string(),
        source: required("source")?,
        source_type: required("sourceType")?,
        source_url: optional("sourceUrl")?.unwrap_or_default(),
        reference: optional("ref")?,
        skill_path: optional("skillPath")?,
        skill_folder_hash: required("computedHash")?,
        installed_at: None,
        updated_at: None,
        plugin_name: None,
        source_base_url: None,
        well_known_digest: None,
    })
}

fn entry_error(name: &str, detail: impl AsRef<str>) -> AppError {
    AppError::project_lock_corrupt(format!("skills.{name}: {}", detail.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    fn entry() -> serde_json::Value {
        serde_json::json!({
            "source": "acme/skills",
            "sourceUrl": "https://github.com/acme/skills.git",
            "ref": "main",
            "sourceType": "github",
            "skillPath": "skills/alpha/SKILL.md",
            "computedHash": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        })
    }

    fn lock_with(entry: serde_json::Value) -> String {
        serde_json::json!({
            "version": 1,
            "skills": { "alpha": entry }
        })
        .to_string()
    }

    #[test]
    fn missing_lock_is_an_empty_project_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = ProjectLockFile::read(&tmp.path().join("skills-lock.json")).unwrap();
        assert!(lock.is_empty());
        assert_eq!(lock.len(), 0);
        assert!(lock.entries().is_empty());
    }

    #[test]
    fn parses_v1_entries_and_maps_computed_hash_without_timestamps() {
        let lock = ProjectLockFile::parse(&lock_with(entry())).unwrap();
        assert_eq!(lock.len(), 1);
        let alpha = lock.get("alpha").unwrap();
        assert_eq!(alpha.name, "alpha");
        assert_eq!(alpha.source, "acme/skills");
        assert_eq!(alpha.source_url, "https://github.com/acme/skills.git");
        assert_eq!(alpha.reference.as_deref(), Some("main"));
        assert_eq!(alpha.source_type, "github");
        assert_eq!(alpha.skill_path.as_deref(), Some("skills/alpha/SKILL.md"));
        assert_eq!(
            alpha.skill_folder_hash,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        );
        assert!(alpha.installed_at.is_none());
        assert!(alpha.updated_at.is_none());
        assert!(alpha.plugin_name.is_none());
    }

    #[test]
    fn reads_v2_project_lock_without_rejecting_its_version() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("skills-lock.json");
        let mut skill = entry();
        skill.as_object_mut().unwrap().remove("ref");
        let raw = serde_json::json!({
            "version": 2,
            "skills": { "alpha": skill }
        });
        std::fs::write(&path, raw.to_string()).unwrap();

        let lock = ProjectLockFile::read(&path).unwrap();
        let alpha = lock.get("alpha").unwrap();
        assert_eq!(alpha.source, "acme/skills");
        assert_eq!(alpha.skill_path.as_deref(), Some("skills/alpha/SKILL.md"));
        assert_eq!(alpha.skill_folder_hash, skill["computedHash"]);
        assert!(alpha.reference.is_none());
    }

    #[test]
    fn optional_fields_may_be_absent_or_null() {
        let mut minimal = serde_json::json!({
            "source": "acme/skills",
            "sourceType": "github",
            "computedHash": "abc"
        });
        let lock = ProjectLockFile::parse(&lock_with(minimal.clone())).unwrap();
        let alpha = lock.get("alpha").unwrap();
        assert_eq!(alpha.source_url, "");
        assert!(alpha.reference.is_none());
        assert!(alpha.skill_path.is_none());

        minimal["sourceUrl"] = serde_json::Value::Null;
        minimal["ref"] = serde_json::Value::Null;
        let lock = ProjectLockFile::parse(&lock_with(minimal)).unwrap();
        assert!(lock.get("alpha").unwrap().reference.is_none());
    }

    #[test]
    fn corrupt_json_or_shape_maps_to_lock_corrupt() {
        for raw in [
            "{not json",
            r#"{"version":1}"#,
            r#"{"version":1,"skills":[]}"#,
            r#"{"skills":{}}"#,
            r#"{"version":"1","skills":{}}"#,
        ] {
            let error = ProjectLockFile::parse(raw).unwrap_err();
            assert_eq!(error.code, ErrorCode::LockCorrupt, "{raw}");
        }
    }

    #[test]
    fn unknown_versions_are_rejected() {
        for version in [0, 3, 99] {
            let raw = format!(r#"{{"version":{version},"skills":{{}}}}"#);
            let error = ProjectLockFile::parse(&raw).unwrap_err();
            assert_eq!(error.code, ErrorCode::ProjectLockUnsupported);
            assert!(error.message.contains("v1"));
            assert!(error.message.contains("v2"));
            assert!(error.message.contains(&format!("v{version}")));
        }
    }

    #[test]
    fn entries_missing_required_fields_are_corrupt_with_entry_context() {
        for field in ["source", "sourceType", "computedHash"] {
            let mut broken = entry();
            broken.as_object_mut().unwrap().remove(field);
            let error = ProjectLockFile::parse(&lock_with(broken)).unwrap_err();
            assert_eq!(error.code, ErrorCode::LockCorrupt, "{field}");
            let detail = error.detail.unwrap();
            assert!(detail.contains("alpha"), "{detail}");
            assert!(detail.contains(field), "{detail}");
        }
    }

    #[test]
    fn non_string_optional_fields_are_corrupt() {
        for field in ["sourceUrl", "ref", "skillPath"] {
            let mut broken = entry();
            broken[field] = serde_json::json!(1);
            let error = ProjectLockFile::parse(&lock_with(broken)).unwrap_err();
            assert_eq!(error.code, ErrorCode::LockCorrupt, "{field}");
        }
    }

    #[test]
    fn an_unreadable_file_is_a_lock_read_failure() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("skills-lock.json");
        std::fs::write(&path, lock_with(entry())).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        let result = ProjectLockFile::read(&path);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        if std::fs::read_to_string(&path).is_ok() {
            // Elevated permissions: the read succeeds, so there is no error to assert.
            return;
        }
        let error = result.unwrap_err();
        assert_eq!(error.code, ErrorCode::LockReadFailed);
    }
}
