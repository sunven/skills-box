//! Exercise the CLI's original lock name and normalized folder across mutations.
use std::{collections::BTreeMap, fs, sync::Arc, time::Duration};

use crate::{
    baseline::{BaselineStore, LocalModification},
    cli::{CliRuntime, CliRuntimeProvider},
    error::AppError,
    inventory::ManagementState,
    paths::AppPaths,
    process::{CancelToken, CommandSpec, ExecError, ExecOutput, ProcessExecutor},
    projects::ProjectRegistry,
    read_model::ReadModel,
    scope::ScopeId,
    scope_layout::ScopeLayout,
    tasks::{
        batch::{BatchAction, BatchRequest},
        conflicts::InventoryConflictProvider,
        install::{InstallRequest, conflicts_for, snapshot_install},
        manager::{EventSink, TaskEvent, TaskManager, TaskManagerConfig, TaskSkillStatus},
    },
};
use async_trait::async_trait;

struct SkillsCli {
    paths: AppPaths,
    layout: ScopeLayout,
}

#[async_trait]
impl CliRuntimeProvider for SkillsCli {
    async fn cli_runtime(&self) -> Result<CliRuntime, AppError> {
        Ok(CliRuntime {
            npx_path: "/test/bin/npx".into(),
            node_dir: "/test/bin".into(),
            git_dir: None,
            cwd: self.paths.cli_cwd(),
            home: self.paths.home().into(),
            env: BTreeMap::new(),
            pinned_version: crate::cli::CLI_VERSION.into(),
        })
    }
}

#[async_trait]
impl ProcessExecutor for SkillsCli {
    async fn run(&self, spec: CommandSpec, _: CancelToken) -> Result<ExecOutput, ExecError> {
        assert!(spec.args.iter().any(|arg| arg == "add"));
        assert!(spec.args.windows(2).any(|args| args == ["-s", "My Skill"]));
        let canonical = self.layout.canonical_dir.join("my-skill");
        fs::create_dir_all(&canonical).unwrap();
        fs::write(
            canonical.join("SKILL.md"),
            "---\nname: My Skill\ndescription: test\n---\n",
        )
        .unwrap();
        let mut entry = serde_json::json!({
            "source": "acme/skills", "sourceType": "github", "sourceUrl": "https://github.com/acme/skills.git",
            "skillPath": "skills/my-skill/SKILL.md",
        });
        if self.layout.is_global() {
            entry["skillFolderHash"] = "a".repeat(40).into();
            entry["installedAt"] = "2026-11-20T00:00:00Z".into();
            entry["updatedAt"] = "2026-11-20T00:00:00Z".into();
        } else {
            entry["computedHash"] = "a".repeat(64).into();
        }
        fs::write(&self.layout.lock_path, serde_json::json!({
            "version": if self.layout.is_global() { 3 } else { 2 }, "skills": { "My Skill": entry },
        }).to_string()).unwrap();
        Ok(ExecOutput {
            code: Some(0),
            signal: None,
            stdout: r#"[{"name":"My Skill","status":"installed"}]"#.into(),
            stderr: String::new(),
            duration: Duration::ZERO,
        })
    }
}

fn events() -> (EventSink, tokio::sync::mpsc::UnboundedReceiver<TaskEvent>) {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    (
        Arc::new(move |event| {
            sender.send(event).unwrap();
        }),
        receiver,
    )
}

async fn finished(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<TaskEvent>,
    status: TaskSkillStatus,
) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(event) = receiver.recv().await {
            if event.phase.is_final() {
                let result = event.result.expect("task result");
                assert_eq!(result.skills[0].status, status, "{result:?}");
                assert!(result.warnings.is_empty(), "{result:?}");
                return;
            }
        }
        panic!("missing final event");
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn original_names_survive_install_update_attachments_backup_and_project_copy() {
    for project_scope in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let paths = AppPaths::new(root.join("home"), root.join("app-data"));
        paths.ensure_dirs().unwrap();
        fs::create_dir_all(paths.home().join(".workbuddy")).unwrap();
        let registry = Arc::new(ProjectRegistry::new(paths.clone()));
        let project_root = root.join("project");
        fs::create_dir_all(&project_root).unwrap();
        let project = registry.add(project_root.to_str().unwrap()).unwrap();
        let scope = if project_scope {
            project.project.scope()
        } else {
            ScopeId::Global
        };
        let layout = ScopeLayout::resolve_detected(&paths, &registry, &scope).unwrap();
        let cli = Arc::new(SkillsCli {
            paths: paths.clone(),
            layout: layout.clone(),
        });
        let manager = TaskManager::new(
            cli.clone(),
            cli,
            Arc::new(InventoryConflictProvider::new(paths.clone())),
            paths.clone(),
            TaskManagerConfig::default(),
        );
        let request = InstallRequest {
            source: "acme/skills".into(),
            skills: vec!["My Skill".into()],
            agents: vec!["workbuddy".into()],
            retry: false,
        };
        let (sink, mut receiver) = events();
        manager
            .start_install(scope.clone(), request, sink)
            .await
            .unwrap();
        finished(&mut receiver, TaskSkillStatus::Installed).await;

        let layout = ScopeLayout::resolve_detected(&paths, &registry, &scope).unwrap();
        let canonical = layout.canonical_dir.join("my-skill");
        let agent_root = if project_scope {
            &layout.root
        } else {
            paths.home()
        };
        let attachment = agent_root.join(".workbuddy/skills/my-skill");
        assert_eq!(fs::canonicalize(&attachment).unwrap(), canonical);
        let snapshot = ReadModel::with_projects(paths.clone(), registry.clone())
            .snapshot(&scope)
            .unwrap();
        assert_eq!(snapshot.skills.len(), 1);
        assert_eq!(snapshot.skills[0].name, "My Skill");
        assert_eq!(snapshot.skills[0].management, ManagementState::Managed);
        assert_eq!(snapshot.anomalies.stale.count, 0);
        let baselines = BaselineStore::for_scope(&paths, &scope);
        assert!(baselines.load("My Skill").unwrap().verified);
        assert_eq!(
            baselines.assess("My Skill", Some(&canonical), None).state,
            LocalModification::Clean
        );
        assert!(
            conflicts_for(&paths, &layout, &["My Skill".into()], Some("acme/skills"))
                .unwrap()
                .is_none()
        );
        assert!(
            conflicts_for(&paths, &layout, &["my-skill".into()], Some("acme/skills"))
                .unwrap()
                .is_some()
        );
        let backup = snapshot_install(&paths, &layout, "retry", &["My Skill".into()]).unwrap();
        assert!(backup.join("skills/My Skill/canonical/SKILL.md").is_file());

        let (sink, mut receiver) = events();
        manager
            .start_update(scope.clone(), vec!["My Skill".into()], sink)
            .await
            .unwrap();
        finished(&mut receiver, TaskSkillStatus::Updated).await;
        let id = scope.skill_id("My Skill");
        crate::detach::detach_agent(&paths, &layout, &id, "workbuddy").unwrap();
        assert!(!attachment.exists());
        crate::attach::attach_agent(&paths, &layout, &id, "workbuddy").unwrap();
        assert!(attachment.is_symlink());
        let removal = crate::tasks::uninstall::plan_uninstall(&paths, &layout, &id).unwrap();
        assert_eq!(removal.canonical, Some(canonical.clone()));
        assert!(
            crate::tasks::stale_records::list(&paths, &layout)
                .unwrap()
                .is_empty()
        );
        assert!(crate::tasks::stale_records::absence(&paths, &layout, "My Skill").is_err());

        if !project_scope {
            let copy = crate::tasks::copy_to_project::plan(
                &paths,
                &layout,
                BatchRequest {
                    action: BatchAction::CopyToProject,
                    skill_ids: vec![id],
                    agent_ids: vec![],
                    target_keys: None,
                    project_root: Some(project_root.to_string_lossy().into()),
                },
                "copy".into(),
                None,
            )
            .unwrap();
            assert!(copy.targets[0].target.eligible, "{:?}", copy.targets[0]);
            crate::tasks::copy_to_project::execute(&paths, &copy, &copy.targets[0], false, &|_| {})
                .unwrap();
            assert!(
                project_root
                    .join(".agents/skills/my-skill/SKILL.md")
                    .is_file()
            );
            let copied = ReadModel::with_projects(paths.clone(), registry.clone())
                .snapshot(&project.project.scope())
                .unwrap();
            assert_eq!(copied.skills[0].name, "My Skill");
            assert_eq!(copied.skills[0].management, ManagementState::Managed);
        } else {
            fs::remove_file(&attachment).unwrap();
            fs::remove_dir_all(&canonical).unwrap();
            let stale = crate::tasks::stale_records::list(&paths, &layout).unwrap();
            assert_eq!(stale.len(), 1);
            assert_eq!(stale[0].name, "My Skill");
            let (target, install) = crate::tasks::stale_records::inspect(
                &paths,
                &layout,
                BatchAction::Restore,
                &id,
                "My Skill",
                &["workbuddy".into()],
            )
            .unwrap();
            assert!(target.target.eligible, "{:?}", target.target);
            assert_eq!(install.unwrap().skills, vec!["My Skill"]);
            assert!(
                target
                    .target
                    .paths
                    .iter()
                    .all(|path| path.ends_with("/my-skill"))
            );
        }
    }
}
