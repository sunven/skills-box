use super::*;
use crate::cli::{CliRuntime, CliRuntimeProvider};
use crate::tasks::{InventoryConflictProvider, TaskManagerConfig};
use std::fs;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

struct UnusedRuntime;
#[async_trait::async_trait]
impl CliRuntimeProvider for UnusedRuntime {
    async fn cli_runtime(&self) -> Result<CliRuntime, AppError> {
        Err(AppError::env_node_missing("test"))
    }
}
struct FakeTransport {
    commit: AtomicU64,
    names: Mutex<Vec<String>>,
    versions: Mutex<Vec<String>>,
    previews: AtomicUsize,
    active: AtomicUsize,
    maximum: AtomicUsize,
    preview_error: Mutex<Option<AppError>>,
    version_error: Mutex<Option<GitHubFetchError>>,
    gate: Mutex<Option<Arc<tokio::sync::Semaphore>>>,
    changed: Mutex<Option<String>>,
}
impl Default for FakeTransport {
    fn default() -> Self {
        Self {
            commit: AtomicU64::new(1),
            names: Mutex::new(vec!["alpha".into()]),
            versions: Mutex::new(vec![]),
            previews: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            maximum: AtomicUsize::new(0),
            preview_error: Mutex::new(None),
            version_error: Mutex::new(None),
            gate: Mutex::new(None),
            changed: Mutex::new(None),
        }
    }
}
struct Active<'a>(&'a AtomicUsize);
impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
#[async_trait::async_trait]
impl DiscoveryTransport for FakeTransport {
    async fn version(
        &self,
        source: &str,
        _cancel: CancelToken,
    ) -> Result<String, GitHubFetchError> {
        self.versions.lock().unwrap().push(source.to_string());
        let n = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.maximum.fetch_max(n, Ordering::SeqCst);
        let _active = Active(&self.active);
        tokio::time::sleep(Duration::from_millis(2)).await;
        if is_github(source)
            && let Some(error) = self.version_error.lock().unwrap().clone()
        {
            return Err(error);
        }
        Ok(format!("{:040x}", self.commit.load(Ordering::SeqCst)))
    }
    async fn preview(
        &self,
        _source: &str,
        cancel: CancelToken,
    ) -> Result<Vec<ParsedSkill>, AppError> {
        self.previews.fetch_add(1, Ordering::SeqCst);
        let gate = self.gate.lock().unwrap().clone();
        if let Some(gate) = gate {
            tokio::select! {
                permit = gate.acquire() => { permit.unwrap().forget(); },
                _ = cancel.cancelled() => return Err(AppError::task_cancelled()),
            }
        }
        if let Some(error) = self.preview_error.lock().unwrap().clone() {
            return Err(error);
        }
        if let Some(next) = self.changed.lock().unwrap().take() {
            self.commit.store(next.parse().unwrap(), Ordering::SeqCst);
        }
        Ok(self
            .names
            .lock()
            .unwrap()
            .iter()
            .map(|name| ParsedSkill {
                name: name.clone(),
                description: "description".into(),
                group: None,
            })
            .collect())
    }
}
struct Fixture {
    _tmp: tempfile::TempDir,
    paths: AppPaths,
    projects: Arc<ProjectRegistry>,
    service: DiscoveryService,
    transport: Arc<FakeTransport>,
    now: Arc<AtomicU64>,
    settings: Arc<crate::settings::SettingsService>,
}
impl Fixture {
    fn new(sources: &[&str]) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(tmp.path().join("home"), tmp.path().join("app"));
        fs::create_dir_all(paths.home()).unwrap();
        paths.ensure_dirs().unwrap();
        write_global(&paths, sources);
        let now = Arc::new(AtomicU64::new(1_000_000));
        let tick = now.clone();
        let projects = Arc::new(ProjectRegistry::new(paths.clone()));
        let read_model = Arc::new(ReadModel::with_projects(paths.clone(), projects.clone()));
        let tasks = TaskManager::with_projects(
            Arc::new(UnusedRuntime),
            Arc::new(crate::process::tokio_executor::TokioExecutor),
            Arc::new(InventoryConflictProvider::new(paths.clone())),
            paths.clone(),
            projects.clone(),
            TaskManagerConfig::default(),
        );
        let settings = Arc::new(crate::settings::SettingsService::new(paths.clone(), None));
        let api = GitHubApiClient::new(settings.clone());
        let transport = Arc::new(FakeTransport::default());
        let service = DiscoveryService::with_transport(
            paths.clone(),
            projects.clone(),
            read_model,
            tasks,
            api,
            transport.clone(),
            Arc::new(move || tick.load(Ordering::SeqCst)),
        );
        Self {
            _tmp: tmp,
            paths,
            projects,
            service,
            transport,
            now,
            settings,
        }
    }
    fn names(&self, names: &[&str], commit: u64) {
        *self.transport.names.lock().unwrap() = names.iter().map(|s| s.to_string()).collect();
        self.transport.commit.store(commit, Ordering::SeqCst);
    }
    async fn scan(&self) {
        self.service.start(true).unwrap();
        finish(&self.service).await;
    }
    fn id(&self, name: &str) -> DiscoveryId {
        DiscoveryId {
            source: "acme/skills".into(),
            name: name.into(),
        }
    }
}
fn write_global(paths: &AppPaths, sources: &[&str]) {
    let skills: serde_json::Map<String,serde_json::Value> = sources.iter().enumerate().map(|(i, source)| {
        let github = !source.starts_with("https://");
        (format!("installed-{i}"), serde_json::json!({ "source": source, "sourceType": if github {"github"} else {"git"},
            "sourceUrl": if github {format!("https://github.com/{source}")} else {source.to_string()},
            "skillFolderHash": "", "installedAt": "2026-11-02T00:00:00Z", "updatedAt": "2026-11-02T00:00:00Z", "ref": "pinned-branch" }))
    }).collect();
    fs::create_dir_all(paths.lock_path().parent().unwrap()).unwrap();
    fs::write(
        paths.lock_path(),
        serde_json::json!({"version":3, "skills":skills}).to_string(),
    )
    .unwrap();
}
fn write_project(root: &std::path::Path, source: &str) {
    fs::create_dir_all(root).unwrap();
    fs::write(
        root.join("skills-lock.json"),
        serde_json::json!({"version":1,"skills":{"installed":{"source": source,
        "sourceType":"github", "computedHash":"hash"}}})
        .to_string(),
    )
    .unwrap();
}
async fn finish(service: &DiscoveryService) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while service.lock().progress.running {
            tokio::time::sleep(Duration::from_millis(3)).await;
        }
    })
    .await
    .unwrap();
}
async fn previews(transport: &FakeTransport, count: usize) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while transport.previews.load(Ordering::SeqCst) < count {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn polling_keeps_installation_identity_fresh_without_computing_content_baselines() {
    let f = Fixture::new(&["acme/skills"]);
    let canonical = f.paths.canonical_skills_dir().join("installed-0");
    fs::create_dir_all(&canonical).unwrap();
    let markdown = "---\nname: Installed 0\ndescription: Test\n---\n";
    fs::write(canonical.join("SKILL.md"), markdown).unwrap();
    fs::write(
        canonical.join("large-reference.txt"),
        vec![b'a'; 1024 * 1024],
    )
    .unwrap();
    f.names(&["Installed 0"], 1);
    f.scan().await;
    let baselines = crate::baseline::BaselineStore::new(&f.paths);
    assert!(
        baselines.load("installed-0").is_none(),
        "source scanning must not fingerprint installed content"
    );

    for _ in 0..3 {
        assert!(!f.service.start(false).unwrap().running);
        let state = f.service.get_state();
        assert_eq!(
            state.skills[0].installations[0].status,
            InstallationStatus::Installed
        );
        assert!(
            baselines.load("installed-0").is_none(),
            "polling must not create content baselines"
        );
    }

    // SKILL.md edits and newly detected agent copies must remain visible on
    // the next poll, even though content fingerprints are no longer needed.
    fs::write(canonical.join("SKILL.md"), "---\nname: Different\n---\n").unwrap();
    assert_eq!(
        f.service.get_state().skills[0].installations[0].status,
        InstallationStatus::Conflict
    );
    fs::remove_dir_all(&canonical).unwrap();
    assert_eq!(
        f.service.get_state().skills[0].installations[0].status,
        InstallationStatus::Stale
    );
    let copy = f.paths.home().join(".claude/skills/installed-0");
    fs::create_dir_all(&copy).unwrap();
    fs::write(copy.join("SKILL.md"), markdown).unwrap();
    assert_eq!(
        f.service.get_state().skills[0].installations[0].status,
        InstallationStatus::Installed
    );
    assert!(baselines.load("installed-0").is_none());
}

#[tokio::test]
async fn first_baseline_cache_manual_check_failure_revalidation_and_restart() {
    let f = Fixture::new(&["acme/skills"]);
    f.scan().await;
    assert_eq!(f.service.get_state().unread_count, 0);
    assert_eq!(f.transport.previews.load(Ordering::SeqCst), 1);
    f.scan().await;
    assert_eq!(f.transport.previews.load(Ordering::SeqCst), 1);
    assert_eq!(f.transport.versions.lock().unwrap().len(), 3);
    f.names(&["alpha", "beta"], 2);
    f.scan().await;
    assert_eq!(f.service.get_state().unread_count, 1);
    f.service.invalidate("acme/skills".into()).unwrap();
    f.scan().await;
    assert_eq!(f.transport.previews.load(Ordering::SeqCst), 3);
    let success = f.service.get_state().sources[0].last_success_at;
    *f.transport.preview_error.lock().unwrap() = Some(AppError::env_node_missing("test"));
    f.names(&[], 3);
    f.now.fetch_add(1, Ordering::SeqCst);
    f.scan().await;
    let state = f.service.get_state();
    assert_eq!(state.skills.len(), 2);
    assert_eq!(state.unread_count, 1);
    assert!(state.sources[0].error.is_some());
    assert_eq!(state.sources[0].last_success_at, success);
    let reloaded = DiscoveryService::with_transport(
        f.paths.clone(),
        f.projects.clone(),
        f.service.inner.read_model.clone(),
        f.service.inner.tasks.clone(),
        f.service.inner.api.clone(),
        f.transport.clone(),
        f.service.inner.clock.clone(),
    );
    assert_eq!(reloaded.get_state().unread_count, 1);
    assert!(!reloaded.start(false).unwrap().running);
}
#[tokio::test]
async fn failed_initial_scan_is_not_an_empty_baseline_and_successful_empty_is() {
    let f = Fixture::new(&["acme/skills"]);
    *f.transport.preview_error.lock().unwrap() =
        Some(AppError::source_preview_failed("unparseable"));
    f.scan().await;
    assert!(!f.service.lock().history.as_ref().unwrap().sources["acme/skills"].baseline);
    *f.transport.preview_error.lock().unwrap() = None;
    f.names(&[], 2);
    f.scan().await;
    f.names(&["new"], 3);
    f.scan().await;
    assert_eq!(f.service.get_state().unread_count, 1);
}
#[tokio::test]
async fn forty_five_sources_complete_fair_batches_with_four_concurrent_and_single_flight() {
    let names = (0..45)
        .map(|n| format!("acme/repo-{n}"))
        .collect::<Vec<_>>();
    let f = Fixture::new(&names.iter().map(|s| s.as_str()).collect::<Vec<_>>());
    let run = f.service.start(true).unwrap();
    assert_eq!(f.service.start(false).unwrap().run_id, run.run_id);
    assert_eq!(f.service.start(true).unwrap().run_id, run.run_id);
    finish(&f.service).await;
    assert_eq!(f.service.get_state().sources.len(), 45);
    assert_eq!(f.transport.previews.load(Ordering::SeqCst), 45);
    assert_eq!(f.service.lock().progress.completed, 45);
    assert!(f.transport.maximum.load(Ordering::SeqCst) <= 4);
    assert!(!f.service.start(false).unwrap().running);
}
#[tokio::test]
async fn concurrent_publication_preserves_user_actions_and_all_read_does_not_ack_later_items() {
    let f = Fixture::new(&["acme/skills"]);
    f.scan().await;
    f.names(&["alpha", "beta"], 2);
    f.scan().await;
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    *f.transport.gate.lock().unwrap() = Some(gate.clone());
    f.names(&["ALPHA", "beta", "gamma"], 3);
    f.service.start(true).unwrap();
    previews(&f.transport, 3).await;
    f.service.acknowledge(None).unwrap();
    f.service.set_ignored(f.id("alpha"), true).unwrap();
    gate.add_permits(1);
    finish(&f.service).await;
    let state = f.service.get_state();
    assert_eq!(state.unread_count, 1);
    assert!(
        state
            .skills
            .iter()
            .find(|s| s.name == "ALPHA")
            .unwrap()
            .ignored
    );
    assert!(
        !state
            .skills
            .iter()
            .find(|s| s.name == "beta")
            .unwrap()
            .unread
    );
    assert!(
        state
            .skills
            .iter()
            .find(|s| s.name == "gamma")
            .unwrap()
            .unread
    );
    f.service.set_ignored(f.id("alpha"), false).unwrap();
    assert_eq!(f.service.get_state().unread_count, 1);
}
#[tokio::test]
async fn inactive_sources_hide_keep_history_and_reactivation_invalidates_late_results() {
    let f = Fixture::new(&["acme/skills"]);
    f.scan().await;
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    *f.transport.gate.lock().unwrap() = Some(gate.clone());
    f.names(&["alpha", "late"], 2);
    f.service.start(true).unwrap();
    previews(&f.transport, 2).await;
    write_global(&f.paths, &[]);
    assert!(f.service.get_state().skills.is_empty());
    write_global(&f.paths, &["acme/skills"]);
    f.service.refresh();
    gate.add_permits(1);
    finish(&f.service).await;
    assert_eq!(f.service.get_state().skills.len(), 1);
    assert_eq!(f.service.get_state().unread_count, 0);
    *f.transport.gate.lock().unwrap() = None;
    assert!(f.service.start(false).unwrap().running);
    finish(&f.service).await;
    assert_eq!(f.service.get_state().unread_count, 1);
    f.service.acknowledge(Some(vec![f.id("late")])).unwrap();
    write_global(&f.paths, &[]);
    f.service.refresh();
    write_global(&f.paths, &["acme/skills"]);
    f.scan().await;
    assert_eq!(f.service.get_state().unread_count, 0);
}
#[tokio::test]
async fn default_branch_moved_during_preview_retains_previous_catalog() {
    let f = Fixture::new(&["acme/skills"]);
    f.scan().await;
    f.names(&["wrong"], 2);
    *f.transport.changed.lock().unwrap() = Some("3".into());
    f.scan().await;
    assert_eq!(f.service.get_state().skills[0].name, "alpha");
    assert!(
        f.service.get_state().sources[0]
            .error
            .as_ref()
            .unwrap()
            .contains("默认分支发生变化")
    );
}
#[tokio::test]
async fn project_snapshot_is_revision_bound_unreadable_retains_and_forget_revokes_even_on_write_failure()
 {
    let f = Fixture::new(&["ACME/Skills"]);
    let root = f.paths.app_data().join("project-a");
    write_project(&root, "acme/skills");
    let project = f.projects.add(root.to_str().unwrap()).unwrap();
    f.scan().await;
    assert_eq!(f.transport.previews.load(Ordering::SeqCst), 1);
    write_global(&f.paths, &[]);
    fs::write(root.join("skills-lock.json"), "broken").unwrap();
    let state = f.service.get_state();
    assert_eq!(state.sources.len(), 1);
    assert!(!state.warnings.is_empty());
    assert_eq!(
        state.skills[0]
            .installations
            .iter()
            .find(|s| s.scope == project.project.scope())
            .unwrap()
            .status,
        InstallationStatus::Unknown
    );
    let b = f.paths.app_data().join("project-b");
    fs::create_dir_all(&b).unwrap();
    f.projects
        .relocate(&project.project.id.to_string(), b.to_str().unwrap())
        .unwrap();
    assert!(f.service.get_state().sources.is_empty());
    f.projects
        .relocate(&project.project.id.to_string(), root.to_str().unwrap())
        .unwrap();
    assert!(f.service.get_state().sources.is_empty()); // old revision cannot borrow A's snapshot
    write_project(&root, "acme/skills");
    f.scan().await;
    fs::remove_file(f.paths.app_data().join("discovery.json")).unwrap();
    fs::create_dir(f.paths.app_data().join("discovery.json")).unwrap();
    f.projects.forget(&project.project.id.to_string()).unwrap();
    assert!(f.service.get_state().sources.is_empty());
    assert!(f.service.get_state().error.is_some());
}
#[tokio::test]
async fn failed_user_state_write_does_not_acknowledge_or_ignore() {
    let f = Fixture::new(&["acme/skills"]);
    f.scan().await;
    f.names(&["alpha", "beta"], 2);
    f.scan().await;
    fs::remove_file(f.paths.app_data().join("discovery.json")).unwrap();
    fs::create_dir(f.paths.app_data().join("discovery.json")).unwrap();
    assert!(f.service.acknowledge(None).is_err());
    assert!(f.service.set_ignored(f.id("beta"), true).is_err());
    assert_eq!(f.service.get_state().unread_count, 1);
    assert!(!f.service.get_state().skills.iter().any(|s| s.ignored));
}
#[tokio::test]
async fn rate_limit_pauses_only_github_manual_respects_reset_new_credentials_escape_old_cooldown() {
    let names = (0..25)
        .map(|n| format!("acme/repo-{n}"))
        .collect::<Vec<_>>();
    let mut sources = names.iter().map(|s| s.as_str()).collect::<Vec<_>>();
    sources.push("https://cnb.cool/team/skills.git");
    let f = Fixture::new(&sources);
    let context = f.service.inner.api.context().unwrap();
    *f.transport.version_error.lock().unwrap() = Some(GitHubFetchError::RateLimited {
        detail: "limit".into(),
        reset_at: Some(2_000_000),
        context: Some(context),
        revision: Some(0),
    });
    f.scan().await;
    let count = f.transport.versions.lock().unwrap().len();
    assert!(count <= 6);
    assert_eq!(f.service.get_state().skills.len(), 1); // generic Git still completes
    assert_eq!(
        f.service
            .get_state()
            .sources
            .iter()
            .filter(|s| s.paused_until.is_some())
            .count(),
        25
    );
    f.scan().await;
    assert_eq!(f.transport.versions.lock().unwrap().len(), count + 1); // only unchanged generic source
    *f.transport.version_error.lock().unwrap() = None;
    f.settings.save("dummy-test-token").unwrap();
    f.scan().await;
    assert_eq!(f.service.get_state().skills.len(), 26);
}
#[tokio::test]
async fn shutdown_cancels_preview_and_never_publishes_it() {
    let f = Fixture::new(&["acme/skills"]);
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    *f.transport.gate.lock().unwrap() = Some(gate);
    f.service.start(true).unwrap();
    previews(&f.transport, 1).await;
    f.service.shutdown();
    finish(&f.service).await;
    assert!(f.service.get_state().skills.is_empty());
    assert!(f.service.start(true).is_err());
}

#[tokio::test]
async fn stale_credentials_do_not_set_cooldown_or_clear_a_new_one_after_a_b_a() {
    let f = Fixture::new(&["acme/skills"]);
    f.settings.save("dummy-a").unwrap();
    let context = f.service.inner.api.context().unwrap();
    let revision = f.service.inner.api.revision();
    f.settings.save("dummy-b").unwrap();
    f.settings.save("dummy-a").unwrap();
    *f.transport.version_error.lock().unwrap() = Some(GitHubFetchError::RateLimited {
        detail: "old request".into(),
        reset_at: Some(2_000_000),
        context: Some(context.clone()),
        revision: Some(revision),
    });
    f.scan().await;
    assert!(
        f.service
            .lock()
            .history
            .as_ref()
            .unwrap()
            .cooldown
            .is_none()
    );
    f.service
        .edit(|h, _| {
            h.cooldown = Some(Cooldown {
                context: context.clone(),
                revision: f.service.inner.api.revision(),
                until: 2_000_000,
            });
        })
        .unwrap();
    f.service.auth_succeeded(&context, revision, 5000);
    assert!(
        f.service
            .lock()
            .history
            .as_ref()
            .unwrap()
            .cooldown
            .is_some()
    );
    f.service
        .auth_succeeded(&context, f.service.inner.api.revision(), 5000);
    assert!(
        f.service
            .lock()
            .history
            .as_ref()
            .unwrap()
            .cooldown
            .is_none()
    );
}
#[tokio::test]
async fn projection_uses_lock_and_physical_presence_for_managed_conflict_unmanaged_and_stale() {
    let f = Fixture::new(&["acme/skills"]);
    fs::create_dir_all(f.paths.canonical_skills_dir().join("installed-0")).unwrap();
    fs::write(
        f.paths.canonical_skills_dir().join("installed-0/SKILL.md"),
        "---\nname: installed-0\ndescription: Installed\n---\nbody",
    )
    .unwrap();
    fs::create_dir_all(f.paths.canonical_skills_dir().join("unmanaged")).unwrap();
    fs::write(
        f.paths.canonical_skills_dir().join("unmanaged/SKILL.md"),
        "---\nname: unmanaged\ndescription: Unmanaged\n---\nbody",
    )
    .unwrap();
    f.names(&["installed-0", "unmanaged", "available"], 1);
    f.scan().await;
    let state = f.service.get_state();
    let status = |name: &str| {
        state
            .skills
            .iter()
            .find(|s| s.name == name)
            .unwrap()
            .installations[0]
            .status
    };
    assert_eq!(status("installed-0"), InstallationStatus::Installed);
    assert_eq!(status("unmanaged"), InstallationStatus::Conflict);
    assert_eq!(status("available"), InstallationStatus::NotInstalled);
    fs::remove_dir_all(f.paths.canonical_skills_dir().join("installed-0")).unwrap();
    assert_eq!(
        f.service
            .get_state()
            .skills
            .iter()
            .find(|s| s.name == "installed-0")
            .unwrap()
            .installations[0]
            .status,
        InstallationStatus::Stale
    );
    // Same name from a different source is a distinct candidate and a conflict.
    write_global(&f.paths, &["other/repo"]);
    fs::create_dir_all(f.paths.canonical_skills_dir().join("installed-0")).unwrap();
    fs::write(
        f.paths.canonical_skills_dir().join("installed-0/SKILL.md"),
        "---\nname: installed-0\ndescription: Installed\n---\nbody",
    )
    .unwrap();
    f.service.refresh();
    let identities = f
        .service
        .inner
        .read_model
        .skill_identities(&ScopeId::Global)
        .unwrap();
    assert_eq!(
        installation(
            &f.service.lock().projections[0],
            Some(&identities),
            "acme/skills",
            "installed-0"
        ),
        InstallationStatus::Conflict
    );
}

struct TestRuntime(CliRuntime);
#[async_trait::async_trait]
impl CliRuntimeProvider for TestRuntime {
    async fn cli_runtime(&self) -> Result<CliRuntime, AppError> {
        Ok(self.0.clone())
    }
}
#[tokio::test]
async fn native_catalog_uses_actual_read_only_preview_strips_credentials_and_rejects_nonzero_or_unparseable()
 {
    use crate::process::fake::{ScriptedExecutor, ScriptedResponse};
    let f = Fixture::new(&["acme/skills"]);
    let runtime = Arc::new(TestRuntime(CliRuntime {
        npx_path: "/test/npx".into(),
        node_dir: "/test".into(),
        git_dir: None,
        cwd: f.paths.cli_cwd(),
        home: f.paths.home().to_path_buf(),
        pinned_version: "latest".into(),
        env: BTreeMap::from([
            ("GITHUB_TOKEN".into(), "dummy-secret".into()),
            ("GH_TOKEN".into(), "dummy-secret".into()),
        ]),
    }));
    let cases = [
        (
            ScriptedResponse::ok(include_str!("../../tests/fixtures/cli/single-skill-ci.txt")),
            Some(1),
        ),
        (
            ScriptedResponse::ok(include_str!("../../tests/fixtures/cli/no-skills.txt")),
            Some(0),
        ),
        (
            ScriptedResponse::ok("◇  Available Skills\n│ unrecognized output"),
            None,
        ),
        (
            ScriptedResponse::Output {
                code: 1,
                stdout: include_str!("../../tests/fixtures/cli/single-skill-ci.txt").into(),
                stderr: "error".into(),
            },
            None,
        ),
    ];
    let lock = fs::read(f.paths.lock_path()).unwrap();
    for (response, count) in cases {
        let expected_status = match &response {
            ScriptedResponse::Output { code: 0, .. } => "succeeded",
            _ => "failed",
        };
        let executor = Arc::new(ScriptedExecutor::new().expect("npx", &["add", "-l"], response));
        let commands = Arc::new(crate::cli::commands::CliCommandStore::default());
        let native = NativeTransport {
            versions: SourceArchiveClient::with_api(f.paths.clone(), f.service.inner.api.clone()),
            paths: f.paths.clone(),
            runtime: runtime.clone(),
            executor: Arc::new(crate::cli::commands::ObservedExecutor {
                executor: executor.clone(),
                commands: commands.clone(),
            }),
        };
        let result = native.preview("acme/skills", CancelToken::new()).await;
        assert_eq!(result.ok().map(|s| s.len()), count);
        let calls = executor.calls();
        let spec = &calls[0];
        assert_eq!(
            spec.args,
            ["-y", "skills@latest", "add", "acme/skills", "-l", "-g"]
        );
        assert!(!spec.env.contains_key("GITHUB_TOKEN"));
        assert!(!spec.env.contains_key("GH_TOKEN"));
        let records = commands.records();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].command,
            crate::cli::commands::display_command(spec)
        );
        assert_eq!(records[0].context.origin, "discovery");
        assert_eq!(records[0].context.source.as_deref(), Some("acme/skills"));
        assert_eq!(records[0].status, expected_status);
        assert_eq!(fs::read(f.paths.lock_path()).unwrap(), lock);
    }
}

#[tokio::test]
async fn metadata_names_project_onto_sanitized_lock_keys_and_partial_installs_only_ack_success() {
    let f = Fixture::new(&["acme/skills"]);
    let dir = f.paths.canonical_skills_dir().join("installed-0");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        "---\nname: Installed 0\ndescription: Test\n---\nbody",
    )
    .unwrap();
    f.names(&["Installed 0"], 1);
    f.scan().await;
    assert_eq!(
        f.service.get_state().skills[0].installations[0].status,
        InstallationStatus::Installed
    );
    f.names(&["Installed 0", "success", "failure"], 2);
    f.scan().await;
    assert_eq!(f.service.get_state().unread_count, 2);
    let results = vec![
        crate::tasks::TaskSkillResult {
            name: "success".into(),
            status: crate::tasks::TaskSkillStatus::Installed,
            error: None,
            backup_path: None,
        },
        crate::tasks::TaskSkillResult {
            name: "failure".into(),
            status: crate::tasks::TaskSkillStatus::Failed,
            error: None,
            backup_path: None,
        },
    ];
    f.service
        .acknowledge_installed("acme/skills", &results)
        .unwrap();
    let state = f.service.get_state();
    assert_eq!(state.unread_count, 1);
    assert!(
        state
            .skills
            .iter()
            .find(|s| s.name == "failure")
            .unwrap()
            .unread
    );
}

#[tokio::test]
async fn an_elapsed_rate_limit_reset_does_not_block_manual_retry() {
    let f = Fixture::new(&["acme/skills"]);
    *f.transport.version_error.lock().unwrap() = Some(GitHubFetchError::RateLimited {
        detail: "limit".into(),
        reset_at: Some(900_000),
        context: Some(f.service.inner.api.context().unwrap()),
        revision: Some(0),
    });
    f.scan().await;
    assert!(f.service.get_state().sources[0].paused_until.is_none());
    *f.transport.version_error.lock().unwrap() = None;
    f.scan().await;
    assert_eq!(f.service.get_state().skills.len(), 1);
}
#[tokio::test]
async fn project_a_b_a_and_forgotten_association_cannot_publish_an_old_request() {
    let f = Fixture::new(&[]);
    let root = f.paths.app_data().join("project-a");
    write_project(&root, "acme/skills");
    let project = f.projects.add(root.to_str().unwrap()).unwrap();
    f.scan().await;
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    *f.transport.gate.lock().unwrap() = Some(gate.clone());
    f.names(&["alpha", "late"], 2);
    f.service.start(true).unwrap();
    previews(&f.transport, 2).await;
    let b = f.paths.app_data().join("project-b");
    fs::create_dir_all(&b).unwrap();
    f.projects
        .relocate(&project.project.id.to_string(), b.to_str().unwrap())
        .unwrap();
    f.service.refresh();
    f.projects
        .relocate(&project.project.id.to_string(), root.to_str().unwrap())
        .unwrap();
    f.service.refresh();
    gate.add_permits(1);
    finish(&f.service).await;
    assert_eq!(f.service.get_state().skills.len(), 1);
    *f.transport.gate.lock().unwrap() = None;
    assert!(f.service.start(false).unwrap().running);
    finish(&f.service).await;
    assert_eq!(f.service.get_state().unread_count, 1);
    *f.transport.gate.lock().unwrap() = Some(gate.clone());
    f.names(&["alpha", "late", "forgotten"], 3);
    f.service.start(true).unwrap();
    previews(&f.transport, 4).await;
    f.projects.forget(&project.project.id.to_string()).unwrap();
    f.service.refresh();
    gate.add_permits(1);
    finish(&f.service).await;
    assert!(f.service.get_state().skills.is_empty());
    assert!(
        !f.service.lock().history.as_ref().unwrap().sources["acme/skills"]
            .history
            .contains_key("forgotten")
    );
}
