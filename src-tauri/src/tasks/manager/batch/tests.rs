use super::*;
use crate::baseline::BaselineStore;
use crate::cli::{CliRuntime, CliRuntimeProvider};
use crate::inventory::UpdateState;
use crate::paths::AppPaths;
use crate::process::{CommandSpec, ExecError, ExecOutput, ProcessExecutor};
use crate::tasks::{InventoryConflictProvider, TaskManagerConfig};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::{Notify, mpsc};

struct Runtime {
    paths: AppPaths,
}
#[async_trait::async_trait]
impl CliRuntimeProvider for Runtime {
    async fn cli_runtime(&self) -> Result<CliRuntime, AppError> {
        Ok(CliRuntime {
            npx_path: PathBuf::from("/test/node/npx"),
            node_dir: PathBuf::from("/test/node"),
            git_dir: None,
            cwd: self.paths.cli_cwd(),
            home: self.paths.home().into(),
            env: BTreeMap::new(),
            pinned_version: "1.7.4".into(),
        })
    }
}

struct Executor {
    paths: AppPaths,
    calls: Mutex<Vec<CommandSpec>>,
    failures: Mutex<HashSet<String>>,
    pause: Mutex<Option<String>>,
    entered: Notify,
    release: Notify,
}
#[async_trait::async_trait]
impl ProcessExecutor for Executor {
    async fn run(&self, spec: CommandSpec, cancel: CancelToken) -> Result<ExecOutput, ExecError> {
        let name = spec.args.windows(2).find(|a| a[0] == "-s").unwrap()[1].clone();
        self.calls.lock().unwrap().push(spec.clone());
        let pause = self.pause.lock().unwrap().as_deref() == Some(&name);
        if pause {
            self.entered.notify_one();
            self.release.notified().await;
        }
        assert!(
            !cancel.is_cancelled(),
            "batch stop must not cancel the active writer"
        );
        let failed = self.failures.lock().unwrap().contains(&name);
        if !failed {
            let global = spec.args.iter().any(|a| a == "-g");
            let canonical = if global {
                self.paths.canonical_skills_dir()
            } else {
                spec.cwd.join(".agents/skills")
            };
            let lock_path = if global {
                self.paths.lock_path()
            } else {
                spec.cwd.join("skills-lock.json")
            };
            let mut lock: serde_json::Value =
                serde_json::from_slice(&fs::read(&lock_path).unwrap()).unwrap();
            if spec.args.iter().any(|a| a == "remove") {
                let canonical_path = canonical.join(&name);
                if canonical_path.exists() {
                    fs::remove_dir_all(canonical_path).unwrap();
                }
                if global {
                    for dir in [".claude/skills", ".gemini/skills", ".workbuddy/skills"] {
                        let path = self.paths.home().join(dir).join(&name);
                        if let Ok(metadata) = fs::symlink_metadata(&path) {
                            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                                fs::remove_dir_all(path).unwrap();
                            } else {
                                fs::remove_file(path).unwrap();
                            }
                        }
                    }
                }
                lock["skills"].as_object_mut().unwrap().remove(&name);
            } else {
                fs::write(
                    canonical.join(&name).join("SKILL.md"),
                    format!("# {name} updated\n"),
                )
                .unwrap();
                lock["skills"][&name][if global {
                    "skillFolderHash"
                } else {
                    "computedHash"
                }] = "b".repeat(40).into();
            }
            fs::write(lock_path, lock.to_string()).unwrap();
        }
        Ok(ExecOutput {
            code: Some(if failed { 1 } else { 0 }),
            signal: None,
            stdout: if spec.args.iter().any(|a| a == "add") {
                serde_json::json!([{"name":name,"status":"installed","source":"vercel-labs/skills","ref":null,"hash":"b".repeat(40),"path":self.paths.canonical_skills_dir().join(&name),"scope":"global","agents":["Universal"],"mode":"symlink","security":null}]).to_string()
            } else {
                String::new()
            },
            stderr: if failed {
                "one target failed".into()
            } else {
                String::new()
            },
            duration: Duration::ZERO,
        })
    }
}

struct Fixture {
    _tmp: tempfile::TempDir,
    paths: AppPaths,
    manager: TaskManager,
    executor: Arc<Executor>,
}
impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(tmp.path().join("home"), tmp.path().join("data"));
        paths.ensure_dirs().unwrap();
        for dir in [".claude/skills", ".workbuddy/skills"] {
            fs::create_dir_all(paths.home().join(dir)).unwrap();
        }
        let mut entries = serde_json::Map::new();
        for name in ["alpha", "beta", "gamma"] {
            let canonical = paths.canonical_skills_dir().join(name);
            fs::create_dir_all(&canonical).unwrap();
            fs::write(canonical.join("SKILL.md"), format!("# {name}\n")).unwrap();
            BaselineStore::new(&paths)
                .record_from_dir(name, &canonical, true)
                .unwrap();
            entries.insert(name.into(), serde_json::json!({
                "source": "vercel-labs/skills", "sourceType": "github",
                "sourceUrl": "https://github.com/vercel-labs/skills.git",
                "skillPath": format!("skills/{name}/SKILL.md"), "skillFolderHash": "a".repeat(40),
                "installedAt": "2026-01-01T00:00:00.000Z", "updatedAt": "2026-01-01T00:00:00.000Z"
            }));
        }
        fs::write(
            paths.lock_path(),
            serde_json::json!({"version":3,"skills":entries}).to_string(),
        )
        .unwrap();
        let executor = Arc::new(Executor {
            paths: paths.clone(),
            calls: Mutex::new(Vec::new()),
            failures: Mutex::new(HashSet::new()),
            pause: Mutex::new(None),
            entered: Notify::new(),
            release: Notify::new(),
        });
        let manager = TaskManager::new(
            Arc::new(Runtime {
                paths: paths.clone(),
            }),
            executor.clone(),
            Arc::new(InventoryConflictProvider::new(paths.clone())),
            paths.clone(),
            TaskManagerConfig::default(),
        );
        Self {
            _tmp: tmp,
            paths,
            manager,
            executor,
        }
    }
    fn request(action: BatchAction, names: &[&str], agents: &[&str]) -> BatchRequest {
        BatchRequest {
            project_root: None,
            action,
            skill_ids: names.iter().map(|s| s.to_string()).collect(),
            agent_ids: agents.iter().map(|s| s.to_string()).collect(),
            target_keys: None,
        }
    }
    fn updates(&self) -> HashMap<String, UpdateSkillResult> {
        ["alpha", "beta", "gamma"]
            .into_iter()
            .map(|name| {
                (
                    name.into(),
                    UpdateSkillResult {
                        name: name.into(),
                        source: Some("vercel-labs/skills".into()),
                        reference: None,
                        lock_hash: Some("a".repeat(40)),
                        remote_hash: Some("b".repeat(40)),
                        state: UpdateState::Update,
                        unknown_reason: None,
                        cached: false,
                        last_success_at_epoch_ms: Some(crate::time::now_ms()),
                        previous_state: None,
                        error: None,
                        note: None,
                    },
                )
            })
            .collect()
    }
    async fn preview(&self, request: BatchRequest) -> BatchPreview {
        self.manager
            .preview_batch(ScopeId::Global, request, self.updates())
            .await
            .unwrap()
    }
    fn confirmation(p: &BatchPreview) -> BatchConfirmation {
        BatchConfirmation {
            preview_id: p.id.clone(),
            revision: p.revision.clone(),
            confirmed_delete: true,
            confirmed_copies: true,
        }
    }
    fn link(&self, name: &str, dir: &str) {
        std::os::unix::fs::symlink(
            self.paths.canonical_skills_dir().join(name),
            self.paths.home().join(dir).join(name),
        )
        .unwrap();
    }
    fn edit(&self, name: &str) {
        fs::write(
            self.paths
                .canonical_skills_dir()
                .join(name)
                .join("SKILL.md"),
            "local edit\n",
        )
        .unwrap();
    }
}
fn sink() -> (EventSink, mpsc::UnboundedReceiver<TaskEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (
        Arc::new(move |event| {
            let _ = tx.send(event);
        }),
        rx,
    )
}
async fn finished(rx: &mut mpsc::UnboundedReceiver<TaskEvent>) -> TaskResult {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = rx.recv().await.unwrap();
            if event.phase.is_final() {
                return event.result.unwrap();
            }
        }
    })
    .await
    .expect("batch finishes")
}
fn statuses(result: &TaskResult) -> Vec<BatchStatus> {
    result
        .batch
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .map(|r| r.status)
        .collect()
}

impl Fixture {
    fn copy_request(
        &self,
        root: &std::path::Path,
        names: &[&str],
        agents: &[&str],
    ) -> BatchRequest {
        let mut request = Self::request(BatchAction::CopyToProject, names, agents);
        request.project_root = Some(root.to_string_lossy().into_owned());
        request
    }
    fn project_root(&self) -> std::path::PathBuf {
        let root = self._tmp.path().join("project");
        fs::create_dir_all(&root).unwrap();
        root
    }
    async fn execute_preview(&self, preview: &BatchPreview) -> TaskResult {
        let (on_event, mut rx) = sink();
        self.manager
            .start_batch(ScopeId::Global, Self::confirmation(preview), on_event)
            .await
            .unwrap();
        finished(&mut rx).await
    }
}

#[tokio::test]
async fn copy_to_project_preview_is_read_only_and_copy_is_independent_and_managed() {
    let f = Fixture::new();
    let root = f.project_root();
    let registry = crate::projects::ProjectRegistry::new(f.paths.clone());
    let original_lock = fs::read(f.paths.lock_path()).unwrap();
    let preview = f
        .preview(f.copy_request(&root, &["alpha", "beta"], &["claude-code"]))
        .await;
    assert_eq!(preview.targets.len(), 4);
    assert!(
        preview.targets.iter().all(|t| t.eligible),
        "{:?}",
        preview.targets
    );
    assert!(registry.list().projects.is_empty());
    assert!(!root.join(".agents").exists());
    for _ in 0..3 {
        assert_eq!(
            f.manager
                .refresh_batch_preview(preview.id.clone())
                .await
                .unwrap()
                .revision,
            preview.revision
        );
    }
    let result = f.execute_preview(&preview).await;
    assert_eq!(
        statuses(&result),
        vec![BatchStatus::Success; 4],
        "{:?}",
        result.batch
    );
    let project = &registry.list().projects[0];
    assert_eq!(registry.list().last_active_scope, ScopeId::Global);
    assert_eq!(result.scope, project.project.scope());
    assert_eq!(fs::read(f.paths.lock_path()).unwrap(), original_lock);
    let project_lock =
        crate::project_lock::ProjectLockFile::read(&root.join("skills-lock.json")).unwrap();
    for name in ["alpha", "beta"] {
        let canonical = root.join(".agents/skills").join(name);
        assert!(fs::symlink_metadata(&canonical).unwrap().is_dir());
        assert_eq!(
            fs::canonicalize(root.join(".claude/skills").join(name)).unwrap(),
            fs::canonicalize(&canonical).unwrap()
        );
        let entry = project_lock.get(name).unwrap();
        assert_eq!(entry.source, "vercel-labs/skills");
        assert_eq!(
            entry.skill_folder_hash,
            crate::update::content_hash::folder_hash(&canonical).unwrap()
        );
        assert!(
            BaselineStore::for_scope(&f.paths, &result.scope)
                .load(name)
                .unwrap()
                .verified
        );
    }
    f.edit("alpha");
    fs::remove_dir_all(f.paths.canonical_skills_dir().join("beta")).unwrap();
    assert_eq!(
        fs::read_to_string(root.join(".agents/skills/alpha/SKILL.md")).unwrap(),
        "# alpha\n"
    );
    assert!(root.join(".claude/skills/beta/SKILL.md").exists());
}

#[tokio::test]
async fn copy_to_project_zero_agents_copies_local_changes_and_skips_conflicts_on_repeat() {
    let f = Fixture::new();
    let root = f.project_root();
    f.edit("alpha");
    let request = f.copy_request(&root, &["alpha", "beta"], &[]);
    let preview = f.preview(request.clone()).await;
    assert_eq!(
        preview.targets[0].local_modification,
        crate::baseline::LocalModification::Modified
    );
    assert_eq!(
        statuses(&f.execute_preview(&preview).await),
        vec![BatchStatus::Success; 2]
    );
    let before = fs::read(root.join("skills-lock.json")).unwrap();
    fs::write(root.join(".agents/skills/beta/SKILL.md"), "project edit").unwrap();
    let preview = f
        .preview(f.copy_request(&root, &["alpha", "beta"], &["claude-code"]))
        .await;
    assert_eq!(
        preview
            .targets
            .iter()
            .map(|t| t.eligible)
            .collect::<Vec<_>>(),
        vec![false, true, false, false]
    );
    assert_eq!(
        statuses(&f.execute_preview(&preview).await),
        vec![
            BatchStatus::Skipped,
            BatchStatus::Success,
            BatchStatus::Skipped,
            BatchStatus::Skipped
        ]
    );
    assert_eq!(fs::read(root.join("skills-lock.json")).unwrap(), before);
    assert_eq!(
        fs::read_to_string(root.join(".agents/skills/beta/SKILL.md")).unwrap(),
        "project edit"
    );
}

#[tokio::test]
async fn copy_to_project_preserves_unmanaged_destinations_and_rejects_external_links() {
    let f = Fixture::new();
    let root = f.project_root();
    fs::create_dir_all(root.join(".agents/skills/alpha")).unwrap();
    fs::write(root.join(".agents/skills/alpha/SKILL.md"), "# alpha\n").unwrap();
    std::os::unix::fs::symlink(
        "/etc/hosts",
        f.paths.canonical_skills_dir().join("beta/outside"),
    )
    .unwrap();
    let preview = f
        .preview(f.copy_request(&root, &["alpha", "beta"], &["claude-code"]))
        .await;
    assert!(preview.targets.iter().all(|t| !t.eligible));
    assert!(!root.join("skills-lock.json").exists());
    assert!(
        crate::projects::ProjectRegistry::new(f.paths.clone())
            .list()
            .projects
            .is_empty()
    );
}

#[tokio::test]
async fn copy_to_project_changed_preview_never_registers_or_writes() {
    let f = Fixture::new();
    let root = f.project_root();
    let preview = f.preview(f.copy_request(&root, &["alpha"], &[])).await;
    f.edit("alpha");
    let (events, _) = sink();
    let error = f
        .manager
        .start_batch(ScopeId::Global, Fixture::confirmation(&preview), events)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::BatchPreviewExpired);
    assert!(
        crate::projects::ProjectRegistry::new(f.paths.clone())
            .list()
            .projects
            .is_empty()
    );
    assert!(!root.join(".agents").exists());
}

#[tokio::test]
async fn copy_to_project_preserves_internal_relative_links_and_root_link_content() {
    let f = Fixture::new();
    let root = f.project_root();
    let source = f.paths.canonical_skills_dir().join("alpha");
    std::os::unix::fs::symlink("SKILL.md", source.join("reference.md")).unwrap();
    let actual = f._tmp.path().join("linked-source");
    fs::rename(&source, &actual).unwrap();
    std::os::unix::fs::symlink(&actual, &source).unwrap();
    let preview = f.preview(f.copy_request(&root, &["alpha"], &[])).await;
    assert_eq!(
        statuses(&f.execute_preview(&preview).await),
        vec![BatchStatus::Success]
    );
    let copied = root.join(".agents/skills/alpha");
    assert!(fs::symlink_metadata(&copied).unwrap().is_dir());
    assert_eq!(
        fs::read_link(copied.join("reference.md")).unwrap(),
        std::path::Path::new("SKILL.md")
    );
    fs::remove_dir_all(&actual).unwrap();
    assert_eq!(
        fs::read_to_string(copied.join("reference.md")).unwrap(),
        "# alpha\n"
    );
}

#[tokio::test]
async fn copy_to_project_skips_changed_agent_targets_and_retains_its_copy() {
    let f = Fixture::new();
    let root = f.project_root();
    let preview = f
        .preview(f.copy_request(&root, &["alpha", "beta"], &["claude-code"]))
        .await;
    let occupied = root.join(".claude/skills/alpha");
    let path = occupied.clone();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let events: EventSink = Arc::new(move |event| {
        if event.message == "已处理 1 / 4 个目标" {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "do not overwrite").unwrap();
        }
        let _ = tx.send(event);
    });
    f.manager
        .start_batch(ScopeId::Global, Fixture::confirmation(&preview), events)
        .await
        .unwrap();
    let result = finished(&mut rx).await;
    assert_eq!(
        statuses(&result),
        vec![
            BatchStatus::Success,
            BatchStatus::Skipped,
            BatchStatus::Success,
            BatchStatus::Success
        ]
    );
    assert!(root.join(".agents/skills/alpha/SKILL.md").exists());
    assert_eq!(fs::read_to_string(&occupied).unwrap(), "do not overwrite");
    assert_eq!(
        result.batch.unwrap().rows[1].error.as_ref().unwrap().code,
        ErrorCode::BatchTargetChanged
    );
}

#[tokio::test]
async fn copy_to_project_retains_copy_when_attach_fails_and_retry_only_attaches() {
    use std::os::unix::fs::PermissionsExt;

    let f = Fixture::new();
    let root = f.project_root();
    let agent_dir = root.join(".claude/skills");
    fs::create_dir_all(&agent_dir).unwrap();
    let permissions = fs::metadata(&agent_dir).unwrap().permissions();
    fs::set_permissions(&agent_dir, fs::Permissions::from_mode(0o555)).unwrap();
    let probe = agent_dir.join("permission-probe");
    let writable = fs::write(&probe, "probe").is_ok();
    fs::set_permissions(&agent_dir, permissions.clone()).unwrap();
    // A privileged test process can bypass directory permissions.
    if writable {
        fs::remove_file(probe).unwrap();
        return;
    }
    let mut request = f.copy_request(&root, &["alpha", "beta"], &["claude-code"]);
    let preview = f.preview(request.clone()).await;
    let path = agent_dir.clone();
    let restore = permissions.clone();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let events: EventSink = Arc::new(move |event| {
        if event.message == "已处理 1 / 4 个目标" {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o555)).unwrap();
        } else if event.message == "已处理 2 / 4 个目标" {
            fs::set_permissions(&path, restore.clone()).unwrap();
        }
        let _ = tx.send(event);
    });
    f.manager
        .start_batch(ScopeId::Global, Fixture::confirmation(&preview), events)
        .await
        .unwrap();
    let result = finished(&mut rx).await;
    fs::set_permissions(&agent_dir, permissions).unwrap();
    assert_eq!(
        statuses(&result),
        vec![
            BatchStatus::Success,
            BatchStatus::Failed,
            BatchStatus::Success,
            BatchStatus::Success
        ]
    );
    assert!(root.join(".agents/skills/alpha/SKILL.md").exists());
    assert!(!agent_dir.join("alpha").exists());
    assert_eq!(
        result.batch.unwrap().rows[1].error.as_ref().unwrap().code,
        ErrorCode::AgentAttachFailed
    );
    let lock = fs::read(root.join("skills-lock.json")).unwrap();
    request.target_keys = Some(vec![preview.targets[1].key.clone()]);
    let retry = f.preview(request).await;
    assert_eq!(retry.targets.len(), 1);
    assert_eq!(retry.targets[0].operation.as_deref(), Some("attach"));
    assert_eq!(
        statuses(&f.execute_preview(&retry).await),
        vec![BatchStatus::Success]
    );
    assert_eq!(fs::read(root.join("skills-lock.json")).unwrap(), lock);
}

#[tokio::test]
async fn copy_to_project_stop_retains_registration_and_owns_both_scope_writes() {
    let f = Fixture::new();
    let root = f.project_root();
    let preview = f
        .preview(f.copy_request(&root, &["alpha", "beta"], &["claude-code"]))
        .await;
    let manager = f.manager.clone();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let events: EventSink = Arc::new(move |event| {
        if event.message == "已处理 1 / 4 个目标" {
            assert!(manager.try_acquire_mutation().is_err());
            assert!(manager.begin_registry_change().is_err());
            assert_ne!(manager.active_mutation().unwrap().scope, ScopeId::Global);
            manager.cancel_task(&event.task_id).unwrap();
        }
        let _ = tx.send(event);
    });
    f.manager
        .start_batch(ScopeId::Global, Fixture::confirmation(&preview), events)
        .await
        .unwrap();
    let result = finished(&mut rx).await;
    assert_eq!(
        statuses(&result),
        vec![
            BatchStatus::Success,
            BatchStatus::NotExecuted,
            BatchStatus::NotExecuted,
            BatchStatus::NotExecuted
        ]
    );
    assert!(root.join(".agents/skills/alpha").exists());
    assert!(!root.join(".claude/skills/alpha").exists());
    assert!(!root.join(".agents/skills/beta").exists());
    let registered = crate::projects::ProjectRegistry::new(f.paths.clone()).list();
    assert_eq!(registered.projects.len(), 1);
    assert_eq!(registered.last_active_scope, ScopeId::Global);
}

#[tokio::test]
async fn copy_to_project_baseline_failure_restores_records_and_removes_its_copy() {
    let f = Fixture::new();
    let root = f.project_root();
    let preview = f.preview(f.copy_request(&root, &["alpha"], &[])).await;
    let project = preview.project.as_ref().unwrap();
    let baselines = BaselineStore::for_scope(&f.paths, &project.scope());
    fs::create_dir_all(baselines.dir().parent().unwrap()).unwrap();
    fs::write(baselines.dir(), "block baseline writes").unwrap();
    let result = f.execute_preview(&preview).await;
    assert_eq!(statuses(&result), vec![BatchStatus::Failed]);
    assert!(!root.join(".agents/skills/alpha").exists());
    assert!(!root.join("skills-lock.json").exists());
    assert_eq!(
        crate::projects::ProjectRegistry::new(f.paths.clone())
            .list()
            .projects
            .len(),
        1
    );
}

#[tokio::test]
async fn copy_to_project_keeps_readable_unicode_files_without_inventing_a_version() {
    let f = Fixture::new();
    let root = f.project_root();
    fs::write(
        f.paths.canonical_skills_dir().join("alpha/说明.md"),
        "中文说明",
    )
    .unwrap();
    let preview = f.preview(f.copy_request(&root, &["alpha"], &[])).await;
    assert!(preview.targets[0].eligible);
    assert!(
        preview.targets[0]
            .reason
            .as_deref()
            .unwrap()
            .contains("更新检查会显示未知")
    );
    let result = f.execute_preview(&preview).await;
    assert_eq!(statuses(&result), vec![BatchStatus::Success]);
    assert_eq!(
        fs::read_to_string(root.join(".agents/skills/alpha/说明.md")).unwrap(),
        "中文说明"
    );
    let lock = crate::project_lock::ProjectLockFile::read(&root.join("skills-lock.json")).unwrap();
    assert_eq!(lock.get("alpha").unwrap().skill_folder_hash, "");
    assert_eq!(lock.get("alpha").unwrap().source, "vercel-labs/skills");
    assert!(
        BaselineStore::for_scope(&f.paths, &result.scope)
            .load("alpha")
            .unwrap()
            .verified
    );
}

#[tokio::test]
async fn preview_is_read_only_and_explains_existing_unmanaged_and_shared_targets() {
    let f = Fixture::new();
    f.link("alpha", ".claude/skills");
    let canonical = f.paths.canonical_skills_dir().join("unmanaged");
    fs::create_dir_all(&canonical).unwrap();
    fs::write(canonical.join("SKILL.md"), "# unmanaged").unwrap();
    let lock = fs::read(f.paths.lock_path()).unwrap();
    let p = f
        .preview(Fixture::request(
            BatchAction::Attach,
            &["alpha", "beta", "unmanaged"],
            &["claude-code", "codex"],
        ))
        .await;
    assert_eq!(p.targets.len(), 6);
    assert!(!p.targets[0].eligible);
    assert!(p.targets[0].reason.is_some());
    assert!(p.targets[2].eligible);
    assert!(!p.targets[1].eligible);
    assert!(!p.targets[4].eligible);
    assert_eq!(fs::read(f.paths.lock_path()).unwrap(), lock);
    assert!(f.executor.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn uninstall_mixes_managed_and_unmanaged_targets_without_cli_for_unmanaged() {
    let f = Fixture::new();
    let mut lock: serde_json::Value =
        serde_json::from_slice(&fs::read(f.paths.lock_path()).unwrap()).unwrap();
    lock["skills"].as_object_mut().unwrap().remove("beta");
    fs::write(f.paths.lock_path(), lock.to_string()).unwrap();
    f.link("beta", ".claude/skills");
    let p = f
        .preview(Fixture::request(
            BatchAction::Uninstall,
            &["alpha", "beta", "gamma"],
            &[],
        ))
        .await;
    assert!(p.targets.iter().all(|target| target.eligible));
    assert!(
        p.targets[1]
            .paths
            .iter()
            .any(|path| path.contains(".claude/skills/beta"))
    );
    let (out, mut rx) = sink();
    f.manager
        .start_batch(ScopeId::Global, Fixture::confirmation(&p), out)
        .await
        .unwrap();
    let result = finished(&mut rx).await;
    assert_eq!(statuses(&result), vec![BatchStatus::Success; 3]);
    let names = f
        .executor
        .calls
        .lock()
        .unwrap()
        .iter()
        .map(|call| call.args.windows(2).find(|args| args[0] == "-s").unwrap()[1].clone())
        .collect::<Vec<_>>();
    assert_eq!(names, ["alpha", "gamma"]);
    assert!(!f.paths.canonical_skills_dir().join("beta").exists());
    assert!(!f.paths.home().join(".claude/skills/beta").is_symlink());
    assert!(
        result
            .batch
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .all(|row| row.backup_path.is_some())
    );
}

#[tokio::test]
async fn source_path_missing_skills_with_copy_links_uninstall_in_one_batch() {
    let f = Fixture::new();
    let mut lock: serde_json::Value =
        serde_json::from_slice(&fs::read(f.paths.lock_path()).unwrap()).unwrap();
    lock["skills"].as_object_mut().unwrap().remove("gamma");
    fs::write(f.paths.lock_path(), lock.to_string()).unwrap();
    for dir in [".codex/skills", ".gemini/skills"] {
        fs::create_dir_all(f.paths.home().join(dir)).unwrap();
    }
    for name in ["alpha", "beta", "gamma"] {
        let copy = f.paths.home().join(".claude/skills").join(name);
        if name == "beta" {
            fs::rename(f.paths.canonical_skills_dir().join(name), &copy).unwrap();
        } else {
            fs::create_dir_all(&copy).unwrap();
            fs::write(copy.join("SKILL.md"), format!("# {name} copy\n")).unwrap();
        }
        for dir in [".codex/skills", ".gemini/skills"] {
            std::os::unix::fs::symlink(
                format!("../../.claude/skills/{name}"),
                f.paths.home().join(dir).join(name),
            )
            .unwrap();
        }
    }
    let mut updates = f.updates();
    for update in updates.values_mut() {
        update.state = UpdateState::PathMissing;
        update.remote_hash = None;
    }
    let preview = f
        .manager
        .preview_batch(
            ScopeId::Global,
            Fixture::request(BatchAction::Uninstall, &["alpha", "beta", "gamma"], &[]),
            updates,
        )
        .await
        .unwrap();
    assert!(preview.targets.iter().all(|target| target.eligible));
    for target in &preview.targets {
        assert!(
            target
                .paths
                .iter()
                .any(|path| path == &format!("~/.codex/skills/{}", target.name))
        );
        assert!(
            target
                .paths
                .iter()
                .any(|path| path == &format!("~/.gemini/skills/{}", target.name))
        );
        assert!(
            target
                .paths
                .iter()
                .any(|path| path == &format!("~/.claude/skills/{}", target.name))
        );
    }
    let (out, mut rx) = sink();
    f.manager
        .start_batch(ScopeId::Global, Fixture::confirmation(&preview), out)
        .await
        .unwrap();
    let result = finished(&mut rx).await;
    assert_eq!(statuses(&result), [BatchStatus::Success; 3]);
    assert_eq!(f.executor.calls.lock().unwrap().len(), 2);
    for name in ["alpha", "beta", "gamma"] {
        for dir in [
            ".agents/skills",
            ".claude/skills",
            ".codex/skills",
            ".gemini/skills",
        ] {
            assert!(fs::symlink_metadata(f.paths.home().join(dir).join(name)).is_err());
        }
    }
    assert!(
        result
            .batch
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .all(|row| row.backup_path.is_some())
    );
}

#[tokio::test]
async fn selecting_an_outside_copy_dependency_does_not_expand_uninstall_authorization() {
    let f = Fixture::new();
    let copy = f.paths.home().join(".claude/skills/alpha");
    fs::create_dir_all(&copy).unwrap();
    fs::write(copy.join("SKILL.md"), "# alpha copy\n").unwrap();
    let legacy = f.paths.home().join(".codex/skills");
    fs::create_dir_all(&legacy).unwrap();
    std::os::unix::fs::symlink(&copy, legacy.join("beta")).unwrap();
    let preview = f
        .preview(Fixture::request(
            BatchAction::Uninstall,
            &["alpha", "beta"],
            &[],
        ))
        .await;
    assert!(!preview.targets[0].eligible);
    assert!(preview.targets[0].reason.as_ref().unwrap().contains("引用"));
    assert!(preview.targets[1].eligible);
    assert!(copy.join("SKILL.md").is_file());
    assert!(f.executor.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn update_skips_modified_unknown_current_and_stale_lock_versions() {
    let f = Fixture::new();
    f.edit("alpha");
    fs::remove_file(BaselineStore::new(&f.paths).baseline_path("beta")).unwrap();
    let mut updates = f.updates();
    updates.get_mut("gamma").unwrap().state = UpdateState::Current;
    let p = f
        .manager
        .preview_batch(
            ScopeId::Global,
            Fixture::request(BatchAction::Update, &["alpha", "beta", "gamma"], &[]),
            updates,
        )
        .await
        .unwrap();
    assert!(p.targets.iter().all(|t| !t.eligible && t.reason.is_some()));
    let mut updates = f.updates();
    updates.get_mut("gamma").unwrap().lock_hash = Some("c".repeat(40));
    let p = f
        .manager
        .preview_batch(
            ScopeId::Global,
            Fixture::request(BatchAction::Update, &["gamma"], &[]),
            updates,
        )
        .await
        .unwrap();
    assert!(!p.targets[0].eligible);
}

#[tokio::test]
async fn start_rejects_unconfirmed_deletion_and_changed_preview_without_a_task() {
    let f = Fixture::new();
    let p = f
        .preview(Fixture::request(BatchAction::Uninstall, &["alpha"], &[]))
        .await;
    let mut c = Fixture::confirmation(&p);
    c.confirmed_delete = false;
    assert_eq!(
        f.manager
            .start_batch(ScopeId::Global, c, sink().0)
            .await
            .unwrap_err()
            .code,
        ErrorCode::BatchConfirmationNeeded
    );
    f.edit("alpha");
    assert_eq!(
        f.manager
            .start_batch(ScopeId::Global, Fixture::confirmation(&p), sink().0)
            .await
            .unwrap_err()
            .code,
        ErrorCode::BatchPreviewExpired
    );
    assert!(f.manager.active_mutation().is_none());
    let fresh = f.manager.refresh_batch_preview(p.id).await.unwrap();
    assert_ne!(p.revision, fresh.revision);
}

#[tokio::test]
async fn uninstall_agents_checks_confirmation_and_freezes_selected_paths() {
    let f = Fixture::new();
    f.link("alpha", ".claude/skills");
    let preview = f
        .preview(Fixture::request(
            BatchAction::UninstallAgents,
            &["alpha"],
            &["claude-code"],
        ))
        .await;
    let mut confirmation = Fixture::confirmation(&preview);
    confirmation.confirmed_delete = false;
    assert_eq!(
        f.manager
            .start_batch(ScopeId::Global, confirmation, sink().0)
            .await
            .unwrap_err()
            .code,
        ErrorCode::BatchConfirmationNeeded
    );
    let link = f.paths.home().join(".claude/skills/alpha");
    fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(f.paths.canonical_skills_dir().join("beta"), &link).unwrap();
    assert_eq!(
        f.manager
            .start_batch(ScopeId::Global, Fixture::confirmation(&preview), sink().0)
            .await
            .unwrap_err()
            .code,
        ErrorCode::BatchPreviewExpired
    );
    assert!(link.is_symlink());
}

#[tokio::test]
async fn uninstall_agents_stops_between_targets_and_retries_only_unfinished_pairs() {
    let f = Fixture::new();
    for name in ["alpha", "beta", "gamma"] {
        f.link(name, ".claude/skills");
    }
    let lock = fs::read(f.paths.lock_path()).unwrap();
    let mut request = Fixture::request(
        BatchAction::UninstallAgents,
        &["alpha", "beta", "gamma"],
        &["claude-code"],
    );
    let preview = f.preview(request.clone()).await;
    let (tx, mut rx) = mpsc::unbounded_channel();
    let manager = f.manager.clone();
    let out: EventSink = Arc::new(move |event| {
        if event.message == "已处理 1 / 3 个目标" {
            assert!(manager.try_acquire_mutation().is_err());
            manager.cancel_task(&event.task_id).unwrap();
        }
        let _ = tx.send(event);
    });
    f.manager
        .start_batch(ScopeId::Global, Fixture::confirmation(&preview), out)
        .await
        .unwrap();
    let result = finished(&mut rx).await;
    assert_eq!(
        statuses(&result),
        [
            BatchStatus::Success,
            BatchStatus::NotExecuted,
            BatchStatus::NotExecuted
        ]
    );
    request.target_keys = Some(
        result
            .batch
            .unwrap()
            .rows
            .iter()
            .filter(|r| r.status == BatchStatus::NotExecuted)
            .map(|r| r.target.key.clone())
            .collect(),
    );
    let retry = f.preview(request).await;
    assert_eq!(retry.targets.len(), 2);
    let result = f.execute_preview(&retry).await;
    assert_eq!(statuses(&result), [BatchStatus::Success; 2]);
    assert!(f.executor.calls.lock().unwrap().is_empty());
    assert_eq!(fs::read(f.paths.lock_path()).unwrap(), lock);
    for name in ["alpha", "beta", "gamma"] {
        assert!(f.paths.canonical_skills_dir().join(name).exists());
        assert!(fs::symlink_metadata(f.paths.home().join(".claude/skills").join(name)).is_err());
    }
}

#[tokio::test]
async fn uninstall_agents_runs_selected_dependency_links_before_last_copy() {
    let f = Fixture::new();
    let copy = f.paths.home().join(".claude/skills/alpha");
    fs::rename(f.paths.canonical_skills_dir().join("alpha"), &copy).unwrap();
    std::os::unix::fs::symlink(&copy, f.paths.home().join(".workbuddy/skills/alpha")).unwrap();
    let preview = f
        .preview(Fixture::request(
            BatchAction::UninstallAgents,
            &["alpha"],
            &["claude-code", "workbuddy"],
        ))
        .await;
    assert_eq!(preview.targets[0].agent_id.as_deref(), Some("workbuddy"));
    assert_eq!(preview.targets[1].removes_last_copy, Some(true));
    let mut confirmation = Fixture::confirmation(&preview);
    confirmation.confirmed_copies = false;
    assert_eq!(
        f.manager
            .start_batch(ScopeId::Global, confirmation, sink().0)
            .await
            .unwrap_err()
            .code,
        ErrorCode::BatchConfirmationNeeded
    );
    let result = f.execute_preview(&preview).await;
    assert_eq!(statuses(&result), [BatchStatus::Success; 2]);
    assert!(
        result
            .batch
            .unwrap()
            .rows
            .iter()
            .all(|r| r.backup_path.is_some())
    );
    assert!(!copy.exists());
    assert!(f.executor.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn native_batch_owns_writer_permit_and_stops_between_targets() {
    let f = Fixture::new();
    let p = f
        .preview(Fixture::request(
            BatchAction::Attach,
            &["alpha", "beta", "gamma"],
            &["claude-code"],
        ))
        .await;
    let (tx, mut rx) = mpsc::unbounded_channel();
    let manager = f.manager.clone();
    let out: EventSink = Arc::new(move |event| {
        if event.message == "已处理 1 / 3 个目标" {
            assert_eq!(manager.active_mutation().unwrap().kind, MutationKind::Batch);
            assert!(manager.try_acquire_mutation().is_err());
            manager.cancel_task(&event.task_id).unwrap();
        }
        let _ = tx.send(event);
    });
    f.manager
        .start_batch(ScopeId::Global, Fixture::confirmation(&p), out)
        .await
        .unwrap();
    let result = finished(&mut rx).await;
    assert_eq!(
        statuses(&result),
        [
            BatchStatus::Success,
            BatchStatus::NotExecuted,
            BatchStatus::NotExecuted
        ]
    );
    assert!(f.paths.home().join(".claude/skills/alpha").is_symlink());
    assert!(!f.paths.home().join(".claude/skills/beta").exists());
    assert!(f.executor.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn ordinary_target_failure_continues_and_backup_failure_preserves_prior_results() {
    let f = Fixture::new();
    f.executor.failures.lock().unwrap().insert("alpha".into());
    let p = f
        .preview(Fixture::request(
            BatchAction::Uninstall,
            &["alpha", "beta", "gamma"],
            &[],
        ))
        .await;
    let (out, mut rx) = sink();
    f.manager
        .start_batch(ScopeId::Global, Fixture::confirmation(&p), out)
        .await
        .unwrap();
    let result = finished(&mut rx).await;
    assert_eq!(
        statuses(&result),
        [
            BatchStatus::Failed,
            BatchStatus::Success,
            BatchStatus::Success
        ]
    );
    assert_eq!(f.executor.calls.lock().unwrap().len(), 3);

    let f = Fixture::new();
    let p = f
        .preview(Fixture::request(
            BatchAction::Uninstall,
            &["alpha", "beta", "gamma"],
            &[],
        ))
        .await;
    let paths = f.paths.clone();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let out: EventSink = Arc::new(move |event| {
        if event.message == "已处理 1 / 3 个目标" {
            fs::rename(
                paths.backups_dir(),
                paths.app_data().join("retained-backups"),
            )
            .unwrap();
            fs::write(paths.backups_dir(), "blocked").unwrap();
        }
        let _ = tx.send(event);
    });
    f.manager
        .start_batch(ScopeId::Global, Fixture::confirmation(&p), out)
        .await
        .unwrap();
    let result = finished(&mut rx).await;
    assert_eq!(
        statuses(&result),
        [
            BatchStatus::Success,
            BatchStatus::Failed,
            BatchStatus::NotExecuted
        ]
    );
    let rows = &result.batch.unwrap().rows;
    assert!(rows[0].backup_path.is_some());
    assert_eq!(
        rows[1].error.as_ref().unwrap().code,
        ErrorCode::BackupFailed
    );
    assert_eq!(f.executor.calls.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn stop_and_exit_wait_for_active_writer_then_leave_later_targets_unexecuted() {
    let f = Fixture::new();
    *f.executor.pause.lock().unwrap() = Some("beta".into());
    let p = f
        .preview(Fixture::request(
            BatchAction::Uninstall,
            &["alpha", "beta", "gamma"],
            &[],
        ))
        .await;
    let (out, mut rx) = sink();
    let id = f
        .manager
        .start_batch(ScopeId::Global, Fixture::confirmation(&p), out)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), f.executor.entered.notified())
        .await
        .unwrap();
    f.manager.cancel_task(&id).unwrap();
    f.manager.cancel_task(&id).unwrap();
    let manager = f.manager.clone();
    let shutdown = tokio::spawn(async move {
        manager.shutdown_batch().await;
    });
    tokio::task::yield_now().await;
    assert!(!shutdown.is_finished());
    assert!(f.paths.canonical_skills_dir().join("beta").exists());
    f.executor.release.notify_one();
    let result = finished(&mut rx).await;
    shutdown.await.unwrap();
    assert_eq!(
        statuses(&result),
        [
            BatchStatus::Success,
            BatchStatus::Success,
            BatchStatus::NotExecuted
        ]
    );
    assert_eq!(result.overall, TaskOverall::Cancelled);
    assert!(!f.paths.interrupted_marker_path().exists());
    assert!(f.manager.try_acquire_mutation().is_err());
}

#[tokio::test]
async fn changed_later_content_is_skipped_without_deleting_it() {
    let f = Fixture::new();
    let p = f
        .preview(Fixture::request(
            BatchAction::Uninstall,
            &["alpha", "beta", "gamma"],
            &[],
        ))
        .await;
    let path = f.paths.canonical_skills_dir().join("beta/SKILL.md");
    let (tx, mut rx) = mpsc::unbounded_channel();
    let out: EventSink = Arc::new(move |event| {
        if event.message == "已处理 1 / 3 个目标" {
            fs::write(&path, "external edit").unwrap();
        }
        let _ = tx.send(event);
    });
    f.manager
        .start_batch(ScopeId::Global, Fixture::confirmation(&p), out)
        .await
        .unwrap();
    let result = finished(&mut rx).await;
    assert_eq!(
        statuses(&result),
        [
            BatchStatus::Success,
            BatchStatus::Skipped,
            BatchStatus::Success
        ]
    );
    assert_eq!(
        fs::read_to_string(f.paths.canonical_skills_dir().join("beta/SKILL.md")).unwrap(),
        "external edit"
    );
}

#[tokio::test]
async fn changes_during_backup_skip_cli_and_native_writers_and_keep_snapshot() {
    for unmanaged in [false, true] {
        let f = Fixture::new();
        if unmanaged {
            let mut lock: serde_json::Value =
                serde_json::from_slice(&fs::read(f.paths.lock_path()).unwrap()).unwrap();
            lock["skills"].as_object_mut().unwrap().remove("alpha");
            fs::write(f.paths.lock_path(), lock.to_string()).unwrap();
        }
        let preview = f
            .preview(Fixture::request(BatchAction::Uninstall, &["alpha"], &[]))
            .await;
        let plan = f.manager.batch_plan(&preview.id).unwrap();
        let layout = f.manager.scope_layout(&ScopeId::Global).unwrap();
        let prepared = uninstall::plan_uninstall(&f.paths, &layout, "alpha").unwrap();
        let action = CheckedAction {
            inner: uninstall::UninstallAction::new(
                f.paths.clone(),
                layout.clone(),
                Duration::from_secs(5),
            ),
            paths: f.paths.clone(),
            layout,
            plan: plan.clone(),
            target: plan.targets[0].clone(),
        };
        let (out, _) = sink();
        let edit_path = f.paths.canonical_skills_dir().join("alpha/SKILL.md");
        let ctx = session::SessionContext {
            task_id: "backup-change".into(),
            sink: out,
            cancel: CancelToken::new(),
            envelope: TaskEnvelope {
                action: TaskAction::Uninstall,
                scope: ScopeId::Global,
                started_at: crate::time::now_ms(),
            },
            runtime: f.manager.inner.runtime.clone(),
            executor: f.executor.clone(),
            record_snapshot: Arc::new(move |snapshot| {
                assert!(snapshot.exists());
                fs::write(&edit_path, "changed after backup").unwrap();
            }),
            set_cancellable: Arc::new(|_| {}),
        };
        match session::run(ctx, action, prepared).await {
            FinishOutcome::Completed(result, _) => {
                assert_eq!(
                    result.skills[0].error.as_ref().unwrap().code,
                    ErrorCode::BatchTargetChanged
                );
                assert!(result.skills[0].backup_path.is_some());
            }
            other => panic!("expected retained snapshot: {other:?}"),
        }
        assert!(f.executor.calls.lock().unwrap().is_empty());
        assert!(f.paths.canonical_skills_dir().join("alpha").exists());
    }
}

#[tokio::test]
async fn multiple_agents_use_confirmed_order_and_other_targets_do_not_invalidate_fingerprints() {
    let f = Fixture::new();
    let p = f
        .preview(Fixture::request(
            BatchAction::Attach,
            &["beta", "alpha"],
            &["workbuddy", "claude-code"],
        ))
        .await;
    assert_eq!(
        p.targets
            .iter()
            .map(|t| (&*t.name, t.agent_id.as_deref().unwrap()))
            .collect::<Vec<_>>(),
        [
            ("beta", "workbuddy"),
            ("beta", "claude-code"),
            ("alpha", "workbuddy"),
            ("alpha", "claude-code")
        ]
    );
    let (out, mut rx) = sink();
    f.manager
        .start_batch(ScopeId::Global, Fixture::confirmation(&p), out)
        .await
        .unwrap();
    assert_eq!(
        statuses(&finished(&mut rx).await),
        [BatchStatus::Success; 4]
    );
}

#[tokio::test]
async fn project_attach_can_create_missing_agent_directory_and_keeps_global_skill_unchanged() {
    let f = Fixture::new();
    let root = f._tmp.path().join("project");
    let canonical = root.join(".agents/skills/alpha");
    fs::create_dir_all(&canonical).unwrap();
    fs::write(canonical.join("SKILL.md"), "# project alpha\n").unwrap();
    fs::write(root.join("skills-lock.json"), serde_json::json!({"version":1,"skills":{"alpha":{"source":"vercel-labs/skills","sourceType":"github","sourceUrl":"https://github.com/vercel-labs/skills.git","skillPath":"skills/alpha/SKILL.md","computedHash":"a".repeat(40)}}}).to_string()).unwrap();
    let project = f
        .manager
        .inner
        .projects
        .add(root.to_str().unwrap())
        .unwrap()
        .project;
    let scope = project.scope();
    let request = BatchRequest {
        project_root: None,
        action: BatchAction::Attach,
        skill_ids: vec![scope.skill_id("alpha")],
        agent_ids: vec!["claude-code".into()],
        target_keys: None,
    };
    let p = f
        .manager
        .preview_batch(scope.clone(), request, HashMap::new())
        .await
        .unwrap();
    assert!(p.targets[0].eligible);
    assert_eq!(p.targets[0].agent_name.as_deref(), Some("Claude Code"));
    let (out, mut rx) = sink();
    f.manager
        .start_batch(scope.clone(), Fixture::confirmation(&p), out)
        .await
        .unwrap();
    let result = finished(&mut rx).await;
    assert_eq!(result.scope, scope);
    assert_eq!(statuses(&result), [BatchStatus::Success]);
    assert_eq!(
        fs::canonicalize(root.join(".claude/skills/alpha")).unwrap(),
        fs::canonicalize(canonical).unwrap()
    );
    assert_eq!(
        fs::read_to_string(f.paths.canonical_skills_dir().join("alpha/SKILL.md")).unwrap(),
        "# alpha\n"
    );
}

#[tokio::test]
async fn copy_detach_requires_confirmation_and_detects_edits_after_backup() {
    let f = Fixture::new();
    let copy = f.paths.home().join(".claude/skills/alpha");
    fs::create_dir_all(&copy).unwrap();
    fs::write(copy.join("SKILL.md"), "# alpha\n").unwrap();
    let p = f
        .preview(Fixture::request(
            BatchAction::Detach,
            &["alpha"],
            &["claude-code"],
        ))
        .await;
    assert!(p.targets[0].eligible && p.targets[0].deletes_copy);
    let mut c = Fixture::confirmation(&p);
    c.confirmed_copies = false;
    assert_eq!(
        f.manager
            .start_batch(ScopeId::Global, c, sink().0)
            .await
            .unwrap_err()
            .code,
        ErrorCode::BatchConfirmationNeeded
    );
    let plan = f.manager.batch_plan(&p.id).unwrap();
    let layout = f.manager.scope_layout(&ScopeId::Global).unwrap();
    let changed = crate::detach::detach_agent_checked(
        &f.paths,
        &layout,
        "alpha",
        "claude-code",
        &|_| {
            fs::write(copy.join("SKILL.md"), "copy edit").unwrap();
        },
        &|| verify_target(&f.paths, &layout, &plan, &plan.targets[0]),
    );
    assert_eq!(changed.unwrap_err().code, ErrorCode::BatchTargetChanged);
    assert!(copy.exists());
}

#[tokio::test]
async fn retry_restricts_execution_to_requested_pairs_and_cannot_cross_scope() {
    let f = Fixture::new();
    f.link("alpha", ".claude/skills");
    let mut request = Fixture::request(
        BatchAction::Attach,
        &["alpha", "beta"],
        &["claude-code", "workbuddy"],
    );
    request.target_keys = Some(vec![
        serde_json::to_string(&("beta", Some("workbuddy"))).unwrap(),
    ]);
    let p = f.preview(request).await;
    assert_eq!(p.targets.len(), 1);
    assert_eq!(p.targets[0].name, "beta");
    let (out, mut rx) = sink();
    f.manager
        .start_batch(ScopeId::Global, Fixture::confirmation(&p), out)
        .await
        .unwrap();
    assert_eq!(statuses(&finished(&mut rx).await), [BatchStatus::Success]);
    assert!(!f.paths.home().join(".claude/skills/beta").exists());
    let request = Fixture::request(
        BatchAction::Attach,
        &["project:foreign/alpha"],
        &["claude-code"],
    );
    assert!(
        f.manager
            .preview_batch(ScopeId::Global, request, HashMap::new())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn update_runs_each_clean_candidate_and_does_not_expand_original_skips() {
    let f = Fixture::new();
    fs::remove_file(BaselineStore::new(&f.paths).baseline_path("beta")).unwrap();
    let p = f
        .preview(Fixture::request(
            BatchAction::Update,
            &["alpha", "beta", "gamma"],
            &[],
        ))
        .await;
    assert!(!p.targets[1].eligible);
    let paths = f.paths.clone();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let out: EventSink = Arc::new(move |event| {
        if event.message == "已处理 1 / 2 个目标" {
            BaselineStore::new(&paths)
                .record_from_dir("beta", &paths.canonical_skills_dir().join("beta"), true)
                .unwrap();
        }
        let _ = tx.send(event);
    });
    f.manager
        .start_batch(ScopeId::Global, Fixture::confirmation(&p), out)
        .await
        .unwrap();
    let result = finished(&mut rx).await;
    assert_eq!(
        statuses(&result),
        [
            BatchStatus::Success,
            BatchStatus::Skipped,
            BatchStatus::Success
        ],
        "{:?}",
        result.batch
    );
    assert_eq!(f.executor.calls.lock().unwrap().len(), 2);
    assert_eq!(
        fs::read_to_string(f.paths.canonical_skills_dir().join("beta/SKILL.md")).unwrap(),
        "# beta\n"
    );
}

#[tokio::test]
async fn changed_source_or_skill_path_invalidates_cached_update_evidence() {
    let f = Fixture::new();
    let mut updates = f.updates();
    updates.get_mut("alpha").unwrap().source = Some("other/skills".into());
    let preview = f
        .manager
        .preview_batch(
            ScopeId::Global,
            Fixture::request(BatchAction::Update, &["alpha"], &[]),
            updates,
        )
        .await
        .unwrap();
    assert!(!preview.targets[0].eligible);
    let p = f
        .preview(Fixture::request(BatchAction::Update, &["alpha"], &[]))
        .await;
    assert!(p.targets[0].eligible);
    let records = f
        .manager
        .batch_update_records(ScopeId::Global)
        .await
        .unwrap();
    let mut lock: serde_json::Value =
        serde_json::from_slice(&fs::read(f.paths.lock_path()).unwrap()).unwrap();
    lock["skills"]["alpha"]["skillPath"] = "moved/alpha/SKILL.md".into();
    fs::write(f.paths.lock_path(), lock.to_string()).unwrap();
    assert_eq!(
        f.manager
            .preview_checked_batch(
                ScopeId::Global,
                Fixture::request(BatchAction::Update, &["alpha"], &[]),
                f.updates(),
                Some(records)
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::BatchPreviewExpired
    );
    let refreshed = f.manager.refresh_batch_preview(p.id).await.unwrap();
    assert!(!refreshed.targets[0].eligible);
    assert!(
        refreshed.targets[0]
            .reason
            .as_deref()
            .unwrap()
            .contains("检查结果")
    );
}
