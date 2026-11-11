//! Record-only cleanup and a dedicated stale-only restoration authorization.
use super::{
    batch::{BatchAction, BatchPlan, BatchTarget, PlannedTarget},
    install::{self, InstallPlan, InstallRequest},
    session::{self, MutationAction},
};
use crate::{
    agents::{self, DetectionContext},
    backup::{BackupAction, BackupStore},
    baseline::BaselineStore,
    error::AppError,
    paths::AppPaths,
    scope_layout::ScopeLayout,
};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, HashMap},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StaleRecord {
    pub id: String,
    pub name: String,
    pub source: String,
    pub reference: Option<String>,
    pub clean_reason: Option<String>,
    pub restore_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StalePlan {
    pub expected: Vec<u8>,
    pub installs: HashMap<String, InstallPlan>,
    pub source_checked: bool,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Resolve every catalog directory, even for an agent whose detector is absent.
fn directories(paths: &AppPaths, layout: &ScopeLayout) -> Result<BTreeSet<PathBuf>, AppError> {
    let extras = agents::extras::load(paths);
    if !extras.warnings.is_empty() || !layout.warnings.is_empty() || !layout.escapes.is_empty() {
        return Err(AppError::path_invalid(format!(
            "目录检查不完整：{}",
            extras
                .warnings
                .iter()
                .chain(&layout.warnings)
                .chain(&layout.escapes)
                .cloned()
                .collect::<Vec<_>>()
                .join("；")
        )));
    }
    let mut dirs = BTreeSet::from([layout.canonical_dir.clone()]);
    dirs.extend(layout.agents.iter().map(|a| a.skills_dir.clone()));
    let definitions = agents::catalog()
        .agents
        .iter()
        .cloned()
        .chain(extras.agents.iter().map(|e| e.definition()));
    if layout.is_global() {
        let parent = crate::env::capture_process_env();
        let env = crate::env::child_env::build_child_env(crate::env::child_env::ChildEnvParams {
            home: paths.home(),
            node_dir: None,
            git_dir: None,
            parent: &parent,
        });
        let context = DetectionContext::new(paths.home(), paths.cli_cwd(), env);
        for def in definitions {
            if let Some(dir) = agents::resolve_global_skills_dir(&def, &context) {
                dirs.insert(dir);
            } else if def.global_skills_dir.is_some() {
                return Err(AppError::path_invalid(format!(
                    "无法解析 {} 的技能目录。",
                    def.display_name
                )));
            }
        }
    } else {
        for def in &agents::catalog().agents {
            dirs.insert(layout.root.join(&def.skills_dir));
        }
        for extra in &extras.agents {
            if let Some(dir) = &extra.project_skills_dir {
                dirs.insert(layout.root.join(dir));
            }
        }
    }
    if dirs.iter().any(|p| !p.is_absolute()) {
        return Err(AppError::path_invalid(
            "Agent 目录未解析为绝对路径，无法完整检查。",
        ));
    }
    Ok(dirs)
}

fn readable_directory(dir: &Path, layout: &ScopeLayout) -> Result<bool, AppError> {
    if !layout.is_global() {
        crate::project_path::lexical(&layout.root, dir)
            .and_then(|p| {
                crate::project_path::resolve(
                    &p,
                    &layout.root,
                    crate::project_path::Existence::AllowMissing,
                )
            })
            .map_err(|e| AppError::path_invalid(format!("项目目录无法安全检查：{e:?}")))?;
    }
    // Walk ancestors: ENOENT behind a broken link or a non-directory is not absence.
    let mut candidate = Some(dir);
    let mut exists = false;
    while let Some(path) = candidate {
        match fs::symlink_metadata(path) {
            Ok(meta) => {
                if !meta.is_dir() || meta.file_type().is_symlink() {
                    return Err(AppError::path_invalid(format!(
                        "目录不是可检查的普通目录：{}",
                        path.display()
                    )));
                }
                for entry in fs::read_dir(path).map_err(|e| {
                    AppError::path_invalid(format!("无法完整读取 {}：{e}", path.display()))
                })? {
                    entry.map_err(|e| AppError::path_invalid(e.to_string()))?;
                }
                exists = path == dir;
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => candidate = path.parent(),
            Err(e) => {
                return Err(AppError::path_invalid(format!(
                    "无法检查 {}：{e}",
                    path.display()
                )));
            }
        }
    }
    if exists {
        fs::canonicalize(dir).map_err(|e| AppError::path_invalid(e.to_string()))?;
    }
    Ok(exists)
}

pub fn absence(
    paths: &AppPaths,
    layout: &ScopeLayout,
    name: &str,
) -> Result<Vec<PathBuf>, AppError> {
    install::validate_skill_name(name)?;
    let dirs = directories(paths, layout)?;
    for dir in &dirs {
        if !readable_directory(dir, layout)? {
            continue;
        }
        match fs::symlink_metadata(dir.join(name)) {
            Ok(_) => {
                return Err(AppError::batch_target_changed(format!(
                    "{} 已有目录、文件或挂载，请重新预览。",
                    dir.join(name).display()
                )));
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(AppError::path_invalid(format!(
                    "无法确认 {name} 不存在：{e}"
                )));
            }
        }
    }
    Ok(dirs.into_iter().collect())
}

pub fn read_lock(layout: &ScopeLayout) -> Result<Vec<u8>, AppError> {
    layout.ensure_mutation_ready()?;
    layout.lock()?;
    let parent = layout
        .lock_path
        .parent()
        .ok_or_else(|| AppError::path_invalid("Lock 没有父目录。"))?;
    readable_directory(parent, layout)?;
    if !layout.lock_path.exists()
        && (crate::marketplace::records::path(&layout.lock_path).exists()
            || crate::expert_packs::records::path(&layout.lock_path).exists())
    {
        return Ok(format!(
            "{{\"version\":{},\"skills\":{{}}}}",
            if layout.is_global() { 3 } else { 1 }
        )
        .into_bytes());
    }
    let meta = fs::symlink_metadata(&layout.lock_path)
        .map_err(|e| AppError::lock_read_failed(e.to_string()))?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(AppError::lock_read_failed("Lock 必须是普通文件。"));
    }
    let bytes =
        fs::read(&layout.lock_path).map_err(|e| AppError::lock_read_failed(e.to_string()))?;
    let raw = std::str::from_utf8(&bytes).map_err(|e| AppError::lock_read_failed(e.to_string()))?;
    if layout.is_global() {
        crate::lock::LockFile::parse(raw)?;
    } else {
        crate::project_lock::ProjectLockFile::parse(raw)?;
    }
    Ok(bytes)
}

fn baseline_bytes(
    paths: &AppPaths,
    layout: &ScopeLayout,
    name: &str,
) -> Result<Option<Vec<u8>>, AppError> {
    let path = BaselineStore::for_scope(paths, &layout.scope).baseline_path(name);
    let root =
        fs::canonicalize(paths.app_data()).map_err(|e| AppError::backup_failed(e.to_string()))?;
    crate::project_path::resolve(&path, &root, crate::project_path::Existence::AllowMissing)
        .map_err(|e| AppError::backup_failed(format!("基线路径无法安全备份：{e:?}")))?;
    match fs::symlink_metadata(&path) {
        Ok(m) if m.is_file() && !m.file_type().is_symlink() => fs::read(&path)
            .map(Some)
            .map_err(|e| AppError::backup_failed(e.to_string())),
        Ok(_) => Err(AppError::backup_failed("基线不是普通文件。")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(AppError::backup_failed(e.to_string())),
    }
}

pub fn list(paths: &AppPaths, layout: &ScopeLayout) -> Result<Vec<StaleRecord>, AppError> {
    let lock = layout.lock()?;
    let catalog = layout.catalog(paths)?;
    let lock_reason = read_lock(layout)
        .err()
        .map(|e| e.detail.unwrap_or(e.message));
    let mut records = Vec::new();
    for (name, entry) in lock.entries() {
        if catalog.canonical.contains_key(name) || catalog.aggregates.contains_key(name) {
            continue;
        }
        let clean_reason = lock_reason.clone().or_else(|| {
            absence(paths, layout, name)
                .err()
                .map(|e| e.detail.unwrap_or(e.message))
        });
        let restore_reason = clean_reason.clone().or_else(|| {
            if crate::expert_packs::is_native_source(&entry.source_type) {
                return Some("请先清理市场记录，再到技能市场重新安装。".into());
            }
            crate::update_target::resolve(entry)
                .err()
                .map(|e| e.detail().to_string())
        });
        records.push(StaleRecord {
            id: layout.scope.skill_id(name),
            name: name.clone(),
            source: entry.source.clone(),
            reference: entry.reference.clone(),
            clean_reason,
            restore_reason,
        });
    }
    records.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(records)
}

pub fn inspect(
    paths: &AppPaths,
    layout: &ScopeLayout,
    action: BatchAction,
    skill_id: &str,
    name: &str,
    agents: &[String],
) -> Result<(PlannedTarget, Option<InstallPlan>), AppError> {
    let raw = read_lock(layout)?;
    let value: Value =
        serde_json::from_slice(&raw).map_err(|e| AppError::lock_read_failed(e.to_string()))?;
    let entry = layout.lock()?.get(name).cloned();
    let mut target = BatchTarget {
        paths: vec![
            paths.display(&layout.lock_path),
            paths.display(&BaselineStore::for_scope(paths, &layout.scope).baseline_path(name)),
        ],
        source: entry.as_ref().map(|e| e.source.clone()),
        reference: entry.as_ref().and_then(|e| e.reference.clone()),
        ..BatchTarget::new(skill_id, name, None, None)
    };
    let mut install_plan = None;
    let assessment = (|| {
        let entry = entry
            .as_ref()
            .ok_or_else(|| AppError::batch_target_changed("安装记录已不存在。"))?;
        let dirs = absence(paths, layout, name)?;
        let baseline = baseline_bytes(paths, layout, name)?;
        if action == BatchAction::Restore {
            if crate::expert_packs::is_native_source(&entry.source_type) {
                return Err(AppError::batch_target_changed(
                    "请先清理市场记录，再到技能市场重新安装。",
                ));
            }
            let source = crate::update_target::resolve(entry)
                .map_err(|e| AppError::path_invalid(e.detail()))?;
            let mut plan = install::validate_request(
                paths,
                layout,
                &InstallRequest {
                    source: source.source.clone(),
                    skills: vec![name.into()],
                    agents: agents.to_vec(),
                    retry: false,
                },
            )?;
            plan.source = source.cli_source();
            target.source = Some(source.source);
            target.reference = source.reference;
            target.paths = vec![paths.display(&layout.canonical_dir.join(name))];
            target.paths.extend(
                plan.targets
                    .iter()
                    .filter(|t| !t.universal)
                    .map(|t| paths.display(&t.skills_dir.join(name))),
            );
            install_plan = Some(plan);
        }
        Ok::<_, AppError>(digest(
            format!("{:?}|{:?}|{:?}|{:?}", entry, baseline, dirs, install_plan).as_bytes(),
        ))
    })();
    let fingerprint = match assessment {
        Ok(fingerprint) => {
            target.eligible = true;
            fingerprint
        }
        Err(error) => {
            target.reason = Some(error.detail.unwrap_or(error.message));
            digest(format!("{:?}|{:?}", value["skills"][name], target.reason).as_bytes())
        }
    };
    Ok((
        PlannedTarget {
            target,
            fingerprint,
            lock_entry: entry,
        },
        install_plan,
    ))
}

pub fn revision(plan: &mut BatchPlan) {
    plan.preview.targets = plan.targets.iter().map(|t| t.target.clone()).collect();
    plan.preview.revision = digest(
        format!(
            "{:?}|{:?}|{:?}|{:?}|{:?}",
            plan.root,
            plan.project_revision,
            plan.request,
            plan.targets,
            plan.stale.as_ref().map(|s| &s.expected)
        )
        .as_bytes(),
    );
}

/// Reuse source checks only for the identical local authorization and document.
pub fn replan(
    paths: &AppPaths,
    layout: &ScopeLayout,
    original: &BatchPlan,
) -> Result<BatchPlan, AppError> {
    let mut fresh = super::batch::plan(
        paths,
        layout,
        original.request.clone(),
        original.preview.id.clone(),
        original.updates.clone(),
    )?;
    if fresh.stale.as_ref().map(|s| &s.expected) == original.stale.as_ref().map(|s| &s.expected) {
        for target in &mut fresh.targets {
            if let Some(old) = original
                .targets
                .iter()
                .find(|t| t.target.key == target.target.key && t.fingerprint == target.fingerprint)
            {
                target.target = old.target.clone();
            }
        }
    }
    if fresh
        .targets
        .iter()
        .zip(&original.targets)
        .all(|(a, b)| a.fingerprint == b.fingerprint)
        && fresh.stale.as_ref().map(|s| &s.expected) == original.stale.as_ref().map(|s| &s.expected)
    {
        fresh.stale.as_mut().unwrap().source_checked =
            original.stale.as_ref().unwrap().source_checked;
    }
    revision(&mut fresh);
    Ok(fresh)
}

pub fn verify(
    paths: &AppPaths,
    layout: &ScopeLayout,
    plan: &BatchPlan,
    target: &PlannedTarget,
) -> Result<(), AppError> {
    if read_lock(layout)? != plan.stale.as_ref().expect("stale plan").expected {
        return Err(AppError::lock_read_failed(
            "Lock 已被外部修改，请重新预览。",
        ));
    }
    if let Err(e) = absence(paths, layout, &target.target.name) {
        return Err(if e.code == crate::error::ErrorCode::BatchTargetChanged {
            e
        } else {
            AppError::lock_read_failed(format!("目录检查不完整：{}", e.detail.unwrap_or(e.message)))
        });
    }
    let (fresh, _) = inspect(
        paths,
        layout,
        plan.preview.action,
        &target.target.skill_id,
        &target.target.name,
        &plan.request.agent_ids,
    )?;
    if !fresh.target.eligible || fresh.fingerprint != target.fingerprint {
        return Err(AppError::batch_target_changed(
            fresh
                .target
                .reason
                .unwrap_or_else(|| "记录、基线或目录范围已变化，请重新预览。".into()),
        ));
    }
    Ok(())
}

pub fn backup(
    paths: &AppPaths,
    layout: &ScopeLayout,
    name: &str,
    action: BackupAction,
    record: &dyn Fn(&Path),
) -> Result<PathBuf, AppError> {
    let baseline = baseline_bytes(paths, layout, name)?;
    let snapshot = BackupStore::for_layout(paths, layout).create(action, name, None, &[], &[])?;
    if let Some(bytes) = baseline {
        fs::write(snapshot.path.join("baseline.json"), bytes)
            .map_err(|e| AppError::backup_failed(e.to_string()))?;
    } else {
        fs::write(
            snapshot.path.join("baseline-absent.txt"),
            b"baseline absent\n",
        )
        .map_err(|e| AppError::backup_failed(e.to_string()))?;
    }
    record(&snapshot.path);
    Ok(snapshot.path)
}

pub fn clean(
    paths: &AppPaths,
    layout: &ScopeLayout,
    plan: &BatchPlan,
    target: &PlannedTarget,
    record: &dyn Fn(&Path),
    check: &dyn Fn() -> Result<(), AppError>,
) -> Result<Vec<AppError>, AppError> {
    check()?;
    backup(
        paths,
        layout,
        &target.target.name,
        BackupAction::CleanRecords,
        record,
    )?;
    check()?;
    if target
        .lock_entry
        .as_ref()
        .is_some_and(|e| crate::expert_packs::is_native_source(&e.source_type))
    {
        if target
            .lock_entry
            .as_ref()
            .is_some_and(|e| e.source_type == "skillhub-pack")
        {
            let current = crate::expert_packs::records::Records::read(&layout.lock_path)?;
            if current
                .workflows
                .get(&target.target.name)
                .map(|r| r.entry())
                .as_ref()
                != target.lock_entry.as_ref()
            {
                return Err(AppError::batch_target_changed("工作流记录已变化。"));
            }
            crate::expert_packs::records::Records::remove_workflow(
                &layout.lock_path,
                &target.target.name,
            )?;
        } else {
            let current = crate::marketplace::records::Records::read(&layout.lock_path)?;
            if current
                .skills
                .get(&target.target.name)
                .map(|r| r.entry())
                .as_ref()
                != target.lock_entry.as_ref()
            {
                return Err(AppError::batch_target_changed("市场记录已变化。"));
            }
            crate::marketplace::records::Records::remove(&layout.lock_path, &target.target.name)?;
        }
        if layout.lock()?.contains(&target.target.name) {
            return Err(AppError::verify_failed("市场记录清理后仍然存在。"));
        }
        let mut warnings = vec![];
        if let Err(e) = BaselineStore::for_scope(paths, &layout.scope).clear(&target.target.name) {
            warnings.push(AppError::baseline_clear_failed(e));
        }
        return Ok(warnings);
    }
    let mut value: Value =
        serde_json::from_slice(&plan.stale.as_ref().expect("stale plan").expected)
            .map_err(|e| AppError::lock_read_failed(e.to_string()))?;
    value["skills"]
        .as_object_mut()
        .ok_or_else(|| AppError::lock_read_failed("Lock 缺少 skills。"))?
        .remove(&target.target.name);
    let bytes =
        serde_json::to_vec_pretty(&value).map_err(|e| AppError::lock_read_failed(e.to_string()))?;
    let parent = layout.lock_path.parent().unwrap();
    let mode = fs::metadata(&layout.lock_path)
        .map_err(|e| AppError::lock_read_failed(e.to_string()))?
        .permissions();
    let mut temp = tempfile::NamedTempFile::new_in(parent)
        .map_err(|e| AppError::lock_read_failed(e.to_string()))?;
    temp.as_file()
        .set_permissions(mode)
        .and_then(|_| temp.write_all(&bytes))
        .and_then(|_| temp.as_file().sync_all())
        .map_err(|e| AppError::lock_read_failed(e.to_string()))?;
    check()?;
    temp.persist(&layout.lock_path)
        .map_err(|e| AppError::lock_read_failed(e.to_string()))?;
    if read_lock(layout)? != bytes {
        return Err(AppError::lock_read_failed(
            "写入后 Lock 校验失败，操作已写入，请查看备份。",
        ));
    }
    let mut warnings = Vec::new();
    if let Err(e) = BaselineStore::for_scope(paths, &layout.scope).clear(&target.target.name) {
        warnings.push(crate::baseline::write_warning(AppError::verify_failed(
            e.to_string(),
        )));
    }
    Ok(warnings)
}

/// Advance only to the document our writer observed, never adopt a later edit.
pub fn advance(
    layout: &ScopeLayout,
    plan: &BatchPlan,
    name: &str,
    succeeded: bool,
    observed: Option<Vec<u8>>,
) -> Result<Vec<u8>, AppError> {
    let before = &plan.stale.as_ref().unwrap().expected;
    let market = plan
        .targets
        .iter()
        .find(|t| t.target.name == name)
        .and_then(|t| t.lock_entry.as_ref())
        .is_some_and(|e| crate::expert_packs::is_native_source(&e.source_type));
    let expected = if market {
        before.clone()
    } else if plan.preview.action == BatchAction::CleanRecords && succeeded {
        let mut value: Value = serde_json::from_slice(before)
            .map_err(|e| AppError::lock_read_failed(e.to_string()))?;
        value["skills"].as_object_mut().unwrap().remove(name);
        serde_json::to_vec_pretty(&value).map_err(|e| AppError::lock_read_failed(e.to_string()))?
    } else {
        observed.unwrap_or_else(|| before.clone())
    };
    let mut old: Value =
        serde_json::from_slice(before).map_err(|e| AppError::lock_read_failed(e.to_string()))?;
    let mut new: Value =
        serde_json::from_slice(&expected).map_err(|e| AppError::lock_read_failed(e.to_string()))?;
    old["skills"].as_object_mut().unwrap().remove(name);
    new["skills"].as_object_mut().unwrap().remove(name);
    if old["skills"] != new["skills"] || read_lock(layout)? != expected {
        return Err(AppError::lock_read_failed(
            "Lock 在写入后变化，已停止后续操作，请重新预览。",
        ));
    }
    Ok(expected)
}

pub fn observed_state(layout: &ScopeLayout, name: &str) -> String {
    let canonical = fs::symlink_metadata(layout.canonical_dir.join(name)).is_ok();
    let lock = layout.lock().map(|l| l.contains(name));
    format!(
        "实际状态：共享目录{}；安装记录{}。请查看现有文件与备份。",
        if canonical { "存在" } else { "不存在" },
        match lock {
            Ok(true) => "保留",
            Ok(false) => "不存在",
            Err(_) => "无法读取",
        }
    )
}

pub(crate) struct RestoreAction {
    pub inner: install::InstallAction,
    pub paths: AppPaths,
    pub layout: ScopeLayout,
    pub plan: BatchPlan,
    pub target: PlannedTarget,
    pub observed: std::sync::Arc<std::sync::Mutex<Option<Vec<u8>>>>,
}
impl MutationAction for RestoreAction {
    type Plan = InstallPlan;
    type Unit = InstallPlan;
    fn profile(&self, p: &InstallPlan) -> session::SessionProfile {
        self.inner.profile(p)
    }
    fn begin(&self, p: &InstallPlan) -> Result<session::StartOutcome<InstallPlan>, AppError> {
        self.inner.begin(p)
    }
    fn preflight(
        &self,
        _: &InstallPlan,
        _r: &session::Reporter,
    ) -> Result<session::Preflight, AppError> {
        self.before_write(&self.plan.stale.as_ref().unwrap().installs[&self.target.target.name])?;
        let snapshot = backup(
            &self.paths,
            &self.layout,
            &self.target.target.name,
            BackupAction::Restore,
            &|_| {},
        )?;
        Ok(session::Preflight {
            snapshot,
            post_message: None,
            warnings: Vec::new(),
        })
    }
    fn before_write(&self, _: &InstallPlan) -> Result<(), AppError> {
        super::batch::verify_target(&self.paths, &self.layout, &self.plan, &self.target)
    }
    fn command(
        &self,
        p: &InstallPlan,
        rt: &crate::cli::CliRuntime,
        r: &session::Reporter,
    ) -> crate::process::CommandSpec {
        self.inner.command(p, rt, r)
    }
    fn complete(
        &self,
        p: &InstallPlan,
        o: crate::process::ExecOutput,
        f: &session::Preflight,
        r: &session::Reporter,
    ) -> Result<session::UnitOutcome, AppError> {
        *self.observed.lock().unwrap_or_else(|p| p.into_inner()) = Some(read_lock(&self.layout)?);
        if let install::InstallOutput::Results(results) = install::parse_install_output(&o.stdout) {
            if results
                .iter()
                .filter_map(|r| r.name.as_ref())
                .any(|name| !p.skills.contains(name))
            {
                return Err(AppError::lock_read_failed(
                    "CLI 返回了未确认的技能，已停止后续操作，请检查实际状态。",
                ));
            }
            for result in results
                .iter()
                .filter(|r| r.status == install::CliSkillStatus::Installed)
            {
                let name = result.name.as_ref().expect("installed result has name");
                let entry_point = self.layout.canonical_dir.join(name).join("SKILL.md");
                if !fs::metadata(&entry_point).is_ok_and(|metadata| metadata.is_file()) {
                    return Err(AppError::verify_failed(format!(
                        "恢复后的技能缺少可读取的 SKILL.md：{}",
                        entry_point.display()
                    )));
                }
            }
        }
        let outcome = self.inner.complete(p, o, f, r);
        // Even a failed CLI can have written files or records. Never claim rollback.
        let before: Value = serde_json::from_slice(&self.plan.stale.as_ref().unwrap().expected)
            .map_err(|e| AppError::lock_read_failed(e.to_string()))?;
        let after: Value = serde_json::from_slice(&read_lock(&self.layout)?)
            .map_err(|e| AppError::lock_read_failed(e.to_string()))?;
        let mut old = before["skills"].clone();
        let mut new = after["skills"].clone();
        old.as_object_mut()
            .unwrap()
            .remove(&self.target.target.name);
        new.as_object_mut()
            .unwrap()
            .remove(&self.target.target.name);
        if old != new {
            return Err(AppError::lock_read_failed(
                "恢复后其他安装记录变化，已停止后续操作，请检查实际状态与备份。",
            ));
        }
        outcome
    }
    fn on_cancelled(&self, _: &InstallPlan, _: &session::Preflight, _: Option<AppError>) {}
    fn on_infrastructure_failure(
        &self,
        u: &InstallPlan,
        e: AppError,
        p: Option<&session::Preflight>,
        r: &session::Reporter,
    ) -> session::FailureDisposition {
        *self.observed.lock().unwrap_or_else(|p| p.into_inner()) = read_lock(&self.layout).ok();
        self.inner.on_infrastructure_failure(u, e, p, r)
    }
}

#[cfg(test)]
mod tests {
    use super::super::batch::{self, BatchRequest};
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn fixture() -> (tempfile::TempDir, AppPaths, ScopeLayout) {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(tmp.path().join("home"), tmp.path().join("data"));
        paths.ensure_dirs().unwrap();
        fs::create_dir_all(paths.canonical_skills_dir()).unwrap();
        let entry = serde_json::json!({ "source":"acme/skills", "sourceType":"github", "sourceUrl":"https://github.com/acme/skills.git", "skillFolderHash":"abc", "installedAt":"2026-01-01", "updatedAt":"2026-01-01", "unknownEntry":{"keep":42} });
        fs::write(paths.lock_path(), serde_json::json!({"version":3,"unknownTop":{"keep":true},"skills":{"alpha":entry,"beta":entry,"retained":entry}}).to_string()).unwrap();
        let layout = ScopeLayout::global(&paths, Vec::new());
        (tmp, paths, layout)
    }
    fn plan(paths: &AppPaths, layout: &ScopeLayout, names: &[&str]) -> BatchPlan {
        batch::plan(
            paths,
            layout,
            BatchRequest {
                action: BatchAction::CleanRecords,
                skill_ids: names.iter().map(|n| layout.scope.skill_id(n)).collect(),
                agent_ids: Vec::new(),
                target_keys: None,
                project_root: None,
            },
            "preview".into(),
            HashMap::new(),
        )
        .unwrap()
    }

    #[test]
    fn cleanup_preserves_unknown_fields_files_permissions_and_advances_own_document() {
        let (_tmp, paths, layout) = fixture();
        fs::set_permissions(paths.lock_path(), fs::Permissions::from_mode(0o640)).unwrap();
        let baseline = BaselineStore::new(&paths).baseline_path("alpha");
        fs::write(&baseline, b"original baseline bytes").unwrap();
        let retained = paths.canonical_skills_dir().join("retained");
        fs::create_dir_all(&retained).unwrap();
        fs::write(retained.join("attachment.txt"), b"retain").unwrap();
        let before = read_lock(&layout).unwrap();
        let original: Value = serde_json::from_slice(&before).unwrap();
        let mut plan = plan(&paths, &layout, &["alpha", "beta"]);
        let backups = std::cell::RefCell::new(Vec::new());
        for target in &plan.targets.clone() {
            clean(
                &paths,
                &layout,
                &plan,
                target,
                &|p| backups.borrow_mut().push(p.to_path_buf()),
                &|| verify(&paths, &layout, &plan, target),
            )
            .unwrap();
            plan.stale.as_mut().unwrap().expected = read_lock(&layout).unwrap();
        }
        let after: Value = serde_json::from_slice(&read_lock(&layout).unwrap()).unwrap();
        assert!(after["skills"].get("alpha").is_none());
        assert!(after["skills"].get("beta").is_none());
        assert_eq!(after["skills"]["retained"], original["skills"]["retained"]);
        assert_eq!(after["unknownTop"], original["unknownTop"]);
        assert_eq!(
            fs::read(retained.join("attachment.txt")).unwrap(),
            b"retain"
        );
        assert!(!baseline.exists());
        assert_eq!(
            fs::metadata(paths.lock_path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
        assert_eq!(
            fs::read(backups.borrow()[0].join("skill-lock.json")).unwrap(),
            before
        );
        assert_eq!(
            fs::read(backups.borrow()[0].join("baseline.json")).unwrap(),
            b"original baseline bytes"
        );
    }

    #[test]
    fn any_occupancy_or_incomplete_scan_blocks_cleanup_even_for_undetected_agents() {
        let (_tmp, paths, layout) = fixture();
        let dir = paths.home().join(".codex/skills");
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("alpha");
        fs::write(&target, b"file").unwrap();
        assert!(absence(&paths, &layout, "alpha").is_err());
        fs::remove_file(&target).unwrap();
        fs::create_dir(&target).unwrap();
        assert!(absence(&paths, &layout, "alpha").is_err());
        fs::remove_dir(&target).unwrap();
        symlink("missing", &target).unwrap();
        assert!(absence(&paths, &layout, "alpha").is_err());
        fs::remove_file(&target).unwrap();
        fs::remove_dir(&dir).unwrap();
        symlink("missing", &dir).unwrap();
        assert!(absence(&paths, &layout, "alpha").is_err());
    }

    #[test]
    fn external_edit_backup_failure_and_final_check_failure_leave_lock_untouched() {
        let (_tmp, paths, layout) = fixture();
        let plan = plan(&paths, &layout, &["alpha"]);
        let target = &plan.targets[0];
        let original = read_lock(&layout).unwrap();
        let edited = [original.as_slice(), b"\n"].concat();
        fs::write(paths.lock_path(), &edited).unwrap();
        assert!(
            clean(&paths, &layout, &plan, target, &|_| {}, &|| verify(
                &paths, &layout, &plan, target
            ))
            .is_err()
        );
        assert_eq!(read_lock(&layout).unwrap(), edited);
        fs::write(paths.lock_path(), &original).unwrap();
        fs::remove_dir_all(paths.backups_dir()).unwrap();
        fs::write(paths.backups_dir(), b"blocked").unwrap();
        assert!(
            clean(&paths, &layout, &plan, target, &|_| {}, &|| verify(
                &paths, &layout, &plan, target
            ))
            .is_err()
        );
        assert_eq!(read_lock(&layout).unwrap(), original);
        fs::remove_file(paths.backups_dir()).unwrap();
        let checks = std::cell::Cell::new(0);
        assert!(
            clean(&paths, &layout, &plan, target, &|_| {}, &|| {
                checks.set(checks.get() + 1);
                if checks.get() == 3 {
                    Err(AppError::lock_read_failed("external change before rename"))
                } else {
                    Ok(())
                }
            })
            .is_err()
        );
        assert_eq!(read_lock(&layout).unwrap(), original);
    }

    #[test]
    fn corrupt_unsupported_and_symlink_locks_are_never_written() {
        let (_tmp, paths, layout) = fixture();
        for raw in ["{bad", "{\"version\":99,\"skills\":{}}"] {
            fs::write(paths.lock_path(), raw).unwrap();
            assert!(read_lock(&layout).is_err());
        }
        let outside = paths.app_data().join("external.json");
        fs::rename(paths.lock_path(), &outside).unwrap();
        symlink(&outside, paths.lock_path()).unwrap();
        assert!(read_lock(&layout).is_err());
    }

    #[test]
    fn temporary_file_and_atomic_replace_failures_preserve_the_original_lock() {
        for block_publish in [false, true] {
            let (_tmp, paths, layout) = fixture();
            let plan = plan(&paths, &layout, &["alpha"]);
            let original = read_lock(&layout).unwrap();
            let parent = layout.lock_path.parent().unwrap();
            let mode = fs::metadata(parent).unwrap().permissions();
            let checks = std::cell::Cell::new(0);
            let result = clean(
                &paths,
                &layout,
                &plan,
                &plan.targets[0],
                &|_| {
                    if !block_publish {
                        fs::set_permissions(parent, fs::Permissions::from_mode(0o500)).unwrap();
                    }
                },
                &|| {
                    checks.set(checks.get() + 1);
                    if block_publish && checks.get() == 3 {
                        fs::set_permissions(parent, fs::Permissions::from_mode(0o500)).unwrap();
                    }
                    verify(&paths, &layout, &plan, &plan.targets[0])
                },
            );
            fs::set_permissions(parent, mode).unwrap();
            assert!(result.is_err(), "publication phase: {block_publish}");
            assert_eq!(read_lock(&layout).unwrap(), original);
        }
    }

    #[test]
    fn baseline_cleanup_failure_is_a_warning_after_the_lock_was_removed() {
        let (_tmp, paths, layout) = fixture();
        let baseline = BaselineStore::new(&paths).baseline_path("alpha");
        fs::write(&baseline, b"original baseline").unwrap();
        let plan = plan(&paths, &layout, &["alpha"]);
        let target = &plan.targets[0];
        let directory = baseline.parent().unwrap();
        let warnings = clean(
            &paths,
            &layout,
            &plan,
            target,
            &|_| {
                fs::set_permissions(directory, fs::Permissions::from_mode(0o500)).unwrap();
            },
            &|| verify(&paths, &layout, &plan, target),
        )
        .unwrap();
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(!layout.lock().unwrap().contains("alpha"));
        assert!(baseline.is_file());
        assert_eq!(warnings.len(), 1);
        advance(&layout, &plan, "alpha", true, None).unwrap();
    }
}
