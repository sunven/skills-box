//! SkillHub writes only into a private staging directory. Publication uses the
//! same scope ownership, snapshots, attachment checks and task results as Git.
use super::{
    install::InstallTarget,
    manager::{TaskAction, TaskPhase, TaskSkillResult, TaskSkillStatus},
    session::*,
};
use crate::{
    backup::{BackupAction, BackupStore},
    baseline::{BaselineStore, fingerprint_folder},
    cli::{
        CliRuntime,
        preview::{CliCommandEntry, CliCommandPreview, CommandPlan},
    },
    error::AppError,
    inventory::AttachmentMode,
    marketplace::{
        provider::Detail,
        records::{Installation, Records},
        runtime::Runtime,
    },
    paths::AppPaths,
    process::{CommandSpec, ExecOutput},
    scope_layout::ScopeLayout,
};
use serde::Serialize;
use std::{fs, sync::Arc};

#[derive(Debug)]
pub struct StagingDirectory {
    _directory: tempfile::TempDir,
    path: std::path::PathBuf,
}
impl StagingDirectory {
    pub fn new() -> Result<Arc<Self>, AppError> {
        let directory = tempfile::Builder::new()
            .prefix("skills-box-skillhub-")
            .tempdir()
            .map_err(AppError::marketplace_unavailable)?;
        let path = fs::canonicalize(directory.path()).map_err(AppError::marketplace_unavailable)?;
        Ok(Arc::new(Self {
            _directory: directory,
            path,
        }))
    }
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
    fn check(&self) -> Result<(), AppError> {
        if !fs::symlink_metadata(&self.path).is_ok_and(|m| m.is_dir())
            || fs::canonicalize(&self.path).ok().as_ref() != Some(&self.path)
        {
            return Err(AppError::cli_command_changed());
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub detail: Detail,
    pub runtime: Runtime,
    pub stage: Arc<StagingDirectory>,
    pub layout: ScopeLayout,
    pub targets: Vec<InstallTarget>,
    pub previous: Option<Installation>,
    pub previous_paths: Option<super::uninstall::UninstallPlan>,
    pub previous_fingerprint: Option<String>,
    pub created_at: u64,
}
impl Plan {
    pub fn name(&self) -> &str {
        self.detail
            .original_name
            .as_deref()
            .expect("validated original name")
    }
    pub fn command(&self) -> CommandSpec {
        self.runtime.install(
            &self.detail.summary.coordinate,
            &self.detail.summary.version,
            self.stage.path(),
        )
    }
    pub fn check(&self, paths: &AppPaths, layout: &ScopeLayout) -> Result<(), AppError> {
        self.stage.check()?;
        layout.ensure_mutation_ready()?;
        layout.lock()?;
        if Records::read(&layout.lock_path)?
            .skills
            .values()
            .any(|record| {
                record.coordinate == self.detail.summary.coordinate && record.name != self.name()
            })
        {
            return Err(AppError::skill_conflict(
                self.name(),
                "当前作用域已安装同一市场坐标，请查看已有安装。",
            ));
        }
        if layout.scope != self.layout.scope
            || layout.root != self.layout.root
            || layout.project.as_ref().map(|p| &p.check_revision)
                != self.layout.project.as_ref().map(|p| &p.check_revision)
        {
            return Err(AppError::batch_target_changed(
                "项目目录已变化，请重新预览。",
            ));
        }
        if Records::read(&layout.lock_path)?.skills.get(self.name()) != self.previous.as_ref() {
            return Err(AppError::cli_command_changed());
        }
        if let Some(previous) = &self.previous_paths {
            let current = super::uninstall::plan_uninstall(
                paths,
                layout,
                &layout.scope.skill_id(self.name()),
            )?;
            if &current != previous
                || Some(
                    fingerprint_folder(&layout.canonical_dir.join(self.name()))
                        .map_err(AppError::skill_fingerprint_failed)?
                        .content_hash,
                ) != self.previous_fingerprint
            {
                return Err(AppError::batch_target_changed(
                    "本地内容或挂载已变化，请重新确认更新。",
                ));
            }
        } else if let Some(error) =
            super::install::conflicts_for(paths, layout, &[self.name().into()], None)?
        {
            return Err(error);
        }
        let ids = self
            .targets
            .iter()
            .map(|t| t.id.clone())
            .collect::<Vec<_>>();
        // Updates may retain attachments to agents no longer detected. Their
        // concrete path state is validated by the captured uninstall plan.
        if self.previous.is_none() {
            let (_, targets) = super::install::validate_selected_agents(paths, layout, &ids)?;
            if targets != self.targets {
                return Err(AppError::cli_command_changed());
            }
            let located = crate::inventory::LocatedSkill {
                primary_path: Some(layout.canonical_dir.join(self.name())),
                name: self.name().into(),
                management: crate::inventory::ManagementState::Managed,
                canonical_path: Some(layout.canonical_dir.join(self.name())),
                lock_entry: Some(self.installation(String::new()).entry()),
                attachments: vec![],
            };
            let agents = if layout.is_global() {
                layout.agents.clone()
            } else {
                layout.install_targets(&crate::inventory::detected_agent_dirs(paths))
            };
            for target in self.targets.iter().filter(|t| !t.universal) {
                crate::attachment_target::resolve_copy_attach(
                    &layout.canonical_dir,
                    &located,
                    &agents,
                    &target.id,
                    crate::attachment_target::TargetContext::for_layout(layout),
                )?;
            }
        }
        if let Some(root) = layout.project_root() {
            crate::project_path::resolve(
                &layout.canonical_dir,
                root,
                crate::project_path::Existence::AllowMissing,
            )
            .map_err(|e| AppError::path_invalid(format!("{e:?}")))?;
        }
        if fs::symlink_metadata(&layout.canonical_dir).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(AppError::path_invalid("技能目录不能是符号链接"));
        }
        Ok(())
    }
    fn installation(&self, hash: String) -> Installation {
        let now = httpdate::fmt_http_date(std::time::SystemTime::now());
        Installation {
            name: self.name().into(),
            coordinate: self.detail.summary.coordinate.clone(),
            version: self.detail.summary.version.clone(),
            content_hash: hash,
            installed_at: self
                .previous
                .as_ref()
                .map(|r| r.installed_at.clone())
                .unwrap_or_else(|| now.clone()),
            updated_at: now,
        }
    }
}
impl CommandPlan for Plan {
    fn needs_cli(&self) -> bool {
        false
    }
    fn command_preview(&self, layout: &ScopeLayout, _: Option<&CliRuntime>) -> CliCommandPreview {
        CliCommandPreview::new(
            &layout.scope,
            if self.previous.is_some() {
                "marketplace-update"
            } else {
                "marketplace-install"
            },
            vec![CliCommandEntry {
                name: self.name().into(),
                command: Some(crate::cli::commands::display_command(&self.command())),
                reason: Some(format!(
                    "SkillHub {} · {}",
                    self.detail.summary.coordinate.canonical(),
                    self.detail.summary.version
                )),
            }],
        )
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub detail: Detail,
    pub commands: CliCommandPreview,
}

pub(crate) fn publish(
    paths: &AppPaths,
    plan: &Plan,
    output: &ExecOutput,
) -> Result<Vec<AppError>, AppError> {
    let root = validate_staged(plan, output)?;
    publish_staged(paths, plan, &root)
}

pub(crate) fn validate_staged(
    plan: &Plan,
    output: &ExecOutput,
) -> Result<std::path::PathBuf, AppError> {
    plan.stage.check()?;
    if !output.success() {
        return Err(AppError::cli_exit_nonzero(format!(
            "SkillHub：{}",
            output.stderr
        )));
    }
    let result: serde_json::Value = serde_json::from_str(output.stdout.trim())
        .map_err(|e| AppError::cli_invalid_json(e.to_string()))?;
    let root = fs::canonicalize(plan.stage.path())
        .map_err(|e| AppError::verify_failed(e.to_string()))?
        .join(plan.detail.summary.coordinate.canonical());
    if fs::canonicalize(&root).map_err(|e| AppError::verify_failed(e.to_string()))? != root {
        return Err(AppError::verify_failed("CLI 暂存目录包含间接路径。"));
    }
    if result["success"] != true
        || result["slug"] != plan.detail.summary.coordinate.canonical()
        || result["targetDir"].as_str() != root.to_str()
    {
        return Err(AppError::verify_failed(
            "SkillHub 返回的坐标或目录与请求不一致。",
        ));
    }
    let lock: serde_json::Value = serde_json::from_slice(
        &fs::read(plan.stage.path().join(".skills_store_lock.json"))
            .map_err(|e| AppError::verify_failed(e.to_string()))?,
    )
    .map_err(|e| AppError::verify_failed(e.to_string()))?;
    let download = lock["skills"][plan.detail.summary.coordinate.canonical()]["zip_url"]
        .as_str()
        .and_then(|url| reqwest::Url::parse(url).ok())
        .ok_or_else(|| AppError::verify_failed("CLI 未记录具体版本的下载地址。"))?;
    let params: std::collections::BTreeMap<_, _> = download.query_pairs().into_owned().collect();
    if download.scheme() != "https"
        || download.host_str() != Some("api.skillhub.cn")
        || download.path() != "/api/v1/download"
        || params.get("slug") != Some(&plan.detail.summary.coordinate.slug)
        || params.get("namespace") != Some(&plan.detail.summary.coordinate.namespace)
        || params.get("version") != Some(&plan.detail.summary.version)
    {
        return Err(AppError::verify_failed(
            "CLI 下载来源、坐标或请求版本与确认内容不一致。",
        ));
    }
    // Never trust CLI JSON version or _meta.json: both can report latest even
    // when the explicit release was downloaded. Validate the selected content.
    for item in walkdir::WalkDir::new(&root).follow_links(false) {
        let item = item.map_err(|e| AppError::verify_failed(e.to_string()))?;
        if item.file_type().is_symlink()
            || (!item.file_type().is_file() && !item.file_type().is_dir())
        {
            return Err(AppError::verify_failed("技能包包含符号链接或特殊文件。"));
        }
    }
    let markdown = fs::read_to_string(root.join("SKILL.md"))
        .map_err(|e| AppError::verify_failed(e.to_string()))?;
    if crate::marketplace::original_name(&markdown)? != plan.name()
        || markdown != plan.detail.markdown
    {
        return Err(AppError::verify_failed(
            "下载内容与确认版本的 SKILL.md 不一致，请刷新详情。",
        ));
    }
    Ok(root)
}

pub(crate) fn publish_staged(
    paths: &AppPaths,
    plan: &Plan,
    root: &std::path::Path,
) -> Result<Vec<AppError>, AppError> {
    let registry = crate::projects::ProjectRegistry::new(paths.clone());
    let layout = ScopeLayout::resolve_detected(paths, &registry, &plan.layout.scope)?;
    plan.check(paths, &layout)?;
    fs::create_dir_all(&layout.canonical_dir)
        .map_err(|e| AppError::verify_failed(e.to_string()))?;
    // Copy onto the target volume before replacing anything. Publication is
    // no-overwrite rename; the completed backup survives subsequent failures.
    let publication = tempfile::Builder::new()
        .prefix(".marketplace-")
        .tempdir_in(&layout.canonical_dir)
        .map_err(|e| AppError::verify_failed(e.to_string()))?;
    let candidate = publication.path().join("candidate");
    crate::backup::copy_tree(&root, &candidate)
        .map_err(|e| AppError::verify_failed(e.to_string()))?;
    plan.check(paths, &layout)?;
    let canonical = layout.canonical_dir.join(plan.name());
    if plan.previous.is_some() {
        crate::fsutil::rename_directory_new(&canonical, &publication.path().join("previous"))
            .map_err(|e| AppError::verify_failed(e.to_string()))?;
    }
    crate::fsutil::rename_directory_new(&candidate, &canonical)
        .map_err(|e| AppError::verify_failed(e.to_string()))?;
    let hash = fingerprint_folder(&canonical).map_err(AppError::skill_fingerprint_failed)?;
    let installed = plan.installation(hash.content_hash.clone());
    let mut records = Records::read(&layout.lock_path)?;
    if records.skills.get(plan.name()) != plan.previous.as_ref() {
        return Err(AppError::verify_failed(
            "安装记录在发布期间变化，已保留备份。",
        ));
    }
    records.skills.insert(plan.name().into(), installed.clone());
    records.save(&layout.lock_path)?;
    if let Some(previous) = &plan.previous_paths {
        for attachment in &previous.attachments {
            if attachment.mode == AttachmentMode::Copy {
                fs::remove_dir_all(&attachment.path)
                    .map_err(|e| AppError::agent_attach_failed(e.to_string()))?;
            }
        }
    }
    for target in plan.targets.iter().filter(|t| !t.universal) {
        let destination = target.skills_dir.join(plan.name());
        if fs::symlink_metadata(&destination).is_ok_and(|m| m.file_type().is_symlink())
            && fs::canonicalize(&destination).ok() == fs::canonicalize(&canonical).ok()
        {
            continue;
        }
        crate::attach::attach_agent_checked(
            paths,
            &layout,
            &layout.scope.skill_id(plan.name()),
            &target.id,
            &|| layout.ensure_mutation_ready(),
        )?;
    }
    if Records::read(&layout.lock_path)?.skills.get(plan.name()) != Some(&installed)
        || crate::marketplace::original_name(
            &fs::read_to_string(canonical.join("SKILL.md"))
                .map_err(|e| AppError::verify_failed(e.to_string()))?,
        )? != plan.name()
    {
        return Err(AppError::verify_failed("市场安装后校验失败"));
    }
    let mut warnings = vec![];
    if let Err(e) =
        BaselineStore::for_scope(paths, &layout.scope).record(plan.name(), &canonical, &hash, true)
    {
        warnings.push(crate::baseline::write_warning(e));
    }
    Ok(warnings)
}

pub(crate) struct Action {
    pub paths: AppPaths,
    pub layout: ScopeLayout,
}
impl MutationAction for Action {
    type Plan = Plan;
    type Unit = Plan;
    fn uses_cli(&self, _: &Plan) -> bool {
        false
    }
    fn independent_command(&self, plan: &Plan, _: &Reporter) -> Option<CommandSpec> {
        Some(plan.command())
    }
    fn profile(&self, _: &Plan) -> SessionProfile {
        SessionProfile {
            action: TaskAction::Install,
            initial_message: "正在准备 SkillHub 安装…".into(),
            success_status: TaskSkillStatus::Installed,
            total: 1,
            cancellation: CancellationPolicy::SealOnly,
        }
    }
    fn begin(&self, plan: &Plan) -> Result<StartOutcome<Plan>, AppError> {
        Ok(StartOutcome {
            units: vec![plan.clone()],
            initial_rows: vec![],
            log_path: None,
        })
    }
    fn before_write(&self, plan: &Plan) -> Result<(), AppError> {
        plan.check(&self.paths, &self.layout)
    }
    fn preflight(&self, plan: &Plan, _: &Reporter) -> Result<Preflight, AppError> {
        plan.check(&self.paths, &self.layout)?;
        let snapshot = BackupStore::for_layout(&self.paths, &self.layout).create(
            BackupAction::Install,
            plan.name(),
            None,
            &[],
            &plan
                .targets
                .iter()
                .map(|t| t.id.clone())
                .collect::<Vec<_>>(),
        )?;
        Ok(Preflight {
            snapshot: snapshot.path,
            post_message: None,
            warnings: vec![],
        })
    }
    fn command(&self, plan: &Plan, _: &CliRuntime, _: &Reporter) -> CommandSpec {
        plan.command()
    }
    fn complete(
        &self,
        plan: &Plan,
        output: ExecOutput,
        preflight: &Preflight,
        reporter: &Reporter,
    ) -> Result<UnitOutcome, AppError> {
        reporter.progress(
            TaskPhase::Verifying,
            "正在校验与发布技能…",
            Some(plan.name()),
        );
        let result = publish(&self.paths, plan, &output);
        let (status, error, warnings) = match result {
            Ok(warnings) => (TaskSkillStatus::Installed, None, warnings),
            Err(e) => (TaskSkillStatus::Failed, Some(e), vec![]),
        };
        let backup = self.paths.display(&preflight.snapshot);
        let _ = crate::logs::OperationLog::for_scope(&self.paths, &self.layout.scope).append(
            &crate::logs::OperationEntry::new(
                "install",
                plan.name(),
                plan.targets.iter().map(|t| t.id.clone()).collect(),
                super::update::status_label(status),
                Some(backup.clone()),
                error.clone(),
            ),
        );
        Ok(UnitOutcome {
            rows: vec![TaskSkillResult {
                name: plan.name().into(),
                status,
                error,
                backup_path: Some(backup.clone()),
            }],
            warnings,
            backup_path: backup,
            final_progress: None,
        })
    }
    fn on_infrastructure_failure(
        &self,
        plan: &Plan,
        error: AppError,
        preflight: Option<&Preflight>,
        _: &Reporter,
    ) -> FailureDisposition {
        unit_failure(
            &self.paths,
            &self.layout.scope,
            FailedUnit {
                action: "install",
                name: plan.name(),
                agents: &[],
            },
            error,
            preflight,
            None,
        )
    }
    fn on_cancelled(&self, plan: &Plan, preflight: &Preflight, error: Option<AppError>) {
        log_unit_cancelled(
            &self.paths,
            &self.layout.scope,
            FailedUnit {
                action: "install",
                name: plan.name(),
                agents: &[],
            },
            &preflight.snapshot,
            error,
        );
    }
}

#[cfg(test)]
mod tests;
