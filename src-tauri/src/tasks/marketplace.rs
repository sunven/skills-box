//! SkillHub writes only into a private staging directory. Publication uses the
//! same scope ownership, snapshots, attachment checks and task results as Git.
pub(crate) mod clawhub;

use super::{
    manager::{TaskAction, TaskPhase, TaskSkillResult, TaskSkillStatus},
    publication::{self, Publication},
    session::*,
};
use crate::{
    backup::{BackupAction, BackupStore},
    cli::{
        CliRuntime,
        preview::{CliCommandEntry, CliCommandPreview, CommandPlan},
    },
    error::AppError,
    marketplace::{provider::Detail, records::Installation, runtime::Runtime},
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
    pub(crate) publication: publication::Plan,
    pub detail: Detail,
    pub runtime: Runtime,
    pub stage: Arc<StagingDirectory>,
    pub created_at: u64,
}
impl Plan {
    pub fn name(&self) -> &str {
        &self.publication.name
    }
    pub fn command(&self) -> CommandSpec {
        self.runtime.install(
            &self.publication.coordinate,
            &self.detail.summary.version,
            self.stage.path(),
        )
    }
    pub fn check(&self, paths: &AppPaths, layout: &ScopeLayout) -> Result<(), AppError> {
        publication::check(paths, layout, self)
    }
    pub(super) fn installation(&self, hash: String) -> Installation {
        let now = httpdate::fmt_http_date(std::time::SystemTime::now());
        Installation {
            name: self.name().into(),
            coordinate: self.publication.coordinate.clone(),
            version: self.detail.summary.version.clone(),
            content_hash: hash,
            installed_at: self
                .publication
                .previous
                .as_ref()
                .map(|r| r.installed_at.clone())
                .unwrap_or_else(|| now.clone()),
            updated_at: now,
            diagnostics: None,
        }
    }
}
impl publication::MarketplaceInstallation for Plan {
    fn publication(&self) -> &publication::Plan {
        &self.publication
    }
    fn check_staged(&self) -> Result<(), AppError> {
        self.stage.check()
    }
    fn installation(&self, content_hash: String) -> Installation {
        Plan::installation(self, content_hash)
    }
}
impl CommandPlan for Plan {
    fn needs_cli(&self) -> bool {
        false
    }
    fn command_preview(&self, layout: &ScopeLayout, _: Option<&CliRuntime>) -> CliCommandPreview {
        CliCommandPreview::new(
            &layout.scope,
            if self.publication.previous.is_some() {
                "marketplace-update"
            } else {
                "marketplace-install"
            },
            vec![CliCommandEntry {
                name: self.name().into(),
                command: Some(crate::cli::commands::display_command(&self.command())),
                reason: Some(format!(
                    "SkillHub {} · {}",
                    self.publication.coordinate.canonical(),
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

pub(super) fn publish(paths: &AppPaths, plan: &Plan, output: &ExecOutput) -> publication::Outcome {
    match validate_staged(plan, output) {
        Ok(root) => publication::publish(paths, Publication::Marketplace { plan, root: &root }),
        Err(error) => Err(error).into(),
    }
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
        .join(plan.publication.coordinate.canonical());
    if fs::canonicalize(&root).map_err(|e| AppError::verify_failed(e.to_string()))? != root {
        return Err(AppError::verify_failed("CLI 暂存目录包含间接路径。"));
    }
    if result["success"] != true
        || result["slug"] != plan.publication.coordinate.canonical()
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
    let download = lock["skills"][plan.publication.coordinate.canonical()]["zip_url"]
        .as_str()
        .and_then(|url| reqwest::Url::parse(url).ok())
        .ok_or_else(|| AppError::verify_failed("CLI 未记录具体版本的下载地址。"))?;
    let params: std::collections::BTreeMap<_, _> = download.query_pairs().into_owned().collect();
    if download.scheme() != "https"
        || download.host_str() != Some("api.skillhub.cn")
        || download.path() != "/api/v1/download"
        || params.get("slug") != Some(&plan.publication.coordinate.slug)
        || params.get("namespace") != Some(&plan.publication.coordinate.namespace)
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
                .publication
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
        let warnings = result.warnings;
        let (status, error) = match result.result {
            Ok(()) => (TaskSkillStatus::Installed, None),
            Err(e) => (TaskSkillStatus::Failed, Some(e)),
        };
        let backup = self.paths.display(&preflight.snapshot);
        let _ = crate::logs::OperationLog::for_scope(&self.paths, &self.layout.scope).append(
            &crate::logs::OperationEntry::new(
                "install",
                plan.name(),
                plan.publication
                    .targets
                    .iter()
                    .map(|t| t.id.clone())
                    .collect(),
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
            backup_path: Some(backup),
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
