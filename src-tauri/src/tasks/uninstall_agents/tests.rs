use super::*;
use crate::inventory::AgentDir;
use crate::tasks::batch::{BatchAction, BatchPlan, BatchRequest};
use std::collections::HashMap;
use std::os::unix::fs::symlink;

struct Fixture {
    _tmp: tempfile::TempDir,
    paths: AppPaths,
    layout: ScopeLayout,
}
impl Fixture {
    fn new(project: bool, managed: bool, canonical: bool) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(tmp.path().join("home"), tmp.path().join("data"));
        paths.ensure_dirs().unwrap();
        let root = if project {
            tmp.path().join("project")
        } else {
            paths.home().to_path_buf()
        };
        fs::create_dir_all(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let agents: Vec<_> = [
            ("claude-code", ".claude/skills", false),
            ("gemini-cli", ".gemini/skills", true),
            ("codex", ".agents/skills", true),
        ]
        .into_iter()
        .map(|(id, dir, universal)| {
            let skills_dir = root.join(dir);
            fs::create_dir_all(&skills_dir).unwrap();
            AgentDir {
                id: id.into(),
                display_name: id.into(),
                universal,
                skills_dir,
            }
        })
        .collect();
        let mut layout = if project {
            ScopeLayout::project(
                &paths,
                crate::projects::Project {
                    id: uuid::Uuid::new_v4(),
                    root,
                    display_name: "Project".into(),
                    added_at: 0,
                    last_opened_at: 0,
                    check_revision: "test".into(),
                },
            )
        } else {
            ScopeLayout::global(&paths, agents.clone())
        };
        layout.agents = agents;
        if managed {
            let lock = if project {
                serde_json::json!({"version":1,"skills":{"alpha":{"source":"acme/skills","sourceType":"github","computedHash":"a".repeat(40)}}})
            } else {
                serde_json::json!({"version":3,"skills":{"alpha":{"source":"acme/skills","sourceType":"github","sourceUrl":"https://github.com/acme/skills.git","skillPath":"skills/alpha/SKILL.md","skillFolderHash":"a".repeat(40),"installedAt":"2026-01-01T00:00:00.000Z","updatedAt":"2026-01-01T00:00:00.000Z"}}})
            };
            fs::write(&layout.lock_path, lock.to_string()).unwrap();
        }
        let f = Self {
            _tmp: tmp,
            paths,
            layout,
        };
        if canonical {
            Self::content(&f.canonical());
        }
        f
    }
    fn canonical(&self) -> PathBuf {
        self.layout.canonical_dir.join("alpha")
    }
    fn attachment(&self, id: &str) -> PathBuf {
        self.layout
            .agents
            .iter()
            .find(|a| a.id == id)
            .unwrap()
            .skills_dir
            .join("alpha")
    }
    fn content(path: &Path) {
        fs::create_dir_all(path).unwrap();
        fs::write(path.join("SKILL.md"), "# alpha\nlocal content").unwrap();
    }
    fn plan(&self, agents: &[&str]) -> BatchPlan {
        batch::plan(
            &self.paths,
            &self.layout,
            BatchRequest {
                action: BatchAction::UninstallAgents,
                skill_ids: vec![self.layout.scope.skill_id("alpha")],
                agent_ids: agents.iter().map(|id| id.to_string()).collect(),
                target_keys: None,
                project_root: None,
            },
            "preview".into(),
            HashMap::new(),
        )
        .unwrap()
    }
    fn execute(&self, target: &PlannedTarget) -> Result<(), AppError> {
        execute(&self.paths, &self.layout, target, &|_| {}, &|| Ok(()))
    }
}

#[test]
fn removes_selected_links_and_copies_in_both_scopes_without_changing_metadata() {
    for project in [false, true] {
        for managed in [false, true] {
            let f = Fixture::new(project, managed, true);
            symlink(f.canonical(), f.attachment("claude-code")).unwrap();
            Fixture::content(&f.attachment("gemini-cli"));
            let baseline = BaselineStore::for_scope(&f.paths, &f.layout.scope);
            baseline
                .record_from_dir("alpha", &f.canonical(), true)
                .unwrap();
            let before_baseline = fs::read(baseline.baseline_path("alpha")).unwrap();
            let lock = fs::read(&f.layout.lock_path).ok();
            let plan = f.plan(&["claude-code", "codex"]);
            assert!(plan.targets[0].target.eligible);
            assert!(!plan.targets[1].target.eligible);
            f.execute(&plan.targets[0]).unwrap();
            assert!(fs::symlink_metadata(f.attachment("claude-code")).is_err());
            assert!(f.attachment("gemini-cli").join("SKILL.md").is_file());
            let plan = f.plan(&["gemini-cli"]);
            assert!(
                plan.targets[0].target.eligible,
                "universal flag does not mean shared"
            );
            f.execute(&plan.targets[0]).unwrap();
            assert!(f.canonical().join("SKILL.md").is_file());
            assert_eq!(fs::read(&f.layout.lock_path).ok(), lock);
            assert_eq!(
                fs::read(baseline.baseline_path("alpha")).unwrap(),
                before_baseline
            );
        }
    }
}

#[test]
fn last_copy_and_internal_dependencies_are_planned_and_removed_in_order() {
    for managed in [false, true] {
        let f = Fixture::new(false, managed, false);
        let copy = f.attachment("claude-code");
        Fixture::content(&copy);
        symlink(&copy, f.attachment("gemini-cli")).unwrap();
        let blocked = f.plan(&["claude-code"]);
        assert!(!blocked.targets[0].target.eligible);
        let plan = f.plan(&["claude-code", "gemini-cli"]);
        assert_eq!(
            plan.targets[0].target.agent_id.as_deref(),
            Some("gemini-cli")
        );
        assert_eq!(plan.targets[1].target.removes_last_copy, Some(true));
        assert!(
            f.execute(&plan.targets[1]).is_err(),
            "a failed earlier link still protects the copy"
        );
        let lock = fs::read(&f.layout.lock_path).ok();
        for target in &plan.targets {
            f.execute(target).unwrap();
        }
        assert!(!copy.exists());
        assert_eq!(fs::read(&f.layout.lock_path).ok(), lock);
        let backups: Vec<_> = fs::read_dir(f.paths.backups_dir())
            .unwrap()
            .flatten()
            .collect();
        assert!(
            backups
                .iter()
                .any(|b| b.path().join("attachments/claude-code/SKILL.md").is_file())
        );
    }
}

#[test]
fn every_selected_copy_is_warned_when_together_they_remove_the_last_content() {
    let f = Fixture::new(false, true, false);
    Fixture::content(&f.attachment("claude-code"));
    Fixture::content(&f.attachment("gemini-cli"));
    assert_eq!(
        f.plan(&["claude-code"]).targets[0].target.removes_last_copy,
        None
    );
    let plan = f.plan(&["claude-code", "gemini-cli"]);
    assert!(
        plan.targets
            .iter()
            .all(|t| t.target.removes_last_copy == Some(true))
    );
    for target in &plan.targets {
        f.execute(target).unwrap();
    }
}

#[test]
fn unlinks_broken_and_external_project_links_without_reading_their_destinations() {
    let f = Fixture::new(true, false, true);
    let outside = f._tmp.path().join("outside");
    Fixture::content(&outside);
    symlink(&outside, f.attachment("claude-code")).unwrap();
    symlink(f._tmp.path().join("missing"), f.attachment("gemini-cli")).unwrap();
    let plan = f.plan(&["claude-code", "gemini-cli"]);
    assert!(plan.targets.iter().all(|t| t.target.eligible), "{plan:?}");
    for target in &plan.targets {
        f.execute(target).unwrap();
    }
    assert!(outside.join("SKILL.md").is_file());
}

#[test]
fn canonical_and_other_skills_dependencies_cannot_be_authorized_by_agent_selection() {
    let f = Fixture::new(false, true, false);
    let copy = f.attachment("claude-code");
    Fixture::content(&copy);
    symlink(&copy, f.canonical()).unwrap();
    assert!(!f.plan(&["claude-code"]).targets[0].target.eligible);
    fs::remove_file(f.canonical()).unwrap();
    symlink(
        &copy,
        f.attachment("gemini-cli").with_file_name("another-skill"),
    )
    .unwrap();
    assert!(
        !f.plan(&["claude-code", "gemini-cli"])
            .targets
            .iter()
            .any(|t| t.target.eligible)
    );
}

#[test]
fn links_to_content_inside_a_copy_also_protect_it() {
    let f = Fixture::new(false, false, true);
    let copy = f.attachment("claude-code");
    Fixture::content(&copy);
    fs::create_dir(copy.join("scripts")).unwrap();
    symlink(copy.join("scripts"), f.attachment("gemini-cli")).unwrap();
    assert!(!f.plan(&["claude-code"]).targets[0].target.eligible);
}

#[test]
fn backup_failure_and_post_backup_changes_leave_content_untouched() {
    let f = Fixture::new(false, true, true);
    let copy = f.attachment("claude-code");
    Fixture::content(&copy);
    let plan = f.plan(&["claude-code"]);
    fs::remove_dir(f.paths.backups_dir()).unwrap();
    fs::write(f.paths.backups_dir(), "not a directory").unwrap();
    assert!(f.execute(&plan.targets[0]).is_err());
    assert!(copy.exists());
    fs::remove_file(f.paths.backups_dir()).unwrap();
    let result = execute(
        &f.paths,
        &f.layout,
        &plan.targets[0],
        &|_| {
            fs::write(copy.join("SKILL.md"), "new edit during backup").unwrap();
        },
        &|| Ok(()),
    );
    assert!(result.is_err());
    assert_eq!(
        fs::read_to_string(copy.join("SKILL.md")).unwrap(),
        "new edit during backup"
    );
}

#[test]
fn new_dependency_or_loss_of_remaining_content_requires_new_confirmation() {
    let f = Fixture::new(false, true, true);
    let copy = f.attachment("claude-code");
    Fixture::content(&copy);
    let plan = f.plan(&["claude-code"]);
    let result = execute(
        &f.paths,
        &f.layout,
        &plan.targets[0],
        &|_| {
            symlink(&copy, f.attachment("gemini-cli")).unwrap();
        },
        &|| Ok(()),
    );
    assert!(result.is_err());
    fs::remove_file(f.attachment("gemini-cli")).unwrap();
    fs::remove_dir_all(f.canonical()).unwrap();
    assert!(f.execute(&plan.targets[0]).is_err());
    assert!(copy.exists());
}

#[test]
fn changing_parent_or_link_destination_invalidates_authorization() {
    let f = Fixture::new(false, false, true);
    let link = f.attachment("claude-code");
    symlink(f.canonical(), &link).unwrap();
    let plan = f.plan(&["claude-code"]);
    fs::remove_file(&link).unwrap();
    symlink(f._tmp.path().join("elsewhere"), &link).unwrap();
    assert!(f.execute(&plan.targets[0]).is_err());
    let plan = f.plan(&["claude-code"]);
    let parent = link.parent().unwrap();
    let moved = parent.with_file_name("moved");
    fs::rename(parent, &moved).unwrap();
    symlink(&moved, parent).unwrap();
    assert!(f.execute(&plan.targets[0]).is_err());
    assert!(fs::symlink_metadata(&link).is_ok());
}
