//! Native removal of explicitly selected Agent attachments. The canonical
//! store, lock and baselines are never written by this action.
use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use super::batch::{self, BatchTarget, PlannedTarget};
use crate::backup::{BackupAction, BackupAttachment, BackupStore};
use crate::baseline::{BaselineStore, LocalModification, fingerprint_folder};
use crate::error::AppError;
use crate::inventory::{self, AttachmentLocation, AttachmentMode, LocatedSkill};
use crate::logs::{OperationEntry, OperationLog};
use crate::paths::AppPaths;
use crate::scope_layout::ScopeLayout;
use crate::security::normalize_absolute;

#[cfg(test)]
mod tests;

fn resolve(
    paths: &AppPaths,
    layout: &ScopeLayout,
    target: &BatchTarget,
) -> Result<(LocatedSkill, AttachmentLocation), AppError> {
    layout.ensure_mutation_ready()?;
    let name = layout.scope.parse_skill_id(&target.skill_id)?;
    super::install::validate_skill_name(name)?;
    let located = layout
        .locate(paths, name)?
        .ok_or_else(|| AppError::skill_not_found(&target.skill_id))?;
    let agent_id = target.agent_id.as_deref().unwrap_or_default();
    let agent = crate::attachment_target::find_agent(&layout.agents, agent_id)?;
    if inventory::same_dir(&agent.skills_dir, &layout.canonical_dir) {
        return Err(AppError::agent_native(&agent.display_name));
    }
    // Removing a shared physical entry would also affect an unselected alias.
    if layout.agents.iter().any(|other| {
        other.id != agent.id && inventory::same_dir(&other.skills_dir, &agent.skills_dir)
    }) {
        return Err(AppError::path_invalid(
            "该目录由多个 Agent 共用，无法独立移除。",
        ));
    }
    let attachment = located
        .attachments
        .iter()
        .find(|a| a.agent_id == agent_id)
        .ok_or_else(|| AppError::agent_not_attached(&agent.display_name))?
        .clone();
    let root = normalize_absolute(&agent.skills_dir)
        .ok_or_else(|| AppError::path_invalid("Agent 目录必须是绝对路径。"))?;
    let path = normalize_absolute(&attachment.path)
        .ok_or_else(|| AppError::path_invalid("挂载路径无效。"))?;
    if path.parent() != Some(root.as_path())
        || attachment.mode == AttachmentMode::Native
        || located.canonical_path.as_deref() == Some(path.as_path())
    {
        return Err(AppError::path_invalid(
            "移除目标必须是 Agent 目录内的独立挂载。",
        ));
    }
    let directory =
        fs::symlink_metadata(&root).map_err(|e| AppError::path_invalid(e.to_string()))?;
    if directory.file_type().is_symlink() || !directory.is_dir() {
        return Err(AppError::path_invalid(
            "Agent skills 目录不是独立目录或本身是符号链接。",
        ));
    }
    let metadata =
        fs::symlink_metadata(&path).map_err(|e| AppError::path_invalid(e.to_string()))?;
    if !metadata.file_type().is_symlink() && !metadata.is_dir() {
        return Err(AppError::path_invalid("挂载既不是符号链接也不是目录。"));
    }
    if let Some(project_root) = layout.project_root() {
        // A link's destination can be missing or outside the project: only its
        // entry is removed. A real directory must itself stay inside the root.
        let checked = if metadata.file_type().is_symlink() {
            &root
        } else {
            &path
        };
        crate::project_path::existing(project_root, checked)
            .map_err(|_| AppError::path_invalid("移除目标超出项目范围或无法读取。"))?;
    }
    Ok((located, attachment))
}

pub(super) fn inspect(
    paths: &AppPaths,
    layout: &ScopeLayout,
    mut target: BatchTarget,
) -> Result<PlannedTarget, AppError> {
    let (located, attachment) = resolve(paths, layout, &target)?;
    let parent = attachment.path.parent().expect("validated direct child");
    let physical_parent =
        fs::canonicalize(parent).map_err(|e| AppError::path_invalid(e.to_string()))?;
    let directory = fs::metadata(parent).map_err(|e| AppError::path_invalid(e.to_string()))?;
    let metadata = fs::symlink_metadata(&attachment.path)
        .map_err(|e| AppError::path_invalid(e.to_string()))?;
    target.deletes_copy = !metadata.file_type().is_symlink();
    target.agent_name = Some(attachment.agent_name);
    target.paths = vec![paths.display(&attachment.path)];
    target.eligible = true;
    if target.deletes_copy {
        // Assessment must not replace a canonical baseline with this copy's
        // contents merely because the user opened a deletion preview.
        if let Some(baseline) = BaselineStore::for_scope(paths, &layout.scope).load(&located.name)
            && let Ok(fingerprint) = fingerprint_folder(&attachment.path)
        {
            target.local_modification = if baseline.content_hash != fingerprint.content_hash {
                LocalModification::Modified
            } else if baseline.verified {
                LocalModification::Clean
            } else {
                LocalModification::Unknown
            };
        }
    }
    // Do not fingerprint other selected attachments: earlier successful units
    // must not invalidate later ones. Raw link destinations survive link->broken.
    let fingerprint = batch::digest(&format!(
        "{:?}|{:?}|{}|{}|{}|{}|{}",
        located.lock_entry,
        physical_parent,
        directory.dev(),
        directory.ino(),
        metadata.dev(),
        metadata.ino(),
        batch::path_fingerprint(layout, &attachment.path, target.deletes_copy)?
    ));
    Ok(PlannedTarget {
        target,
        fingerprint,
        lock_entry: located.lock_entry,
    })
}

fn dependencies(
    paths: &AppPaths,
    layout: &ScopeLayout,
    attachment: &AttachmentLocation,
    allowed_links: &HashSet<PathBuf>,
) -> Result<(), AppError> {
    let dependents = crate::detach::find_copy_content_dependents(
        &layout.agents,
        &layout.canonical_dir,
        &attachment.path,
    )?;
    let blocked: Vec<_> = dependents
        .iter()
        .filter(|d| !allowed_links.contains(&d.path))
        .map(|d| format!("{}（{}）", d.label, paths.display(&d.path)))
        .collect();
    if blocked.is_empty() {
        Ok(())
    } else {
        Err(AppError::agent_dependency_conflict(
            &attachment.agent_name,
            &blocked,
        ))
    }
}

fn removes_all_content(located: &LocatedSkill, removed_copies: &HashSet<PathBuf>) -> bool {
    let contents: Vec<_> = located
        .canonical_path
        .iter()
        .chain(located.attachments.iter().map(|a| &a.path))
        .filter_map(|path| fs::canonicalize(path).ok())
        .filter(|path| path.is_dir())
        .collect();
    !contents.is_empty() && contents.iter().all(|path| removed_copies.contains(path))
}

/// Plan links before copies, and authorize dependencies only within this Skill.
/// Runtime checks allow no remaining dependents, even if their removal was planned.
pub(super) fn finalize(
    paths: &AppPaths,
    layout: &ScopeLayout,
    targets: &mut [PlannedTarget],
) -> Result<(), AppError> {
    let skills: HashSet<_> = targets.iter().map(|t| t.target.skill_id.clone()).collect();
    for skill_id in skills {
        let links: HashSet<_> = targets
            .iter()
            .filter(|t| {
                t.target.skill_id == skill_id && t.target.eligible && !t.target.deletes_copy
            })
            .map(|t| resolve(paths, layout, &t.target).map(|(_, a)| a.path))
            .collect::<Result<_, _>>()?;
        for planned in targets
            .iter_mut()
            .filter(|t| t.target.skill_id == skill_id && t.target.eligible && t.target.deletes_copy)
        {
            let (_, attachment) = resolve(paths, layout, &planned.target)?;
            if let Err(error) = dependencies(paths, layout, &attachment, &links) {
                planned.target.eligible = false;
                planned.target.reason = Some(error.detail.unwrap_or(error.message));
            }
        }
        let copies: HashSet<_> = targets
            .iter()
            .filter(|t| t.target.skill_id == skill_id && t.target.eligible && t.target.deletes_copy)
            .map(|t| {
                resolve(paths, layout, &t.target).and_then(|(_, a)| {
                    fs::canonicalize(a.path).map_err(|e| AppError::path_invalid(e.to_string()))
                })
            })
            .collect::<Result<_, _>>()?;
        if let Some(planned) = targets
            .iter()
            .find(|t| t.target.skill_id == skill_id && t.target.eligible)
        {
            let (located, _) = resolve(paths, layout, &planned.target)?;
            if removes_all_content(&located, &copies) {
                for target in targets.iter_mut().filter(|t| {
                    t.target.skill_id == skill_id && t.target.eligible && t.target.deletes_copy
                }) {
                    target.target.removes_last_copy = Some(true);
                }
            }
        }
    }
    // Stable sort preserves skill and Agent order within links/copies.
    targets.sort_by_key(|t| t.target.deletes_copy);
    Ok(())
}

pub(crate) fn verify(
    paths: &AppPaths,
    layout: &ScopeLayout,
    planned: &PlannedTarget,
) -> Result<(), AppError> {
    let (located, attachment) = resolve(paths, layout, &planned.target)?;
    let fresh = inspect(paths, layout, planned.target.clone())?;
    if fresh.fingerprint != planned.fingerprint {
        return Err(AppError::batch_target_changed(
            "挂载路径或内容与确认时不同。",
        ));
    }
    if planned.target.deletes_copy {
        dependencies(paths, layout, &attachment, &HashSet::new())?;
        let copies = HashSet::from([fs::canonicalize(&attachment.path)
            .map_err(|e| AppError::path_invalid(e.to_string()))?]);
        if planned.target.removes_last_copy != Some(true) && removes_all_content(&located, &copies)
        {
            return Err(AppError::batch_target_changed(
                "此副本已成为最后一份内容，请重新预览并确认。",
            ));
        }
    }
    Ok(())
}

pub(crate) fn execute(
    paths: &AppPaths,
    layout: &ScopeLayout,
    planned: &PlannedTarget,
    record_snapshot: &dyn Fn(&Path),
    before_write: &dyn Fn() -> Result<(), AppError>,
) -> Result<(), AppError> {
    let mut backup_path = None;
    let result = (|| {
        verify(paths, layout, planned)?;
        let (_, attachment) = resolve(paths, layout, &planned.target)?;
        let backup = BackupStore::for_layout(paths, layout).create(
            BackupAction::Detach,
            &planned.target.skill_id,
            None,
            &[BackupAttachment::from(&attachment)],
            &[attachment.agent_id.clone()],
        )?;
        backup_path = Some(paths.display(&backup.path));
        record_snapshot(&backup.path);
        if !planned.target.deletes_copy {
            let link = fs::read_link(&attachment.path)
                .map_err(|e| AppError::backup_failed(e.to_string()))?;
            fs::write(
                backup.path.join("link-target.txt"),
                link.as_os_str().as_encoded_bytes(),
            )
            .map_err(|e| AppError::backup_failed(e.to_string()))?;
        }
        before_write()?;
        verify(paths, layout, planned)?;
        if planned.target.deletes_copy {
            fs::remove_dir_all(&attachment.path)
        } else {
            fs::remove_file(&attachment.path)
        }
        .map_err(|e| AppError::agent_detach_failed(e.to_string()))?;
        match fs::symlink_metadata(&attachment.path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            _ => Err(AppError::verify_failed("移除后挂载仍存在或无法核对。")),
        }
    })();
    let _ = OperationLog::for_scope(paths, &layout.scope).append(&OperationEntry::new(
        "uninstall_agents",
        &planned.target.skill_id,
        planned.target.agent_id.iter().cloned().collect(),
        if result.is_ok() { "removed" } else { "failed" },
        backup_path,
        result.as_ref().err().cloned(),
    ));
    result
}
