use super::{Snapshot, records::Records};
use crate::{
    error::AppError,
    inventory::{AttachmentMode, ManagementState},
    paths::AppPaths,
    scope_layout::ScopeLayout,
};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillState {
    pub name: String,
    pub present: bool,
    pub missing_agents: Vec<String>,
    pub attached_agents: Vec<String>,
    pub version: Option<String>,
    pub version_changed: bool,
    pub modified: bool,
    pub conflict: bool,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalPack {
    pub snapshot: Snapshot,
    pub complete: bool,
    pub skills: Vec<SkillState>,
}

pub fn list(paths: &AppPaths, layout: &ScopeLayout) -> Result<Vec<LocalPack>, AppError> {
    let records = Records::read(&layout.lock_path)?;
    records
        .packs
        .values()
        .map(|snapshot| observe(paths, layout, snapshot))
        .collect()
}
pub fn observe(
    paths: &AppPaths,
    layout: &ScopeLayout,
    snapshot: &Snapshot,
) -> Result<LocalPack, AppError> {
    let current = ScopeLayout::resolve_detected(
        paths,
        &crate::projects::ProjectRegistry::new(paths.clone()),
        &layout.scope,
    )?;
    if current.root != layout.root
        || current.project.as_ref().map(|p| &p.check_revision)
            != layout.project.as_ref().map(|p| &p.check_revision)
    {
        return Err(AppError::batch_target_changed(
            "项目位置已变化，无法确认专家包状态。",
        ));
    }
    let layout = &current;
    let lock = layout.lock()?;
    let market = crate::marketplace::records::Records::read(&layout.lock_path)?;
    let workflows = Records::read(&layout.lock_path)?;
    let mut skills = vec![];
    for name in snapshot.names() {
        let located = layout.locate(paths, name)?;
        let entry = lock.get(name);
        let member = snapshot.members.iter().find(|m| m.name == name);
        let identity = if let Some(member) = member {
            market
                .skills
                .get(name)
                .is_some_and(|r| r.coordinate == member.coordinate)
        } else {
            workflows.workflows.get(name).is_some_and(|r| {
                r.slug == snapshot.slug && r.markdown == snapshot.installed_workflow
            })
        };
        let managed_content = identity
            && located.as_ref().is_some_and(|l| {
                l.management == ManagementState::Managed && l.canonical_path.is_some()
            });
        let canonical = layout.canonical_dir.join(name);
        let present = managed_content
            && std::fs::symlink_metadata(&canonical)
                .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
            && std::fs::symlink_metadata(canonical.join("SKILL.md"))
                .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink());
        let attached_agents: Vec<_> = located
            .as_ref()
            .map(|l| {
                l.attachments
                    .iter()
                    .filter(|a| match a.mode {
                        AttachmentMode::Native => true,
                        AttachmentMode::Link => std::fs::canonicalize(&canonical)
                            .ok()
                            .zip(std::fs::canonicalize(&a.path).ok())
                            .is_some_and(|(canonical, target)| canonical == target),
                        _ => false,
                    })
                    .map(|a| a.agent_id.clone())
                    .collect()
            })
            .unwrap_or_default();
        let attachment_conflict = located.as_ref().is_some_and(|l| {
            l.attachments.iter().any(|a| {
                snapshot.agents.contains(&a.agent_id) && !attached_agents.contains(&a.agent_id)
            })
        });
        let missing_agents = snapshot
            .agents
            .iter()
            .filter(|id| !present || !attached_agents.contains(id))
            .cloned()
            .collect();
        let modified = if managed_content {
            clean_content(
                layout,
                name,
                entry.map(|e| e.skill_folder_hash.as_str()).unwrap_or(""),
            )
            .is_err()
        } else {
            false
        };
        let version = market.skills.get(name).map(|r| r.version.clone());
        let version_changed =
            member.is_some_and(|m| version.as_ref().is_some_and(|v| v != &m.version));
        skills.push(SkillState {
            name: name.into(),
            present,
            missing_agents,
            attached_agents,
            version,
            version_changed,
            modified,
            conflict: (!identity && (located.is_some() || entry.is_some())) || attachment_conflict,
        });
    }
    Ok(LocalPack {
        snapshot: snapshot.clone(),
        complete: skills
            .iter()
            .all(|s| s.present && s.missing_agents.is_empty()),
        skills,
    })
}

/// Verify the entire canonical tree, not just SKILL.md. Never adopt links or partial copies.
pub fn clean_content(
    layout: &ScopeLayout,
    name: &str,
    expected_hash: &str,
) -> Result<(), AppError> {
    let root = layout.canonical_dir.join(name);
    if !std::fs::symlink_metadata(&root).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink()) {
        return Err(AppError::skill_conflict(
            name,
            "缺少完整受管内容；请先清理陈旧记录或残留文件。",
        ));
    }
    for item in walkdir::WalkDir::new(&root).follow_links(false) {
        let item = item.map_err(|e| AppError::skill_conflict(name, e.to_string()))?;
        if item.file_type().is_symlink()
            || (!item.file_type().is_dir() && !item.file_type().is_file())
        {
            return Err(AppError::skill_conflict(
                name,
                "内容包含符号链接或特殊文件。",
            ));
        }
    }
    let markdown = std::fs::read_to_string(root.join("SKILL.md"))
        .map_err(|e| AppError::skill_conflict(name, e.to_string()))?;
    if crate::marketplace::original_name(&markdown)? != name
        || crate::baseline::fingerprint_folder(&root)
            .map_err(AppError::skill_fingerprint_failed)?
            .content_hash
            != expected_hash
    {
        return Err(AppError::skill_conflict(
            name,
            "内容不完整或存在本地修改，请先处理后再安装专家包。",
        ));
    }
    Ok(())
}
