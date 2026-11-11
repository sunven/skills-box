//! Opt-in official SkillHub integration. Requires an already extracted official
//! CLI kit at SKILLS_BOX_TEST_SKILLHUB_KIT. Only a temporary HOME is mutated.
use skills_box_lib::{
    cli::{CliRuntime, CliRuntimeProvider, preview::CliCommandRequest},
    error::AppError,
    marketplace::{Coordinate, records::Records},
    paths::AppPaths,
    process::tokio_executor::TokioExecutor,
    scope::ScopeId,
    tasks::{
        EventSink, InventoryConflictProvider, TaskEvent, TaskManager, TaskManagerConfig,
        TaskOverall, TaskResult,
    },
};
use std::{fs, path::PathBuf, sync::Arc, time::Duration};

struct NoGitRuntime;
#[async_trait::async_trait]
impl CliRuntimeProvider for NoGitRuntime {
    async fn cli_runtime(&self) -> Result<CliRuntime, AppError> {
        panic!("marketplace operations must not resolve Node/Git");
    }
}
fn events() -> (EventSink, tokio::sync::mpsc::UnboundedReceiver<TaskEvent>) {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    (
        Arc::new(move |event| {
            let _ = sender.send(event);
        }),
        receiver,
    )
}
async fn completed(mut receiver: tokio::sync::mpsc::UnboundedReceiver<TaskEvent>) -> TaskResult {
    tokio::time::timeout(Duration::from_secs(120), async {
        while let Some(event) = receiver.recv().await {
            if event.phase.is_final() {
                return event.result.expect("final task result");
            }
        }
        panic!("task stream ended");
    })
    .await
    .expect("sandbox task finishes within two minutes")
}

#[tokio::test]
#[ignore = "official SkillHub network + supplied CLI kit; writes only temporary HOME"]
async fn skillhub_install_update_and_native_uninstall_use_the_confirmed_release() {
    let kit = PathBuf::from(
        std::env::var_os("SKILLS_BOX_TEST_SKILLHUB_KIT")
            .expect("set SKILLS_BOX_TEST_SKILLHUB_KIT to the extracted official cli directory"),
    );
    let tmp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(tmp.path()).unwrap();
    let paths = AppPaths::new(root.join("home"), root.join("data"));
    paths.ensure_dirs().unwrap();
    let bin = paths.home().join(".local/bin");
    fs::create_dir_all(&bin).unwrap();
    for (source, destination) in [
        ("skills_store_cli.py", "skillhub"),
        ("skills_upgrade.py", "skills_upgrade.py"),
        ("version.json", "version.json"),
        ("metadata.json", "metadata.json"),
    ] {
        fs::copy(kit.join(source), bin.join(destination)).unwrap();
    }
    fs::create_dir_all(paths.home().join(".claude/skills")).unwrap();
    fs::create_dir_all(paths.home().join(".codex")).unwrap();
    let manager = TaskManager::new(
        Arc::new(NoGitRuntime),
        Arc::new(TokioExecutor),
        Arc::new(InventoryConflictProvider::new(paths.clone())),
        paths.clone(),
        TaskManagerConfig::default(),
    );
    let status = manager.marketplace_cli().await;
    assert!(status.available, "{status:?}");
    let coordinate = Coordinate::from_canonical("@tencent-adm/ima-skills").unwrap();
    let preview = manager
        .prepare_marketplace(
            ScopeId::Global,
            coordinate.clone(),
            vec!["claude-code".into(), "codex".into()],
        )
        .await
        .unwrap();
    let name = preview.detail.original_name.clone().unwrap();
    let version = preview.detail.summary.version.clone();
    println!(
        "CLI {:?}; installing {} {} as {}",
        status.version,
        coordinate.canonical(),
        version,
        name
    );
    let (sink, receiver) = events();
    manager
        .start_marketplace(ScopeId::Global, preview.commands.id, sink)
        .await
        .unwrap();
    let result = completed(receiver).await;
    assert_eq!(result.overall, TaskOverall::Success, "{result:?}");
    assert_eq!(
        Records::read(&paths.lock_path()).unwrap().skills[&name].version,
        version
    );
    assert_eq!(
        fs::canonicalize(paths.home().join(".claude/skills").join(&name)).unwrap(),
        paths.canonical_skills_dir().join(&name)
    );
    assert!(!paths.lock_path().exists(), "Git lock stays absent");

    let preview = manager
        .preview_cli_commands(
            ScopeId::Global,
            CliCommandRequest::Update {
                skills: vec![name.clone()],
            },
        )
        .await
        .unwrap();
    let target = preview.marketplace_versions[&name].clone();
    let (sink, receiver) = events();
    manager
        .start_update_with_preview(ScopeId::Global, vec![name.clone()], Some(preview.id), sink)
        .await
        .unwrap();
    let result = completed(receiver).await;
    assert_eq!(result.overall, TaskOverall::Success, "{result:?}");
    assert_eq!(
        Records::read(&paths.lock_path()).unwrap().skills[&name].version,
        target
    );

    fs::remove_file(bin.join("skillhub")).unwrap();
    let (sink, receiver) = events();
    manager
        .start_uninstall(ScopeId::Global, name.clone(), sink)
        .await
        .unwrap();
    let result = completed(receiver).await;
    assert_eq!(result.overall, TaskOverall::Success, "{result:?}");
    assert!(Records::read(&paths.lock_path()).unwrap().skills.is_empty());
    assert!(!paths.canonical_skills_dir().join(&name).exists());
    assert!(!paths.home().join(".claude/skills").join(&name).exists());
    assert!(!paths.lock_path().exists());
    println!("Confirmed version {target}; native uninstall passed after removing sandbox CLI.");
}

#[tokio::test]
#[ignore = "official SkillHub pack + supplied CLI kit; writes only temporary HOME"]
async fn expert_pack_install_supplement_and_forget_keep_independent_skills() {
    let kit = PathBuf::from(
        std::env::var_os("SKILLS_BOX_TEST_SKILLHUB_KIT").expect("set official CLI kit"),
    );
    let tmp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(tmp.path()).unwrap();
    let paths = AppPaths::new(root.join("home"), root.join("data"));
    paths.ensure_dirs().unwrap();
    let bin = paths.home().join(".local/bin");
    fs::create_dir_all(&bin).unwrap();
    for (source, target) in [
        ("skills_store_cli.py", "skillhub"),
        ("skills_upgrade.py", "skills_upgrade.py"),
        ("version.json", "version.json"),
        ("metadata.json", "metadata.json"),
    ] {
        fs::copy(kit.join(source), bin.join(target)).unwrap();
    }
    fs::create_dir_all(paths.home().join(".claude/skills")).unwrap();
    fs::create_dir_all(paths.home().join(".codex")).unwrap();
    let manager = TaskManager::new(
        Arc::new(NoGitRuntime),
        Arc::new(TokioExecutor),
        Arc::new(InventoryConflictProvider::new(paths.clone())),
        paths.clone(),
        TaskManagerConfig::default(),
    );
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(bin.join("skillhub"), fs::Permissions::from_mode(0o700)).unwrap();
    let preview = manager
        .prepare_pack(
            ScopeId::Global,
            "hr-resume-screening".into(),
            vec!["claude-code".into(), "codex".into()],
        )
        .await
        .unwrap();
    println!(
        "Resolved {} members: {:?}",
        preview.snapshot.members.len(),
        preview
            .snapshot
            .members
            .iter()
            .map(|m| (m.coordinate.canonical(), &m.name, &m.version))
            .collect::<Vec<_>>()
    );
    let snapshot = preview.snapshot.clone();
    assert!(snapshot.members.iter().any(|m| m.coordinate.slug != m.name));
    let (sink, receiver) = events();
    manager
        .start_pack(ScopeId::Global, preview.commands.id, sink)
        .await
        .unwrap();
    let installed = completed(receiver).await;
    assert_eq!(installed.overall, TaskOverall::Success, "{installed:?}");
    let local = manager.local_packs(ScopeId::Global).await.unwrap();
    assert!(local[0].complete);
    assert_eq!(local[0].snapshot, snapshot);
    // Native detach followed by snapshot-based completion never invokes a download.
    fs::remove_file(
        paths
            .home()
            .join(".claude/skills")
            .join(&snapshot.members[0].name),
    )
    .unwrap();
    assert!(!manager.local_packs(ScopeId::Global).await.unwrap()[0].complete);
    fs::remove_file(bin.join("skillhub")).unwrap();
    let preview = manager
        .prepare_pack(ScopeId::Global, snapshot.slug.clone(), vec![])
        .await
        .unwrap();
    assert!(preview.commands.entries.iter().all(|e| e.command.is_none()));
    let (sink, receiver) = events();
    manager
        .start_pack(ScopeId::Global, preview.commands.id, sink)
        .await
        .unwrap();
    assert_eq!(completed(receiver).await.overall, TaskOverall::Success);
    manager
        .forget_pack(ScopeId::Global, snapshot.slug.clone())
        .await
        .unwrap();
    assert!(
        manager
            .local_packs(ScopeId::Global)
            .await
            .unwrap()
            .is_empty()
    );
    let workflows =
        skills_box_lib::expert_packs::records::Records::read(&paths.lock_path()).unwrap();
    assert!(workflows.workflows.contains_key(&snapshot.workflow_name));
    for name in snapshot.names() {
        assert!(
            paths
                .canonical_skills_dir()
                .join(name)
                .join("SKILL.md")
                .is_file()
        );
    }
    let (sink, receiver) = events();
    manager
        .start_uninstall(ScopeId::Global, snapshot.workflow_name.clone(), sink)
        .await
        .unwrap();
    assert_eq!(completed(receiver).await.overall, TaskOverall::Success);
    assert_eq!(
        Records::read(&paths.lock_path()).unwrap().skills.len(),
        snapshot.members.len()
    );
    assert!(!paths.lock_path().exists());
    println!(
        "Pack installed, completed missing attachment without CLI, forgotten and workflow uninstalled; member records retained."
    );
}
