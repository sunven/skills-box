use super::Snapshot;
use crate::{error::AppError, lock::LockEntry};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub const FILE_NAME: &str = ".skills-box-packs.json";
pub fn path(lock: &Path) -> PathBuf {
    lock.with_file_name(FILE_NAME)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowInstallation {
    pub name: String,
    pub slug: String,
    pub markdown: String,
    pub content_hash: String,
    pub installed_at: String,
}
impl WorkflowInstallation {
    pub fn entry(&self) -> LockEntry {
        LockEntry {
            name: self.name.clone(),
            source: self.slug.clone(),
            source_type: "skillhub-pack".into(),
            source_url: super::url(&self.slug),
            reference: None,
            skill_path: Some("SKILL.md".into()),
            skill_folder_hash: self.content_hash.clone(),
            installed_at: Some(self.installed_at.clone()),
            updated_at: Some(self.installed_at.clone()),
            plugin_name: None,
            source_base_url: None,
            well_known_digest: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Records {
    version: u32,
    pub packs: BTreeMap<String, Snapshot>,
    pub workflows: BTreeMap<String, WorkflowInstallation>,
}
impl Default for Records {
    fn default() -> Self {
        Self {
            version: 1,
            packs: BTreeMap::new(),
            workflows: BTreeMap::new(),
        }
    }
}
impl Records {
    pub fn read(lock: &Path) -> Result<Self, AppError> {
        let path = path(lock);
        if std::fs::symlink_metadata(&path)
            .is_ok_and(|m| !m.is_file() || m.file_type().is_symlink())
        {
            return Err(AppError::lock_corrupt("专家包记录必须是普通文件"));
        }
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(AppError::lock_read_failed(e.to_string())),
        };
        let result: Self = serde_json::from_slice(&bytes)
            .map_err(|e| AppError::lock_corrupt(format!("专家包记录：{e}")))?;
        result.validate()?;
        Ok(result)
    }
    fn validate(&self) -> Result<(), AppError> {
        if self.version != 1 {
            return Err(AppError::lock_corrupt("不支持的专家包记录版本"));
        }
        for (slug, snapshot) in &self.packs {
            if slug != &snapshot.slug {
                return Err(AppError::lock_corrupt("专家包记录身份不一致"));
            }
            snapshot
                .validate()
                .map_err(|e| AppError::lock_corrupt(e.to_string()))?;
        }
        for (name, record) in &self.workflows {
            super::validate_slug(&record.slug)
                .map_err(|e| AppError::lock_corrupt(e.to_string()))?;
            if name != &record.name
                || record.content_hash.is_empty()
                || crate::marketplace::original_name(&record.markdown)
                    .ok()
                    .as_ref()
                    != Some(name)
            {
                return Err(AppError::lock_corrupt("专家包工作流记录身份不一致"));
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
        crate::fsutil::write_atomic(
            &file,
            &serde_json::to_vec_pretty(self).map_err(|e| AppError::lock_corrupt(e.to_string()))?,
        )
        .map_err(|e| AppError::lock_read_failed(e.to_string()))
    }
    pub fn remove_workflow(lock: &Path, name: &str) -> Result<(), AppError> {
        let mut records = Self::read(lock)?;
        records.workflows.remove(name);
        records.save(lock)
    }
}
