//! Real CNB lifecycle through the app's preview, mutation and check services.
//! Network-dependent; explicitly run with `--ignored --nocapture`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use skills_box_lib::cli::{CliRuntime, CliRuntimeProvider, PINNED_CLI_VERSION};
use skills_box_lib::env::child_env::{ChildEnvParams, build_child_env};
use skills_box_lib::error::AppError;
use skills_box_lib::inventory::UpdateState;
use skills_box_lib::paths::AppPaths;
use skills_box_lib::process::tokio_executor::TokioExecutor;
use skills_box_lib::projects::ProjectRegistry;
use skills_box_lib::scope::ScopeId;
use skills_box_lib::scope_layout::ScopeLayout;
use skills_box_lib::tasks::install::InstallRequest;
use skills_box_lib::tasks::{
    EventSink, InventoryConflictProvider, TaskEvent, TaskManager, TaskManagerConfig, TaskOverall,
    TaskResult,
};
use skills_box_lib::update::{UpdateService, project::ProjectUpdateService};

const SOURCE: &str = "https://cnb.cool/cnb/skills/cnb-skill";
const SKILL: &str = "cnb-api";

struct FixedRuntime(CliRuntime);
#[async_trait::async_trait]
impl CliRuntimeProvider for FixedRuntime {
    async fn cli_runtime(&self) -> Result<CliRuntime, AppError> {
        Ok(self.0.clone())
    }
}

fn executable(name: &str) -> PathBuf {
    let output = std::process::Command::new("which")
        .arg(name)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{name} is required for this explicit test"
    );
    PathBuf::from(String::from_utf8(output.stdout).unwrap().trim())
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

async fn result(receiver: &mut tokio::sync::mpsc::UnboundedReceiver<TaskEvent>) -> TaskResult {
    tokio::time::timeout(Duration::from_secs(180), async {
        while let Some(event) = receiver.recv().await {
            println!("{:?}: {}", event.phase, event.message);
            if event.phase.is_final() {
                assert!(event.error.is_none(), "{event:?}");
                return event.result.expect("final result");
            }
        }
        panic!("task ended without a result");
    })
    .await
    .expect("task deadline")
}

#[tokio::test]
#[ignore = "public CNB source and pinned CLI require network access"]
async fn cnb_preview_install_check_and_update_at_global_and_project_scope() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(temp.path().join("home"), temp.path().join("app-data"));
    paths.ensure_dirs().unwrap();
    std::fs::create_dir_all(paths.home().join(".claude")).unwrap();
    let root = temp.path().join("project");
    std::fs::create_dir_all(&root).unwrap();
    let projects = Arc::new(ProjectRegistry::new(paths.clone()));
    let project = projects.add(root.to_str().unwrap()).unwrap().project;
    let npx = executable("npx");
    let git = executable("git");
    let node_dir = npx.parent().unwrap().to_path_buf();
    let mut env = build_child_env(ChildEnvParams {
        home: paths.home(),
        node_dir: Some(&node_dir),
        git_dir: git.parent(),
        parent: &BTreeMap::new(),
    });
    // Reuse only npm's package cache; all skill and lock writes use temp HOME.
    if let Some(parent_home) = std::env::var_os("HOME") {
        env.insert(
            "npm_config_cache".into(),
            PathBuf::from(parent_home)
                .join(".npm")
                .display()
                .to_string(),
        );
    }
    let runtime = CliRuntime {
        npx_path: npx,
        node_dir,
        git_dir: git.parent().map(PathBuf::from),
        cwd: paths.cli_cwd(),
        home: paths.home().to_path_buf(),
        env,
        pinned_version: PINNED_CLI_VERSION.into(),
    };
    let manager = TaskManager::with_projects(
        Arc::new(FixedRuntime(runtime)),
        Arc::new(TokioExecutor),
        Arc::new(InventoryConflictProvider::new(paths.clone())),
        paths.clone(),
        projects,
        TaskManagerConfig {
            install_timeout: Duration::from_secs(120),
            ..TaskManagerConfig::default()
        },
    );
    let (sink, mut receiver) = events();
    manager
        .start_preview(ScopeId::Global, SOURCE, sink)
        .await
        .unwrap();
    let preview = result(&mut receiver).await.preview.unwrap();
    assert_eq!(preview.source, format!("{SOURCE}.git"));
    assert!(preview.skills.iter().any(|skill| skill.name == SKILL));
    for scope in [ScopeId::Global, project.scope()] {
        let (sink, mut receiver) = events();
        manager
            .start_install(
                scope.clone(),
                InstallRequest {
                    source: SOURCE.into(),
                    skills: vec![SKILL.into()],
                    agents: vec!["claude-code".into()],
                    retry: false,
                },
                sink,
            )
            .await
            .unwrap();
        let installed = result(&mut receiver).await;
        assert_eq!(installed.overall, TaskOverall::Success, "{installed:?}");
        let report = match scope {
            ScopeId::Global => UpdateService::new(paths.clone()).check(None).await.unwrap(),
            _ => ProjectUpdateService::new(paths.clone())
                .check(&ScopeLayout::project(&paths, project.clone()), None)
                .await
                .unwrap(),
        };
        assert_eq!(report.skills[0].state, UpdateState::Current, "{report:?}");
        let (sink, mut receiver) = events();
        manager
            .start_update(scope, vec![SKILL.into()], sink)
            .await
            .unwrap();
        let updated = result(&mut receiver).await;
        assert_eq!(updated.overall, TaskOverall::Success, "{updated:?}");
        assert!(updated.backup_path.is_some());
    }
}
