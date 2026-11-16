use super::Coordinate;
use crate::{error::AppError, lock::LockEntry};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub const FILE_NAME: &str = ".skills-box-marketplaces.json";
pub fn path(lock: &Path) -> PathBuf {
    lock.with_file_name(FILE_NAME)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Installation {
    pub name: String,
    pub coordinate: Coordinate,
    pub version: String,
    pub content_hash: String,
    pub installed_at: String,
    pub updated_at: String,
    /// ClawHub-only provenance. Diagnostic only: update checks compare
    /// `version`, never these fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<ClawhubDiagnostics>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClawhubDiagnostics {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
}
impl ClawhubDiagnostics {
    fn validate(&self) -> Result<(), AppError> {
        let corrupt = |detail: &str| Err(AppError::lock_corrupt(detail.to_string()));
        if let Some(repo) = &self.repo {
            let valid = repo.len() <= 300
                && repo.split_once('/').is_some_and(|(owner, name)| {
                    !owner.is_empty()
                        && !name.is_empty()
                        && !name.contains('/')
                        && [owner, name].iter().all(|part| {
                            !part.starts_with(['.', '-'])
                                && part.len() <= 100
                                && part
                                    .bytes()
                                    .all(|c| c.is_ascii_alphanumeric() || b"-_. ".contains(&c))
                        })
                });
            if !valid {
                return corrupt("ClawHub 诊断仓库无效");
            }
        }
        if let Some(path) = &self.path {
            let valid = !path.is_empty()
                && path.len() <= 400
                && !path.starts_with('/')
                && !path.contains(['\\', ':'])
                && path.split('/').all(|part| {
                    !part.is_empty()
                        && part != "."
                        && part != ".."
                        && !part.bytes().any(|c| c.is_ascii_control())
                });
            if !valid {
                return corrupt("ClawHub 诊断子路径无效");
            }
        }
        if self.commit.as_deref().is_some_and(|v| !super::is_commit(v)) {
            return corrupt("ClawHub 诊断提交必须是 40 位哈希");
        }
        if let Some(hash) = &self.content_hash
            && (hash.len() != 64
                || !hash
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)))
        {
            return corrupt("ClawHub 诊断内容哈希无效");
        }
        Ok(())
    }
}
impl Installation {
    pub fn entry(&self) -> LockEntry {
        LockEntry {
            name: self.name.clone(),
            source: self.coordinate.canonical(),
            source_type: self.coordinate.source.key().into(),
            source_url: self.coordinate.url(),
            reference: Some(self.version.clone()),
            skill_path: Some("SKILL.md".into()),
            skill_folder_hash: self.content_hash.clone(),
            installed_at: Some(self.installed_at.clone()),
            updated_at: Some(self.updated_at.clone()),
            plugin_name: None,
            source_base_url: None,
            well_known_digest: None,
        }
    }
}
#[cfg(test)]
impl Installation {
    /// Minimal valid record for tests that only care about identity and
    /// lifecycle; no ClawHub diagnostics.
    pub(crate) fn fixture(name: &str, coordinate: Coordinate, version: &str) -> Self {
        Self {
            name: name.into(),
            coordinate,
            version: version.into(),
            content_hash: "hash".into(),
            installed_at: "now".into(),
            updated_at: "now".into(),
            diagnostics: None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Records {
    version: u32,
    pub skills: BTreeMap<String, Installation>,
}
impl Default for Records {
    fn default() -> Self {
        Self {
            version: 1,
            skills: BTreeMap::new(),
        }
    }
}
impl Records {
    pub fn read(lock: &Path) -> Result<Self, AppError> {
        Self::read_inner(lock).map_err(|error| {
            error.hint(format!(
                "请检查市场安装记录 {}，或从备份恢复后再试。",
                path(lock).display()
            ))
        })
    }
    fn read_inner(lock: &Path) -> Result<Self, AppError> {
        let file = path(lock);
        if std::fs::symlink_metadata(&file).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(AppError::lock_corrupt("市场安装记录不能是符号链接"));
        }
        let raw = match std::fs::read(&file) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(AppError::lock_read_failed(e.to_string())),
        };
        let records: Self = serde_json::from_slice(&raw)
            .map_err(|e| AppError::lock_corrupt(format!("市场安装记录：{e}")))?;
        records.validate()?;
        Ok(records)
    }
    fn validate(&self) -> Result<(), AppError> {
        if self.version != 1 {
            return Err(AppError::lock_corrupt("不支持的市场安装记录版本"));
        }
        let mut coordinates = std::collections::BTreeSet::new();
        for (name, record) in &self.skills {
            if name != &record.name || record.content_hash.is_empty() {
                return Err(AppError::lock_corrupt("市场安装记录身份不一致"));
            }
            crate::tasks::install::validate_skill_name(name)
                .map_err(|e| AppError::lock_corrupt(e.to_string()))?;
            record
                .coordinate
                .validate()
                .and_then(|()| super::validate_reference(record.coordinate.source, &record.version))
                .map_err(|e| AppError::lock_corrupt(e.to_string()))?;
            if let Some(diagnostics) = &record.diagnostics {
                if record.coordinate.source != super::MarketplaceSource::Clawhub {
                    return Err(AppError::lock_corrupt("非 ClawHub 记录不能携带诊断信息"));
                }
                diagnostics.validate()?;
            }
            if !coordinates.insert((
                serde_json::to_string(&record.coordinate.source).unwrap(),
                record.coordinate.canonical(),
            )) {
                return Err(AppError::lock_corrupt("同一市场坐标存在重复安装记录"));
            }
        }
        Ok(())
    }
    pub fn save(&self, lock: &Path) -> Result<(), AppError> {
        self.validate()?;
        Self::read(lock)?;
        let file = path(lock);
        std::fs::create_dir_all(file.parent().unwrap())
            .map_err(|e| AppError::lock_read_failed(e.to_string()))?;
        let bytes =
            serde_json::to_vec_pretty(self).map_err(|e| AppError::lock_corrupt(e.to_string()))?;
        crate::fsutil::write_atomic(&file, &bytes)
            .map_err(|e| AppError::lock_read_failed(e.to_string()))
    }
    pub fn remove(lock: &Path, name: &str) -> Result<(), AppError> {
        let mut records = Self::read(lock)?;
        records.skills.remove(name);
        records.save(lock)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::marketplace::MarketplaceSource;

    fn skillhub_record(name: &str) -> Installation {
        Installation::fixture(
            name,
            Coordinate::from_canonical("@owner/slug").unwrap(),
            "1.0.0",
        )
    }

    fn clawhub_record(name: &str) -> Installation {
        let mut record = Installation::fixture(
            name,
            Coordinate::from_canonical_for(MarketplaceSource::Clawhub, "@aws/agents-build")
                .unwrap(),
            "b4416dddac9b6a0cc5412136e6dbf8f07ffdb31c",
        );
        record.diagnostics = Some(ClawhubDiagnostics {
            repo: Some("aws/agent-toolkit-for-aws".into()),
            path: Some("plugins/aws-agents/skills/agents-build".into()),
            commit: Some("b4416dddac9b6a0cc5412136e6dbf8f07ffdb31c".into()),
            content_hash: Some(
                "d79709bf46ca8cd0fffe392a914cd56539b4b7c0ff0fe34c9583f24057300195".into(),
            ),
        });
        record
    }

    #[test]
    fn old_skillhub_records_load_and_serialize_without_new_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = tmp.path().join("skills-lock.json");
        let original = serde_json::json!({
            "version": 1,
            "skills": {
                "original": {
                    "name": "original",
                    "coordinate": { "source": "skillhub", "namespace": "owner", "slug": "slug" },
                    "version": "1.0.0",
                    "contentHash": "hash",
                    "installedAt": "now",
                    "updatedAt": "now"
                }
            }
        });
        std::fs::write(path(&lock), serde_json::to_vec(&original).unwrap()).unwrap();
        let records = Records::read(&lock).unwrap();
        assert_eq!(records.skills["original"], skillhub_record("original"));
        assert_eq!(serde_json::to_value(&records).unwrap(), original);
    }

    #[test]
    fn clawhub_records_round_trip_source_url_reference_and_diagnostics() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = tmp.path().join("skills-lock.json");
        let record = clawhub_record("agents-build");
        let mut records = Records::default();
        records.skills.insert(record.name.clone(), record.clone());
        records.save(&lock).unwrap();
        assert_eq!(Records::read(&lock).unwrap().skills["agents-build"], record);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(path(&lock)).unwrap())
                .unwrap()["skills"]["agents-build"]["diagnostics"]["commit"],
            "b4416dddac9b6a0cc5412136e6dbf8f07ffdb31c"
        );
        assert_eq!(
            record.coordinate.url(),
            "https://clawhub.ai/aws/skills/agents-build"
        );
        let entry = record.entry();
        assert_eq!(entry.source, "@aws/agents-build");
        assert_eq!(entry.source_type, "clawhub");
        assert_eq!(
            entry.source_url,
            "https://clawhub.ai/aws/skills/agents-build"
        );
        assert_eq!(entry.reference.as_deref(), Some(record.version.as_str()));
    }

    #[test]
    fn corrupt_or_unknown_clawhub_records_are_rejected_without_overwriting() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = tmp.path().join("skills-lock.json");
        let record = clawhub_record("agents-build");
        let mut records = Records::default();
        records.skills.insert(record.name.clone(), record.clone());
        records.save(&lock).unwrap();
        let valid = records;
        for change in [
            "unknown-source",
            "short-reference",
            "invalid-diagnostics-commit",
            "invalid-diagnostics-path",
        ] {
            let mut value = serde_json::to_value(&valid).unwrap();
            match change {
                "unknown-source" => {
                    value["skills"]["agents-build"]["coordinate"]["source"] = "other".into()
                }
                "short-reference" => value["skills"]["agents-build"]["version"] = "b4416ddd".into(),
                "invalid-diagnostics-commit" => {
                    value["skills"]["agents-build"]["diagnostics"]["commit"] = "not-a-commit".into()
                }
                _ => value["skills"]["agents-build"]["diagnostics"]["path"] = "../escape".into(),
            }
            let bytes = serde_json::to_vec(&value).unwrap();
            std::fs::write(path(&lock), &bytes).unwrap();
            assert!(Records::read(&lock).is_err(), "{change}");
            assert!(valid.save(&lock).is_err(), "{change}");
            assert_eq!(std::fs::read(path(&lock)).unwrap(), bytes, "{change}");
        }
    }

    #[test]
    fn same_coordinate_can_coexist_across_sources() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = tmp.path().join("skills-lock.json");
        let mut records = Records::default();
        let mut skillhub = skillhub_record("skillhub-copy");
        skillhub.coordinate =
            Coordinate::from_canonical_for(MarketplaceSource::Skillhub, "@owner/slug").unwrap();
        let mut clawhub = clawhub_record("clawhub-copy");
        clawhub.coordinate =
            Coordinate::from_canonical_for(MarketplaceSource::Clawhub, "@owner/slug").unwrap();
        records.skills.insert("skillhub-copy".into(), skillhub);
        records.skills.insert("clawhub-copy".into(), clawhub);
        records.save(&lock).unwrap();
        assert_eq!(Records::read(&lock).unwrap().skills.len(), 2);
    }

    #[test]
    fn marketplace_records_reject_corruption_and_symlinks_without_overwriting_git_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = tmp.path().join("skills-lock.json");
        let git = r#"{"version":1,"skills":{}}"#;
        std::fs::write(&lock, git).unwrap();
        let record = skillhub_record("original");
        let mut records = Records::default();
        records.skills.insert(record.name.clone(), record.clone());
        records.save(&lock).unwrap();
        for change in [
            "schema",
            "identity",
            "coordinate",
            "version",
            "hash",
            "duplicate",
        ] {
            let mut value = serde_json::to_value(&records).unwrap();
            match change {
                "schema" => value["version"] = 2.into(),
                "identity" => value["skills"]["original"]["name"] = "other".into(),
                "coordinate" => {
                    value["skills"]["original"]["coordinate"]["slug"] = "../escape".into()
                }
                "version" => value["skills"]["original"]["version"] = "latest".into(),
                "hash" => value["skills"]["original"]["contentHash"] = "".into(),
                _ => {
                    value["skills"]["second"] = value["skills"]["original"].clone();
                    value["skills"]["second"]["name"] = "second".into();
                }
            }
            let bytes = serde_json::to_vec(&value).unwrap();
            std::fs::write(path(&lock), &bytes).unwrap();
            assert!(Records::read(&lock).is_err(), "{change}");
            assert!(records.save(&lock).is_err());
            assert_eq!(std::fs::read(path(&lock)).unwrap(), bytes);
        }
        std::fs::remove_file(path(&lock)).unwrap();
        std::os::unix::fs::symlink(&lock, path(&lock)).unwrap();
        assert!(Records::read(&lock).is_err());
        assert!(records.save(&lock).is_err());
        assert_eq!(std::fs::read_to_string(&lock).unwrap(), git);
    }
}
