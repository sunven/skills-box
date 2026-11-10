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
}
impl Installation {
    pub fn entry(&self) -> LockEntry {
        LockEntry {
            name: self.name.clone(),
            source: self.coordinate.canonical(),
            source_type: "skillhub".into(),
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
                .and_then(|()| super::validate_version(&record.version))
                .map_err(|e| AppError::lock_corrupt(e.to_string()))?;
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
    #[test]
    fn marketplace_records_reject_corruption_and_symlinks_without_overwriting_git_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = tmp.path().join("skills-lock.json");
        let git = r#"{"version":1,"skills":{}}"#;
        std::fs::write(&lock, git).unwrap();
        let record = Installation {
            name: "original".into(),
            coordinate: Coordinate::from_canonical("@owner/slug").unwrap(),
            version: "1.0.0".into(),
            content_hash: "hash".into(),
            installed_at: "now".into(),
            updated_at: "now".into(),
        };
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
