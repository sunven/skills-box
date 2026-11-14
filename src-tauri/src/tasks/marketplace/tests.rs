use super::*;
use crate::{
    baseline::BaselineStore,
    cli::CliRuntimeProvider,
    marketplace::{Coordinate, provider::Summary},
    process::{CancelToken, ExecError, ProcessExecutor},
    scope::ScopeId,
    tasks::{
        manager::{FinishOutcome, TaskEnvelope, TaskOverall},
        session,
    },
};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

struct Fixture {
    _tmp: tempfile::TempDir,
    paths: AppPaths,
    plan: Plan,
}
impl Fixture {
    fn new(project: bool) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let paths = AppPaths::new(root.join("home"), root.join("data"));
        paths.ensure_dirs().unwrap();
        fs::create_dir_all(paths.home().join(".claude/skills")).unwrap();
        let registry = crate::projects::ProjectRegistry::new(paths.clone());
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
        let coordinate = Coordinate::from_canonical("@publisher/public-slug").unwrap();
        let detail = Detail {
            summary: Summary {
                coordinate: coordinate.clone(),
                name: "Display title".into(),
                publisher: "publisher".into(),
                description: "description".into(),
                version: "1.0.0".into(),
                official_url: coordinate.url(),
                stars: None,
                downloads: None,
            },
            markdown: "---\nname: original-name\ndescription: test\n---\n# Test\n".into(),
            original_name: Some("original-name".into()),
            install_error: None,
            requirements: vec![],
        };
        let plan = Plan {
            detail,
            runtime: Runtime {
                program: PathBuf::from("/fake/skillhub"),
                env: BTreeMap::new(),
            },
            stage: StagingDirectory::new().unwrap(),
            layout,
            targets,
            previous: None,
            previous_paths: None,
            previous_fingerprint: None,
            created_at: crate::time::now_ms(),
        };
        Self {
            _tmp: tmp,
            paths,
            plan,
        }
    }
    fn downloaded(&self, plan: &Plan) -> ExecOutput {
        let root = fs::canonicalize(plan.stage.path())
            .unwrap()
            .join(plan.detail.summary.coordinate.canonical());
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("SKILL.md"), &plan.detail.markdown).unwrap();
        fs::write(
            root.join("_meta.json"),
            r#"{"version":"wrong-package-version"}"#,
        )
        .unwrap();
        fs::write(plan.stage.path().join(".skills_store_lock.json"), serde_json::json!({"skills":{plan.detail.summary.coordinate.canonical():{"zip_url":format!("https://api.skillhub.cn/api/v1/download?slug={}&namespace={}&version={}",plan.detail.summary.coordinate.slug,plan.detail.summary.coordinate.namespace,plan.detail.summary.version),"version":"wrong-cli-version"}}}).to_string()).unwrap();
        ExecOutput { code: Some(0), signal: None, stdout: serde_json::json!({"success":true,"slug":plan.detail.summary.coordinate.canonical(),"targetDir":root,"version":"newer-than-confirmed"}).to_string(), stderr: String::new(), duration: Duration::ZERO }
    }
    fn update(&self) -> Plan {
        let mut plan = self.plan.clone();
        plan.previous = Records::read(&plan.layout.lock_path)
            .unwrap()
            .skills
            .get(plan.name())
            .cloned();
        plan.previous_paths = Some(
            crate::tasks::uninstall::plan_uninstall(
                &self.paths,
                &plan.layout,
                &plan.layout.scope.skill_id(plan.name()),
            )
            .unwrap(),
        );
        plan.previous_fingerprint = Some(
            fingerprint_folder(&plan.layout.canonical_dir.join(plan.name()))
                .unwrap()
                .content_hash,
        );
        plan.detail.summary.version = "2.0.0".into();
        plan.detail.markdown.push_str("\nSecond release\n");
        plan.stage = StagingDirectory::new().unwrap();
        plan
    }
}
struct NoGitRuntime;
#[async_trait::async_trait]
impl CliRuntimeProvider for NoGitRuntime {
    async fn cli_runtime(&self) -> Result<CliRuntime, AppError> {
        panic!("marketplace must not resolve Node/Git runtime")
    }
}
struct Executor {
    expected: CommandSpec,
    output: ExecOutput,
}
#[async_trait::async_trait]
impl ProcessExecutor for Executor {
    async fn run(&self, mut spec: CommandSpec, _: CancelToken) -> Result<ExecOutput, ExecError> {
        assert_eq!(spec.cli_context.as_ref().unwrap().origin, "task");
        spec.cli_context = None;
        assert_eq!(spec, self.expected);
        Ok(self.output.clone())
    }
}
fn ctx(plan: &Plan, output: ExecOutput, action: TaskAction) -> SessionContext {
    SessionContext {
        task_id: "marketplace-test".into(),
        sink: Arc::new(|_| {}),
        cancel: CancelToken::new(),
        envelope: TaskEnvelope {
            action,
            scope: plan.layout.scope.clone(),
            started_at: crate::time::now_ms(),
        },
        runtime: Arc::new(NoGitRuntime),
        executor: Arc::new(Executor {
            expected: plan.command(),
            output,
        }),
        record_snapshot: Arc::new(|p| assert!(p.is_dir())),
        set_cancellable: Arc::new(|_| {}),
        expected_commands: Some(vec![crate::cli::commands::display_command(&plan.command())]),
    }
}

#[tokio::test]
async fn marketplace_install_is_managed_scoped_and_preserves_confirmed_version_without_git_runtime()
{
    for project in [false, true] {
        let f = Fixture::new(project);
        let output = f.downloaded(&f.plan);
        assert!(f.plan.command().args.contains(&"public-slug@1.0.0".into()));
        let result = session::run(
            ctx(&f.plan, output, TaskAction::Install),
            Action {
                paths: f.paths.clone(),
                layout: f.plan.layout.clone(),
            },
            f.plan.clone(),
        )
        .await;
        let FinishOutcome::Completed(result, _) = result else {
            panic!("{result:?}");
        };
        assert_eq!(result.overall, TaskOverall::Success, "{:?}", result.skills);
        let record =
            Records::read(&f.plan.layout.lock_path).unwrap().skills["original-name"].clone();
        assert_eq!(record.version, "1.0.0");
        let located = f
            .plan
            .layout
            .locate(&f.paths, "original-name")
            .unwrap()
            .unwrap();
        assert_eq!(
            located.management,
            crate::inventory::ManagementState::Managed
        );
        assert!(
            f.plan.targets[0]
                .skills_dir
                .join("original-name")
                .is_symlink()
        );
        assert!(
            BaselineStore::for_scope(&f.paths, &f.plan.layout.scope)
                .load("original-name")
                .unwrap()
                .verified
        );
        assert!(
            !f.plan.layout.lock_path.exists(),
            "marketplace must not create Git lock"
        );
        if project {
            assert!(
                !f.paths
                    .canonical_skills_dir()
                    .join("original-name")
                    .exists()
            );
        }
    }
}

#[tokio::test]
async fn publication_baseline_and_attachment_failures_preserve_results_and_backups() {
    use std::os::unix::fs::PermissionsExt;

    for project in [false, true] {
        for updating in [false, true] {
            for baseline_failure in [false, true] {
                for attachment_failure in [false, true] {
                    let mut f = Fixture::new(project);
                    let name = "original-name";
                    let canonical = f.plan.layout.canonical_dir.join(name);
                    let agent_dir = &f.plan.targets[0].skills_dir;
                    fs::create_dir_all(agent_dir).unwrap();
                    let attachment = agent_dir.join(name);
                    if updating {
                        publish(&f.paths, &f.plan, &f.downloaded(&f.plan))
                            .result
                            .unwrap();
                        // Exercise replacement of a Copy attachment, including
                        // failure during its removal after the new baseline.
                        fs::remove_file(&attachment).unwrap();
                        crate::backup::copy_tree(&canonical, &attachment).unwrap();
                        // A new request resolves the current layout, including
                        // project Agent directories created by the first install.
                        let registry = crate::projects::ProjectRegistry::new(f.paths.clone());
                        f.plan.layout = ScopeLayout::resolve_detected(
                            &f.paths,
                            &registry,
                            &f.plan.layout.scope,
                        )
                        .unwrap();
                    }
                    let plan = if updating { f.update() } else { f.plan.clone() };
                    let update_plan = updating.then(|| {
                        let mut update = crate::tasks::update::plan_updates(
                            &f.paths,
                            &f.plan.layout,
                            &[name.into()],
                        )
                        .unwrap();
                        update.items[0].marketplace_plan = Some(plan.clone());
                        update
                    });
                    let baselines = BaselineStore::for_scope(&f.paths, &plan.layout.scope);
                    if baseline_failure {
                        baselines.clear(name).unwrap();
                        fs::create_dir_all(baselines.baseline_path(name)).unwrap();
                    }
                    let permissions = fs::metadata(agent_dir).unwrap().permissions();
                    if attachment_failure {
                        fs::set_permissions(agent_dir, fs::Permissions::from_mode(0o555)).unwrap();
                        let probe = agent_dir.join("permission-probe");
                        if fs::write(&probe, "probe").is_ok() {
                            fs::remove_file(probe).unwrap();
                            fs::set_permissions(agent_dir, permissions).unwrap();
                            continue; // A privileged process bypasses directory permissions.
                        }
                    }
                    let context = ctx(
                        &plan,
                        f.downloaded(&plan),
                        if updating {
                            TaskAction::Update
                        } else {
                            TaskAction::Install
                        },
                    );
                    let outcome = if let Some(update_plan) = update_plan {
                        session::run(
                            context,
                            crate::tasks::update::UpdateAction::new(
                                f.paths.clone(),
                                f.plan.layout.clone(),
                                Duration::from_secs(5),
                            ),
                            update_plan,
                        )
                        .await
                    } else {
                        session::run(
                            context,
                            Action {
                                paths: f.paths.clone(),
                                layout: plan.layout.clone(),
                            },
                            plan.clone(),
                        )
                        .await
                    };
                    fs::set_permissions(agent_dir, permissions).unwrap();
                    let FinishOutcome::Completed(mut result, _) = outcome else {
                        panic!("{outcome:?}");
                    };
                    let baseline_warnings: Vec<_> = result
                        .warnings
                        .iter()
                        .filter(|warning| {
                            warning.code == crate::error::ErrorCode::BaselineWriteFailed
                        })
                        .collect();
                    assert_eq!(
                        baseline_warnings.len(),
                        usize::from(baseline_failure),
                        "{result:?}"
                    );
                    if baseline_failure {
                        assert!(baselines.load(name).is_none());
                    } else {
                        let baseline = baselines.load(name).unwrap();
                        assert!(baseline.verified);
                        assert_eq!(
                            baseline.content_hash,
                            fingerprint_folder(&canonical).unwrap().content_hash
                        );
                    }
                    assert_eq!(
                        fs::read_to_string(canonical.join("SKILL.md")).unwrap(),
                        plan.detail.markdown,
                        "project={project} updating={updating} baseline_failure={baseline_failure} attachment_failure={attachment_failure}: {result:?}"
                    );
                    assert_eq!(
                        Records::read(&plan.layout.lock_path).unwrap().skills[name].version,
                        plan.detail.summary.version
                    );
                    assert_eq!(
                        result.overall,
                        if attachment_failure {
                            TaskOverall::Failed
                        } else {
                            TaskOverall::Success
                        },
                        "{result:?}"
                    );
                    if attachment_failure {
                        assert_eq!(
                            result.skills[0].error.as_ref().unwrap().code,
                            crate::error::ErrorCode::AgentAttachFailed
                        );
                    } else {
                        assert!(attachment.is_symlink());
                    }
                    // Exercise the same retention policy used when TaskManager
                    // finishes: warnings alone do not protect successful backups.
                    let backup = PathBuf::from(result.skills[0].backup_path.as_ref().unwrap());
                    crate::settings::SettingsService::new(f.paths.clone(), None)
                        .save_data_preferences(crate::settings::DataPreferences {
                            retain_backups: false,
                            ..Default::default()
                        })
                        .unwrap();
                    crate::app_data::finish_successful_backups(&f.paths, &mut result, &[]);
                    assert_eq!(backup.exists(), attachment_failure, "{result:?}");
                }
            }
        }
    }
}

#[test]
fn publication_record_failure_stops_before_baseline_and_attachments() {
    use std::os::unix::fs::PermissionsExt;

    for project in [false, true] {
        let f = Fixture::new(project);
        let output = f.downloaded(&f.plan);
        let root = validate_staged(&f.plan, &output).unwrap();
        fs::create_dir_all(&f.plan.layout.canonical_dir).unwrap();
        let record_dir = f.plan.layout.lock_path.parent().unwrap();
        let permissions = fs::metadata(record_dir).unwrap().permissions();
        fs::set_permissions(record_dir, fs::Permissions::from_mode(0o555)).unwrap();
        let probe = record_dir.join("permission-probe");
        if fs::write(&probe, "probe").is_ok() {
            fs::remove_file(probe).unwrap();
            fs::set_permissions(record_dir, permissions).unwrap();
            continue;
        }
        let outcome = publication::publish(
            &f.paths,
            Publication::Marketplace {
                plan: &f.plan,
                root: &root,
            },
        );
        fs::set_permissions(record_dir, permissions).unwrap();
        assert_eq!(
            outcome.result.unwrap_err().code,
            crate::error::ErrorCode::LockReadFailed
        );
        assert!(outcome.warnings.is_empty());
        assert!(
            f.plan
                .layout
                .canonical_dir
                .join(f.plan.name())
                .join("SKILL.md")
                .is_file()
        );
        assert!(
            Records::read(&f.plan.layout.lock_path)
                .unwrap()
                .skills
                .is_empty()
        );
        assert!(
            BaselineStore::for_scope(&f.paths, &f.plan.layout.scope)
                .load(f.plan.name())
                .is_none()
        );
        assert!(!f.plan.targets[0].skills_dir.join(f.plan.name()).exists());
    }
}

#[test]
fn marketplace_staging_failure_and_local_races_preserve_existing_installation() {
    let f = Fixture::new(false);
    publish(&f.paths, &f.plan, &f.downloaded(&f.plan))
        .result
        .unwrap();
    let update = f.update();
    let old = fs::read(f.plan.layout.canonical_dir.join("original-name/SKILL.md")).unwrap();
    let mut failed = f.downloaded(&update);
    failed.code = Some(1);
    assert!(publish(&f.paths, &update, &failed).result.is_err());
    assert_eq!(
        fs::read(f.plan.layout.canonical_dir.join("original-name/SKILL.md")).unwrap(),
        old
    );
    fs::write(
        f.plan.layout.canonical_dir.join("original-name/local-edit"),
        "edited",
    )
    .unwrap();
    assert!(
        publish(&f.paths, &update, &f.downloaded(&update))
            .result
            .is_err()
    );
    assert_eq!(
        Records::read(&f.plan.layout.lock_path).unwrap().skills["original-name"].version,
        "1.0.0"
    );
}

#[tokio::test]
async fn marketplace_update_uses_pinned_command_and_uninstalls_natively_with_record_backup() {
    let f = Fixture::new(false);
    publish(&f.paths, &f.plan, &f.downloaded(&f.plan))
        .result
        .unwrap();
    let update = f.update();
    let mut plan =
        crate::tasks::update::plan_updates(&f.paths, &f.plan.layout, &["original-name".into()])
            .unwrap();
    plan.items[0].marketplace_plan = Some(update.clone());
    let result = session::run(
        ctx(&update, f.downloaded(&update), TaskAction::Update),
        crate::tasks::update::UpdateAction::new(
            f.paths.clone(),
            f.plan.layout.clone(),
            Duration::from_secs(5),
        ),
        plan,
    )
    .await;
    let FinishOutcome::Completed(result, _) = result else {
        panic!("{result:?}");
    };
    assert_eq!(result.overall, TaskOverall::Success, "{:?}", result.skills);
    assert_eq!(
        Records::read(&f.plan.layout.lock_path).unwrap().skills["original-name"].version,
        "2.0.0"
    );
    let uninstall =
        crate::tasks::uninstall::plan_uninstall(&f.paths, &f.plan.layout, "original-name").unwrap();
    assert!(!uninstall.needs_cli());
    let mut context = ctx(&update, f.downloaded(&update), TaskAction::Uninstall);
    context.expected_commands = Some(vec![]);
    let result = session::run(
        context,
        crate::tasks::uninstall::UninstallAction::new(
            f.paths.clone(),
            f.plan.layout.clone(),
            Duration::from_secs(5),
        ),
        uninstall,
    )
    .await;
    let FinishOutcome::Completed(result, _) = result else {
        panic!("{result:?}");
    };
    assert_eq!(result.overall, TaskOverall::Success, "{:?}", result.skills);
    assert!(
        Records::read(&f.plan.layout.lock_path)
            .unwrap()
            .skills
            .is_empty()
    );
    assert!(!f.plan.layout.canonical_dir.join("original-name").exists());
    assert!(
        PathBuf::from(result.backup_path.unwrap())
            .join(crate::marketplace::records::FILE_NAME)
            .is_file()
    );
}

#[test]
fn marketplace_rejects_names_conflicting_with_unmanaged_or_stale_installations_and_duplicate_coordinates()
 {
    let f = Fixture::new(false);
    let canonical = f.plan.layout.canonical_dir.join(f.plan.name());
    fs::create_dir_all(&canonical).unwrap();
    fs::write(canonical.join("SKILL.md"), &f.plan.detail.markdown).unwrap();
    assert!(f.plan.check(&f.paths, &f.plan.layout).is_err());
    fs::remove_dir_all(&canonical).unwrap();
    publish(&f.paths, &f.plan, &f.downloaded(&f.plan))
        .result
        .unwrap();
    fs::remove_file(f.plan.targets[0].skills_dir.join(f.plan.name())).unwrap();
    fs::remove_dir_all(&canonical).unwrap();
    assert!(f.plan.check(&f.paths, &f.plan.layout).is_err());
    let mut renamed = f.plan.clone();
    renamed.detail.original_name = Some("new-name".into());
    assert!(renamed.check(&f.paths, &renamed.layout).is_err());
}

#[test]
fn marketplace_rejects_forged_success_wrong_content_and_symlink_packages_before_publication() {
    for failure in ["coordinate", "content", "symlink", "requested-version"] {
        let f = Fixture::new(false);
        let mut output = f.downloaded(&f.plan);
        let root = fs::canonicalize(f.plan.stage.path())
            .unwrap()
            .join(f.plan.detail.summary.coordinate.canonical());
        match failure {
            "coordinate" => output.stdout = output.stdout.replace("@publisher/public-slug", "@other/public-slug"),
            "content" => fs::write(root.join("SKILL.md"), "---\nname: renamed\n---\n").unwrap(),
            "requested-version" => fs::write(f.plan.stage.path().join(".skills_store_lock.json"), r#"{"skills":{"@publisher/public-slug":{"zip_url":"https://api.skillhub.cn/api/v1/download?slug=public-slug&namespace=publisher&version=latest"}}}"#).unwrap(),
            _ => std::os::unix::fs::symlink("/tmp", root.join("escape")).unwrap(),
        }
        assert!(publish(&f.paths, &f.plan, &output).result.is_err());
        assert!(
            Records::read(&f.plan.layout.lock_path)
                .unwrap()
                .skills
                .is_empty()
        );
        assert!(!f.plan.layout.canonical_dir.join(f.plan.name()).exists());
    }
}

#[test]
fn marketplace_record_changes_invalidate_inventory_and_stale_cleanup_never_writes_git_lock() {
    let f = Fixture::new(false);
    publish(&f.paths, &f.plan, &f.downloaded(&f.plan))
        .result
        .unwrap();
    let read = crate::read_model::ReadModel::new(f.paths.clone());
    assert_eq!(
        read.snapshot(&ScopeId::Global).unwrap().skills[0]
            .source_version
            .as_deref(),
        Some("1.0.0")
    );
    let mut records = Records::read(&f.plan.layout.lock_path).unwrap();
    records.skills.get_mut("original-name").unwrap().version = "2.0.0-longer".into();
    records.save(&f.plan.layout.lock_path).unwrap();
    assert_eq!(
        read.snapshot(&ScopeId::Global).unwrap().skills[0]
            .source_version
            .as_deref(),
        Some("2.0.0-longer")
    );
    fs::remove_file(f.plan.targets[0].skills_dir.join(f.plan.name())).unwrap();
    fs::remove_dir_all(f.plan.layout.canonical_dir.join(f.plan.name())).unwrap();
    let stale = crate::tasks::stale_records::list(&f.paths, &f.plan.layout).unwrap();
    assert!(stale[0].clean_reason.is_none(), "{:?}", stale);
    assert!(
        stale[0]
            .restore_reason
            .as_ref()
            .unwrap()
            .contains("技能市场")
    );
    let request = crate::tasks::batch::BatchRequest {
        action: crate::tasks::batch::BatchAction::CleanRecords,
        skill_ids: vec!["original-name".into()],
        agent_ids: vec![],
        target_keys: None,
        project_root: None,
    };
    let plan = crate::tasks::batch::plan(
        &f.paths,
        &f.plan.layout,
        request,
        "test".into(),
        Default::default(),
    )
    .unwrap();
    assert!(plan.targets[0].target.eligible);
    crate::tasks::stale_records::clean(
        &f.paths,
        &f.plan.layout,
        &plan,
        &plan.targets[0],
        &|_| {},
        &|| crate::tasks::stale_records::verify(&f.paths, &f.plan.layout, &plan, &plan.targets[0]),
    )
    .unwrap();
    crate::tasks::stale_records::advance(&f.plan.layout, &plan, "original-name", true, None)
        .unwrap();
    assert!(
        Records::read(&f.plan.layout.lock_path)
            .unwrap()
            .skills
            .is_empty()
    );
    assert!(!f.plan.layout.lock_path.exists());
}

#[test]
fn removed_or_redirected_marketplace_stage_returns_a_changed_preview_error() {
    let f = Fixture::new(false);
    let command = f.plan.command();
    fs::remove_dir_all(f.plan.stage.path()).unwrap();
    assert_eq!(f.plan.command(), command);
    assert_eq!(
        f.plan.check(&f.paths, &f.plan.layout).unwrap_err().code,
        crate::error::ErrorCode::CliCommandChanged
    );
    std::os::unix::fs::symlink(f.paths.home(), f.plan.stage.path()).unwrap();
    assert!(f.plan.check(&f.paths, &f.plan.layout).is_err());
    fs::remove_file(f.plan.stage.path()).unwrap();
}
