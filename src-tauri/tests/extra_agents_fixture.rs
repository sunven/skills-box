//! End-to-end backend fixture for extra agents: detection, CLI routing, native
//! links, attach/detach and uninstall cleanup. Everything runs in a temp HOME;
//! the real HOME is never read.

use std::fs;

use skills_box_lib::agents::extras::{self, AgentOrigin, UserExtraAgent};
use skills_box_lib::inventory::{self, AttachmentMode, ManagementState};
use skills_box_lib::paths::AppPaths;
use skills_box_lib::tasks::install::{self, InstallRequest, validate_request};
use skills_box_lib::tasks::uninstall;
use skills_box_lib::{attach, detach};

fn write_lock(paths: &AppPaths, name: &str) {
    fs::create_dir_all(paths.agents_root()).unwrap();
    fs::write(
        paths.lock_path(),
        serde_json::json!({
            "version": 3,
            "skills": {
                name: {
                    "source": "acme/skills",
                    "sourceType": "github",
                    "sourceUrl": "https://github.com/acme/skills.git",
                    "skillPath": format!("skills/{name}/SKILL.md"),
                    "skillFolderHash": "0123456789abcdef0123456789abcdef01234567",
                    "installedAt": "2026-01-01T00:00:00.000Z",
                    "updatedAt": "2026-01-01T00:00:00.000Z"
                }
            }
        })
        .to_string(),
    )
    .unwrap();
}

#[test]
fn extra_agent_lifecycle_from_install_to_uninstall() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let paths = AppPaths::new(&home, tmp.path().join("app-data"));
    paths.ensure_dirs().unwrap();

    // WorkBuddy is installed (parent exists); Qoder is a catalog agent.
    fs::create_dir_all(home.join(".workbuddy/skills")).unwrap();
    fs::create_dir_all(home.join(".qoder/skills")).unwrap();
    let canonical = paths.canonical_skills_dir().join("alpha");
    fs::create_dir_all(&canonical).unwrap();
    fs::write(
        canonical.join("SKILL.md"),
        "---\nname: alpha\ndescription: alpha\n---\n",
    )
    .unwrap();
    write_lock(&paths, "alpha");

    let agents = inventory::detected_agent_dirs(&paths);
    assert!(
        agents
            .iter()
            .any(|agent| agent.id == "workbuddy" && !agent.universal),
        "builtin extra agents are detected by their parent directory"
    );
    assert!(agents.iter().any(|agent| agent.id == "qoder"));

    let plan = validate_request(
        &paths,
        &InstallRequest {
            source: "acme/skills".to_string(),
            skills: vec!["alpha".to_string()],
            agents: vec!["workbuddy".to_string()],
            retry: false,
        },
    )
    .unwrap();
    assert_eq!(plan.agent_ids, vec!["universal".to_string()]);
    assert_eq!(plan.targets.len(), 1);
    assert_eq!(plan.targets[0].id, "workbuddy");
    let spec = install::install_command(
        &skills_box_lib::cli::CliRuntime {
            npx_path: "/node/bin/npx".into(),
            node_dir: "/node/bin".into(),
            git_dir: None,
            cwd: paths.cli_cwd(),
            home: home.clone(),
            env: Default::default(),
            pinned_version: skills_box_lib::cli::PINNED_CLI_VERSION.to_string(),
        },
        &plan,
        std::time::Duration::from_secs(900),
    );
    assert!(!spec.args.iter().any(|arg| arg == "workbuddy"));

    let errors = install::ensure_extra_attachments(&paths, &plan, &["alpha".to_string()]);
    assert!(errors.is_empty(), "{errors:?}");
    assert!(
        fs::symlink_metadata(home.join(".workbuddy/skills/alpha"))
            .unwrap()
            .file_type()
            .is_symlink()
    );

    let snapshot = inventory::read(&paths).unwrap();
    let alpha = snapshot
        .skills
        .iter()
        .find(|skill| skill.name == "alpha")
        .unwrap();
    assert_eq!(alpha.management, ManagementState::Managed);
    let workbuddy = alpha
        .attachments
        .iter()
        .find(|attachment| attachment.agent_id == "workbuddy")
        .unwrap();
    assert_eq!(workbuddy.mode, AttachmentMode::Link);
    assert!(workbuddy.detachable);
    let row = snapshot
        .agents
        .iter()
        .find(|agent| agent.id == "workbuddy")
        .unwrap();
    assert_eq!(row.origin, AgentOrigin::Builtin);
    assert!(row.detected);

    attach::attach_agent(&paths, &agents, "alpha", "qoder").unwrap();
    assert!(
        fs::symlink_metadata(home.join(".qoder/skills/alpha"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let snapshot = inventory::read(&paths).unwrap();
    let alpha = snapshot
        .skills
        .iter()
        .find(|skill| skill.name == "alpha")
        .unwrap();
    assert_eq!(alpha.attachments.len(), 2);
    detach::detach_agent(&paths, &agents, "alpha", "qoder").unwrap();
    assert!(fs::symlink_metadata(home.join(".qoder/skills/alpha")).is_err());

    let uninstall_plan = uninstall::plan_uninstall(&paths, &agents, "alpha").unwrap();
    assert!(
        uninstall_plan
            .attachments
            .iter()
            .any(|attachment| attachment.agent_id == "workbuddy")
    );
    let mut warnings = Vec::new();
    uninstall::remove_extra_attachments(&uninstall_plan, &mut warnings);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(fs::symlink_metadata(home.join(".workbuddy/skills/alpha")).is_err());
}

#[test]
fn user_defined_extras_join_detection_and_inventory_after_add() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let paths = AppPaths::new(&home, tmp.path().join("app-data"));
    paths.ensure_dirs().unwrap();
    fs::create_dir_all(home.join(".acme")).unwrap();

    extras::add_user_agent(
        &paths,
        &UserExtraAgent {
            id: "acme".to_string(),
            display_name: "Acme Agent".to_string(),
            skills_dir: "~/.acme/skills".to_string(),
        },
    )
    .unwrap();

    let agents = inventory::detected_agent_dirs(&paths);
    assert!(agents.iter().any(|agent| agent.id == "acme"));

    let snapshot = inventory::read(&paths).unwrap();
    let row = snapshot
        .agents
        .iter()
        .find(|agent| agent.id == "acme")
        .unwrap();
    assert_eq!(row.origin, AgentOrigin::User);
    assert!(row.detected);
    assert_eq!(row.global_skills_dir.as_deref(), Some("~/.acme/skills"));

    extras::remove_user_agent(&paths, "acme").unwrap();
    assert!(
        !inventory::detected_agent_dirs(&paths)
            .iter()
            .any(|agent| agent.id == "acme")
    );
}
