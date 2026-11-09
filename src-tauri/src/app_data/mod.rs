//! App-owned files only. Deletion consumes a bounded, server-owned preview and
//! revalidates identities, protected snapshots and paths before touching disk.
pub(crate) mod coordination;
mod retention;
#[cfg(test)]
mod tests;
use crate::{
    error::{AppError, ErrorCode},
    interrupted::{MarkerRead, MarkerStore},
    paths::AppPaths,
    settings::DataPreferences,
};
pub(crate) use retention::{finish_successful_backups, remove_successful_snapshot};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::os::unix::fs::MetadataExt;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

pub(crate) fn error(message: impl Into<String>) -> AppError {
    AppError {
        code: ErrorCode::AppDataFailed,
        message: message.into(),
        detail: None,
        hint: None,
        retryable: true,
    }
}
pub(crate) fn busy() -> AppError {
    error("有任务正在使用应用数据，请完成后重试清理。")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    Backups,
    Logs,
    Cache,
    Preserved,
    Other,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub category: Category,
    pub bytes: u64,
    pub cleanable_bytes: u64,
    pub count: usize,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupItem {
    pub id: String,
    pub name: String,
    pub action: Option<String>,
    pub scope: Option<String>,
    pub created_at_epoch_ms: Option<u64>,
    pub bytes: u64,
    pub protected_reason: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub directory: String,
    pub preferences: DataPreferences,
    pub total_bytes: u64,
    pub cleanable_bytes: u64,
    pub categories: Vec<Usage>,
    pub backups: Vec<BackupItem>,
    pub warnings: Vec<String>,
    pub scanned_at_epoch_ms: u64,
}
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    #[serde(default)]
    pub item_ids: Option<Vec<String>>,
    #[serde(default)]
    pub backup_ids: Vec<String>,
    #[serde(default)]
    pub all_backups: bool,
    #[serde(default)]
    pub logs: bool,
    #[serde(default)]
    pub cache: bool,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewItem {
    pub id: String,
    pub label: String,
    pub category: Category,
    pub bytes: u64,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub id: String,
    pub items: Vec<PreviewItem>,
    pub total_bytes: u64,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupItem {
    pub id: String,
    pub label: String,
    pub freed_bytes: u64,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupResult {
    pub items: Vec<CleanupItem>,
    pub freed_bytes: u64,
}
#[derive(Clone)]
struct Entry {
    view: PreviewItem,
    path: PathBuf,
    fingerprint: String,
    protected: Option<String>,
}
struct Scan {
    report: Report,
    entries: Vec<Entry>,
}
struct Plan {
    entries: Vec<Entry>,
    created: u64,
}
#[derive(Default)]
pub struct DataService {
    plans: Mutex<BTreeMap<String, Plan>>,
}

impl DataService {
    pub fn report(&self, paths: &AppPaths) -> Result<Report, AppError> {
        Ok(scan(paths)?.report)
    }
    pub fn preview(&self, paths: &AppPaths, selection: Selection) -> Result<Preview, AppError> {
        let scan = scan(paths)?;
        let ids: BTreeSet<_> = selection.backup_ids.iter().cloned().collect();
        if ids.iter().any(|id| {
            !scan
                .entries
                .iter()
                .any(|e| e.view.id == *id && e.view.category == Category::Backups)
        }) {
            return Err(error("所选备份已变化，请刷新后重新选择。"));
        }
        let mut entries = Vec::new();
        for e in scan.entries {
            let selected = if let Some(items) = &selection.item_ids {
                items.contains(&e.view.id)
                    && matches!(
                        e.view.category,
                        Category::Backups | Category::Logs | Category::Cache
                    )
            } else {
                match e.view.category {
                    Category::Backups => selection.all_backups || ids.contains(&e.view.id),
                    Category::Logs => selection.logs,
                    Category::Cache => selection.cache,
                    _ => false,
                }
            };
            if selected && e.protected.is_none() {
                entries.push(e);
            } else if ids.contains(&e.view.id) {
                return Err(error("所选备份受保护，暂不可清理。"));
            }
        }
        if entries.is_empty() {
            return Err(error("所选类别没有可清理的文件。"));
        }
        let now = crate::time::now_ms();
        let id = uuid::Uuid::new_v4().to_string();
        let preview = Preview {
            id: id.clone(),
            total_bytes: entries.iter().map(|e| e.view.bytes).sum(),
            items: entries.iter().map(|e| e.view.clone()).collect(),
        };
        let mut plans = self.plans.lock().unwrap_or_else(|p| p.into_inner());
        plans.retain(|_, p| now.saturating_sub(p.created) < 600_000);
        if plans.len() >= 16 {
            plans.clear();
        }
        plans.insert(
            id,
            Plan {
                entries,
                created: now,
            },
        );
        Ok(preview)
    }
    /// Caller owns the mutation permit and exclusive data coordination guard
    /// inside a blocking worker, even if the requesting IPC future disappears.
    pub fn clean(&self, paths: &AppPaths, id: &str) -> Result<CleanupResult, AppError> {
        let plan = self
            .plans
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(id)
            .ok_or_else(|| error("清理预览已失效，请重新预览。"))?;
        if crate::time::now_ms().saturating_sub(plan.created) >= 600_000 {
            return Err(error("清理预览已过期，请重新预览。"));
        }
        let current = scan(paths)?;
        // Validate the entire confirmed set first. Never silently expand it to
        // files created after the user saw the preview.
        for entry in &plan.entries {
            if !current.entries.iter().any(|e| {
                e.view.id == entry.view.id
                    && e.fingerprint == entry.fingerprint
                    && e.protected.is_none()
            }) {
                return Err(error("文件或保护状态已变化，请重新预览后确认清理。"));
            }
        }
        let mut result = CleanupResult {
            items: Vec::new(),
            freed_bytes: 0,
        };
        for entry in plan.entries {
            let removed = match measure(&entry.path) {
                Ok((_, fingerprint)) if fingerprint == entry.fingerprint => {
                    checked_remove(paths, &entry.path)
                }
                _ => Err(error("文件在清理期间发生变化，已跳过，请重新预览。")),
            };
            let remaining = measure(&entry.path)
                .map(|m| m.0)
                .unwrap_or(entry.view.bytes);
            let freed = entry.view.bytes.saturating_sub(remaining);
            result.freed_bytes += freed;
            result.items.push(CleanupItem {
                id: entry.view.id,
                label: entry.view.label,
                freed_bytes: freed,
                error: removed.err().map(|e| e.message),
            });
        }
        Ok(result)
    }
    pub fn backup_directory(&self, paths: &AppPaths, id: &str) -> Result<PathBuf, AppError> {
        let entry = scan(paths)?
            .entries
            .into_iter()
            .find(|e| e.view.id == id && e.view.category == Category::Backups)
            .ok_or_else(|| error("备份不存在，请刷新列表。"))?;
        validate_path(paths, &entry.path)?;
        if !real_dir(&entry.path) {
            return Err(error("备份目录已变化，请刷新列表。"));
        }
        Ok(entry.path)
    }
}

fn scan(paths: &AppPaths) -> Result<Scan, AppError> {
    let mut scan = Scan {
        report: Report {
            directory: paths.display(paths.app_data()),
            preferences: paths.data_preferences(),
            total_bytes: 0,
            cleanable_bytes: 0,
            categories: [
                Category::Backups,
                Category::Logs,
                Category::Cache,
                Category::Preserved,
                Category::Other,
            ]
            .into_iter()
            .map(|category| Usage {
                category,
                bytes: 0,
                cleanable_bytes: 0,
                count: 0,
            })
            .collect(),
            backups: Vec::new(),
            warnings: Vec::new(),
            scanned_at_epoch_ms: crate::time::now_ms(),
        },
        entries: Vec::new(),
    };
    match fs::symlink_metadata(paths.app_data()) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(scan),
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
        _ => return Err(error("应用数据目录无法读取或是符号链接。")),
    }
    let marker = MarkerStore::new(paths).read();
    for path in children(paths.app_data(), &mut scan.report.warnings) {
        let name = path.file_name().unwrap().to_string_lossy();
        match name.as_ref() {
            "backups" if real_dir(&path) => {
                for child in children(&path, &mut scan.report.warnings) {
                    if child
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with("project-")
                        && real_dir(&child)
                    {
                        for backup in children(&child, &mut scan.report.warnings) {
                            add_backup(&mut scan, paths, backup, &marker);
                        }
                    } else {
                        add_backup(&mut scan, paths, child, &marker);
                    }
                }
            }
            "logs" | "cache" if real_dir(&path) => {
                let category = if name == "logs" {
                    Category::Logs
                } else {
                    Category::Cache
                };
                for child in children(&path, &mut scan.report.warnings) {
                    let n = child.file_name().unwrap().to_string_lossy();
                    let known = if category == Category::Logs {
                        is_log_name(&n)
                    } else {
                        (n.starts_with("update-tree-") && n.ends_with(".json")) || n == "archives"
                    };
                    add_entry(
                        &mut scan,
                        paths,
                        child,
                        if known { category } else { Category::Other },
                        if known {
                            None
                        } else {
                            Some("其他文件，仅统计".into())
                        },
                    );
                }
            }
            "settings.json" | "agents.json" | "projects.json" | "discovery.json"
            | "update-check.json" | "baselines" | "runtime" | "cli-cwd" => {
                add_entry(
                    &mut scan,
                    paths,
                    path,
                    Category::Preserved,
                    Some("应用运行所需数据".into()),
                );
            }
            _ => {
                add_entry(
                    &mut scan,
                    paths,
                    path,
                    Category::Other,
                    Some("其他文件，仅统计".into()),
                );
            }
        }
    }
    scan.report.backups.sort_by(|a, b| {
        b.created_at_epoch_ms
            .cmp(&a.created_at_epoch_ms)
            .then(a.id.cmp(&b.id))
    });
    Ok(scan)
}
fn is_log_name(n: &str) -> bool {
    n == "operations.jsonl" || (1..=4).any(|i| n == format!("operations.{i}.jsonl"))
}
fn real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
}
fn children(path: &Path, warnings: &mut Vec<String>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    match fs::read_dir(path) {
        Ok(entries) => {
            for e in entries {
                match e {
                    Ok(e) => out.push(e.path()),
                    Err(_) => warnings.push(format!("部分文件无法读取：{}", path.display())),
                }
            }
        }
        Err(_) => warnings.push(format!("无法统计目录：{}", path.display())),
    }
    out.sort();
    out
}
fn add_backup(scan: &mut Scan, paths: &AppPaths, path: PathBuf, marker: &MarkerRead) {
    // Finder metadata and other loose files are not snapshots. Count them as
    // other app data without exposing a backup-open or deletion action.
    if !real_dir(&path) {
        add_entry(
            scan,
            paths,
            path,
            Category::Other,
            Some("其他文件，仅统计".into()),
        );
        return;
    }
    let manifest = path.join(crate::backup::BACKUP_MANIFEST_FILE);
    let raw = if fs::symlink_metadata(&manifest).is_ok_and(|m| m.is_file() && m.len() < 1_048_576) {
        fs::read(&manifest).ok()
    } else {
        None
    };
    let value = raw.and_then(|v| serde_json::from_slice::<serde_json::Value>(&v).ok());
    let action = value
        .as_ref()
        .and_then(|v| v["action"].as_str())
        .map(str::to_string);
    let scope = value
        .as_ref()
        .and_then(|v| v["scope"].as_str())
        .map(str::to_string);
    let name = value
        .as_ref()
        .and_then(|v| v["skill"].as_str())
        .unwrap_or_else(|| path.file_name().unwrap().to_str().unwrap_or("未知备份"))
        .to_string();
    let created = value.as_ref().and_then(|v| v["createdAtEpochMs"].as_u64());
    let reason = match marker {
        MarkerRead::Corrupt(_) => Some("中断记录无法读取，暂时保护全部备份".into()),
        MarkerRead::Present(m) if m.snapshot_paths.iter().any(|p| Path::new(p) == path) => {
            Some("运行中或中断操作正在引用".into())
        }
        _ if action.is_none() || created.is_none() => Some("备份信息不完整，请打开目录检查".into()),
        _ => None,
    };
    if let Some(entry) = add_entry(scan, paths, path, Category::Backups, reason) {
        scan.report.backups.push(BackupItem {
            id: entry.view.id,
            name,
            action,
            scope,
            created_at_epoch_ms: created,
            bytes: entry.view.bytes,
            protected_reason: entry.protected,
        });
    }
}
fn add_entry(
    scan: &mut Scan,
    paths: &AppPaths,
    path: PathBuf,
    category: Category,
    mut protected: Option<String>,
) -> Option<Entry> {
    let (bytes, fingerprint) = match measure(&path) {
        Ok(value) => value,
        Err(e) => {
            scan.report.warnings.push(e.message);
            return None;
        }
    };
    if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        protected = Some("符号链接不参与清理".into());
        scan.report
            .warnings
            .push(format!("未跟随符号链接：{}", paths.display(&path)));
    }
    let relative = path
        .strip_prefix(paths.app_data())
        .unwrap()
        .to_string_lossy();
    let id = format!("{:x}", Sha256::digest(relative.as_bytes()));
    let entry = Entry {
        view: PreviewItem {
            id,
            label: relative.to_string(),
            category,
            bytes,
        },
        path,
        fingerprint,
        protected,
    };
    let usage = scan
        .report
        .categories
        .iter_mut()
        .find(|u| u.category == category)
        .unwrap();
    usage.bytes += bytes;
    usage.count += 1;
    scan.report.total_bytes += bytes;
    if entry.protected.is_none() {
        usage.cleanable_bytes += bytes;
        scan.report.cleanable_bytes += bytes;
    }
    scan.entries.push(entry.clone());
    Some(entry)
}
fn measure(path: &Path) -> Result<(u64, String), AppError> {
    if !path
        .try_exists()
        .map_err(|_| error("无法读取文件元数据。"))?
        && fs::symlink_metadata(path).is_err()
    {
        return Ok((0, String::new()));
    }
    let mut bytes = 0u64;
    let mut hash = Sha256::new();
    for entry in walkdir::WalkDir::new(path)
        .follow_links(false)
        .follow_root_links(false)
        .sort_by_file_name()
    {
        let entry = entry.map_err(|_| error(format!("无法完整统计：{}", path.display())))?;
        let m = fs::symlink_metadata(entry.path())
            .map_err(|_| error(format!("无法统计：{}", entry.path().display())))?;
        hash.update(
            entry
                .path()
                .strip_prefix(path)
                .unwrap()
                .as_os_str()
                .as_encoded_bytes(),
        );
        for n in [
            m.dev(),
            m.ino(),
            m.len(),
            m.mtime() as u64,
            m.mtime_nsec() as u64,
            m.ctime() as u64,
            m.ctime_nsec() as u64,
            m.mode() as u64,
        ] {
            hash.update(n.to_le_bytes());
        }
        if m.is_file() || m.file_type().is_symlink() {
            bytes = bytes.saturating_add(m.len());
        }
    }
    Ok((bytes, format!("{:x}", hash.finalize())))
}
pub(crate) fn validate_path(paths: &AppPaths, path: &Path) -> Result<(), AppError> {
    let relative = path
        .strip_prefix(paths.app_data())
        .map_err(|_| error("文件不在应用数据目录中。"))?;
    if relative.components().count() < 2
        || relative
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(error("应用数据路径无效。"));
    }
    let mut current = paths.app_data().to_path_buf();
    if !real_dir(&current) {
        return Err(error("应用数据目录已变化。"));
    }
    for part in relative.components() {
        current.push(part);
        let m = fs::symlink_metadata(&current).map_err(|_| error("文件已变化，请重新预览。"))?;
        if m.file_type().is_symlink() || (!m.is_dir() && current != path) {
            return Err(error("路径包含符号链接或不再是目录，已跳过。"));
        }
    }
    Ok(())
}
pub(crate) fn checked_remove(paths: &AppPaths, path: &Path) -> Result<(), AppError> {
    validate_path(paths, path)?;
    let result = if real_dir(path) {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    result.map_err(|e| error(format!("无法删除 {}：{e}", paths.display(path))))
}
