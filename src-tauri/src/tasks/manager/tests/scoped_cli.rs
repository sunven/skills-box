use super::*;
use crate::baseline::Baseline;
use crate::cli::{commands::display_command, preview::CliCommandRequest};
use crate::scope::ScopeId;

const EMPTY_PROJECT_LOCK: &str = r#"{"version":1,"skills":{}}"#;
const GLOBAL_CONTENT: &str = "# find-skills global\n";
const PROJECT_CONTENT: &str = "# find-skills project\n";

struct Fixture {
    _tmp: tempfile::TempDir,
    paths: AppPaths,
    root: PathBuf,
    scope: ScopeId,
    manager: TaskManager,
    executor: Arc<InstallExecutor>,
    global_lock: Vec<u8>,
    global_baseline: Baseline,
}

impl Fixture {
    fn new(writes_project: bool) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(tmp.path().join("home"), tmp.path().join("data"));
        paths.ensure_dirs().unwrap();
        let global_canonical = paths.canonical_skills_dir().join("find-skills");
        fs::create_dir_all(&global_canonical).unwrap();
        fs::write(global_canonical.join("SKILL.md"), GLOBAL_CONTENT).unwrap();
        let global_agent_dir = paths.home().join(".qoder/skills");
        fs::create_dir_all(&global_agent_dir).unwrap();
        std::os::unix::fs::symlink(&global_canonical, global_agent_dir.join("find-skills"))
            .unwrap();
        write_pre_lock(&paths, &["find-skills"]);
        let global_lock = fs::read(paths.lock_path()).unwrap();
        let global_baseline = BaselineStore::new(&paths)
            .record_from_dir("find-skills", &global_canonical, true)
            .unwrap();

        let root = tmp.path().join("project");
        fs::create_dir_all(root.join(".agents/skills")).unwrap();
        fs::create_dir_all(root.join(".qoder/skills")).unwrap();
        fs::write(root.join("skills-lock.json"), EMPTY_PROJECT_LOCK).unwrap();
        let projects = Arc::new(ProjectRegistry::new(paths.clone()));
        let project = projects.add(root.to_str().unwrap()).unwrap().project;
        let scope = project.scope();
        let root = project.root;
        let written_root = root.clone();
        let executor = Arc::new(InstallExecutor::new(InstallResponse::success(
            INSTALL_SUCCESS,
            move || {
                if !writes_project {
                    return;
                }
                let canonical = written_root.join(".agents/skills/find-skills");
                fs::create_dir_all(&canonical).unwrap();
                fs::write(canonical.join("SKILL.md"), PROJECT_CONTENT).unwrap();
                std::os::unix::fs::symlink(
                    &canonical,
                    written_root.join(".qoder/skills/find-skills"),
                )
                .unwrap();
                fs::write(
                    written_root.join("skills-lock.json"),
                    serde_json::json!({
                        "version": 1,
                        "skills": {
                            "find-skills": {
                                "source": "vercel-labs/skills",
                                "sourceType": "github",
                                "sourceUrl": "https://github.com/vercel-labs/skills.git",
                                "skillPath": "skills/find-skills/SKILL.md",
                                "computedHash": "a".repeat(64)
                            }
                        }
                    })
                    .to_string(),
                )
                .unwrap();
            },
        )));
        let manager = TaskManager::with_projects(
            Arc::new(FakeRuntime::ok()),
            executor.clone(),
            Arc::new(FakeConflicts::default()),
            paths.clone(),
            projects,
            TaskManagerConfig {
                preview_timeout: Duration::from_secs(60),
                install_timeout: Duration::from_secs(5),
            },
        );
        Self {
            _tmp: tmp,
            paths,
            root,
            scope,
            manager,
            executor,
            global_lock,
            global_baseline,
        }
    }

    async fn install(&self) -> TaskEvent {
        let (sink, mut receiver) = sink();
        self.manager
            .start_install(
                self.scope.clone(),
                install_request(&["find-skills"], &["qoder"]),
                sink,
            )
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), final_event(&mut receiver))
            .await
            .expect("install completes")
    }

    fn assert_global_preserved(&self) {
        assert_eq!(
            fs::read_to_string(
                self.paths
                    .canonical_skills_dir()
                    .join("find-skills/SKILL.md")
            )
            .unwrap(),
            GLOBAL_CONTENT
        );
        assert_eq!(fs::read(self.paths.lock_path()).unwrap(), self.global_lock);
        assert_eq!(
            fs::canonicalize(self.paths.home().join(".qoder/skills/find-skills")).unwrap(),
            fs::canonicalize(self.paths.canonical_skills_dir().join("find-skills")).unwrap()
        );
        assert_eq!(
            BaselineStore::new(&self.paths).load("find-skills").unwrap(),
            self.global_baseline
        );
    }
}

#[tokio::test]
async fn project_install_command_verification_backup_and_baseline_share_scope() {
    let fixture = Fixture::new(true);
    let event = fixture.install().await;
    assert_eq!(event.phase, TaskPhase::Completed);
    let result = event.result.unwrap();
    assert_eq!(result.scope, fixture.scope);
    assert_eq!(result.overall, TaskOverall::Success);
    assert_eq!(result.skills[0].status, TaskSkillStatus::Installed);
    assert!(result.warnings.is_empty());

    let calls = fixture.executor.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].cwd, fixture.root);
    assert_eq!(
        calls[0].args,
        vec![
            "-y",
            "skills@latest",
            "add",
            "vercel-labs/skills",
            "-s",
            "find-skills",
            "-a",
            "qoder",
            "-a",
            "universal",
            "-y",
            "--json"
        ]
    );

    let canonical = fixture.root.join(".agents/skills/find-skills");
    assert_eq!(
        fs::read_to_string(canonical.join("SKILL.md")).unwrap(),
        PROJECT_CONTENT
    );
    assert_eq!(
        fs::canonicalize(fixture.root.join(".qoder/skills/find-skills")).unwrap(),
        canonical
    );
    let baseline = BaselineStore::for_scope(&fixture.paths, &fixture.scope)
        .load("find-skills")
        .unwrap();
    assert!(baseline.verified);
    assert_eq!(PathBuf::from(baseline.path), canonical);
    assert_ne!(baseline.content_hash, fixture.global_baseline.content_hash);

    let backup = PathBuf::from(result.backup_path.unwrap());
    assert_eq!(
        backup.parent().unwrap(),
        fixture
            .paths
            .backups_dir()
            .join(format!("project-{}", fixture.scope.project_id().unwrap()))
    );
    assert_eq!(
        fs::read_to_string(backup.join("skills-lock.json")).unwrap(),
        EMPTY_PROJECT_LOCK
    );
    assert!(!backup.join("skill-lock.json").exists());
    fixture.assert_global_preserved();
    assert!(fixture.manager.active_mutation().is_none());
    assert!(fixture.manager.interrupted_report().is_none());
}

#[tokio::test]
async fn global_skill_cannot_satisfy_project_install_verification() {
    let fixture = Fixture::new(false);
    let result = fixture.install().await.result.unwrap();
    assert_eq!(result.scope, fixture.scope);
    assert_eq!(result.overall, TaskOverall::Failed);
    assert_eq!(result.skills[0].status, TaskSkillStatus::Failed);
    assert_eq!(
        result.skills[0].error.as_ref().unwrap().code,
        ErrorCode::VerifyCanonicalMissing
    );
    assert!(
        BaselineStore::for_scope(&fixture.paths, &fixture.scope)
            .load("find-skills")
            .is_none()
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("skills-lock.json")).unwrap(),
        EMPTY_PROJECT_LOCK
    );
    fixture.assert_global_preserved();
}

#[tokio::test]
async fn preview_uses_selected_scope_and_keeps_its_read_only_lifecycle() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(tmp.path().join("home"), tmp.path().join("data"));
    paths.ensure_dirs().unwrap();
    let root = tmp.path().join("project");
    fs::create_dir_all(&root).unwrap();
    let projects = Arc::new(ProjectRegistry::new(paths.clone()));
    let project = projects.add(root.to_str().unwrap()).unwrap().project;
    let executor = Arc::new(
        ScriptedExecutor::new()
            .expect_any(ScriptedResponse::ok(SINGLE_CI))
            .expect_any(ScriptedResponse::ok(SINGLE_CI)),
    );
    let manager = TaskManager::with_projects(
        Arc::new(FakeRuntime::ok()),
        executor.clone(),
        Arc::new(FakeConflicts::default()),
        paths.clone(),
        projects,
        TaskManagerConfig::default(),
    );
    let _guard = manager.begin_registry_change().unwrap();
    for scope in [ScopeId::Global, project.scope()] {
        let preview = manager
            .preview_cli_commands(
                scope.clone(),
                CliCommandRequest::Preview {
                    source: "owner/repo".into(),
                },
            )
            .await
            .unwrap();
        assert_eq!(executor.call_count(), if scope.is_global() { 0 } else { 1 });
        let (sink, mut receiver) = sink();
        manager
            .start_preview(scope.clone(), "owner/repo", sink)
            .await
            .unwrap();
        let event = tokio::time::timeout(Duration::from_secs(2), final_event(&mut receiver))
            .await
            .unwrap();
        assert_eq!(event.phase, TaskPhase::Completed);
        assert_eq!(event.result.unwrap().scope, scope);
        assert_eq!(
            preview.entries[0].command.as_deref(),
            Some(display_command(executor.calls().last().unwrap()).as_str())
        );
    }
    let calls = executor.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].cwd, paths.cli_cwd());
    assert_eq!(
        calls[0].args,
        vec!["-y", "skills@latest", "add", "owner/repo", "-l", "-g"]
    );
    assert_eq!(calls[1].cwd, project.root);
    assert_eq!(
        calls[1].args,
        vec!["-y", "skills@latest", "add", "owner/repo", "-l"]
    );
    assert!(manager.active_mutation().is_none());
    assert!(manager.interrupted_report().is_none());
}

#[tokio::test]
async fn install_preview_agrees_with_the_guarded_invocation() {
    let fixture = Fixture::new(true);
    let request = install_request(&["find-skills"], &["qoder"]);
    let preview = fixture
        .manager
        .preview_cli_commands(
            fixture.scope.clone(),
            CliCommandRequest::Install {
                request: request.clone(),
            },
        )
        .await
        .unwrap();
    assert!(fixture.executor.calls().is_empty());
    let (sink, mut receiver) = sink();
    fixture
        .manager
        .start_install_with_preview(fixture.scope.clone(), request, Some(preview.id), sink)
        .await
        .unwrap();
    assert_eq!(final_event(&mut receiver).await.phase, TaskPhase::Completed);
    assert_eq!(
        preview.entries[0].command.as_deref(),
        Some(display_command(&fixture.executor.calls()[0]).as_str())
    );
}

#[tokio::test]
async fn source_or_agent_changes_reject_before_registration_and_release_the_permit() {
    let fixture = Fixture::new(true);
    let request = install_request(&["find-skills"], &["qoder"]);
    let preview = fixture
        .manager
        .preview_cli_commands(
            fixture.scope.clone(),
            CliCommandRequest::Install {
                request: request.clone(),
            },
        )
        .await
        .unwrap();
    for changed in [
        InstallRequest {
            source: "other/skills".into(),
            ..request.clone()
        },
        InstallRequest {
            agents: Vec::new(),
            ..request
        },
    ] {
        let (sink, _) = sink();
        let error = fixture
            .manager
            .start_install_with_preview(
                fixture.scope.clone(),
                changed,
                Some(preview.id.clone()),
                sink,
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::CliCommandChanged);
        assert!(fixture.manager.lock_tasks().is_empty());
        assert!(fixture.manager.active_mutation().is_none());
        assert!(fixture.manager.interrupted_report().is_none());
        assert!(fixture.executor.calls().is_empty());
        assert!(fixture.manager.begin_registry_change().is_ok());
    }
}

struct MutableRuntime(Mutex<CliRuntime>);
#[async_trait::async_trait]
impl CliRuntimeProvider for MutableRuntime {
    async fn cli_runtime(&self) -> Result<CliRuntime, AppError> {
        Ok(self.0.lock().unwrap().clone())
    }
}

#[tokio::test]
async fn runtime_path_changes_reject_every_mutation_start_before_registration() {
    let fixture = Fixture::new(true);
    let runtime = Arc::new(MutableRuntime(Mutex::new(
        FakeRuntime::ok().result.unwrap(),
    )));
    let manager = TaskManager::with_projects(
        runtime.clone(),
        fixture.executor.clone(),
        Arc::new(FakeConflicts::default()),
        fixture.paths.clone(),
        Arc::new(ProjectRegistry::new(fixture.paths.clone())),
        TaskManagerConfig::default(),
    );
    let install = install_request(&["find-skills"], &["qoder"]);
    let requests = [
        (
            fixture.scope.clone(),
            CliCommandRequest::Install {
                request: install.clone(),
            },
        ),
        (
            ScopeId::Global,
            CliCommandRequest::Update {
                skills: vec!["find-skills".into()],
            },
        ),
        (
            ScopeId::Global,
            CliCommandRequest::Uninstall {
                skill: "find-skills".into(),
            },
        ),
    ];
    for (scope, request) in requests {
        runtime.0.lock().unwrap().npx_path = "/node/bin/npx".into();
        let preview = manager
            .preview_cli_commands(scope.clone(), request.clone())
            .await
            .unwrap();
        runtime.0.lock().unwrap().npx_path = "/new node/bin/npx".into();
        let (sink, _) = sink();
        let result = match request {
            CliCommandRequest::Install { request } => {
                manager
                    .start_install_with_preview(scope, request, Some(preview.id), sink)
                    .await
            }
            CliCommandRequest::Update { skills } => {
                manager
                    .start_update_with_preview(scope, skills, Some(preview.id), sink)
                    .await
            }
            CliCommandRequest::Uninstall { skill } => {
                manager
                    .start_uninstall_with_preview(scope, skill, Some(preview.id), sink)
                    .await
            }
            _ => unreachable!(),
        };
        assert_eq!(result.unwrap_err().code, ErrorCode::CliCommandChanged);
        assert!(manager.lock_tasks().is_empty());
        assert!(manager.active_mutation().is_none());
        assert!(fixture.executor.calls().is_empty());
    }
}

#[tokio::test]
async fn update_preview_lists_skips_without_fabricating_commands() {
    let fixture = Fixture::new(false);
    let preview = fixture
        .manager
        .preview_cli_commands(
            ScopeId::Global,
            CliCommandRequest::Update {
                skills: vec!["find-skills".into(), "missing".into()],
            },
        )
        .await
        .unwrap();
    assert_eq!(preview.entries.len(), 2);
    assert!(
        preview.entries[0]
            .command
            .as_ref()
            .unwrap()
            .contains("-s find-skills -a qoder -a universal -y --json")
    );
    assert_eq!(preview.entries[1].name, "missing");
    assert!(preview.entries[1].command.is_none());
    assert!(preview.entries[1].reason.is_some());
    let (sink, mut receiver) = sink();
    fixture
        .manager
        .start_update_with_preview(
            ScopeId::Global,
            vec!["find-skills".into(), "missing".into()],
            Some(preview.id),
            sink,
        )
        .await
        .unwrap();
    final_event(&mut receiver).await;
    let calls = fixture.executor.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        preview.entries[0].command.as_deref(),
        Some(display_command(&calls[0]).as_str())
    );
}

#[tokio::test]
async fn unmanaged_uninstall_preview_and_execution_do_not_require_the_cli() {
    let fixture = Fixture::new(false);
    write_pre_lock(&fixture.paths, &[]);
    let manager = TaskManager::with_projects(
        Arc::new(FakeRuntime::err(AppError::cli_command_changed())),
        fixture.executor.clone(),
        Arc::new(FakeConflicts::default()),
        fixture.paths.clone(),
        Arc::new(ProjectRegistry::new(fixture.paths.clone())),
        TaskManagerConfig::default(),
    );
    let preview = manager
        .preview_cli_commands(
            ScopeId::Global,
            CliCommandRequest::Uninstall {
                skill: "find-skills".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(preview.entries.len(), 1);
    assert!(preview.entries[0].command.is_none());
    let (sink, mut receiver) = sink();
    manager
        .start_uninstall_with_preview(
            ScopeId::Global,
            "find-skills".into(),
            Some(preview.id),
            sink,
        )
        .await
        .unwrap();
    assert_eq!(final_event(&mut receiver).await.phase, TaskPhase::Completed);
    assert!(fixture.executor.calls().is_empty());
}

#[tokio::test]
async fn runtime_change_after_registration_still_prevents_the_cli_spawn() {
    let fixture = Fixture::new(true);
    let runtime = Arc::new(MutableRuntime(Mutex::new(
        FakeRuntime::ok().result.unwrap(),
    )));
    let manager = TaskManager::with_projects(
        runtime.clone(),
        fixture.executor.clone(),
        Arc::new(FakeConflicts::default()),
        fixture.paths.clone(),
        Arc::new(ProjectRegistry::new(fixture.paths.clone())),
        TaskManagerConfig::default(),
    );
    let request = install_request(&["find-skills"], &["qoder"]);
    let preview = manager
        .preview_cli_commands(
            fixture.scope.clone(),
            CliCommandRequest::Install {
                request: request.clone(),
            },
        )
        .await
        .unwrap();
    let (sender, mut receiver) = unbounded_channel();
    let sink = Arc::new(move |event: TaskEvent| {
        if event.phase == TaskPhase::Preparing {
            runtime.0.lock().unwrap().npx_path = "/changed/npx".into();
        }
        sender.send(event).unwrap();
    });
    manager
        .start_install_with_preview(fixture.scope.clone(), request, Some(preview.id), sink)
        .await
        .unwrap();
    let event = final_event(&mut receiver).await;
    assert_eq!(event.phase, TaskPhase::Failed);
    assert_eq!(event.error.unwrap().code, ErrorCode::CliCommandChanged);
    assert!(fixture.executor.calls().is_empty());
    assert!(manager.active_mutation().is_none());
}

#[tokio::test]
async fn skipped_only_update_matches_its_commandless_preview_without_resolving_cli() {
    let fixture = Fixture::new(false);
    let manager = TaskManager::with_projects(
        Arc::new(FakeRuntime::err(AppError::cli_command_changed())),
        fixture.executor.clone(),
        Arc::new(FakeConflicts::default()),
        fixture.paths.clone(),
        Arc::new(ProjectRegistry::new(fixture.paths.clone())),
        TaskManagerConfig::default(),
    );
    let preview = manager
        .preview_cli_commands(
            ScopeId::Global,
            CliCommandRequest::Update {
                skills: vec!["missing".into()],
            },
        )
        .await
        .unwrap();
    assert!(preview.entries[0].command.is_none());
    let (sink, mut receiver) = sink();
    manager
        .start_update_with_preview(
            ScopeId::Global,
            vec!["missing".into()],
            Some(preview.id),
            sink,
        )
        .await
        .unwrap();
    let event = final_event(&mut receiver).await;
    assert_eq!(event.phase, TaskPhase::Completed);
    assert_eq!(
        event.result.unwrap().skills[0].status,
        TaskSkillStatus::Skipped
    );
    assert!(fixture.executor.calls().is_empty());
}

#[tokio::test]
async fn batch_rejects_changed_runtime_commands_and_preserves_the_preview_for_refresh() {
    use crate::tasks::batch::{BatchAction, BatchConfirmation, BatchRequest};
    let fixture = Fixture::new(false);
    let runtime = Arc::new(MutableRuntime(Mutex::new(
        FakeRuntime::ok().result.unwrap(),
    )));
    let manager = TaskManager::with_projects(
        runtime.clone(),
        fixture.executor.clone(),
        Arc::new(FakeConflicts::default()),
        fixture.paths.clone(),
        Arc::new(ProjectRegistry::new(fixture.paths.clone())),
        TaskManagerConfig::default(),
    );
    let request = BatchRequest {
        action: BatchAction::Uninstall,
        skill_ids: vec!["find-skills".into()],
        agent_ids: vec![],
        target_keys: None,
        project_root: None,
    };
    let preview = manager
        .preview_batch(ScopeId::Global, request, HashMap::new())
        .await
        .unwrap();
    assert!(
        preview.targets[0]
            .command
            .as_ref()
            .unwrap()
            .starts_with("cd /tmp/skills-box-cwd && /node/bin/npx")
    );
    runtime.0.lock().unwrap().cwd = "/tmp/New CLI directory".into();
    let (sink, _) = sink();
    let error = manager
        .start_batch(
            ScopeId::Global,
            BatchConfirmation {
                preview_id: preview.id.clone(),
                revision: preview.revision.clone(),
                confirmed_delete: true,
                confirmed_copies: true,
            },
            sink,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::CliCommandChanged);
    assert!(manager.lock_tasks().is_empty());
    assert!(fixture.executor.calls().is_empty());
    let fresh = manager.refresh_batch_preview(preview.id).await.unwrap();
    assert!(
        fresh.targets[0]
            .command
            .as_ref()
            .unwrap()
            .starts_with("cd '/tmp/New CLI directory' && /node/bin/npx")
    );
}
