//! Strict read-only model for `~/.agents/.skill-lock.json` (v3).
//!
//! The GUI never writes the lock. A missing lock is a normal fresh-machine
//! state and yields an empty entry set; malformed JSON, a missing `skills`
//! object, malformed entries, or a version other than 3 are rejected with
//! `lock.*` errors so the UI can refuse mutations instead of silently showing
//! an empty inventory.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockEntry {
    pub name: String,
    pub source: String,
    pub source_type: String,
    pub source_url: String,
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    pub skill_path: Option<String>,
    pub skill_folder_hash: String,
    pub installed_at: String,
    pub updated_at: String,
    pub plugin_name: Option<String>,
    pub source_base_url: Option<String>,
    pub well_known_digest: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LockFile {
    entries: BTreeMap<String, LockEntry>,
}

impl LockFile {
    pub fn read(path: &Path) -> Result<Self, AppError> {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => {
                return Err(AppError::lock_read_failed(error.to_string()));
            }
        };
        Self::parse(&raw)
    }

    pub fn parse(raw: &str) -> Result<Self, AppError> {
        let value: serde_json::Value =
            serde_json::from_str(raw).map_err(|error| AppError::lock_corrupt(error.to_string()))?;

        let Some(version) = value.get("version") else {
            return Err(AppError::lock_corrupt("缺少 version 字段"));
        };
        let Some(version) = version.as_u64() else {
            return Err(AppError::lock_corrupt("version 字段不是整数"));
        };
        if version != 3 {
            return Err(AppError::lock_unsupported_version(version));
        }

        let Some(skills) = value.get("skills") else {
            return Err(AppError::lock_corrupt("缺少 skills 字段"));
        };
        let Some(skills) = skills.as_object() else {
            return Err(AppError::lock_corrupt("skills 字段不是对象"));
        };

        let mut entries = BTreeMap::new();
        for (name, raw_entry) in skills {
            let entry = parse_entry(name, raw_entry)?;
            entries.insert(name.clone(), entry);
        }
        Ok(Self { entries })
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
        source_url: required("sourceUrl")?,
        reference: optional("ref")?,
        skill_path: optional("skillPath")?,
        skill_folder_hash: required("skillFolderHash")?,
        installed_at: required("installedAt")?,
        updated_at: required("updatedAt")?,
        plugin_name: optional("pluginName")?,
        source_base_url: optional("sourceBaseUrl")?,
        well_known_digest: optional("wellKnownDigest")?,
    })
}

fn entry_error(name: &str, detail: impl AsRef<str>) -> AppError {
    AppError::lock_corrupt(format!("skills.{name}: {}", detail.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::codes;

    fn full_entry() -> serde_json::Value {
        serde_json::json!({
            "source": "owner/repo",
            "sourceType": "github",
            "sourceUrl": "https://github.com/owner/repo.git",
            "skillPath": "skills/alpha/SKILL.md",
            "skillFolderHash": "0123456789abcdef0123456789abcdef01234567",
            "installedAt": "2026-01-02T03:04:05.000Z",
            "updatedAt": "2026-02-03T04:05:06.000Z"
        })
    }

    #[test]
    fn missing_lock_is_an_empty_lock_not_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = LockFile::read(&tmp.path().join(".skill-lock.json")).unwrap();
        assert!(lock.is_empty());
        assert_eq!(lock.len(), 0);
    }

    #[test]
    fn parses_v3_entries_and_optional_fields() {
        let raw = serde_json::json!({
            "version": 3,
            "skills": {
                "alpha": full_entry(),
                "beta": {
                    "source": "other/repo",
                    "sourceType": "github",
                    "sourceUrl": "https://github.com/other/repo.git",
                    "ref": "main",
                    "skillFolderHash": "",
                    "installedAt": "2026-01-01T00:00:00.000Z",
                    "updatedAt": "2026-01-02T00:00:00.000Z",
                    "pluginName": "core",
                    "unknownFutureField": true
                }
            },
            "dismissed": { "findSkillsPrompt": true }
        })
        .to_string();

        let lock = LockFile::parse(&raw).unwrap();
        assert_eq!(lock.len(), 2);
        let alpha = lock.get("alpha").unwrap();
        assert_eq!(alpha.source, "owner/repo");
        assert_eq!(alpha.skill_path.as_deref(), Some("skills/alpha/SKILL.md"));
        assert_eq!(alpha.updated_at, "2026-02-03T04:05:06.000Z");
        assert_eq!(alpha.reference, None);

        let beta = lock.get("beta").unwrap();
        assert_eq!(beta.reference.as_deref(), Some("main"));
        assert_eq!(beta.skill_folder_hash, "");
        assert_eq!(beta.plugin_name.as_deref(), Some("core"));
    }

    #[test]
    fn corrupt_json_maps_to_lock_corrupt() {
        let error = LockFile::parse("{not json").unwrap_err();
        assert_eq!(error.code, codes::LOCK_CORRUPT);
        assert!(error.detail.is_some());
    }

    #[test]
    fn non_v3_versions_are_rejected_including_newer_versions() {
        for version in [2, 4] {
            let raw = format!(r#"{{"version":{version},"skills":{{}}}}"#);
            let error = LockFile::parse(&raw).unwrap_err();
            assert_eq!(error.code, codes::LOCK_UNSUPPORTED_VERSION);
            assert!(error.message.contains(&format!("v{version}")));
        }
    }

    #[test]
    fn missing_or_non_numeric_version_is_corrupt() {
        for raw in [r#"{"skills":{}}"#, r#"{"version":"3","skills":{}}"#] {
            let error = LockFile::parse(raw).unwrap_err();
            assert_eq!(error.code, codes::LOCK_CORRUPT);
        }
    }

    #[test]
    fn missing_skills_or_non_object_skills_is_corrupt() {
        for raw in [r#"{"version":3}"#, r#"{"version":3,"skills":[]}"#] {
            let error = LockFile::parse(raw).unwrap_err();
            assert_eq!(error.code, codes::LOCK_CORRUPT);
        }
    }

    #[test]
    fn entries_missing_required_fields_are_corrupt_with_entry_context() {
        let mut entry = full_entry();
        entry.as_object_mut().unwrap().remove("sourceUrl");
        let raw = serde_json::json!({ "version": 3, "skills": { "alpha": entry } }).to_string();
        let error = LockFile::parse(&raw).unwrap_err();
        assert_eq!(error.code, codes::LOCK_CORRUPT);
        assert!(error.detail.as_deref().unwrap().contains("alpha"));
        assert!(error.detail.as_deref().unwrap().contains("sourceUrl"));
    }

    #[test]
    fn empty_hash_and_missing_optional_fields_are_accepted() {
        let mut entry = full_entry();
        entry["skillFolderHash"] = serde_json::json!("");
        let raw = serde_json::json!({ "version": 3, "skills": { "alpha": entry } }).to_string();
        let lock = LockFile::parse(&raw).unwrap();
        assert!(lock.get("alpha").unwrap().skill_folder_hash.is_empty());
    }
}
