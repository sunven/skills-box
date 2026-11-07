//! Read-only batch plans and frozen authorization for one Scope.
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{install, uninstall, update};
use crate::attachment_target::{self, TargetContext};
use crate::baseline::{BaselineStore, LocalModification};
use crate::cli::ScopedCli;
use crate::error::{AppError, ErrorCode};
use crate::inventory::{AttachmentMode, LocatedSkill, ManagementState, UpdateState};
use crate::paths::AppPaths;
use crate::scope::ScopeId;
use crate::scope_layout::ScopeLayout;
use crate::update::UpdateSkillResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchAction {
    Update,
    Uninstall,
    UninstallAgents,
    Attach,
    Detach,
    CopyToProject,
    CleanRecords,
    Restore,
}
impl BatchAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Update => "更新",
            Self::Uninstall => "卸载",
            Self::UninstallAgents => "移除",
            Self::Attach => "挂载",
            Self::Detach => "解除挂载",
            Self::CopyToProject => "复制到项目",
            Self::CleanRecords => "清理记录",
            Self::Restore => "恢复技能",
        }
    }
    pub fn has_agents(self) -> bool {
        matches!(
            self,
            Self::Attach | Self::Detach | Self::CopyToProject | Self::UninstallAgents
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchRequest {
    pub action: BatchAction,
    pub skill_ids: Vec<String>,
    #[serde(default)]
    pub agent_ids: Vec<String>,
    /// Restricts a retry to the original failed/unexecuted pairs.
    pub target_keys: Option<Vec<String>>,
    pub project_root: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchTarget {
    pub key: String,
    pub skill_id: String,
    pub name: String,
    pub agent_id: Option<String>,
    pub agent_name: Option<String>,
    pub eligible: bool,
    pub reason: Option<String>,
    pub local_modification: LocalModification,
    pub deletes_copy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removes_last_copy: Option<bool>,
    pub paths: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    /// Display-only; execution rebuilds arguments from the validated plan.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    pub source: Option<String>,
    pub reference: Option<String>,
}

impl BatchTarget {
    pub fn new(
        skill_id: &str,
        name: &str,
        agent_id: Option<&str>,
        agent_name: Option<String>,
    ) -> Self {
        Self {
            key: serde_json::to_string(&(skill_id, agent_id)).expect("string tuple"),
            skill_id: skill_id.to_string(),
            name: name.to_string(),
            agent_id: agent_id.map(str::to_string),
            agent_name,
            eligible: false,
            reason: None,
            local_modification: LocalModification::Unknown,
            deletes_copy: false,
            removes_last_copy: None,
            paths: Vec::new(),
            operation: None,
            command: None,
            source: None,
            reference: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchPreview {
    pub id: String,
    pub revision: String,
    pub scope: ScopeId,
    pub action: BatchAction,
    pub targets: Vec<BatchTarget>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<crate::projects::Project>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agents: Option<Vec<crate::inventory::InventoryAgent>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchStatus {
    Success,
    Failed,
    Skipped,
    NotExecuted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchRow {
    #[serde(flatten)]
    pub target: BatchTarget,
    pub status: BatchStatus,
    pub error: Option<AppError>,
    pub backup_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchResult {
    pub action: BatchAction,
    pub rows: Vec<BatchRow>,
}

#[derive(Debug, Clone)]
pub struct PlannedTarget {
    pub target: BatchTarget,
    pub fingerprint: String,
    pub lock_entry: Option<crate::lock::LockEntry>,
}

#[derive(Debug, Clone)]
pub struct BatchPlan {
    pub preview: BatchPreview,
    pub request: BatchRequest,
    pub root: PathBuf,
    pub project_revision: Option<String>,
    pub targets: Vec<PlannedTarget>,
    pub updates: HashMap<String, UpdateSkillResult>,
    pub created_at: u64,
    pub copy: Option<super::copy_to_project::CopyPlan>,
    pub stale: Option<super::stale_records::StalePlan>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchConfirmation {
    pub preview_id: String,
    pub revision: String,
    pub confirmed_delete: bool,
    pub confirmed_copies: bool,
}

pub fn plan(
    paths: &AppPaths,
    layout: &ScopeLayout,
    request: BatchRequest,
    id: String,
    updates: HashMap<String, UpdateSkillResult>,
) -> Result<BatchPlan, AppError> {
    if request.action == BatchAction::CopyToProject {
        return super::copy_to_project::plan(paths, layout, request, id, None);
    }
    if request.project_root.is_some() {
        return Err(AppError::path_invalid("只有复制到项目可以选择目标目录。"));
    }
    if request.skill_ids.is_empty() || request.skill_ids.len() > 10_000 {
        return Err(AppError::skill_invalid_name("请选择要操作的技能。"));
    }
    let mut seen = HashSet::new();
    let mut names = Vec::new();
    for skill_id in &request.skill_ids {
        let name = layout.scope.parse_skill_id(skill_id)?;
        install::validate_skill_name(name)?;
        if !seen.insert(skill_id.clone()) {
            return Err(AppError::skill_invalid_name("技能选择重复。"));
        }
        names.push((skill_id.clone(), name.to_string()));
    }
    let selected_agents: HashSet<_> = request.agent_ids.iter().collect();
    if selected_agents.len() != request.agent_ids.len() || request.agent_ids.len() > 200 {
        return Err(AppError::skill_invalid_name("Agent 选择重复或过多。"));
    }
    // The selector sends its display order. Keep it fixed even if detection
    // changes; unknown/removed agents remain explicit skipped targets.
    let agent_ids = &request.agent_ids;
    let keys = request
        .target_keys
        .as_ref()
        .map(|keys| keys.iter().cloned().collect::<HashSet<_>>());
    let mut targets = Vec::new();
    let is_stale = matches!(
        request.action,
        BatchAction::CleanRecords | BatchAction::Restore
    );
    let mut installs = HashMap::new();
    let expected = if is_stale {
        Some(super::stale_records::read_lock(layout)?)
    } else {
        None
    };
    for (skill_id, name) in names {
        if is_stale {
            let (target, install) = super::stale_records::inspect(
                paths,
                layout,
                request.action,
                &skill_id,
                &name,
                agent_ids,
            )?;
            if keys
                .as_ref()
                .is_none_or(|keys| keys.contains(&target.target.key))
            {
                if let Some(install) = install {
                    installs.insert(name, install);
                }
                targets.push(target);
            }
            continue;
        }
        if request.action.has_agents() {
            for agent_id in agent_ids {
                let target = preview_target(
                    paths,
                    layout,
                    request.action,
                    &skill_id,
                    &name,
                    Some(agent_id),
                    &updates,
                )?;
                if keys
                    .as_ref()
                    .is_none_or(|keys| keys.contains(&target.target.key))
                {
                    targets.push(target);
                }
            }
        } else {
            let target = preview_target(
                paths,
                layout,
                request.action,
                &skill_id,
                &name,
                None,
                &updates,
            )?;
            if keys
                .as_ref()
                .is_none_or(|keys| keys.contains(&target.target.key))
            {
                targets.push(target);
            }
        }
    }
    if request.action == BatchAction::UninstallAgents {
        super::uninstall_agents::finalize(paths, layout, &mut targets)?;
    }
    let revision = digest(&format!(
        "{:?}|{:?}|{:?}",
        layout.root,
        layout.project.as_ref().map(|p| &p.check_revision),
        targets
    ));
    let preview = BatchPreview {
        id,
        revision,
        scope: layout.scope.clone(),
        action: request.action,
        targets: targets.iter().map(|t| t.target.clone()).collect(),
        project: None,
        agents: None,
    };
    let mut plan = BatchPlan {
        preview,
        request,
        root: layout.root.clone(),
        project_revision: layout.project.as_ref().map(|p| p.check_revision.clone()),
        targets,
        updates,
        created_at: crate::time::now_ms(),
        copy: None,
        stale: expected.map(|expected| super::stale_records::StalePlan {
            expected,
            installs,
            source_checked: false,
        }),
    };
    if is_stale {
        super::stale_records::revision(&mut plan);
    }
    Ok(plan)
}

fn preview_target(
    paths: &AppPaths,
    layout: &ScopeLayout,
    action: BatchAction,
    skill_id: &str,
    name: &str,
    agent_id: Option<&str>,
    updates: &HashMap<String, UpdateSkillResult>,
) -> Result<PlannedTarget, AppError> {
    match inspect(paths, layout, action, skill_id, name, agent_id, updates) {
        Ok(target) => Ok(target),
        Err(error) if !stops_batch(&error) => {
            let reason = error.detail.unwrap_or(error.message);
            Ok(PlannedTarget {
                fingerprint: digest(&reason),
                lock_entry: None,
                target: BatchTarget {
                    reason: Some(reason),
                    ..BatchTarget::new(
                        skill_id,
                        name,
                        agent_id,
                        agent_id.map(|id| {
                            layout
                                .agents
                                .iter()
                                .find(|a| a.id == id)
                                .map_or_else(|| id.to_string(), |a| a.display_name.clone())
                        }),
                    )
                },
            })
        }
        Err(error) => Err(error),
    }
}

pub fn inspect(
    paths: &AppPaths,
    layout: &ScopeLayout,
    action: BatchAction,
    skill_id: &str,
    name: &str,
    agent_id: Option<&str>,
    updates: &HashMap<String, UpdateSkillResult>,
) -> Result<PlannedTarget, AppError> {
    let agents = target_agents(paths, layout, action);
    let agent = agent_id.and_then(|id| agents.iter().find(|a| a.id == id));
    let mut target = BatchTarget::new(
        skill_id,
        name,
        agent_id,
        agent_id.map(|id| agent.map_or_else(|| id.to_string(), |a| a.display_name.clone())),
    );
    if action == BatchAction::UninstallAgents {
        return super::uninstall_agents::inspect(paths, layout, target);
    }
    let located = layout.locate(paths, name)?;
    let Some(located) = located else {
        target.reason = Some("技能已不存在。".into());
        return Ok(PlannedTarget {
            target,
            fingerprint: "missing".into(),
            lock_entry: None,
        });
    };
    let assessment_path = located
        .canonical_path
        .as_deref()
        .or(located.primary_path.as_deref());
    if let Some(path) = assessment_path {
        ensure_readable(layout, path)?;
    }
    if located.management != ManagementState::Unmanaged {
        target.local_modification = BaselineStore::for_scope(paths, &layout.scope)
            .assess(
                name,
                assessment_path,
                located
                    .lock_entry
                    .as_ref()
                    .map(|e| e.skill_folder_hash.as_str()),
            )
            .state;
    }
    let eligibility: Result<(), AppError> = match action {
        BatchAction::Update => (|| {
            let planned = update::plan_updates(paths, layout, &[name.to_string()])?;
            if let Some(skip) = planned.skipped.first() {
                return Err(skip.error.clone());
            }
            if target.local_modification != LocalModification::Clean {
                return Err(AppError::batch_target_changed(
                    "本地有改动或无法确认本地改动，请单独确认更新。",
                ));
            }
            let result = updates
                .get(name)
                .ok_or_else(|| AppError::batch_target_changed("尚无有效更新检查结果。"))?;
            if result.state != UpdateState::Update || result.remote_hash.is_none() {
                return Err(AppError::batch_target_changed(
                    result
                        .note
                        .clone()
                        .unwrap_or_else(|| "没有可用更新。".into()),
                ));
            }
            if located
                .lock_entry
                .as_ref()
                .map(|e| e.skill_folder_hash.as_str())
                != result.lock_hash.as_deref()
            {
                return Err(AppError::batch_target_changed(
                    "更新检查版本与当前安装记录不一致。",
                ));
            }
            let checked_source = result
                .source
                .as_ref()
                .map(|source| match &result.reference {
                    Some(reference) => format!("{source}#{reference}"),
                    None => source.clone(),
                });
            if planned.items.first().map(|item| item.source.as_str()) != checked_source.as_deref() {
                return Err(AppError::batch_target_changed(
                    "来源或版本引用已变化，请重新检查更新。",
                ));
            }
            if let Some(item) = planned.items.first() {
                target.command = Some(ScopedCli::install_preview(
                    layout,
                    &item.source,
                    std::slice::from_ref(&item.name),
                    &item.agent_ids,
                ));
            }
            Ok(())
        })(),
        BatchAction::Uninstall => uninstall::plan_uninstall(paths, layout, skill_id).map(|p| {
            target
                .paths
                .extend(p.canonical.iter().map(|p| paths.display(p)));
            target
                .paths
                .extend(p.attachments.iter().map(|a| paths.display(&a.path)));
        }),
        BatchAction::Attach | BatchAction::Detach => (|| {
            let id = agent_id.ok_or_else(|| AppError::agent_not_detected("请选择 Agent"))?;
            let context = TargetContext::for_layout(layout);
            let resolved = if action == BatchAction::Attach {
                attachment_target::resolve_attach(
                    &layout.canonical_dir,
                    &located,
                    &agents,
                    id,
                    context,
                )?
            } else {
                attachment_target::resolve_detach(
                    &layout.canonical_dir,
                    &located,
                    &layout.agents,
                    id,
                    context,
                )?
            };
            target.paths.push(paths.display(&resolved.target));
            if action == BatchAction::Detach
                && resolved
                    .existing
                    .as_ref()
                    .is_some_and(|a| a.mode == AttachmentMode::Copy)
            {
                target.deletes_copy = true;
                let dependents = crate::detach::find_copy_dependents(
                    &layout.agents,
                    &layout.canonical_dir,
                    &resolved.target,
                )?;
                if !dependents.is_empty() {
                    return Err(AppError::agent_dependency_conflict(
                        &resolved.agent.display_name,
                        &dependents
                            .iter()
                            .map(|d| paths.display(&d.path))
                            .collect::<Vec<_>>(),
                    ));
                }
            }
            Ok(())
        })(),
        BatchAction::CleanRecords | BatchAction::Restore => {
            unreachable!("stale records have their own inspection")
        }
        BatchAction::CopyToProject => unreachable!("copy has its own plan"),
        BatchAction::UninstallAgents => unreachable!("agent removal has its own inspection"),
    };
    match eligibility {
        Ok(()) => target.eligible = true,
        Err(error) => target.reason = Some(error.detail.clone().unwrap_or(error.message)),
    }
    let fingerprint = if target.eligible {
        capture(layout, &located, action, agent_id, &agents)?
    } else {
        digest(&format!("{:?}|{:?}", target.reason, located.lock_entry))
    };
    Ok(PlannedTarget {
        target,
        fingerprint,
        lock_entry: located.lock_entry,
    })
}

fn ensure_readable(layout: &ScopeLayout, path: &Path) -> Result<(), AppError> {
    if layout.project.is_some() {
        let root = fs::canonicalize(&layout.root)
            .map_err(|_| AppError::project_root_unavailable("项目", "项目根目录无法读取。"))?;
        let physical =
            fs::canonicalize(path).map_err(|e| AppError::batch_target_changed(e.to_string()))?;
        if !physical.starts_with(root) {
            return Err(AppError::path_invalid("目标目录超出项目范围。"));
        }
    }
    Ok(())
}

pub(super) fn path_fingerprint(
    layout: &ScopeLayout,
    path: &Path,
    content: bool,
) -> Result<String, AppError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok("missing".into()),
        Err(e) => return Err(AppError::batch_target_changed(e.to_string())),
    };
    let link = if metadata.file_type().is_symlink() {
        fs::read_link(path).ok()
    } else {
        None
    };
    let hash = if content {
        ensure_readable(layout, path)?;
        let mut hasher = Sha256::new();
        capture_contents(path, &mut hasher)
            .map_err(|e| AppError::batch_target_changed(e.to_string()))?;
        Some(format!("{:x}", hasher.finalize()))
    } else {
        None
    };
    Ok(format!(
        "{path:?}|{link:?}|{hash:?}|{:?}",
        metadata.file_type()
    ))
}

/// Authorization includes all content a write might remove. The local-change
/// baseline intentionally excludes installer noise, so it is insufficient to
/// detect changes to metadata or excluded files during a destructive batch.
fn capture_contents(path: &Path, hasher: &mut Sha256) -> std::io::Result<()> {
    use std::io::Read;
    let metadata = fs::symlink_metadata(path)?;
    hasher.update(format!("{:?}|{:?}", metadata.file_type(), metadata.permissions()).as_bytes());
    if metadata.file_type().is_symlink() {
        hasher.update(format!("{:?}", fs::read_link(path)?).as_bytes());
    } else if metadata.is_dir() {
        let mut entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            hasher.update(format!("{:?}", entry.file_name()).as_bytes());
            capture_contents(&entry.path(), hasher)?;
        }
    } else if metadata.is_file() {
        hasher.update(metadata.len().to_le_bytes());
        let mut file = fs::File::open(path)?;
        let mut buffer = [0; 65536];
        loop {
            let len = file.read(&mut buffer)?;
            if len == 0 {
                break;
            }
            hasher.update(&buffer[..len]);
        }
    } else {
        return Err(std::io::Error::other("目标中存在无法校验的特殊文件。"));
    }
    Ok(())
}

fn target_agents(
    paths: &AppPaths,
    layout: &ScopeLayout,
    action: BatchAction,
) -> Vec<crate::inventory::AgentDir> {
    if action == BatchAction::Attach && !layout.is_global() {
        layout.install_targets(&crate::inventory::detected_agent_dirs(paths))
    } else {
        layout.agents.clone()
    }
}

fn capture(
    layout: &ScopeLayout,
    located: &LocatedSkill,
    action: BatchAction,
    agent_id: Option<&str>,
    agents: &[crate::inventory::AgentDir],
) -> Result<String, AppError> {
    let mut value = format!(
        "{:?}|{:?}|{:?}",
        located.management, located.lock_entry, located.canonical_path
    );
    if let Some(canonical) = &located.canonical_path {
        value.push_str(&path_fingerprint(layout, canonical, true)?);
    }
    if action.has_agents() {
        let id = agent_id.unwrap_or_default();
        if let Some(agent) = agents.iter().find(|a| a.id == id) {
            let attachment = located.attachments.iter().find(|a| a.agent_id == id);
            value.push_str(&path_fingerprint(
                layout,
                &agent.skills_dir.join(&located.name),
                attachment.is_some_and(|a| a.mode == AttachmentMode::Copy),
            )?);
        }
    } else {
        for a in &located.attachments {
            value.push_str(&format!(
                "{:?}|{}",
                a.mode,
                path_fingerprint(layout, &a.path, a.mode == AttachmentMode::Copy)?
            ));
        }
    }
    Ok(digest(&value))
}

pub(super) fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

pub fn stops_batch(error: &AppError) -> bool {
    matches!(
        error.code,
        ErrorCode::BackupFailed
            | ErrorCode::TaskInternal
            | ErrorCode::TaskShuttingDown
            | ErrorCode::ProjectNotFound
            | ErrorCode::ProjectRootUnavailable
            | ErrorCode::ProjectRegistryCorrupt
            | ErrorCode::ProjectLockUnsupported
            | ErrorCode::LockCorrupt
            | ErrorCode::LockUnsupportedVersion
            | ErrorCode::LockReadFailed
            | ErrorCode::EnvNodeMissing
            | ErrorCode::EnvNpxMissing
            | ErrorCode::EnvGitMissing
            | ErrorCode::EnvCliMissing
            | ErrorCode::CliSpawnFailed
    )
}
