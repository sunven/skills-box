use super::*;
use crate::{
    baseline::BaselineStore,
    cli::CliRuntimeProvider,
    expert_packs::{Member, state},
    marketplace::{Coordinate, runtime::Runtime},
    process::{CancelToken, CommandSpec, ExecError, ExecOutput, ProcessExecutor},
    scope::ScopeId,
    tasks::{
        manager::{FinishOutcome, TaskAction, TaskEnvelope, TaskOverall, TaskSkillStatus},
        session::SessionContext,
    },
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
struct Fixture {
    _tmp: tempfile::TempDir,
    paths: AppPaths,
    plan: Plan,
}
impl Fixture {
    fn new(project: bool) -> Self {
        Self::with_agents(
            project,
            if project {
                &["claude-code"]
            } else {
                &["claude-code", "codex"]
            },
        )
    }

    fn with_agents(project: bool, selected: &[&str]) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let paths = AppPaths::new(root.join("home"), root.join("data"));
        paths.ensure_dirs().unwrap();
        fs::create_dir_all(paths.home().join(".claude/skills")).unwrap();
        fs::create_dir_all(paths.home().join(".codex")).unwrap();
        if selected.contains(&"cursor") {
            fs::create_dir_all(paths.home().join(".cursor")).unwrap();
        }
        let registry = crate::projects::ProjectRegistry::new(paths.clone());
        let scope = if project {
            let dir = root.join("project");
            fs::create_dir(&dir).unwrap();
            registry.add(dir.to_str().unwrap()).unwrap().project.scope()
        } else {
            ScopeId::Global
        };
        let layout = ScopeLayout::resolve_detected(&paths, &registry, &scope).unwrap();
        let agents = selected
            .iter()
            .map(|id| (*id).to_string())
            .collect::<Vec<_>>();
        let (_, targets) =
            super::super::install::validate_selected_agents(&paths, &layout, &agents).unwrap();
        let members: Vec<_> = ["first", "second"]
            .iter()
            .map(|name| Member {
                reference: format!("{name}-slug"),
                coordinate: Coordinate::from_canonical(&format!("@publisher/{name}-slug")).unwrap(),
                name: (*name).into(),
                version: "1.0.0".into(),
                markdown: format!("---\nname: {name}\ndescription: member\n---\n# {name}\n"),
            })
            .collect();
        let raw = "---\nscene: research\n---\n# Workflow\nUse first-slug, then second-slug.\n";
        let (name, installed) =
            crate::expert_packs::workflow::adapt("pack", "Test pack", "Research", raw, &members)
                .unwrap();
        let snapshot = Snapshot {
            slug: "pack".into(),
            display_name: "Test pack".into(),
            summary: "Research".into(),
            original_workflow: raw.into(),
            workflow_name: name,
            installed_workflow: installed,
            members: members.clone(),
            agents,
            installed_at: 1,
        };
        let downloads = members
            .iter()
            .map(|m| marketplace::Plan {
                detail: m.detail(),
                runtime: Runtime {
                    program: "/fake/skillhub".into(),
                    env: BTreeMap::new(),
                },
                stage: marketplace::StagingDirectory::new().unwrap(),
                layout: layout.clone(),
                targets: targets.clone(),
                previous: None,
                previous_paths: None,
                previous_fingerprint: None,
                created_at: crate::time::now_ms(),
            })
            .collect();
        let plan = Plan::build(&paths, layout, snapshot, None, downloads).unwrap();
        Self {
            _tmp: tmp,
            paths,
            plan,
        }
    }
    fn ctx(&self, fail: Option<usize>) -> SessionContext {
        SessionContext {
            task_id: "pack-test".into(),
            sink: Arc::new(|_| {}),
            cancel: CancelToken::new(),
            envelope: TaskEnvelope {
                action: TaskAction::Install,
                scope: self.plan.layout.scope.clone(),
                started_at: 1,
            },
            runtime: Arc::new(NoGit),
            executor: Arc::new(Downloader {
                plans: self
                    .plan
                    .units
                    .iter()
                    .filter_map(|u| u.download.clone())
                    .collect(),
                index: AtomicUsize::new(0),
                fail,
            }),
            record_snapshot: Arc::new(|p| assert!(p.is_dir())),
            set_cancellable: Arc::new(|_| {}),
            expected_commands: Some(
                self.plan
                    .preview()
                    .commands
                    .entries
                    .into_iter()
                    .filter_map(|e| e.command)
                    .collect(),
            ),
        }
    }
    async fn install(&self) -> crate::tasks::TaskResult {
        result(run(self.paths.clone(), self.ctx(None), self.plan.clone()).await)
    }
}
struct NoGit;
#[async_trait::async_trait]
impl CliRuntimeProvider for NoGit {
    async fn cli_runtime(&self) -> Result<CliRuntime, AppError> {
        panic!("pack must not use Git CLI")
    }
}
struct Downloader {
    plans: Vec<marketplace::Plan>,
    index: AtomicUsize,
    fail: Option<usize>,
}
#[async_trait::async_trait]
impl ProcessExecutor for Downloader {
    async fn run(&self, mut command: CommandSpec, _: CancelToken) -> Result<ExecOutput, ExecError> {
        let i = self.index.fetch_add(1, Ordering::SeqCst);
        let plan = &self.plans[i];
        assert_eq!(command.cli_context.as_ref().unwrap().origin, "task");
        command.cli_context = None;
        assert_eq!(command, plan.command());
        // No member may have been published while later downloads are running.
        for other in &self.plans {
            assert!(!other.layout.canonical_dir.join(other.name()).exists());
        }
        let root = plan
            .stage
            .path()
            .join(plan.detail.summary.coordinate.canonical());
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("SKILL.md"),
            if self.fail == Some(i) {
                "wrong content"
            } else {
                &plan.detail.markdown
            },
        )
        .unwrap();
        fs::write(root.join("resource.txt"), "complete package").unwrap();
        let c = &plan.detail.summary.coordinate;
        fs::write(plan.stage.path().join(".skills_store_lock.json"), serde_json::json!({"skills": {c.canonical(): {"zip_url": format!("https://api.skillhub.cn/api/v1/download?namespace={}&slug={}&version={}", c.namespace, c.slug, plan.detail.summary.version)}}}).to_string()).unwrap();
        Ok(ExecOutput {
            code: Some(0),
            signal: None,
            stdout: serde_json::json!({"success":true, "slug": c.canonical(), "targetDir": root})
                .to_string(),
            stderr: String::new(),
            duration: Duration::ZERO,
        })
    }
}
fn result(outcome: FinishOutcome) -> crate::tasks::TaskResult {
    match outcome {
        FinishOutcome::Completed(result, _) => result,
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn publication_member_and_workflow_failures_retain_warnings_content_and_prior_attachments() {
    use std::os::unix::fs::PermissionsExt;

    for project in [false, true] {
        for name in ["first", "pack"] {
            for baseline_failure in [false, true] {
                for attachment_failure in [false, true] {
                    let f = Fixture::with_agents(
                        project,
                        if project {
                            &["claude-code"]
                        } else {
                            &["cursor", "claude-code"]
                        },
                    );
                    let baselines = BaselineStore::for_scope(&f.paths, &f.plan.layout.scope);
                    if baseline_failure {
                        fs::create_dir_all(baselines.baseline_path(name)).unwrap();
                    }
                    let agent_dir = f.plan.layout.root.join(".claude/skills");
                    fs::create_dir_all(&agent_dir).unwrap();
                    let permissions = fs::metadata(&agent_dir).unwrap().permissions();
                    if attachment_failure {
                        fs::set_permissions(&agent_dir, fs::Permissions::from_mode(0o555)).unwrap();
                        let probe = agent_dir.join("permission-probe");
                        let writable = fs::write(&probe, "probe").is_ok();
                        fs::set_permissions(&agent_dir, permissions.clone()).unwrap();
                        if writable {
                            fs::remove_file(probe).unwrap();
                            continue;
                        }
                    }
                    let mut ctx = f.ctx(None);
                    let blocked = agent_dir.clone();
                    let restore = permissions.clone();
                    ctx.sink = Arc::new(move |event| {
                        if attachment_failure && event.message == format!("正在安装 {name}…")
                        {
                            fs::set_permissions(&blocked, fs::Permissions::from_mode(0o555))
                                .unwrap();
                        } else if event.message.starts_with("已处理 ") {
                            fs::set_permissions(&blocked, restore.clone()).unwrap();
                        }
                    });
                    let mut result = result(run(f.paths.clone(), ctx, f.plan.clone()).await);
                    fs::set_permissions(&agent_dir, permissions).unwrap();
                    assert_eq!(
                        result.warnings.len(),
                        usize::from(baseline_failure),
                        "{result:?}"
                    );
                    if baseline_failure {
                        assert_eq!(
                            result.warnings[0].code,
                            crate::error::ErrorCode::BaselineWriteFailed
                        );
                        assert!(baselines.load(name).is_none());
                    } else {
                        let baseline = baselines.load(name).unwrap();
                        assert!(baseline.verified);
                        assert_eq!(
                            baseline.content_hash,
                            fingerprint_folder(&f.plan.layout.canonical_dir.join(name))
                                .unwrap()
                                .content_hash
                        );
                    }
                    assert!(
                        f.plan
                            .layout
                            .canonical_dir
                            .join(name)
                            .join("SKILL.md")
                            .is_file()
                    );
                    if name == "pack" {
                        assert!(
                            Records::read(&f.plan.layout.lock_path)
                                .unwrap()
                                .workflows
                                .contains_key(name)
                        );
                    } else {
                        assert!(
                            crate::marketplace::records::Records::read(&f.plan.layout.lock_path)
                                .unwrap()
                                .skills
                                .contains_key(name)
                        );
                    }
                    assert_eq!(
                        result.overall,
                        if attachment_failure {
                            TaskOverall::Partial
                        } else {
                            TaskOverall::Success
                        },
                        "{result:?}"
                    );
                    let row = result.skills.iter().find(|row| row.name == name).unwrap();
                    assert_eq!(
                        row.status,
                        if attachment_failure {
                            TaskSkillStatus::Failed
                        } else {
                            TaskSkillStatus::Installed
                        }
                    );
                    assert_eq!(agent_dir.join(name).is_symlink(), !attachment_failure);
                    if attachment_failure {
                        assert_eq!(
                            row.error.as_ref().unwrap().code,
                            crate::error::ErrorCode::AgentAttachFailed
                        );
                        if name == "first" {
                            assert_eq!(result.skills[1].status, TaskSkillStatus::Installed);
                            assert_eq!(result.skills[2].status, TaskSkillStatus::Skipped);
                        }
                    }
                    if !project {
                        assert!(
                            f.paths
                                .home()
                                .join(".cursor/skills")
                                .join(name)
                                .is_symlink(),
                            "earlier attachments survive"
                        );
                    }
                    let backup = PathBuf::from(row.backup_path.as_ref().unwrap());
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

#[tokio::test]
async fn publication_workflow_record_failure_stops_before_baseline_and_attachments() {
    use std::os::unix::fs::PermissionsExt;

    for project in [false, true] {
        let f = Fixture::new(project);
        fs::create_dir_all(&f.plan.layout.canonical_dir).unwrap();
        let record_dir = f.plan.layout.lock_path.parent().unwrap().to_path_buf();
        let permissions = fs::metadata(&record_dir).unwrap().permissions();
        fs::set_permissions(&record_dir, fs::Permissions::from_mode(0o555)).unwrap();
        let probe = record_dir.join("permission-probe");
        let writable = fs::write(&probe, "probe").is_ok();
        fs::set_permissions(&record_dir, permissions.clone()).unwrap();
        if writable {
            fs::remove_file(probe).unwrap();
            continue;
        }
        let mut ctx = f.ctx(None);
        let blocked = record_dir.clone();
        ctx.sink = Arc::new(move |event| {
            if event.message == "正在安装 pack…" {
                fs::set_permissions(&blocked, fs::Permissions::from_mode(0o555)).unwrap();
            }
        });
        let result = result(run(f.paths.clone(), ctx, f.plan.clone()).await);
        fs::set_permissions(&record_dir, permissions).unwrap();
        assert_eq!(result.overall, TaskOverall::Partial, "{result:?}");
        assert_eq!(
            result.skills[2].error.as_ref().unwrap().code,
            crate::error::ErrorCode::LockReadFailed
        );
        assert!(result.warnings.is_empty());
        assert!(f.plan.layout.canonical_dir.join("pack/SKILL.md").is_file());
        assert!(
            Records::read(&f.plan.layout.lock_path)
                .unwrap()
                .workflows
                .is_empty()
        );
        assert!(
            BaselineStore::for_scope(&f.paths, &f.plan.layout.scope)
                .load("pack")
                .is_none()
        );
        assert!(!f.plan.layout.root.join(".claude/skills/pack").exists());
        assert!(PathBuf::from(result.skills[2].backup_path.as_ref().unwrap()).is_dir());
    }
}

#[tokio::test]
async fn cursor_pack_install_and_supplement_create_required_global_attachments() {
    let mut f = Fixture::with_agents(false, &["cursor", "codex"]);

    let installed = f.install().await;
    assert_eq!(installed.overall, TaskOverall::Success, "{installed:?}");
    let cursor = f.paths.home().join(".cursor/skills");
    for name in f.plan.snapshot.names() {
        assert!(cursor.join(name).is_symlink());
        assert_eq!(
            fs::canonicalize(cursor.join(name)).unwrap(),
            fs::canonicalize(f.plan.layout.canonical_dir.join(name)).unwrap()
        );
    }
    assert!(
        state::observe(&f.paths, &f.plan.layout, &f.plan.snapshot)
            .unwrap()
            .complete
    );

    fs::remove_file(cursor.join("first")).unwrap();
    fs::remove_file(cursor.join("pack")).unwrap();
    let before = fs::read(f.plan.layout.canonical_dir.join("first/SKILL.md")).unwrap();
    let baselines = BaselineStore::for_scope(&f.paths, &f.plan.layout.scope);
    let baseline_bytes: Vec<_> = f
        .plan
        .snapshot
        .names()
        .map(|name| {
            (
                name.to_string(),
                fs::read(baselines.baseline_path(name)).unwrap(),
            )
        })
        .collect();
    let records = fs::read(crate::expert_packs::records::path(&f.plan.layout.lock_path)).unwrap();
    let member_records =
        fs::read(crate::marketplace::records::path(&f.plan.layout.lock_path)).unwrap();
    let supplement = Plan::build(
        &f.paths,
        f.plan.layout.clone(),
        f.plan.snapshot.clone(),
        Some(f.plan.snapshot.clone()),
        vec![],
    )
    .unwrap();
    assert_eq!(supplement.preview().items[0].missing_agents, ["cursor"]);
    assert_eq!(supplement.preview().items[2].missing_agents, ["cursor"]);
    f.plan = supplement;
    let completed = f.install().await;
    assert_eq!(completed.overall, TaskOverall::Success, "{completed:?}");
    assert_eq!(
        fs::read(f.plan.layout.canonical_dir.join("first/SKILL.md")).unwrap(),
        before
    );
    for (name, bytes) in baseline_bytes {
        assert_eq!(fs::read(baselines.baseline_path(&name)).unwrap(), bytes);
    }
    assert_eq!(
        fs::read(crate::expert_packs::records::path(&f.plan.layout.lock_path)).unwrap(),
        records
    );
    assert_eq!(
        fs::read(crate::marketplace::records::path(&f.plan.layout.lock_path)).unwrap(),
        member_records
    );
    assert!(
        state::observe(&f.paths, &f.plan.layout, &f.plan.snapshot)
            .unwrap()
            .complete
    );
}

#[tokio::test]
async fn regression_pack_completeness_rejects_links_to_other_content() {
    use std::os::unix::fs::symlink;

    for (project, agent, directory) in [
        (false, "claude-code", ".claude/skills"),
        (true, "claude-code", ".claude/skills"),
        (false, "cursor", ".cursor/skills"),
    ] {
        let f = Fixture::with_agents(project, &[agent]);
        f.install().await;
        for name in ["first", f.plan.snapshot.workflow_name.as_str()] {
            let attachment = f.plan.layout.root.join(directory).join(name);
            let unrelated = f.plan.layout.root.join("unrelated").join(name);
            fs::create_dir_all(&unrelated).unwrap();
            fs::write(unrelated.join("SKILL.md"), "unrelated content").unwrap();
            fs::remove_file(&attachment).unwrap();
            symlink(&unrelated, &attachment).unwrap();

            let observed = state::observe(&f.paths, &f.plan.layout, &f.plan.snapshot).unwrap();
            let skill = observed.skills.iter().find(|s| s.name == name).unwrap();
            assert!(!observed.complete, "{observed:?}");
            assert_eq!(skill.missing_agents, [agent]);
            assert!(!skill.attached_agents.iter().any(|id| id == agent));
            assert!(skill.conflict);
            let error = Plan::build(
                &f.paths,
                f.plan.layout.clone(),
                f.plan.snapshot.clone(),
                Some(f.plan.snapshot.clone()),
                vec![],
            )
            .unwrap_err();
            assert_eq!(error.code, crate::error::ErrorCode::SkillConflict);
            assert_eq!(
                fs::read_to_string(attachment.join("SKILL.md")).unwrap(),
                "unrelated content"
            );

            fs::remove_file(&attachment).unwrap();
            symlink(f.plan.layout.canonical_dir.join(name), &attachment).unwrap();
            assert!(
                state::observe(&f.paths, &f.plan.layout, &f.plan.snapshot)
                    .unwrap()
                    .complete
            );
        }
    }
}

#[tokio::test]
async fn stages_every_member_before_publication_in_both_scopes_and_workflow_is_managed() {
    for project in [false, true] {
        let f = Fixture::new(project);
        let result = f.install().await;
        assert_eq!(result.overall, TaskOverall::Success, "{result:?}");
        let observed = state::observe(&f.paths, &f.plan.layout, &f.plan.snapshot).unwrap();
        assert!(observed.complete, "{observed:?}");
        assert!(observed.skills.iter().all(|s| !s.modified));
        let lock = f.plan.layout.lock().unwrap();
        assert_eq!(lock.get("pack").unwrap().source_type, "skillhub-pack");
        let saved = Records::read(&f.plan.layout.lock_path).unwrap();
        assert_eq!(saved.packs["pack"], f.plan.snapshot);
        let update =
            crate::tasks::update::plan_updates(&f.paths, &f.plan.layout, &["pack".into()]).unwrap();
        assert!(update.items.is_empty());
        let current = ScopeLayout::resolve_detected(
            &f.paths,
            &crate::projects::ProjectRegistry::new(f.paths.clone()),
            &f.plan.layout.scope,
        )
        .unwrap();
        let workflow = crate::tasks::uninstall::plan_uninstall(
            &f.paths,
            &current,
            &current.scope.skill_id("pack"),
        )
        .unwrap();
        assert!(workflow.marketplace);
        let removed = crate::tasks::session::run(
            f.ctx(None),
            crate::tasks::uninstall::UninstallAction::new(
                f.paths.clone(),
                current,
                Duration::from_secs(1),
            ),
            workflow,
        )
        .await;
        assert_eq!(result_overall(removed), TaskOverall::Success);
        assert!(f.plan.layout.canonical_dir.join("first").exists());
        assert!(
            !state::observe(&f.paths, &f.plan.layout, &f.plan.snapshot)
                .unwrap()
                .complete
        );
        assert!(
            Records::read(&f.plan.layout.lock_path)
                .unwrap()
                .packs
                .contains_key("pack")
        );
    }
}
fn result_overall(outcome: FinishOutcome) -> TaskOverall {
    result(outcome).overall
}

#[tokio::test]
async fn download_validation_failure_publishes_nothing_and_retains_snapshot() {
    let f = Fixture::new(false);
    let result = result(run(f.paths.clone(), f.ctx(Some(1)), f.plan.clone()).await);
    assert_eq!(result.overall, TaskOverall::Failed);
    for name in f.plan.snapshot.names() {
        assert!(!f.plan.layout.canonical_dir.join(name).exists());
    }
    assert_eq!(result.skills[1].status, TaskSkillStatus::Failed);
    assert_eq!(result.skills[0].status, TaskSkillStatus::Skipped);
    assert_eq!(
        Records::read(&f.plan.layout.lock_path).unwrap().packs["pack"],
        f.plan.snapshot
    );
}

#[tokio::test]
async fn cancellation_keeps_successes_and_supplement_does_not_reinstall_or_change_snapshot() {
    let f = Fixture::new(false);
    let mut ctx = f.ctx(None);
    let token = ctx.cancel.clone();
    ctx.sink = Arc::new(move |event| {
        if event.message.starts_with("已处理 1") {
            token.cancel();
        }
    });
    let partial = result(run(f.paths.clone(), ctx, f.plan.clone()).await);
    assert_eq!(partial.overall, TaskOverall::Cancelled);
    assert!(f.plan.layout.canonical_dir.join("first").exists());
    assert!(!f.plan.layout.canonical_dir.join("pack").exists());
    // Simulate an independent, clean member update. Completion must retain it.
    let canonical = f.plan.layout.canonical_dir.join("first");
    let changed = format!(
        "{}\nUpdated independently",
        f.plan.snapshot.members[0].markdown
    );
    fs::write(canonical.join("SKILL.md"), changed).unwrap();
    let mut market = crate::marketplace::records::Records::read(&f.plan.layout.lock_path).unwrap();
    let first = market.skills.get_mut("first").unwrap();
    first.version = "2.0.0".into();
    first.content_hash = fingerprint_folder(&canonical).unwrap().content_hash;
    market.save(&f.plan.layout.lock_path).unwrap();
    let supplement = Plan::build(
        &f.paths,
        f.plan.layout.clone(),
        f.plan.snapshot.clone(),
        Some(f.plan.snapshot.clone()),
        vec![f.plan.units[1].download.clone().unwrap()],
    )
    .unwrap();
    assert_eq!(
        supplement.preview().items[0].version.as_deref(),
        Some("2.0.0")
    );
    let mut ctx = f.ctx(None);
    ctx.executor = Arc::new(Downloader {
        plans: vec![f.plan.units[1].download.clone().unwrap()],
        index: AtomicUsize::new(0),
        fail: None,
    });
    ctx.expected_commands = Some(
        supplement
            .preview()
            .commands
            .entries
            .into_iter()
            .filter_map(|e| e.command)
            .collect(),
    );
    let result = result(run(f.paths.clone(), ctx, supplement).await);
    assert_eq!(result.overall, TaskOverall::Success, "{result:?}");
    let state = state::observe(&f.paths, &f.plan.layout, &f.plan.snapshot).unwrap();
    assert!(state.complete);
    assert!(state.skills[0].version_changed);
    assert!(!state.skills[0].modified);
    assert_eq!(
        Records::read(&f.plan.layout.lock_path).unwrap().packs["pack"],
        f.plan.snapshot
    );
}

#[tokio::test]
async fn forgetting_preserves_sources_and_clean_workflow_can_be_reused() {
    let f = Fixture::new(false);
    assert_eq!(f.install().await.overall, TaskOverall::Success);
    let mut records = Records::read(&f.plan.layout.lock_path).unwrap();
    records.packs.clear();
    records.save(&f.plan.layout.lock_path).unwrap();
    assert_eq!(
        f.plan
            .layout
            .lock()
            .unwrap()
            .get("pack")
            .unwrap()
            .source_type,
        "skillhub-pack"
    );
    let reinstalled = Plan::build(
        &f.paths,
        f.plan.layout.clone(),
        f.plan.snapshot.clone(),
        None,
        vec![],
    )
    .unwrap();
    assert!(
        reinstalled
            .preview()
            .items
            .iter()
            .all(|i| i.action == "reuse")
    );
    let mut different = f.plan.snapshot.clone();
    different.installed_workflow.push_str("\nupstream changed");
    assert!(Plan::build(&f.paths, f.plan.layout.clone(), different, None, vec![]).is_err());
    fs::write(
        f.plan.layout.canonical_dir.join("first/resource.txt"),
        "local edit",
    )
    .unwrap();
    assert!(
        Plan::build(
            &f.paths,
            f.plan.layout.clone(),
            f.plan.snapshot.clone(),
            None,
            vec![]
        )
        .is_err()
    );
}

#[tokio::test]
async fn conflicts_stale_records_and_changed_attachments_are_not_adopted() {
    let f = Fixture::new(false);
    fs::create_dir_all(f.plan.layout.canonical_dir.join("first")).unwrap();
    assert!(f.plan.check(&f.paths, &f.plan.layout).is_err());
    fs::remove_dir(f.plan.layout.canonical_dir.join("first")).unwrap();
    assert_eq!(f.install().await.overall, TaskOverall::Success);
    let saved = f.plan.snapshot.clone();
    let plan = Plan::build(
        &f.paths,
        f.plan.layout.clone(),
        saved.clone(),
        Some(saved.clone()),
        vec![],
    )
    .unwrap();
    fs::remove_file(f.paths.home().join(".claude/skills/first")).unwrap();
    assert!(plan.check(&f.paths, &f.plan.layout).is_err());
    fs::remove_dir_all(f.plan.layout.canonical_dir.join("first")).unwrap();
    assert!(
        Plan::build(
            &f.paths,
            f.plan.layout.clone(),
            saved.clone(),
            Some(saved),
            vec![]
        )
        .is_err()
    );
}

#[test]
fn adapts_only_missing_fields_preserves_prose_and_rejects_conflicting_identity() {
    let f = Fixture::new(false);
    let raw =
        "---\nname: official-name\ndescription: original description\n---\n# unchanged prose\n";
    let (name, adapted) = crate::expert_packs::workflow::adapt(
        "pack",
        "display",
        "summary",
        raw,
        &f.plan.snapshot.members,
    )
    .unwrap();
    assert_eq!(name, "official-name");
    assert!(adapted.starts_with(raw));
    assert!(adapted.contains("`first-slug` | `@publisher/first-slug` | `first`"));
    for invalid in [
        "---\nname: a\nname: b\n---\ntext",
        "---\nname: ../escape\n---\ntext",
        "---\nname: [a,b]\n---\ntext",
        "---\nname: \"bad\n---",
        "",
    ] {
        assert!(
            crate::expert_packs::workflow::adapt("pack", "title", "summary", invalid, &[]).is_err(),
            "{invalid}"
        );
    }
    let mut collision = f.plan.snapshot.clone();
    collision.workflow_name = "first".into();
    collision.installed_workflow = f.plan.snapshot.members[0].markdown.clone();
    assert!(collision.validate().is_err());
}

#[test]
fn preview_rejects_case_only_collisions_between_members_and_workflow() {
    let f = Fixture::new(false);
    for workflow_collision in [false, true] {
        let mut snapshot = f.plan.snapshot.clone();
        let markdown = "---\nname: FIRST\ndescription: conflicting identity\n---\n";
        if workflow_collision {
            snapshot.workflow_name = "FIRST".into();
            snapshot.installed_workflow = markdown.into();
        } else {
            snapshot.members[1].name = "FIRST".into();
            snapshot.members[1].markdown = markdown.into();
        }
        let error =
            Plan::build(&f.paths, f.plan.layout.clone(), snapshot, None, vec![]).unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::SkillConflict);
    }
}

#[tokio::test]
async fn ordinary_publication_failure_keeps_later_successes_but_never_publishes_workflow() {
    let f = Fixture::new(false);
    let mut ctx = f.ctx(None);
    let root = f.plan.layout.canonical_dir.clone();
    ctx.sink = Arc::new(move |event| {
        if event.message == "正在安装 first…" {
            fs::create_dir_all(root.join("first")).unwrap();
        }
    });
    let result = result(run(f.paths.clone(), ctx, f.plan.clone()).await);
    assert_eq!(result.overall, TaskOverall::Partial, "{result:?}");
    assert_eq!(result.skills[0].status, TaskSkillStatus::Failed);
    assert_eq!(result.skills[1].status, TaskSkillStatus::Installed);
    assert_eq!(result.skills[2].status, TaskSkillStatus::Skipped);
    assert!(f.plan.layout.canonical_dir.join("second/SKILL.md").exists());
    assert!(!f.plan.layout.canonical_dir.join("pack").exists());
}

#[tokio::test]
async fn backup_failure_stops_later_units_and_preserves_completed_content() {
    let f = Fixture::new(false);
    let mut ctx = f.ctx(None);
    let backups = f.paths.backups_dir();
    ctx.sink = Arc::new(move |event| {
        if event.message.starts_with("已处理 1") {
            fs::rename(&backups, backups.with_file_name("saved-backups")).unwrap();
            fs::write(&backups, "blocked").unwrap();
        }
    });
    let result = result(run(f.paths.clone(), ctx, f.plan.clone()).await);
    assert_eq!(result.overall, TaskOverall::Partial);
    assert_eq!(result.skills[0].status, TaskSkillStatus::Installed);
    assert_eq!(result.skills[1].status, TaskSkillStatus::Failed);
    assert!(f.plan.layout.canonical_dir.join("first/SKILL.md").exists());
    assert!(!f.plan.layout.canonical_dir.join("second").exists());
    assert!(!f.plan.layout.canonical_dir.join("pack").exists());
}

#[tokio::test]
async fn shared_members_retain_current_versions_and_workflows_have_independent_baselines() {
    let f = Fixture::new(false);
    assert_eq!(f.install().await.overall, TaskOverall::Success);
    let mut second = f.plan.snapshot.clone();
    second.slug = "second-pack".into();
    let (name, text) = crate::expert_packs::workflow::adapt(
        &second.slug,
        &second.display_name,
        &second.summary,
        &second.original_workflow,
        &second.members,
    )
    .unwrap();
    second.workflow_name = name;
    second.installed_workflow = text;
    let plan = Plan::build(
        &f.paths,
        f.plan.layout.clone(),
        second.clone(),
        None,
        vec![],
    )
    .unwrap();
    let mut ctx = f.ctx(None);
    ctx.expected_commands = Some(vec![]);
    assert_eq!(
        result_overall(run(f.paths.clone(), ctx, plan).await),
        TaskOverall::Success
    );
    assert_eq!(state::list(&f.paths, &f.plan.layout).unwrap().len(), 2);
    let member =
        crate::tasks::uninstall::plan_uninstall(&f.paths, &f.plan.layout, "first").unwrap();
    assert_eq!(
        result_overall(
            crate::tasks::session::run(
                f.ctx(None),
                crate::tasks::uninstall::UninstallAction::new(
                    f.paths.clone(),
                    f.plan.layout.clone(),
                    Duration::from_secs(1)
                ),
                member
            )
            .await
        ),
        TaskOverall::Success
    );
    assert!(
        state::list(&f.paths, &f.plan.layout)
            .unwrap()
            .iter()
            .all(|p| !p.complete)
    );
    assert!(f.plan.layout.canonical_dir.join("pack").exists());
    assert!(f.plan.layout.canonical_dir.join("second-pack").exists());
}

#[tokio::test]
async fn stale_workflow_cleanup_keeps_pack_snapshot_and_other_sources() {
    let f = Fixture::new(false);
    assert_eq!(f.install().await.overall, TaskOverall::Success);
    fs::remove_file(f.paths.home().join(".claude/skills/pack")).unwrap();
    fs::remove_dir_all(f.plan.layout.canonical_dir.join("pack")).unwrap();
    let stale = crate::tasks::stale_records::list(&f.paths, &f.plan.layout).unwrap();
    assert!(
        stale
            .iter()
            .any(|s| s.name == "pack" && s.clean_reason.is_none())
    );
    let plan = crate::tasks::batch::plan(
        &f.paths,
        &f.plan.layout,
        crate::tasks::batch::BatchRequest {
            action: crate::tasks::batch::BatchAction::CleanRecords,
            skill_ids: vec!["pack".into()],
            agent_ids: vec![],
            target_keys: None,
            project_root: None,
        },
        "clean".into(),
        Default::default(),
    )
    .unwrap();
    crate::tasks::stale_records::clean(
        &f.paths,
        &f.plan.layout,
        &plan,
        &plan.targets[0],
        &|_| {},
        &|| crate::tasks::stale_records::verify(&f.paths, &f.plan.layout, &plan, &plan.targets[0]),
    )
    .unwrap();
    crate::tasks::stale_records::advance(&f.plan.layout, &plan, "pack", true, None).unwrap();
    let records = Records::read(&f.plan.layout.lock_path).unwrap();
    assert!(records.workflows.is_empty());
    assert!(records.packs.contains_key("pack"));
    assert!(!f.plan.layout.lock_path.exists());
    assert_eq!(
        crate::marketplace::records::Records::read(&f.plan.layout.lock_path)
            .unwrap()
            .skills
            .len(),
        2
    );
}

#[test]
fn corrupt_pack_records_and_symlinks_are_never_overwritten() {
    let f = Fixture::new(false);
    let mut records = Records::default();
    records.packs.insert("pack".into(), f.plan.snapshot.clone());
    records.save(&f.plan.layout.lock_path).unwrap();
    let path = crate::expert_packs::records::path(&f.plan.layout.lock_path);
    for field in ["schema", "identity", "member", "unknown"] {
        let mut json = serde_json::to_value(&records).unwrap();
        match field {
            "schema" => json["version"] = 2.into(),
            "identity" => json["packs"]["pack"]["slug"] = "different".into(),
            "member" => json["packs"]["pack"]["members"][0]["name"] = "other".into(),
            _ => json["unknown"] = true.into(),
        }
        let bytes = serde_json::to_vec(&json).unwrap();
        fs::write(&path, &bytes).unwrap();
        assert!(Records::read(&f.plan.layout.lock_path).is_err());
        assert!(records.save(&f.plan.layout.lock_path).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
    fs::remove_file(&path).unwrap();
    let unrelated = f.paths.home().join("untouched");
    fs::write(&unrelated, "keep").unwrap();
    std::os::unix::fs::symlink(&unrelated, &path).unwrap();
    assert!(Records::read(&f.plan.layout.lock_path).is_err());
    assert!(records.save(&f.plan.layout.lock_path).is_err());
    assert_eq!(fs::read_to_string(unrelated).unwrap(), "keep");
}

#[tokio::test]
async fn fresh_preview_rejects_a_reused_member_updated_while_resolving_the_pack() {
    let f = Fixture::new(false);
    assert_eq!(f.install().await.overall, TaskOverall::Success);
    let mut records = Records::read(&f.plan.layout.lock_path).unwrap();
    records.packs.clear();
    records.save(&f.plan.layout.lock_path).unwrap();
    let mut market = crate::marketplace::records::Records::read(&f.plan.layout.lock_path).unwrap();
    market.skills.get_mut("first").unwrap().version = "2.0.0".into();
    market.save(&f.plan.layout.lock_path).unwrap();
    assert!(
        Plan::build(
            &f.paths,
            f.plan.layout.clone(),
            f.plan.snapshot.clone(),
            None,
            vec![]
        )
        .is_err()
    );
    let mut current = f.plan.snapshot.clone();
    current.members[0].version = "2.0.0".into();
    assert!(Plan::build(&f.paths, f.plan.layout.clone(), current, None, vec![]).is_ok());
}

#[tokio::test]
async fn missing_skill_document_is_incomplete_and_cannot_be_overwritten_by_supplement() {
    let f = Fixture::new(false);
    assert_eq!(f.install().await.overall, TaskOverall::Success);
    fs::remove_file(f.plan.layout.canonical_dir.join("first/SKILL.md")).unwrap();
    let observed = state::observe(&f.paths, &f.plan.layout, &f.plan.snapshot).unwrap();
    assert!(!observed.complete);
    assert!(!observed.skills[0].present);
    assert!(observed.skills[0].modified);
    assert!(
        Plan::build(
            &f.paths,
            f.plan.layout.clone(),
            f.plan.snapshot.clone(),
            Some(f.plan.snapshot.clone()),
            vec![]
        )
        .is_err()
    );
}
