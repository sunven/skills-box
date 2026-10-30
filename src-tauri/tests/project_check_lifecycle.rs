//! Behavior tests through the same Project check interface as the commands.
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use skills_box_lib::error::ErrorCode;
use skills_box_lib::paths::AppPaths;
use skills_box_lib::projects::ProjectRegistry;
use skills_box_lib::scope::ScopeId;
use skills_box_lib::update::archive::ArchiveClient;
use skills_box_lib::update::github::GitHubFetchError;
use skills_box_lib::update::project::{ProjectCheckConfig, ProjectUpdateService};
use skills_box_lib::update::project_checks::ProjectUpdateChecks;
use skills_box_lib::update::state::{UpdateCheckOutcome, UpdateCheckStateStore};
use skills_box_lib::update::{SystemClock, UpdateCheckResponse};
use tokio::sync::{mpsc, oneshot};

struct PausedClient(mpsc::UnboundedSender<oneshot::Sender<()>>);

#[async_trait::async_trait]
impl ArchiveClient for PausedClient {
    async fn latest_commit(&self, _: &str, _: Option<&str>) -> Result<String, GitHubFetchError> {
        let (resume, wait) = oneshot::channel();
        self.0.send(resume).unwrap();
        wait.await.unwrap();
        Err(GitHubFetchError::offline("fixture offline"))
    }
    async fn download_archive(&self, _: &str, _: &str) -> Result<Vec<u8>, GitHubFetchError> {
        unreachable!("offline fixture never downloads")
    }
}

struct Fixture {
    tmp: tempfile::TempDir,
    paths: AppPaths,
    projects: Arc<ProjectRegistry>,
    state: Arc<UpdateCheckStateStore>,
    checks: Arc<ProjectUpdateChecks>,
    requests: mpsc::UnboundedReceiver<oneshot::Sender<()>>,
    scope: ScopeId,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(tmp.path().join("home"), tmp.path().join("data"));
        fs::create_dir_all(paths.home()).unwrap();
        let projects = Arc::new(ProjectRegistry::new(paths.clone()));
        let root = tmp.path().join("project-a");
        Self::write_root(&root);
        let scope = projects
            .add(root.to_str().unwrap())
            .unwrap()
            .project
            .scope();
        let state = Arc::new(UpdateCheckStateStore::new(&paths));
        let (send, requests) = mpsc::unbounded_channel();
        let checker = ProjectUpdateService::with_dependencies(
            paths.clone(),
            Arc::new(PausedClient(send)),
            Arc::new(SystemClock),
            ProjectCheckConfig::default(),
        );
        let checks = Arc::new(ProjectUpdateChecks::with_checker(
            paths.clone(),
            projects.clone(),
            state.clone(),
            checker,
        ));
        Self {
            tmp,
            paths,
            projects,
            state,
            checks,
            requests,
            scope,
            root,
        }
    }

    fn write_root(root: &PathBuf) {
        fs::create_dir_all(root).unwrap();
        fs::write(root.join("skills-lock.json"), serde_json::json!({"version":1,"skills":{"alpha":{
            "source":"acme/skills","sourceType":"github","skillPath":"alpha/SKILL.md","computedHash":"a".repeat(64)
        }}}).to_string()).unwrap();
    }
    fn revision(&self) -> String {
        self.projects
            .lookup(self.scope.project_id().unwrap())
            .unwrap()
            .project
            .check_revision
    }
    fn start(&self, ids: Option<Vec<String>>) -> tokio::task::JoinHandle<UpdateCheckResponse> {
        let checks = self.checks.clone();
        let scope = self.scope.clone();
        let revision = self.revision();
        tokio::spawn(async move { checks.check(scope, ids, Some(revision)).await.unwrap() })
    }
    async fn paused(&mut self) -> oneshot::Sender<()> {
        tokio::time::timeout(std::time::Duration::from_secs(2), self.requests.recv())
            .await
            .unwrap()
            .unwrap()
    }
    async fn full_check(&mut self) -> UpdateCheckResponse {
        let check = self.start(None);
        self.paused().await.send(()).unwrap();
        check.await.unwrap()
    }
    fn restart(&self) -> ProjectUpdateChecks {
        ProjectUpdateChecks::new(
            self.paths.clone(),
            Arc::new(ProjectRegistry::new(self.paths.clone())),
            Arc::new(UpdateCheckStateStore::new(&self.paths)),
        )
    }
}

#[tokio::test]
async fn relocate_including_same_root_and_a_b_a_discards_old_checks() {
    for return_to_a in [false, true] {
        let mut fixture = Fixture::new();
        let old = fixture.start(None);
        let resume = fixture.paused().await;
        let revision = fixture.revision();
        let root_b = fixture.tmp.path().join("project-b");
        Fixture::write_root(&root_b);
        let target = if return_to_a { &root_b } else { &fixture.root };
        fixture
            .checks
            .relocate(
                &fixture.scope.project_id().unwrap().to_string(),
                target.to_str().unwrap(),
            )
            .unwrap();
        if return_to_a {
            fixture
                .checks
                .relocate(
                    &fixture.scope.project_id().unwrap().to_string(),
                    fixture.root.to_str().unwrap(),
                )
                .unwrap();
        }
        assert_ne!(revision, fixture.revision());
        resume.send(()).unwrap();
        assert!(matches!(
            old.await.unwrap(),
            UpdateCheckResponse::Invalidated { .. }
        ));
        assert!(fixture.checks.read_state(&fixture.scope).is_none());
        assert!(fixture.restart().read_state(&fixture.scope).is_none());
    }
}

#[tokio::test]
async fn new_revision_finishes_while_old_check_is_still_waiting() {
    let mut fixture = Fixture::new();
    let old = fixture.start(None);
    let old_resume = fixture.paused().await;
    fixture
        .checks
        .relocate(
            &fixture.scope.project_id().unwrap().to_string(),
            fixture.root.to_str().unwrap(),
        )
        .unwrap();
    assert!(matches!(
        fixture.full_check().await,
        UpdateCheckResponse::Applied { .. }
    ));
    let state = fixture.checks.read_state(&fixture.scope).unwrap();
    assert_eq!(
        state.check_revision.as_deref(),
        Some(fixture.revision().as_str())
    );
    old_resume.send(()).unwrap();
    assert!(matches!(
        old.await.unwrap(),
        UpdateCheckResponse::Invalidated { .. }
    ));
    assert_eq!(
        fixture.checks.read_state(&fixture.scope),
        Some(state.clone())
    );
    assert_eq!(fixture.restart().read_state(&fixture.scope), Some(state));
}

#[tokio::test]
async fn different_project_scopes_check_independently_and_persist_failures() {
    let mut fixture = Fixture::new();
    let first = fixture.start(None);
    let first_resume = fixture.paused().await;
    let root_b = fixture.tmp.path().join("project-b");
    Fixture::write_root(&root_b);
    let entry = fixture.projects.add(root_b.to_str().unwrap()).unwrap();
    let scope_b = entry.project.scope();
    let checks = fixture.checks.clone();
    let requested_scope = scope_b.clone();
    let second = tokio::spawn(async move {
        checks
            .check(requested_scope, None, Some(entry.project.check_revision))
            .await
            .unwrap()
    });
    fixture.paused().await.send(()).unwrap();
    assert!(matches!(
        second.await.unwrap(),
        UpdateCheckResponse::Applied { .. }
    ));
    let state_b = fixture.checks.read_state(&scope_b).unwrap();
    assert_eq!(state_b.last_outcome, UpdateCheckOutcome::Failed);
    assert_eq!(state_b.failed_source_count, 1);
    assert!(fixture.checks.read_state(&fixture.scope).is_none());
    first_resume.send(()).unwrap();
    assert!(matches!(
        first.await.unwrap(),
        UpdateCheckResponse::Applied { .. }
    ));
    assert_eq!(fixture.checks.read_state(&scope_b), Some(state_b));
    assert_eq!(fixture.restart().read_states().len(), 2);
}

#[tokio::test]
async fn forget_discards_a_waiting_check_and_rejects_late_requests_normally() {
    let mut fixture = Fixture::new();
    let revision = fixture.revision();
    let old = fixture.start(None);
    let resume = fixture.paused().await;
    fixture
        .checks
        .forget(&fixture.scope.project_id().unwrap().to_string())
        .unwrap();
    resume.send(()).unwrap();
    assert!(matches!(
        old.await.unwrap(),
        UpdateCheckResponse::Invalidated { .. }
    ));
    assert!(matches!(
        fixture
            .checks
            .check(fixture.scope.clone(), None, Some(revision))
            .await
            .unwrap(),
        UpdateCheckResponse::Invalidated { .. }
    ));
    assert!(fixture.checks.read_states().is_empty());
}

#[tokio::test]
async fn failed_cleanup_preserves_the_change_and_hides_residual_state_after_restart() {
    for forget in [false, true] {
        let mut fixture = Fixture::new();
        fixture.full_check().await;
        let path = fixture.state.path();
        let residual = fs::read(path).unwrap();
        // Force a deterministic cleanup read failure; then restore stale data
        // to model residual state becoming readable after restarting.
        fs::remove_file(path).unwrap();
        fs::create_dir(path).unwrap();
        let id = fixture.scope.project_id().unwrap().to_string();
        let warnings = if forget {
            fixture.checks.forget(&id).unwrap().warnings
        } else {
            fixture
                .checks
                .relocate(&id, fixture.root.to_str().unwrap())
                .unwrap()
                .warnings
        };
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, ErrorCode::UpdateStateCleanupFailed);
        fs::remove_dir(path).unwrap();
        fs::write(path, residual).unwrap();
        let restarted = fixture.restart();
        assert!(restarted.read_state(&fixture.scope).is_none());
        assert!(restarted.read_states().is_empty());
        assert_eq!(fixture.projects.list().projects.len(), usize::from(!forget));
    }
}

#[tokio::test]
async fn legacy_project_summaries_are_dropped_while_global_history_survives() {
    let mut fixture = Fixture::new();
    fixture.full_check().await;
    let mut legacy = fixture.checks.read_state(&fixture.scope).unwrap();
    legacy.check_revision = None;
    fixture.state.store(&fixture.scope, &legacy).unwrap();
    fixture.state.store(&ScopeId::Global, &legacy).unwrap();
    let restarted = fixture.restart();
    assert!(restarted.read_state(&fixture.scope).is_none());
    assert_eq!(restarted.read_state(&ScopeId::Global), Some(legacy));
    assert_eq!(restarted.read_states().len(), 1);
}

#[tokio::test]
async fn scope_switching_and_failed_relocate_preserve_a_check_and_single_skill_does_not_advance_state()
 {
    let mut fixture = Fixture::new();
    fixture.full_check().await;
    let state = fixture.checks.read_state(&fixture.scope).unwrap();
    let check = fixture.start(Some(vec![fixture.scope.skill_id("alpha")]));
    let resume = fixture.paused().await;
    fixture.projects.set_active_scope(&ScopeId::Global).unwrap();
    assert!(
        fixture
            .checks
            .relocate(
                &fixture.scope.project_id().unwrap().to_string(),
                "/missing-fixture-project"
            )
            .is_err()
    );
    resume.send(()).unwrap();
    assert!(matches!(
        check.await.unwrap(),
        UpdateCheckResponse::Applied { state: None, .. }
    ));
    assert_eq!(fixture.checks.read_state(&fixture.scope), Some(state));
}
