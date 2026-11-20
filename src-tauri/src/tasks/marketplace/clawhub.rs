//! Native archive installation for ClawHub.
//!
//! Preview resolves the current archive release, reads the install-time
//! moderation gate and reads the original skill name. Start re-resolves the
//! exact reference, stages and verifies the package (ticket 11), then publishes
//! through the source-neutral pipeline with no CLI command.

use crate::{
    backup::{BackupAction, BackupStore},
    baseline::fingerprint_folder,
    cli::preview::{CliCommandEntry, CliCommandPreview, CommandPlan},
    error::AppError,
    logs::{OperationEntry, OperationLog},
    marketplace::{Coordinate, clawhub, original_name},
    paths::AppPaths,
    scope_layout::ScopeLayout,
    tasks::{
        install::{InstallTarget, validate_selected_agents},
        manager::{TaskAction, TaskPhase, TaskSkillResult, TaskSkillStatus},
        publication::{self, MarketplaceInstallation, Publication},
        session::{
            CancellationPolicy, FailedUnit, FailureDisposition, MutationAction, Preflight,
            Reporter, SessionProfile, StartOutcome, UnitOutcome, log_unit_cancelled, unit_failure,
        },
    },
};
use serde::Serialize;
use std::{sync::Arc, time::SystemTime};

#[derive(Debug, Clone)]
pub struct Plan {
    pub(crate) publication: publication::Plan,
    pub(crate) version: String,
    pub(crate) canonical_url: String,
    pub(crate) staged: Option<Arc<clawhub::StagedPackage>>,
    pub(crate) github: Option<clawhub::github::GithubReference>,
    pub(crate) created_at: u64,
}

impl Plan {
    pub(crate) fn name(&self) -> &str {
        &self.publication.name
    }

    pub(crate) fn attach_staged(&mut self, staged: clawhub::StagedPackage) {
        self.staged = Some(Arc::new(staged));
    }

    pub(crate) fn attach_staged_shared(&mut self, staged: Arc<clawhub::StagedPackage>) {
        self.staged = Some(staged);
    }

    pub(crate) fn reference_label(&self) -> String {
        match &self.github {
            Some(reference) => reference.label(),
            None => format!("v{}", self.version),
        }
    }

    pub(crate) fn check(&self, paths: &AppPaths, layout: &ScopeLayout) -> Result<(), AppError> {
        publication::check(paths, layout, self)
    }

    fn installation(&self, hash: String) -> crate::marketplace::records::Installation {
        let now = httpdate::fmt_http_date(SystemTime::now());
        crate::marketplace::records::Installation {
            name: self.publication.name.clone(),
            coordinate: self.publication.coordinate.clone(),
            version: self.version.clone(),
            content_hash: hash,
            installed_at: self
                .publication
                .previous
                .as_ref()
                .map(|record| record.installed_at.clone())
                .unwrap_or_else(|| now.clone()),
            updated_at: now,
            diagnostics: self.github.as_ref().map(|reference| {
                crate::marketplace::records::ClawhubDiagnostics {
                    repo: Some(reference.repo.clone()),
                    path: Some(reference.path.clone()),
                    commit: Some(reference.commit.clone()),
                    content_hash: Some(reference.content_hash.clone()),
                }
            }),
        }
    }
}

impl MarketplaceInstallation for Plan {
    fn publication(&self) -> &publication::Plan {
        &self.publication
    }
    fn check_staged(&self) -> Result<(), AppError> {
        if let Some(staged) = &self.staged {
            staged.check()?;
        }
        Ok(())
    }
    fn installation(&self, content_hash: String) -> crate::marketplace::records::Installation {
        Plan::installation(self, content_hash)
    }
}

impl CommandPlan for Plan {
    fn needs_cli(&self) -> bool {
        false
    }
    fn command_preview(
        &self,
        layout: &ScopeLayout,
        _runtime: Option<&crate::cli::CliRuntime>,
    ) -> CliCommandPreview {
        CliCommandPreview::new(
            &layout.scope,
            "clawhub-install",
            vec![CliCommandEntry {
                name: self.publication.name.clone(),
                command: None,
                reason: Some(format!(
                    "由应用原生下载并校验 ClawHub {} {}，不执行 CLI 命令。",
                    self.publication.coordinate.canonical(),
                    self.reference_label()
                )),
            }],
        )
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub coordinate: Coordinate,
    pub name: String,
    pub version: String,
    pub reference_label: String,
    pub canonical_url: String,
    pub commands: CliCommandPreview,
}

#[allow(clippy::too_many_arguments)]
async fn build_plan(
    paths: &AppPaths,
    layout: ScopeLayout,
    coordinate: Coordinate,
    agents: Vec<String>,
    name: String,
    version: String,
    github: Option<clawhub::github::GithubReference>,
) -> Result<Plan, AppError> {
    let paths = paths.clone();
    let canonical_url = coordinate.url();
    tokio::task::spawn_blocking(move || {
        let (_, targets) = validate_selected_agents(&paths, &layout, &agents)?;
        let plan = Plan {
            publication: crate::tasks::publication::Plan {
                coordinate,
                name,
                layout,
                targets,
                previous: None,
                previous_paths: None,
                previous_fingerprint: None,
            },
            version,
            canonical_url,
            staged: None,
            github,
            created_at: crate::time::now_ms(),
        };
        plan.check(&paths, &plan.publication.layout)?;
        Ok(plan)
    })
    .await
    .map_err(|error| AppError::task_internal("ClawHub 安装预览", error))?
}

pub(crate) async fn prepare_plan(
    paths: &AppPaths,
    layout: ScopeLayout,
    coordinate: Coordinate,
    agents: Vec<String>,
) -> Result<Plan, AppError> {
    match clawhub::resolve_install_target(&coordinate).await? {
        clawhub::InstallTarget::Archive(version) => {
            let (listing, gate) = tokio::join!(
                clawhub::version_files(&coordinate, &version),
                clawhub::ensure_installable(&coordinate, &version),
            );
            let listing = listing?;
            gate?;
            if !listing.files.iter().any(|file| file.path == "SKILL.md") {
                return Err(AppError::clawhub_package_invalid(
                    "目标版本的文件列表缺少 SKILL.md",
                ));
            }
            let skill_md =
                clawhub::read_file_with_listing(&coordinate, &version, "SKILL.md", &listing)
                    .await?;
            if skill_md.status != crate::content::SkillFileStatus::Ok
                || skill_md.kind != crate::content::SkillFileKind::Markdown
            {
                return Err(AppError::clawhub_package_invalid(
                    "无法读取目标版本的 SKILL.md",
                ));
            }
            let markdown = skill_md
                .content
                .ok_or_else(|| AppError::clawhub_package_invalid("SKILL.md 内容不可用"))?;
            let name = original_name(&markdown)
                .map_err(|error| AppError::clawhub_package_invalid(error.message))?;
            build_plan(paths, layout, coordinate, agents, name, version, None).await
        }
        clawhub::InstallTarget::Github(reference) => {
            let markdown = clawhub::github::read_skill_markdown(&reference).await?;
            let name = original_name(&markdown)
                .map_err(|error| AppError::clawhub_package_invalid(error.message))?;
            clawhub::ensure_installable_at(&coordinate, None).await?;
            let version = reference.commit.clone();
            build_plan(
                paths,
                layout,
                coordinate,
                agents,
                name,
                version,
                Some(reference),
            )
            .await
        }
    }
}

/// Re-resolve the target and stage the exact confirmed reference. Archive and
/// GitHub-backed entries never switch classes after confirmation.
pub(crate) async fn stage_for_start(plan: &Plan) -> Result<clawhub::StagedPackage, AppError> {
    match (
        &plan.github,
        clawhub::resolve_install_target(&plan.publication.coordinate).await?,
    ) {
        (Some(expected), clawhub::InstallTarget::Github(current)) => {
            if current != *expected {
                return Err(AppError::cli_command_changed());
            }
            clawhub::ensure_installable_at(&plan.publication.coordinate, None).await?;
            clawhub::github::stage(&current).await
        }
        (None, clawhub::InstallTarget::Archive(version)) => {
            if version != plan.version {
                return Err(AppError::cli_command_changed());
            }
            clawhub::ensure_installable(&plan.publication.coordinate, &version).await?;
            clawhub::stage_archive(&plan.publication.coordinate, &version).await
        }
        _ => Err(AppError::cli_command_changed()),
    }
}

pub(crate) async fn prepare_update_plan(
    paths: &AppPaths,
    layout: &ScopeLayout,
    name: &str,
    record: &crate::marketplace::records::Installation,
    targets: Vec<InstallTarget>,
) -> Result<Plan, AppError> {
    let coordinate = record.coordinate.clone();
    let (version, github) = match clawhub::resolve_install_target(&coordinate).await? {
        clawhub::InstallTarget::Archive(version) => {
            let (listing, gate) = tokio::join!(
                clawhub::version_files(&coordinate, &version),
                clawhub::ensure_installable(&coordinate, &version),
            );
            let listing = listing?;
            gate?;
            if !listing.files.iter().any(|file| file.path == "SKILL.md") {
                return Err(AppError::clawhub_package_invalid(
                    "目标版本的文件列表缺少 SKILL.md",
                ));
            }
            let skill_md =
                clawhub::read_file_with_listing(&coordinate, &version, "SKILL.md", &listing)
                    .await?;
            if skill_md.status != crate::content::SkillFileStatus::Ok
                || skill_md.kind != crate::content::SkillFileKind::Markdown
            {
                return Err(AppError::clawhub_package_invalid(
                    "无法读取目标版本的 SKILL.md",
                ));
            }
            let markdown = skill_md
                .content
                .ok_or_else(|| AppError::clawhub_package_invalid("SKILL.md 内容不可用"))?;
            let new_name = original_name(&markdown)
                .map_err(|error| AppError::clawhub_package_invalid(error.message))?;
            if new_name != name {
                return Err(AppError::marketplace_unavailable(format!(
                    "新版本的原始技能名已变化（原名 {new_name}），请卸载后重新安装。"
                )));
            }
            (version, None)
        }
        clawhub::InstallTarget::Github(reference) => {
            let markdown = clawhub::github::read_skill_markdown(&reference).await?;
            let new_name = original_name(&markdown)
                .map_err(|error| AppError::clawhub_package_invalid(error.message))?;
            if new_name != name {
                return Err(AppError::marketplace_unavailable(format!(
                    "新版本的原始技能名已变化（原名 {new_name}），请卸载后重新安装。"
                )));
            }
            clawhub::ensure_installable_at(&coordinate, None).await?;
            (reference.commit.clone(), Some(reference))
        }
    };

    let paths = paths.clone();
    let layout = layout.clone();
    let record = record.clone();
    tokio::task::spawn_blocking(move || {
        let previous_paths = crate::tasks::uninstall::plan_uninstall(
            &paths,
            &layout,
            &layout.scope.skill_id(&record.name),
        )?;
        let previous_fingerprint = fingerprint_folder(&layout.canonical_dir.join(&record.name))
            .map_err(AppError::skill_fingerprint_failed)?
            .content_hash;
        let plan = Plan {
            publication: crate::tasks::publication::Plan {
                coordinate: record.coordinate.clone(),
                name: record.name.clone(),
                layout,
                targets,
                previous: Some(record.clone()),
                previous_paths: Some(previous_paths),
                previous_fingerprint: Some(previous_fingerprint),
            },
            version,
            canonical_url: record.coordinate.url(),
            staged: None,
            github,
            created_at: crate::time::now_ms(),
        };
        plan.check(&paths, &plan.publication.layout)?;
        Ok(plan)
    })
    .await
    .map_err(|error| AppError::task_internal("ClawHub 更新预览", error))?
}

pub(crate) fn publish_plan(
    paths: &AppPaths,
    scope: &crate::scope::ScopeId,
    plan: &Plan,
    preflight: &Preflight,
    reporter: &Reporter,
    operation: &str,
    success_status: TaskSkillStatus,
) -> Result<UnitOutcome, AppError> {
    reporter.progress(
        TaskPhase::Verifying,
        "正在校验与发布技能…",
        Some(plan.name()),
    );
    let staged = plan
        .staged
        .as_ref()
        .ok_or_else(|| AppError::clawhub_package_invalid("安装包暂存内容缺失"))?;
    let result = publication::publish(
        paths,
        Publication::Marketplace {
            plan,
            root: staged.path(),
        },
    );
    let warnings = result.warnings;
    let (status, error) = match result.result {
        Ok(()) => (success_status, None),
        Err(error) => (TaskSkillStatus::Failed, Some(error)),
    };
    let backup = paths.display(&preflight.snapshot);
    let _ = OperationLog::for_scope(paths, scope).append(&OperationEntry::new(
        operation,
        plan.name(),
        plan.publication
            .targets
            .iter()
            .map(|target| target.id.clone())
            .collect(),
        crate::tasks::update::status_label(status),
        Some(backup.clone()),
        error.clone(),
    ));
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

pub(crate) struct Action {
    pub paths: AppPaths,
    pub layout: ScopeLayout,
}

impl MutationAction for Action {
    type Plan = Plan;
    type Unit = Plan;

    fn uses_cli(&self, _plan: &Plan) -> bool {
        false
    }

    fn profile(&self, _plan: &Plan) -> SessionProfile {
        SessionProfile {
            action: TaskAction::Install,
            initial_message: "正在准备 ClawHub 安装…".into(),
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

    fn preflight(&self, plan: &Plan, _reporter: &Reporter) -> Result<Preflight, AppError> {
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
                .map(|target| target.id.clone())
                .collect::<Vec<_>>(),
        )?;
        Ok(Preflight {
            snapshot: snapshot.path,
            post_message: None,
            warnings: vec![],
        })
    }

    fn command(
        &self,
        _plan: &Plan,
        _runtime: &crate::cli::CliRuntime,
        _reporter: &Reporter,
    ) -> crate::process::CommandSpec {
        unreachable!("ClawHub archive installation never runs an external command")
    }

    fn complete(
        &self,
        _plan: &Plan,
        _output: crate::process::ExecOutput,
        _preflight: &Preflight,
        _reporter: &Reporter,
    ) -> Result<UnitOutcome, AppError> {
        unreachable!("ClawHub archive installation never produces CLI output")
    }

    fn execute_native(
        &self,
        plan: &Plan,
        preflight: &Preflight,
        reporter: &Reporter,
    ) -> Result<UnitOutcome, AppError> {
        publish_plan(
            &self.paths,
            &self.layout.scope,
            plan,
            preflight,
            reporter,
            "install",
            TaskSkillStatus::Installed,
        )
    }

    fn on_infrastructure_failure(
        &self,
        plan: &Plan,
        error: AppError,
        preflight: Option<&Preflight>,
        _reporter: &Reporter,
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
mod tests {
    use super::*;
    use crate::marketplace::{Coordinate, MarketplaceSource, records::Records};
    use crate::projects::ProjectRegistry;
    use crate::scope::ScopeId;
    use crate::scope_layout::ScopeLayout;
    use std::fs;

    fn coordinate() -> Coordinate {
        Coordinate::from_canonical_for(MarketplaceSource::Clawhub, "@steipete/weather").unwrap()
    }

    #[test]
    fn invalid_stage_keeps_its_source_error_ahead_of_common_validation() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(tmp.path().join("home"), tmp.path().join("data"));
        paths.ensure_dirs().unwrap();
        let layout = ScopeLayout::global(&paths, vec![]);
        let source = tmp.path().join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "---\nname: weather\n---\n").unwrap();
        let staged = clawhub::StagedPackage::from_folder(&source).unwrap();
        fs::remove_dir_all(staged.path()).unwrap();
        fs::create_dir_all(layout.lock_path.parent().unwrap()).unwrap();
        fs::write(&layout.lock_path, "corrupt Git lock").unwrap();
        let mut plan = Plan {
            publication: publication::Plan {
                coordinate: coordinate(),
                name: "weather".into(),
                layout: layout.clone(),
                targets: vec![],
                previous: None,
                previous_paths: None,
                previous_fingerprint: None,
            },
            version: "1.0.0".into(),
            canonical_url: coordinate().url(),
            staged: Some(Arc::new(staged)),
            github: None,
            created_at: 0,
        };
        let error = plan.check(&paths, &layout).unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::VerifyFailed);
        assert_eq!(error.message, "ClawHub 安装包未通过校验。");
        assert_eq!(error.detail.as_deref(), Some("暂存目录已被移动或替换"));
        plan.staged = None;
        assert_eq!(
            plan.check(&paths, &layout).unwrap_err().code,
            crate::error::ErrorCode::LockCorrupt
        );
    }

    #[test]
    fn record_uses_the_archive_semver_without_diagnostics() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(tmp.path().join("home"), tmp.path().join("data"));
        paths.ensure_dirs().unwrap();
        let layout = ScopeLayout::global(&paths, vec![]);
        let plan = Plan {
            publication: crate::tasks::publication::Plan {
                coordinate: coordinate(),
                name: "weather".into(),
                layout,
                targets: vec![],
                previous: None,
                previous_paths: None,
                previous_fingerprint: None,
            },
            version: "1.2.0".into(),
            canonical_url: coordinate().url(),
            staged: None,
            github: None,
            created_at: 0,
        };
        let record = plan.installation("hash".into());
        assert_eq!(record.name, "weather");
        assert_eq!(record.coordinate, coordinate());
        assert_eq!(record.version, "1.2.0");
        assert!(record.diagnostics.is_none());
        assert_eq!(record.entry().source_type, "clawhub");

        let mut plan = plan;
        plan.github = Some(clawhub::github::GithubReference {
            repo: "aws/agent-toolkit-for-aws".into(),
            path: "plugins/aws-agents/skills/agents-build".into(),
            commit: "b4416dddac9b6a0cc5412136e6dbf8f07ffdb31c".into(),
            content_hash: "d79709bf46ca8cd0fffe392a914cd56539b4b7c0ff0fe34c9583f24057300195".into(),
        });
        let record = plan.installation("hash".into());
        let diagnostics = record.diagnostics.expect("GitHub diagnostics");
        assert_eq!(
            diagnostics.commit.as_deref(),
            Some("b4416dddac9b6a0cc5412136e6dbf8f07ffdb31c")
        );
        assert_eq!(
            diagnostics.repo.as_deref(),
            Some("aws/agent-toolkit-for-aws")
        );
        assert_eq!(
            diagnostics.content_hash.as_deref(),
            Some("d79709bf46ca8cd0fffe392a914cd56539b4b7c0ff0fe34c9583f24057300195")
        );
    }

    #[test]
    fn native_publication_installs_into_global_and_project_scopes_with_attachments() {
        for project in [false, true] {
            let tmp = tempfile::tempdir().unwrap();
            let root = fs::canonicalize(tmp.path()).unwrap();
            let paths = AppPaths::new(root.join("home"), root.join("data"));
            paths.ensure_dirs().unwrap();
            fs::create_dir_all(paths.home().join(".claude/skills")).unwrap();
            let registry = ProjectRegistry::new(paths.clone());
            let scope = if project {
                let project_root = root.join("project");
                fs::create_dir_all(&project_root).unwrap();
                registry
                    .add(project_root.to_str().unwrap())
                    .unwrap()
                    .project
                    .scope()
            } else {
                ScopeId::Global
            };
            let layout = ScopeLayout::resolve_detected(&paths, &registry, &scope).unwrap();
            let (_, targets) = crate::tasks::install::validate_selected_agents(
                &paths,
                &layout,
                &["claude-code".into()],
            )
            .unwrap();

            let source = root.join("source");
            fs::create_dir_all(&source).unwrap();
            fs::write(
                source.join("SKILL.md"),
                "---\nname: weather\ndescription: test\n---\n# Weather\n",
            )
            .unwrap();
            fs::write(source.join("notes.txt"), "notes").unwrap();
            let staged = clawhub::StagedPackage::from_folder(&source).unwrap();
            let plan = Plan {
                publication: crate::tasks::publication::Plan {
                    coordinate: coordinate(),
                    name: "weather".into(),
                    layout: layout.clone(),
                    targets,
                    previous: None,
                    previous_paths: None,
                    previous_fingerprint: None,
                },
                version: "1.2.0".into(),
                canonical_url: coordinate().url(),
                staged: None,
                github: None,
                created_at: 0,
            };

            publication::publish(
                &paths,
                Publication::Marketplace {
                    plan: &plan,
                    root: staged.path(),
                },
            )
            .result
            .unwrap();

            let canonical = layout.canonical_dir.join("weather");
            assert!(canonical.join("SKILL.md").is_file(), "project={project}");
            assert!(canonical.join("notes.txt").is_file(), "project={project}");
            let record = Records::read(&layout.lock_path)
                .unwrap()
                .skills
                .get("weather")
                .cloned()
                .unwrap();
            assert_eq!(record.version, "1.2.0", "project={project}");
            assert_eq!(record.coordinate, coordinate(), "project={project}");
            assert!(record.diagnostics.is_none(), "project={project}");
            let attachment = plan.publication.targets[0].skills_dir.join("weather");
            assert!(
                fs::symlink_metadata(&attachment)
                    .unwrap()
                    .file_type()
                    .is_symlink(),
                "project={project}"
            );
        }
    }

    #[test]
    fn scopes_stay_independent_and_unmanaged_name_conflicts_block_install() {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let paths = AppPaths::new(root.join("home"), root.join("data"));
        paths.ensure_dirs().unwrap();
        fs::create_dir_all(paths.home().join(".claude/skills")).unwrap();
        let registry = ProjectRegistry::new(paths.clone());
        let project_root = root.join("project");
        fs::create_dir_all(&project_root).unwrap();
        let project_scope = registry
            .add(project_root.to_str().unwrap())
            .unwrap()
            .project
            .scope();
        let project_layout =
            ScopeLayout::resolve_detected(&paths, &registry, &project_scope).unwrap();
        let global_layout =
            ScopeLayout::resolve_detected(&paths, &registry, &ScopeId::Global).unwrap();

        let source = root.join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "---\nname: weather\n---\n").unwrap();
        let staged = clawhub::StagedPackage::from_folder(&source).unwrap();
        let plan_for = |layout: ScopeLayout| {
            let (_, targets) = crate::tasks::install::validate_selected_agents(
                &paths,
                &layout,
                &["claude-code".into()],
            )
            .unwrap();
            Plan {
                publication: crate::tasks::publication::Plan {
                    coordinate: coordinate(),
                    name: "weather".into(),
                    layout,
                    targets,
                    previous: None,
                    previous_paths: None,
                    previous_fingerprint: None,
                },
                version: "1.2.0".into(),
                canonical_url: coordinate().url(),
                staged: None,
                github: None,
                created_at: 0,
            }
        };
        for layout in [project_layout, global_layout.clone()] {
            let plan = plan_for(layout);
            publication::publish(
                &paths,
                Publication::Marketplace {
                    plan: &plan,
                    root: staged.path(),
                },
            )
            .result
            .unwrap();
        }

        let conflict = global_layout.canonical_dir.join("conflict");
        fs::create_dir_all(&conflict).unwrap();
        fs::write(conflict.join("SKILL.md"), "---\nname: conflict\n---\n").unwrap();
        let plan = Plan {
            publication: crate::tasks::publication::Plan {
                coordinate: Coordinate::from_canonical_for(
                    MarketplaceSource::Clawhub,
                    "@other/conflict",
                )
                .unwrap(),
                name: "conflict".into(),
                layout: global_layout.clone(),
                targets: vec![],
                previous: None,
                previous_paths: None,
                previous_fingerprint: None,
            },
            version: "1.0.0".into(),
            canonical_url: String::new(),
            staged: None,
            github: None,
            created_at: 0,
        };
        let error = plan.check(&paths, &global_layout).unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::SkillConflict);
    }

    #[test]
    fn native_update_replaces_canonical_and_record_and_keeps_the_attachment() {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let paths = AppPaths::new(root.join("home"), root.join("data"));
        paths.ensure_dirs().unwrap();
        fs::create_dir_all(paths.home().join(".claude/skills")).unwrap();
        let registry = ProjectRegistry::new(paths.clone());
        let layout = ScopeLayout::resolve_detected(&paths, &registry, &ScopeId::Global).unwrap();
        let (_, targets) = crate::tasks::install::validate_selected_agents(
            &paths,
            &layout,
            &["claude-code".into()],
        )
        .unwrap();

        let source = root.join("v1");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "---\nname: weather\n---\n# v1\n").unwrap();
        let v1 = Plan {
            publication: crate::tasks::publication::Plan {
                coordinate: coordinate(),
                name: "weather".into(),
                layout: layout.clone(),
                targets: targets.clone(),
                previous: None,
                previous_paths: None,
                previous_fingerprint: None,
            },
            version: "1.0.0".into(),
            canonical_url: coordinate().url(),
            staged: None,
            github: None,
            created_at: 0,
        };
        let installed = clawhub::StagedPackage::from_folder(&source).unwrap();
        publication::publish(
            &paths,
            Publication::Marketplace {
                plan: &v1,
                root: installed.path(),
            },
        )
        .result
        .unwrap();
        let previous = Records::read(&layout.lock_path)
            .unwrap()
            .skills
            .get("weather")
            .cloned()
            .unwrap();

        let source = root.join("v2");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "---\nname: weather\n---\n# v2\n").unwrap();
        fs::write(source.join("notes.txt"), "second").unwrap();
        let staged = clawhub::StagedPackage::from_folder(&source).unwrap();
        let previous_paths = crate::tasks::uninstall::plan_uninstall(
            &paths,
            &layout,
            &layout.scope.skill_id("weather"),
        )
        .unwrap();
        let previous_fingerprint = fingerprint_folder(&layout.canonical_dir.join("weather"))
            .unwrap()
            .content_hash;
        let update = Plan {
            publication: crate::tasks::publication::Plan {
                coordinate: coordinate(),
                name: "weather".into(),
                layout: layout.clone(),
                targets,
                previous: Some(previous.clone()),
                previous_paths: Some(previous_paths),
                previous_fingerprint: Some(previous_fingerprint),
            },
            version: "2.0.0".into(),
            canonical_url: coordinate().url(),
            staged: None,
            github: None,
            created_at: 0,
        };
        update.check(&paths, &layout).unwrap();
        publication::publish(
            &paths,
            Publication::Marketplace {
                plan: &update,
                root: staged.path(),
            },
        )
        .result
        .unwrap();

        let record = Records::read(&layout.lock_path)
            .unwrap()
            .skills
            .get("weather")
            .cloned()
            .unwrap();
        assert_eq!(record.version, "2.0.0");
        assert_eq!(record.installed_at, previous.installed_at);
        assert_eq!(
            fs::read_to_string(layout.canonical_dir.join("weather/SKILL.md")).unwrap(),
            "---\nname: weather\n---\n# v2\n"
        );
        assert!(
            fs::symlink_metadata(update.publication.targets[0].skills_dir.join("weather"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[tokio::test]
    #[ignore = "live ClawHub install preview; requires network and optional system proxy"]
    async fn clawhub_live_install_preview_resolves_the_latest_archive() {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let paths = AppPaths::new(root.join("home"), root.join("data"));
        paths.ensure_dirs().unwrap();
        fs::create_dir_all(paths.home().join(".claude/skills")).unwrap();
        let registry = crate::projects::ProjectRegistry::new(paths.clone());
        let layout = ScopeLayout::resolve_detected(&paths, &registry, &ScopeId::Global).unwrap();
        let coordinate =
            Coordinate::from_canonical_for(MarketplaceSource::Clawhub, "@steipete/weather")
                .unwrap();
        let plan = prepare_plan(&paths, layout, coordinate, vec!["claude-code".into()])
            .await
            .unwrap();
        assert_eq!(plan.version, "1.0.0");
        assert!(!plan.publication.name.is_empty());
        assert!(plan.github.is_none());
        assert_eq!(plan.reference_label(), "v1.0.0");
    }

    #[tokio::test]
    #[ignore = "downloads two real ClawHub archive versions and publishes into isolated Global and Project scopes"]
    async fn clawhub_live_archive_install_and_update_publish_into_isolated_scopes() {
        let coordinate =
            Coordinate::from_canonical_for(MarketplaceSource::Clawhub, "@steipete/nano-banana-pro")
                .unwrap();
        let v1 = clawhub::stage_archive(&coordinate, "1.0.0").await.unwrap();
        let v2 = clawhub::stage_archive(&coordinate, "1.0.1").await.unwrap();
        assert_ne!(v1.name, "");
        assert_eq!(v1.name, v2.name);

        for project in [false, true] {
            let tmp = tempfile::tempdir().unwrap();
            let root = fs::canonicalize(tmp.path()).unwrap();
            let paths = AppPaths::new(root.join("home"), root.join("data"));
            paths.ensure_dirs().unwrap();
            fs::create_dir_all(paths.home().join(".claude/skills")).unwrap();
            let registry = ProjectRegistry::new(paths.clone());
            let scope = if project {
                let project_root = root.join("project");
                fs::create_dir_all(&project_root).unwrap();
                registry
                    .add(project_root.to_str().unwrap())
                    .unwrap()
                    .project
                    .scope()
            } else {
                ScopeId::Global
            };
            let layout = ScopeLayout::resolve_detected(&paths, &registry, &scope).unwrap();
            let (_, targets) = crate::tasks::install::validate_selected_agents(
                &paths,
                &layout,
                &["claude-code".into()],
            )
            .unwrap();

            let install = Plan {
                publication: crate::tasks::publication::Plan {
                    coordinate: coordinate.clone(),
                    name: v1.name.clone(),
                    layout: layout.clone(),
                    targets: targets.clone(),
                    previous: None,
                    previous_paths: None,
                    previous_fingerprint: None,
                },
                version: "1.0.0".into(),
                canonical_url: coordinate.url(),
                staged: None,
                github: None,
                created_at: 0,
            };
            publication::publish(
                &paths,
                Publication::Marketplace {
                    plan: &install,
                    root: v1.path(),
                },
            )
            .result
            .unwrap();
            let previous = Records::read(&layout.lock_path)
                .unwrap()
                .skills
                .get(&v1.name)
                .cloned()
                .unwrap();
            assert_eq!(previous.version, "1.0.0", "project={project}");

            let layout = ScopeLayout::resolve_detected(&paths, &registry, &scope).unwrap();
            let (_, targets) = crate::tasks::install::validate_selected_agents(
                &paths,
                &layout,
                &["claude-code".into()],
            )
            .unwrap();
            let previous_paths = crate::tasks::uninstall::plan_uninstall(
                &paths,
                &layout,
                &layout.scope.skill_id(&v1.name),
            )
            .unwrap();
            let previous_fingerprint = fingerprint_folder(&layout.canonical_dir.join(&v1.name))
                .unwrap()
                .content_hash;
            let update = Plan {
                publication: crate::tasks::publication::Plan {
                    coordinate: coordinate.clone(),
                    name: v1.name.clone(),
                    layout: layout.clone(),
                    targets,
                    previous: Some(previous.clone()),
                    previous_paths: Some(previous_paths),
                    previous_fingerprint: Some(previous_fingerprint),
                },
                version: "1.0.1".into(),
                canonical_url: coordinate.url(),
                staged: None,
                github: None,
                created_at: 0,
            };
            update.check(&paths, &layout).unwrap();
            let outcome = publication::publish(
                &paths,
                Publication::Marketplace {
                    plan: &update,
                    root: v2.path(),
                },
            );
            if let Err(error) = &outcome.result {
                panic!("project={project}: {error:?}");
            }
            let record = Records::read(&layout.lock_path)
                .unwrap()
                .skills
                .get(&v1.name)
                .cloned()
                .unwrap();
            assert_eq!(record.version, "1.0.1", "project={project}");
            assert_eq!(
                record.installed_at, previous.installed_at,
                "project={project}"
            );
            assert!(
                fs::symlink_metadata(update.publication.targets[0].skills_dir.join(&v1.name))
                    .unwrap()
                    .file_type()
                    .is_symlink(),
                "project={project}"
            );
        }
    }
}
