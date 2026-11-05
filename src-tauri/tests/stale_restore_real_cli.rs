//! Explicit network verification; all CLI writes use an isolated temporary HOME.
use skills_box_lib::{
    cli::{CliRuntime, CliRuntimeProvider},
    error::AppError,
    paths::AppPaths,
    process::tokio_executor::TokioExecutor,
    scope::ScopeId,
    tasks::{
        InventoryConflictProvider, TaskManager, TaskManagerConfig, TaskOverall,
        batch::{BatchAction, BatchConfirmation, BatchRequest},
    },
};
use std::{collections::BTreeMap, fs, path::PathBuf, sync::Arc, time::Duration};

struct Runtime(CliRuntime);
#[async_trait::async_trait]
impl CliRuntimeProvider for Runtime {
    async fn cli_runtime(&self) -> Result<CliRuntime, AppError> {
        Ok(self.0.clone())
    }
}

#[tokio::test]
#[ignore = "network + real skills@latest; writes only temporary HOME"]
async fn stale_restore_real_cli() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(temp.path().join("home"), temp.path().join("data"));
    paths.ensure_dirs().unwrap();
    fs::create_dir_all(paths.canonical_skills_dir()).unwrap();
    let output = std::process::Command::new("/usr/bin/which")
        .arg("npx")
        .output()
        .unwrap();
    assert!(output.status.success());
    let npx = PathBuf::from(String::from_utf8(output.stdout).unwrap().trim());
    let mut env = BTreeMap::new();
    env.insert("HOME".into(), paths.home().display().to_string());
    env.insert("PATH".into(), std::env::var("PATH").unwrap());
    env.insert(
        "npm_config_cache".into(),
        temp.path().join("npm-cache").display().to_string(),
    );
    env.insert("GIT_TERMINAL_PROMPT".into(), "0".into());
    let version = std::process::Command::new(&npx)
        .args(["-y", "skills@latest", "--version"])
        .current_dir(paths.cli_cwd())
        .env_clear()
        .envs(&env)
        .output()
        .unwrap();
    assert!(
        version.status.success(),
        "skills@latest version probe failed"
    );
    println!(
        "Resolved skills@latest version: {}",
        String::from_utf8_lossy(&version.stdout).trim()
    );
    let entry = serde_json::json!({"source":"mattpocock/skills","sourceType":"github","sourceUrl":"https://github.com/mattpocock/skills.git","skillPath":"old/code-review/SKILL.md","skillFolderHash":"old","installedAt":"2026-01-01","updatedAt":"2026-01-01"});
    let original =
        serde_json::json!({"version":3,"skills":{"code-review":entry,"untouched":entry}})
            .to_string();
    fs::write(paths.lock_path(), &original).unwrap();
    let runtime = Runtime(CliRuntime {
        node_dir: npx.parent().unwrap().into(),
        git_dir: None,
        npx_path: npx,
        cwd: paths.cli_cwd(),
        home: paths.home().into(),
        env,
        pinned_version: "latest".into(),
    });
    let manager = TaskManager::new(
        Arc::new(runtime),
        Arc::new(TokioExecutor),
        Arc::new(InventoryConflictProvider::new(paths.clone())),
        paths.clone(),
        TaskManagerConfig::default(),
    );
    let preview = manager
        .preview_batch(
            ScopeId::Global,
            BatchRequest {
                action: BatchAction::Restore,
                skill_ids: vec!["code-review".into()],
                agent_ids: Vec::new(),
                target_keys: None,
                project_root: None,
            },
            Default::default(),
        )
        .await
        .unwrap();
    assert!(preview.targets[0].eligible, "{:?}", preview.targets[0]);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    manager
        .start_batch(
            ScopeId::Global,
            BatchConfirmation {
                preview_id: preview.id,
                revision: preview.revision,
                confirmed_delete: false,
                confirmed_copies: false,
            },
            Arc::new(move |event| {
                let _ = tx.send(event);
            }),
        )
        .await
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(180), async {
        loop {
            let event = rx.recv().await.unwrap();
            if event.phase.is_final() {
                break event.result.unwrap();
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(result.overall, TaskOverall::Success, "{result:?}");
    assert!(
        paths
            .canonical_skills_dir()
            .join("code-review/SKILL.md")
            .is_file()
    );
    assert!(!paths.home().join(".claude/skills/code-review").exists());
    let after: serde_json::Value =
        serde_json::from_slice(&fs::read(paths.lock_path()).unwrap()).unwrap();
    assert_eq!(
        after["skills"]["untouched"],
        serde_json::from_str::<serde_json::Value>(&original).unwrap()["skills"]["untouched"]
    );
    let backup = result.batch.unwrap().rows[0].backup_path.clone().unwrap();
    assert_eq!(
        fs::read(PathBuf::from(backup).join("skill-lock.json")).unwrap(),
        original.as_bytes()
    );
    println!(
        "Verified skills@latest restoration, original source/name, canonical-only, retained unrelated lock entry, and original Lock backup."
    );
}
