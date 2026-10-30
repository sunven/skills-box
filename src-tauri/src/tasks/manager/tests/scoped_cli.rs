use super::*;
use crate::baseline::Baseline;
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
            "skills@1.7.4",
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
    }
    let calls = executor.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].cwd, paths.cli_cwd());
    assert_eq!(
        calls[0].args,
        vec!["-y", "skills@1.7.4", "add", "owner/repo", "-l", "-g"]
    );
    assert_eq!(calls[1].cwd, project.root);
    assert_eq!(
        calls[1].args,
        vec!["-y", "skills@1.7.4", "add", "owner/repo", "-l"]
    );
    assert!(manager.active_mutation().is_none());
    assert!(manager.interrupted_report().is_none());
}
