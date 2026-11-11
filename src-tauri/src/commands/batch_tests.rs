use super::*;
use crate::baseline::BaselineStore;
use crate::cli::{CliRuntime, CliRuntimeProvider};
use crate::process::fake::ScriptedExecutor;
use crate::tasks::batch::{BatchAction, BatchRequest};
use crate::update::archive::ArchiveClient;
use crate::update::github::GitHubFetchError;
use crate::update::project::{ProjectCheckConfig, ProjectUpdateService};
use crate::update::project_checks::ProjectUpdateChecks;
use crate::update::state::UpdateCheckStateStore;
use std::fs;
use std::sync::atomic::AtomicUsize;

const NAMES: [&str; 3] = ["bmad", "bmad-dev", "unselected"];
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

struct Runtime(AppPaths);

#[async_trait::async_trait]
impl CliRuntimeProvider for Runtime {
    async fn cli_runtime(&self) -> Result<CliRuntime, AppError> {
        Ok(CliRuntime {
            npx_path: "/test/node/npx".into(),
            node_dir: "/test/node".into(),
            git_dir: None,
            cwd: self.0.cli_cwd(),
            home: self.0.home().into(),
            env: BTreeMap::new(),
            pinned_version: "latest".into(),
        })
    }
}

#[derive(Default)]
struct Archive {
    checks: AtomicUsize,
}

#[async_trait::async_trait]
impl ArchiveClient for Archive {
    async fn latest_commit(
        &self,
        source: &str,
        reference: Option<&str>,
    ) -> Result<String, GitHubFetchError> {
        assert_eq!(source, "acme/skills");
        assert_eq!(reference, None);
        self.checks.fetch_add(1, Ordering::SeqCst);
        Ok(COMMIT.into())
    }

    async fn download_archive(
        &self,
        source: &str,
        commit: &str,
    ) -> Result<Vec<u8>, GitHubFetchError> {
        assert_eq!(source, "acme/skills");
        assert_eq!(commit, COMMIT);
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        for name in NAMES {
            let content = format!("---\nname: {name}\n---\nUpdated upstream content.\n");
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(
                    &mut header,
                    format!("repo/skills/{name}/SKILL.md"),
                    content.as_bytes(),
                )
                .unwrap();
        }
        Ok(builder.into_inner().unwrap().finish().unwrap())
    }
}

struct Fixture {
    _tmp: tempfile::TempDir,
    state: AppState,
    scope: ScopeId,
    root: PathBuf,
    archive: Arc<Archive>,
    executor: Arc<ScriptedExecutor>,
}

impl Fixture {
    fn new(lock_version: u32) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(tmp.path().join("home"), tmp.path().join("data"));
        paths.ensure_dirs().unwrap();
        let mut state = AppState::new(paths.clone());
        let root = tmp.path().join("project");
        fs::create_dir_all(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let scope = state
            .projects
            .add(root.to_str().unwrap())
            .unwrap()
            .project
            .scope();
        let mut entries = serde_json::Map::new();
        for name in NAMES {
            let canonical = root.join(".agents/skills").join(name);
            fs::create_dir_all(&canonical).unwrap();
            fs::write(
                canonical.join("SKILL.md"),
                format!("---\nname: {name}\n---\nLocal content.\n"),
            )
            .unwrap();
            BaselineStore::for_scope(&paths, &scope)
                .record_from_dir(name, &canonical, true)
                .unwrap();
            entries.insert(
                name.into(),
                serde_json::json!({
                    "source": "acme/skills", "sourceType": "github",
                    "skillPath": format!("skills/{name}/SKILL.md"), "computedHash": "a".repeat(64)
                }),
            );
        }
        fs::write(
            root.join("skills-lock.json"),
            serde_json::json!({"version": lock_version, "skills": entries}).to_string(),
        )
        .unwrap();
        let archive = Arc::new(Archive::default());
        state.project_updates = ProjectUpdateChecks::with_checker(
            paths.clone(),
            state.projects.clone(),
            Arc::new(UpdateCheckStateStore::new(&paths)),
            ProjectUpdateService::with_dependencies(
                paths.clone(),
                archive.clone(),
                Arc::new(crate::update::SystemClock),
                ProjectCheckConfig::default(),
            ),
        );
        let executor = Arc::new(ScriptedExecutor::new());
        state.tasks = crate::tasks::TaskManager::with_projects(
            Arc::new(Runtime(paths.clone())),
            executor.clone(),
            Arc::new(crate::tasks::InventoryConflictProvider::new(paths.clone())),
            paths,
            state.projects.clone(),
            crate::tasks::TaskManagerConfig::default(),
        );
        Self {
            _tmp: tmp,
            state,
            scope,
            root,
            archive,
            executor,
        }
    }

    fn request(&self) -> BatchRequest {
        BatchRequest {
            action: BatchAction::Update,
            skill_ids: self
                .state
                .read_model
                .snapshot(&self.scope)
                .unwrap()
                .skills
                .into_iter()
                .filter(|skill| skill.name != "unselected")
                .map(|skill| skill.id)
                .collect(),
            agent_ids: Vec::new(),
            target_keys: None,
            project_root: None,
        }
    }
}

#[tokio::test]
async fn project_batch_update_previews_selected_inventory_skills() {
    for version in [1, 2] {
        let fixture = Fixture::new(version);
        let request = fixture.request();
        assert_eq!(request.skill_ids.len(), 2);
        let lock_before = fs::read(fixture.root.join("skills-lock.json")).unwrap();
        let preview =
            preview_batch_for_state(&fixture.state, fixture.scope.clone(), request.clone())
                .await
                .expect("selected project skills should produce a batch update preview");
        assert_eq!(preview.scope, fixture.scope);
        assert_eq!(preview.targets.len(), 2);
        for (target, id) in preview.targets.iter().zip(&request.skill_ids) {
            assert_eq!(&target.skill_id, id);
            assert!(target.eligible, "{}: {:?}", target.name, target.reason);
            let command = target.command.as_ref().expect("eligible update command");
            assert!(command.contains(fixture.root.to_str().unwrap()));
            assert!(command.contains(&format!("-s {} ", target.name)));
            assert!(!command.contains(" -g "));
            assert!(!command.contains("project:"));
        }
        assert_eq!(fixture.archive.checks.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.executor.call_count(), 0);
        assert_eq!(
            fs::read(fixture.root.join("skills-lock.json")).unwrap(),
            lock_before
        );
        for name in NAMES {
            assert_eq!(
                fs::read_to_string(
                    fixture
                        .root
                        .join(".agents/skills")
                        .join(name)
                        .join("SKILL.md")
                )
                .unwrap(),
                format!("---\nname: {name}\n---\nLocal content.\n"),
            );
        }
    }
}

#[tokio::test]
async fn project_batch_update_rejects_unscoped_and_foreign_skill_ids_before_checking() {
    let fixture = Fixture::new(2);
    for invalid_id in [
        "bmad".to_string(),
        ScopeId::project(uuid::Uuid::new_v4()).skill_id("bmad"),
    ] {
        let mut request = fixture.request();
        request.skill_ids[0] = invalid_id.clone();
        let error = preview_batch_for_state(&fixture.state, fixture.scope.clone(), request)
            .await
            .unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::PathInvalid);
        assert!(error.detail.unwrap().contains(&invalid_id));
    }
    assert_eq!(fixture.archive.checks.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.executor.call_count(), 0);
}
