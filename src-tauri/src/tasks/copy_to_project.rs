//! Copy current Global content into one independently managed Project.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::batch::{
    BatchAction, BatchPlan, BatchPreview, BatchRequest, BatchTarget, PlannedTarget,
};
use crate::attachment_target::{self, TargetContext};
use crate::baseline::{BaselineStore, LocalModification};
use crate::error::AppError;
use crate::inventory::{self, LocatedSkill, ManagementState};
use crate::lock::LockEntry;
use crate::paths::AppPaths;
use crate::project_path::{self, Existence};
use crate::projects::{Project, ProjectRegistry};
use crate::scope_layout::ScopeLayout;

#[derive(Debug, Clone)]
pub struct CopyPlan {
    pub layout: ScopeLayout,
    root_identity: String,
    skills: BTreeMap<String, CopySkill>,
}

#[derive(Debug, Clone)]
struct CopySkill {
    source: PathBuf,
    source_identity: String,
    content: String,
    entry: LockEntry,
    destination_entry: Option<LockEntry>,
    destination_identity: Option<String>,
    existed: bool,
}

pub fn plan(
    paths: &AppPaths,
    source: &ScopeLayout,
    request: BatchRequest,
    id: String,
    pending: Option<&Project>,
) -> Result<BatchPlan, AppError> {
    if !source.is_global() || request.action != BatchAction::CopyToProject {
        return Err(AppError::path_invalid("仅支持从全局复制到一个项目。"));
    }
    let root = request
        .project_root
        .as_deref()
        .ok_or_else(|| AppError::project_invalid_root("请选择项目目录。"))?;
    let project = ProjectRegistry::new(paths.clone()).preview_root(root, pending)?;
    let layout = ScopeLayout::project(paths, project.clone());
    ensure_inside(&layout, &layout.canonical_dir)?;
    ensure_lock_path(&layout)?;
    let destination_lock = layout.lock()?;
    if request.skill_ids.is_empty() || request.skill_ids.len() > 10_000 {
        return Err(AppError::skill_invalid_name("请选择要复制的技能。"));
    }
    if request.agent_ids.len() > 200
        || request.agent_ids.iter().collect::<HashSet<_>>().len() != request.agent_ids.len()
    {
        return Err(AppError::skill_invalid_name("Agent 选择重复或过多。"));
    }
    let detected = inventory::detected_agent_dirs(paths);
    let agents = layout.install_targets(&detected);
    let mut skills = BTreeMap::new();
    let mut targets = Vec::new();
    let mut seen = HashSet::new();
    for skill_id in &request.skill_ids {
        let name = source.scope.parse_skill_id(skill_id)?;
        super::install::validate_skill_name(name)?;
        if !seen.insert(name) {
            return Err(AppError::skill_invalid_name("技能选择重复。"));
        }
        let mut local = LocalModification::Unknown;
        let inspected: Result<CopySkill, AppError> = (|| {
            let located = source
                .locate(paths, name)?
                .ok_or_else(|| AppError::batch_target_changed("技能已不存在。"))?;
            if located.management == ManagementState::CopyOnly {
                return Err(AppError::skill_copy_only(name));
            }
            if located.management != ManagementState::Managed {
                return Err(AppError::skill_unmanaged(name));
            }
            let canonical = located
                .canonical_path
                .as_ref()
                .ok_or_else(|| AppError::skill_copy_only(name))?;
            let entry = located
                .lock_entry
                .clone()
                .ok_or_else(|| AppError::skill_unmanaged(name))?;
            let content = tree_fingerprint(canonical)?;
            let fingerprint = crate::baseline::fingerprint_folder(canonical)
                .map_err(AppError::skill_fingerprint_failed)?;
            if let Some(baseline) = BaselineStore::for_scope(paths, &source.scope).load(name) {
                if baseline.path == canonical.to_string_lossy() {
                    local = if baseline.content_hash != fingerprint.content_hash {
                        LocalModification::Modified
                    } else if baseline.verified {
                        LocalModification::Clean
                    } else {
                        LocalModification::Unknown
                    };
                }
            }
            let target = layout.canonical_dir.join(name);
            ensure_inside(&layout, &target)?;
            let existed = fs::symlink_metadata(&target).is_ok();
            let destination_entry = destination_lock.get(name).cloned();
            if existed {
                if tree_fingerprint(&target)? != content {
                    return Err(AppError::batch_target_changed(
                        "项目已有同名技能，内容不同，不覆盖。",
                    ));
                }
            } else if destination_entry.is_some() {
                return Err(AppError::batch_target_changed(
                    "项目已有同名安装记录，但内容缺失，不覆盖。",
                ));
            } else if layout.locate(paths, name)?.is_some() {
                return Err(AppError::batch_target_changed(
                    "项目已有同名 Agent 内容，需先处理冲突。",
                ));
            }
            let mut copied_entry = entry.clone();
            copied_entry.installed_at = None;
            copied_entry.updated_at = None;
            copied_entry.plugin_name = None;
            copied_entry.source_base_url = None;
            copied_entry.well_known_digest = None;
            copied_entry.skill_folder_hash = comparable_hash(canonical)?;
            Ok(CopySkill {
                source: canonical.clone(),
                source_identity: identity(canonical)?,
                content,
                entry: copied_entry,
                destination_entry,
                destination_identity: if existed {
                    Some(identity(&target)?)
                } else {
                    None
                },
                existed,
            })
        })();
        let source_reason = inspected.as_ref().err().map(reason);
        let mut operations = vec![None];
        operations.extend(request.agent_ids.iter().map(|s| Some(s.as_str())));
        for agent_id in operations {
            let key = serde_json::to_string(&(skill_id, agent_id)).expect("string tuple");
            if request
                .target_keys
                .as_ref()
                .is_some_and(|keys| !keys.contains(&key))
            {
                continue;
            }
            let agent = agent_id.and_then(|id| agents.iter().find(|a| a.id == id));
            let mut target = BatchTarget {
                key,
                skill_id: skill_id.clone(),
                name: name.into(),
                agent_id: agent_id.map(str::to_string),
                agent_name: agent_id
                    .map(|id| agent.map_or_else(|| id.to_string(), |a| a.display_name.clone())),
                eligible: false,
                reason: source_reason.clone(),
                local_modification: local,
                deletes_copy: false,
                paths: vec![paths.display(&layout.canonical_dir.join(name))],
                operation: Some(if agent_id.is_some() { "attach" } else { "copy" }.into()),
            };
            let mut fingerprint = String::new();
            if let Ok(skill) = &inspected {
                if let Some(agent_id) = agent_id {
                    if skill.existed && skill.destination_entry.is_none() {
                        target.reason =
                            Some("项目已有未受管技能，保留其状态，无法新增独立挂载。".into());
                    } else {
                        match attachment(paths, &layout, name, skill, agent_id, false) {
                            Ok(resolved) => {
                                target.paths = vec![paths.display(&resolved.target)];
                                fingerprint = attachment_identity(&resolved);
                                target.eligible = true;
                            }
                            Err(error) => target.reason = Some(reason(&error)),
                        }
                    }
                } else if skill.existed {
                    target.reason = Some("项目已有相同内容，复用，不重复复制。".into());
                } else {
                    target.eligible = true;
                    if skill.entry.skill_folder_hash.is_empty() {
                        target.reason = Some("包含无法可靠比较上游版本的文件名；内容仍可复制，项目更新检查会显示未知。".into());
                    }
                }
                fingerprint.push_str(&format!(
                    "{:?}|{}|{:?}",
                    skill.source_identity, skill.content, skill.destination_entry
                ));
            }
            targets.push(PlannedTarget {
                target,
                fingerprint,
                lock_entry: source.lock()?.get(name).cloned(),
            });
        }
        if let Ok(skill) = inspected {
            skills.insert(name.into(), skill);
        }
    }
    let root_identity = identity(&layout.root)?;
    let revision = format!(
        "{:x}",
        Sha256::digest(
            format!("{:?}|{:?}|{:?}|{root_identity}", project, targets, skills).as_bytes()
        )
    );
    let preview = BatchPreview {
        id,
        revision,
        scope: source.scope.clone(),
        action: request.action,
        targets: targets.iter().map(|t| t.target.clone()).collect(),
        project: Some(project),
        agents: Some(inventory::install_target_rows(paths, &layout)),
    };
    Ok(BatchPlan {
        preview,
        request,
        root: source.root.clone(),
        project_revision: None,
        targets,
        updates: HashMap::new(),
        created_at: crate::time::now_ms(),
        copy: Some(CopyPlan {
            layout,
            root_identity,
            skills,
        }),
    })
}

pub fn replan(
    paths: &AppPaths,
    source: &ScopeLayout,
    original: &BatchPlan,
) -> Result<BatchPlan, AppError> {
    plan(
        paths,
        source,
        original.request.clone(),
        original.preview.id.clone(),
        original
            .copy
            .as_ref()
            .and_then(|p| p.layout.project.as_ref()),
    )
}

fn reason(error: &AppError) -> String {
    error
        .detail
        .clone()
        .unwrap_or_else(|| error.message.clone())
}

fn comparable_hash(root: &Path) -> Result<String, AppError> {
    match crate::update::content_hash::folder_hash(root) {
        Ok(hash) => Ok(hash),
        // The existing Project checker refuses paths it cannot sort like the
        // CLI. Retain the source and readable content without inventing a hash.
        Err(crate::update::content_hash::FolderHashError::UnsupportedPath(_)) => Ok(String::new()),
        Err(error) => Err(AppError::batch_target_changed(error.detail())),
    }
}

fn ensure_inside(layout: &ScopeLayout, path: &Path) -> Result<(), AppError> {
    project_path::lexical(&layout.root, path)
        .and_then(|_| project_path::resolve(path, &layout.root, Existence::AllowMissing))
        .map_err(|e| AppError::path_invalid(format!("项目路径不可用 {}：{e:?}", path.display())))?;
    Ok(())
}

fn ensure_lock_path(layout: &ScopeLayout) -> Result<(), AppError> {
    ensure_inside(layout, &layout.lock_path)?;
    if fs::symlink_metadata(&layout.lock_path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(AppError::path_invalid("项目安装记录不能是符号链接。"));
    }
    Ok(())
}

fn identity(path: &Path) -> Result<String, AppError> {
    let physical =
        fs::canonicalize(path).map_err(|e| AppError::batch_target_changed(e.to_string()))?;
    #[cfg(unix)]
    let file_identity = {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::symlink_metadata(path)
            .map_err(|e| AppError::batch_target_changed(e.to_string()))?;
        (metadata.dev(), metadata.ino())
    };
    #[cfg(not(unix))]
    let file_identity = ();
    Ok(format!(
        "{path:?}|{physical:?}|{:?}|{file_identity:?}",
        fs::read_link(path).ok()
    ))
}

fn tree_fingerprint(root: &Path) -> Result<String, AppError> {
    let physical =
        fs::canonicalize(root).map_err(|e| AppError::batch_target_changed(e.to_string()))?;
    if !physical.is_dir() {
        return Err(AppError::batch_target_changed("技能内容不是目录。"));
    }
    let mut hash = Sha256::new();
    visit(&physical, &physical, &mut hash)?;
    let entry = fs::read_to_string(physical.join("SKILL.md"))
        .map_err(|e| AppError::batch_target_changed(format!("无法读取 SKILL.md：{e}")))?;
    if entry.trim().is_empty() {
        return Err(AppError::batch_target_changed("SKILL.md 为空。"));
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn visit(root: &Path, path: &Path, hash: &mut Sha256) -> Result<(), AppError> {
    let result = (|| -> std::io::Result<()> {
        let metadata = fs::symlink_metadata(path)?;
        hash.update(format!(
            "{:?}|{:?}|{:?}",
            path.strip_prefix(root).unwrap(),
            metadata.file_type(),
            metadata.permissions()
        ));
        if metadata.file_type().is_symlink() {
            let link = fs::read_link(path)?;
            let resolved = fs::canonicalize(path)?;
            let lexical = crate::security::normalize_absolute(&path.parent().unwrap().join(&link));
            if link.is_absolute()
                || !resolved.starts_with(root)
                || lexical.is_none_or(|p| !p.starts_with(root))
            {
                return Err(std::io::Error::other(format!(
                    "链接必须是技能内部的有效相对链接：{}",
                    path.display()
                )));
            }
            hash.update(format!("{link:?}"));
        } else if metadata.is_dir() {
            let mut entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
            entries.sort_by_key(|e| e.file_name());
            for entry in entries {
                visit(root, &entry.path(), hash).map_err(std::io::Error::other)?;
            }
        } else if metadata.is_file() {
            let mut file = fs::File::open(path)?;
            let mut buffer = [0u8; 64 * 1024];
            loop {
                let n = file.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                hash.update(&buffer[..n]);
            }
        } else {
            return Err(std::io::Error::other("不支持的技能文件类型。"));
        }
        Ok(())
    })();
    result.map_err(|e| AppError::batch_target_changed(e.to_string()))
}

fn attachment(
    paths: &AppPaths,
    layout: &ScopeLayout,
    name: &str,
    skill: &CopySkill,
    agent: &str,
    published: bool,
) -> Result<attachment_target::ResolvedAttachment, AppError> {
    let agents = layout.install_targets(&inventory::detected_agent_dirs(paths));
    let located = if skill.existed || published {
        layout
            .locate(paths, name)?
            .ok_or_else(|| AppError::batch_target_changed("项目副本已不存在。"))?
    } else {
        LocatedSkill {
            name: name.into(),
            management: ManagementState::Managed,
            lock_entry: Some(skill.entry.clone()),
            canonical_path: Some(layout.canonical_dir.join(name)),
            primary_path: Some(layout.canonical_dir.join(name)),
            attachments: Vec::new(),
        }
    };
    attachment_target::resolve_copy_attach(
        &layout.canonical_dir,
        &located,
        &agents,
        agent,
        TargetContext::for_layout(layout),
    )
}

fn attachment_identity(resolved: &attachment_target::ResolvedAttachment) -> String {
    format!(
        "{:?}|{:?}|{:?}",
        resolved.agent, resolved.target, resolved.target_state
    )
}

pub fn verify(
    paths: &AppPaths,
    plan: &BatchPlan,
    target: &PlannedTarget,
    published: bool,
) -> Result<ScopeLayout, AppError> {
    let copy = plan.copy.as_ref().expect("copy plan");
    let registry = ProjectRegistry::new(paths.clone());
    let current = ScopeLayout::resolve_detected(paths, &registry, &copy.layout.scope)?;
    if current.root != copy.layout.root
        || identity(&current.root)? != copy.root_identity
        || current.project.as_ref().map(|p| &p.check_revision)
            != copy.layout.project.as_ref().map(|p| &p.check_revision)
    {
        return Err(AppError::project_root_unavailable(
            "项目",
            "项目根目录已变化。",
        ));
    }
    ensure_inside(&current, &current.canonical_dir)?;
    ensure_lock_path(&current)?;
    let name = &target.target.name;
    let skill = copy
        .skills
        .get(name)
        .ok_or_else(|| AppError::batch_target_changed("来源内容不可用。"))?;
    let source = ScopeLayout::resolve_detected(paths, &registry, &crate::scope::ScopeId::Global)?;
    let located = source
        .locate(paths, name)?
        .ok_or_else(|| AppError::batch_target_changed("全局技能已不存在。"))?;
    if located.lock_entry != target.lock_entry
        || located.canonical_path.as_ref() != Some(&skill.source)
        || identity(&skill.source)? != skill.source_identity
        || tree_fingerprint(&skill.source)? != skill.content
    {
        return Err(AppError::batch_target_changed(
            "全局内容或来源与确认时不同。",
        ));
    }
    let destination = current.canonical_dir.join(name);
    ensure_inside(&current, &destination)?;
    let lock = current.lock()?;
    let expected_entry = if published && !skill.existed {
        Some(&skill.entry)
    } else {
        skill.destination_entry.as_ref()
    };
    if lock.get(name) != expected_entry {
        return Err(AppError::batch_target_changed("项目安装记录与确认时不同。"));
    }
    if skill.existed || published {
        if skill.existed && Some(identity(&destination)?) != skill.destination_identity {
            return Err(AppError::batch_target_changed("项目内容位置与确认时不同。"));
        }
        if tree_fingerprint(&destination)? != skill.content {
            return Err(AppError::batch_target_changed("项目内容与确认时不同。"));
        }
    } else if fs::symlink_metadata(&destination).is_ok() || current.locate(paths, name)?.is_some() {
        return Err(AppError::batch_target_changed(
            "项目出现了同名内容，不覆盖。",
        ));
    }
    if let Some(agent) = target.target.agent_id.as_deref() {
        let resolved = attachment(paths, &current, name, skill, agent, published)
            .map_err(|error| AppError::batch_target_changed(reason(&error)))?;
        if !target
            .fingerprint
            .starts_with(&attachment_identity(&resolved))
        {
            return Err(AppError::batch_target_changed("Agent 目标与确认时不同。"));
        }
    }
    Ok(current)
}

pub fn execute(
    paths: &AppPaths,
    plan: &BatchPlan,
    target: &PlannedTarget,
    published: bool,
    record: &dyn Fn(&Path),
) -> Result<(), AppError> {
    let layout = verify(paths, plan, target, published)?;
    if let Some(agent) = target.target.agent_id.as_deref() {
        crate::attach::attach_agent_checked(
            paths,
            &layout,
            &layout.scope.skill_id(&target.target.name),
            agent,
            &|| verify(paths, plan, target, published).map(|_| ()),
        )?;
        return Ok(());
    }
    let copy = plan.copy.as_ref().unwrap();
    let skill = &copy.skills[&target.target.name];
    let name = &target.target.name;
    let snapshot = crate::backup::BackupStore::for_layout(paths, &layout).create(
        crate::backup::BackupAction::Install,
        name,
        None,
        &[],
        &[],
    )?;
    record(&snapshot.path);
    verify(paths, plan, target, false)?;
    fs::create_dir_all(&layout.canonical_dir)
        .map_err(|e| AppError::agent_attach_failed(e.to_string()))?;
    let staged = tempfile::Builder::new()
        .prefix(".copy-")
        .tempdir_in(&layout.canonical_dir)
        .map_err(|e| AppError::agent_attach_failed(e.to_string()))?;
    copy_tree(&skill.source, staged.path())
        .map_err(|e| AppError::agent_attach_failed(e.to_string()))?;
    if tree_fingerprint(staged.path())? != skill.content {
        return Err(AppError::batch_target_changed("复制期间全局内容已变化。"));
    }
    let hash = comparable_hash(staged.path())?;
    if hash != skill.entry.skill_folder_hash {
        return Err(AppError::batch_target_changed("复制版本与确认时不同。"));
    }
    verify(paths, plan, target, false)?;
    let original_lock = match fs::read(&layout.lock_path) {
        Ok(raw) => Some(raw),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(AppError::project_lock_read_failed(error.to_string())),
    };
    let mut lock: serde_json::Value = match &original_lock {
        Some(raw) => serde_json::from_slice(raw)
            .map_err(|e| AppError::project_lock_corrupt(e.to_string()))?,
        None => serde_json::json!({"version": 1, "skills": {}}),
    };
    lock["skills"][name] = serde_json::json!({
        "source": skill.entry.source, "sourceType": skill.entry.source_type, "sourceUrl": skill.entry.source_url,
        "ref": skill.entry.reference, "skillPath": skill.entry.skill_path, "computedHash": hash,
    });
    let bytes = serde_json::to_vec_pretty(&lock)
        .map_err(|e| AppError::project_lock_corrupt(e.to_string()))?;
    let baselines = BaselineStore::for_scope(paths, &layout.scope);
    let baseline_path = baselines.baseline_path(name);
    let previous_baseline = fs::read(&baseline_path).ok();
    let destination = layout.canonical_dir.join(name);
    crate::fsutil::rename_directory_new(staged.path(), &destination)
        .map_err(|e| AppError::agent_attach_failed(e.to_string()))?;
    let mut lock_written = false;
    let mut baseline_written = false;
    let result = (|| {
        ensure_lock_path(&layout)?;
        if fs::read(&layout.lock_path).ok() != original_lock {
            return Err(AppError::batch_target_changed(
                "项目安装记录在写入前发生变化。",
            ));
        }
        crate::fsutil::write_atomic(&layout.lock_path, &bytes)
            .map_err(|e| AppError::project_lock_read_failed(e.to_string()))?;
        lock_written = true;
        baselines.record_from_dir(name, &destination, true)?;
        baseline_written = true;
        verify(paths, plan, target, true)?;
        Ok(())
    })();
    if result.is_err() {
        let cleanup = (|| -> Result<(), AppError> {
            if identity(&layout.root)? != copy.root_identity {
                return Err(AppError::batch_target_changed("项目目录已变化。"));
            }
            ensure_lock_path(&layout)?;
            ensure_inside(&layout, &destination)?;
            if lock_written {
                if fs::read(&layout.lock_path).ok().as_deref() != Some(bytes.as_slice()) {
                    return Err(AppError::batch_target_changed(
                        "项目安装记录已变化，保留当前内容。",
                    ));
                }
                match &original_lock {
                    Some(raw) => crate::fsutil::write_atomic(&layout.lock_path, raw),
                    None => fs::remove_file(&layout.lock_path),
                }
                .map_err(|e| AppError::project_lock_read_failed(e.to_string()))?;
            }
            if baseline_written {
                match &previous_baseline {
                    Some(raw) => crate::fsutil::write_atomic(&baseline_path, raw),
                    None => baselines.clear(name),
                }
                .map_err(|e| AppError::task_internal("恢复副本基线", e))?;
            }
            if tree_fingerprint(&destination)? != skill.content {
                return Err(AppError::batch_target_changed(
                    "新副本内容已变化，保留当前内容。",
                ));
            }
            fs::remove_dir_all(&destination)
                .map_err(|e| AppError::task_internal("清理未完成副本", e))?;
            Ok(())
        })();
        if let Err(error) = cleanup {
            return Err(AppError::task_internal(
                "复制失败后的清理未完成，请查看备份",
                error,
            ));
        }
    }
    if result.is_ok() {
        let _ = crate::logs::OperationLog::for_scope(paths, &layout.scope).append(
            &crate::logs::OperationEntry::new(
                "copy_to_project",
                &layout.scope.skill_id(name),
                vec![],
                "copied",
                Some(paths.display(&snapshot.path)),
                None,
            ),
        );
    }
    result
}

fn copy_tree(source: &Path, destination: &Path) -> std::io::Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(fs::read_link(entry.path())?, target)?;
        } else if kind.is_dir() {
            fs::create_dir(&target)?;
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), target)?;
        } else {
            return Err(std::io::Error::other("不支持的技能文件类型。"));
        }
    }
    fs::set_permissions(destination, fs::metadata(source)?.permissions())?;
    Ok(())
}
