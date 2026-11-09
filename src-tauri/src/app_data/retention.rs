use crate::{
    error::AppError,
    interrupted::{MarkerRead, MarkerStore},
    paths::AppPaths,
    tasks::{TaskOverall, TaskResult, TaskSkillStatus},
};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Only successful, settled snapshots from this operation are candidates.
/// Old snapshots and snapshots shared with a failed target are never swept.
pub(crate) fn finish_successful_backups(
    paths: &AppPaths,
    result: &mut TaskResult,
    snapshots: &[String],
) {
    if !paths.data_preferences().operation_logging {
        result.log_path = None;
    }
    if paths.data_preferences().retain_backups {
        return;
    }
    let mut candidates = BTreeSet::new();
    let mut failed = BTreeSet::new();
    for row in &result.skills {
        if let Some(path) = &row.backup_path {
            if matches!(
                row.status,
                TaskSkillStatus::Installed | TaskSkillStatus::Updated | TaskSkillStatus::Removed
            ) {
                candidates.insert(path.clone());
            } else {
                failed.insert(path.clone());
            }
        }
    }
    if let Some(batch) = &result.batch {
        for row in &batch.rows {
            if let Some(path) = &row.backup_path {
                if row.status == crate::tasks::batch::BatchStatus::Success {
                    candidates.insert(path.clone());
                } else {
                    failed.insert(path.clone());
                }
            }
        }
    }
    // Standalone cancellation cannot establish that verification completed.
    if result.batch.is_none() && result.overall == TaskOverall::Cancelled {
        return;
    }
    if result.overall == TaskOverall::Success {
        candidates.extend(snapshots.iter().map(|p| paths.display(Path::new(p))));
        candidates.extend(result.backup_path.iter().cloned());
    }
    let mut removed = BTreeSet::new();
    for candidate in candidates.difference(&failed) {
        match remove_successful_snapshot(paths, candidate) {
            Ok(true) => {
                removed.insert(candidate.clone());
            }
            Ok(false) => {}
            Err(e) => result.warnings.push(e),
        }
    }
    for row in &mut result.skills {
        if row
            .backup_path
            .as_ref()
            .is_some_and(|p| removed.contains(p))
        {
            row.backup_path = None;
        }
    }
    if let Some(batch) = &mut result.batch {
        for row in &mut batch.rows {
            if row
                .backup_path
                .as_ref()
                .is_some_and(|p| removed.contains(p))
            {
                row.backup_path = None;
            }
        }
    }
    if result
        .backup_path
        .as_ref()
        .is_some_and(|p| removed.contains(p))
    {
        result.backup_path = result
            .skills
            .iter()
            .rev()
            .find_map(|r| r.backup_path.clone())
            .or_else(|| {
                result
                    .batch
                    .as_ref()
                    .and_then(|b| b.rows.iter().rev().find_map(|r| r.backup_path.clone()))
            });
    }
}

pub(crate) fn remove_successful_snapshot(
    paths: &AppPaths,
    display: &str,
) -> Result<bool, AppError> {
    if paths.data_preferences().retain_backups {
        return Ok(false);
    }
    let path = if let Some(relative) = display.strip_prefix("~/") {
        paths.home().join(relative)
    } else {
        PathBuf::from(display)
    };
    if !path.starts_with(paths.backups_dir()) {
        return Err(super::error("备份路径不在应用备份目录中，已保留。"));
    }
    match MarkerStore::new(paths).read() {
        MarkerRead::Corrupt(_) => return Ok(false),
        MarkerRead::Present(marker)
            if marker.snapshot_paths.iter().any(|p| Path::new(p) == path) =>
        {
            return Ok(false);
        }
        _ => {}
    }
    super::checked_remove(paths, &path)
        .map_err(|e| super::error(format!("操作已完成，但备份未能自动删除：{}", e.message)))?;
    Ok(true)
}
