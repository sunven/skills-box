//! Project-scope read model acceptance matrix.
//!
//! Every fixture lives in a temp HOME + temp project root; the real HOME and
//! `~/.agents` are never read or written. These tests drive the same
//! `ReadModel` the Tauri commands use: inventory, detail, content root
//! resolution, per-scope errors, cache fingerprints and forget revocation.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::json;
use skills_box_lib::attachment_target::{self, AgentDirectoryState, TargetContext};
use skills_box_lib::baseline::{BaselineStore, LocalModification};
use skills_box_lib::content;
use skills_box_lib::error::ErrorCode;
use skills_box_lib::inventory::{
    AgentDir, AttachmentLocation, AttachmentMode, LocatedSkill, ManagementState,
};
use skills_box_lib::paths::AppPaths;
use skills_box_lib::projects::ProjectRegistry;
use skills_box_lib::read_model::ReadModel;
use skills_box_lib::scope::ScopeId;
use skills_box_lib::scope_layout::ScopeLayout;

struct Fixture {
    tmp: tempfile::TempDir,
    paths: AppPaths,
    registry: Arc<ProjectRegistry>,
    model: ReadModel,
    root: PathBuf,
    scope: ScopeId,
}

impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        paths.ensure_dirs().unwrap();
        let registry = Arc::new(ProjectRegistry::with_clock(
            paths.clone(),
            Arc::new(|| 1_000),
        ));
        let root = tmp.path().join("projects/app");
        fs::create_dir_all(&root).unwrap();
        let entry = registry.add(root.to_str().unwrap()).unwrap();
        let model = ReadModel::with_projects(paths.clone(), Arc::clone(&registry));
        Self {
            tmp,
            paths,
            registry,
            model,
            root: fs::canonicalize(root).unwrap(),
            scope: entry.project.scope(),
        }
    }

    fn second_project(&self, name: &str) -> (ScopeId, PathBuf) {
        let root = self.tmp.path().join("projects").join(name);
        fs::create_dir_all(&root).unwrap();
        let entry = self.registry.add(root.to_str().unwrap()).unwrap();
        (entry.project.scope(), fs::canonicalize(root).unwrap())
    }

    fn id(&self, name: &str) -> String {
        format!("{}/{name}", self.scope.as_string())
    }

    fn canonical_skill(&self, root: &Path, name: &str, description: &str) -> PathBuf {
        let dir = root.join(".agents/skills").join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {description}\n---\n# {name}\n"),
        )
        .unwrap();
        dir
    }

    fn agent_copy(&self, root: &Path, agent_dir: &str, name: &str, description: &str) -> PathBuf {
        let dir = root.join(agent_dir).join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {description}\n---\n"),
        )
        .unwrap();
        dir
    }

    fn link(&self, dir: &Path, name: &str, target: &Path) {
        fs::create_dir_all(dir).unwrap();
        symlink(target, dir.join(name)).unwrap();
    }

    fn write_lock(&self, root: &Path, names: &[&str]) {
        let mut skills = serde_json::Map::new();
        for name in names {
            skills.insert((*name).to_string(), project_lock_entry(name));
        }
        let lock = json!({"version": 1, "skills": skills});
        fs::write(root.join("skills-lock.json"), lock.to_string()).unwrap();
    }

    fn write_lock_raw(&self, root: &Path, raw: &str) {
        fs::write(root.join("skills-lock.json"), raw).unwrap();
    }

    fn write_extra(&self, project_skills_dir: &str) {
        fs::create_dir_all(self.paths.app_data()).unwrap();
        fs::write(
            self.paths.agents_config_path(),
            json!({
                "version": 1,
                "agents": [{
                    "id": "acme",
                    "displayName": "Acme Agent",
                    "skillsDir": "~/.acme/skills",
                    "projectSkillsDir": project_skills_dir
                }]
            })
            .to_string(),
        )
        .unwrap();
    }

    fn snapshot(&self) -> skills_box_lib::inventory::InventorySnapshot {
        self.model.snapshot(&self.scope).unwrap()
    }

    fn detail(&self, name: &str) -> skills_box_lib::detail::SkillDetail {
        self.model.detail(&self.scope, &self.id(name)).unwrap()
    }
}

fn project_lock_entry(name: &str) -> serde_json::Value {
    json!({
        "source": format!("acme/{name}"),
        "sourceUrl": format!("https://github.com/acme/{name}.git"),
        "ref": "main",
        "sourceType": "github",
        "skillPath": format!("skills/{name}/SKILL.md"),
        "computedHash": "a".repeat(64)
    })
}

fn skill<'a>(
    snapshot: &'a skills_box_lib::inventory::InventorySnapshot,
    name: &str,
) -> &'a skills_box_lib::inventory::InventorySkill {
    snapshot
        .skills
        .iter()
        .find(|skill| skill.name == name)
        .unwrap_or_else(|| panic!("missing skill {name}: {:?}", snapshot.skills))
}

fn modes(skill: &skills_box_lib::inventory::InventorySkill) -> Vec<(String, AttachmentMode)> {
    skill
        .attachments
        .iter()
        .map(|attachment| (attachment.agent_id.clone(), attachment.mode))
        .collect()
}

#[test]
fn v2_project_inventory_and_detail_read_managed_skills_without_rewriting_the_lock() {
    let fixture = Fixture::new();
    fixture.canonical_skill(&fixture.root, "alpha", "project alpha");
    let raw = json!({"version": 2, "skills": {"alpha": project_lock_entry("alpha")}}).to_string();
    fixture.write_lock_raw(&fixture.root, &raw);

    let snapshot = fixture.snapshot();
    assert_eq!(
        skill(&snapshot, "alpha").management,
        ManagementState::Managed
    );
    assert_eq!(fixture.detail("alpha").name, "alpha");
    assert!(fixture.registry.list().projects[0].lock_error.is_none());
    assert_eq!(
        fs::read_to_string(fixture.root.join("skills-lock.json")).unwrap(),
        raw
    );
}

#[test]
fn project_inventory_covers_canonical_catalog_and_extra_dirs_with_all_management_states() {
    let fixture = Fixture::new();
    fixture.write_lock(
        &fixture.root,
        &["alpha", "beta", "epsilon", "ghost", "zeta"],
    );

    let alpha = fixture.canonical_skill(&fixture.root, "alpha", "managed alpha");
    fixture.link(&fixture.root.join(".claude/skills"), "alpha", &alpha);
    fixture.canonical_skill(&fixture.root, "gamma", "unmanaged canonical");
    // Copy-only: lock entry and a real agent copy, no canonical folder.
    fixture.agent_copy(&fixture.root, ".claude/skills", "beta", "copy only beta");
    // Unmanaged agent-only copy.
    fixture.agent_copy(&fixture.root, ".claude/skills", "delta", "agent only");
    // Managed with a dangling link.
    let epsilon = fixture.canonical_skill(&fixture.root, "epsilon", "broken link");
    fixture.link(&fixture.root.join(".claude/skills"), "epsilon", &epsilon);
    fs::remove_file(fixture.root.join(".claude/skills/epsilon")).unwrap();
    symlink(
        fixture.root.join(".claude/skills/missing-target"),
        fixture.root.join(".claude/skills/epsilon"),
    )
    .unwrap();
    // Managed without SKILL.md.
    fs::create_dir_all(fixture.root.join(".agents/skills/zeta")).unwrap();

    // Configured extra project dir exists, so it joins the scan.
    fixture.write_extra(".acme/skills");
    fixture.agent_copy(&fixture.root, ".acme/skills", "omega", "extra unmanaged");

    let snapshot = fixture.snapshot();

    assert_eq!(snapshot.skills.len(), 7, "{:?}", snapshot.skills);
    for skill in &snapshot.skills {
        assert!(
            skill
                .id
                .starts_with(&format!("{}/", fixture.scope.as_string())),
            "id must be project-prefixed: {}",
            skill.id
        );
        assert_eq!(skill.id, fixture.id(&skill.name));
    }

    let alpha = skill(&snapshot, "alpha");
    assert_eq!(alpha.management, ManagementState::Managed);
    assert_eq!(alpha.source.as_deref(), Some("acme/alpha"));
    assert_eq!(alpha.description.as_deref(), Some("managed alpha"));
    assert!(
        alpha.updated_at.is_none(),
        "project locks have no timestamps"
    );
    assert_eq!(
        modes(alpha),
        vec![("claude-code".to_string(), AttachmentMode::Link)]
    );
    assert!(alpha.attachments[0].detachable);

    let beta = skill(&snapshot, "beta");
    assert_eq!(beta.management, ManagementState::CopyOnly);
    assert!(beta.canonical_path.is_none());
    assert_eq!(beta.description.as_deref(), Some("copy only beta"));
    assert_eq!(
        modes(beta),
        vec![("claude-code".to_string(), AttachmentMode::Copy)]
    );
    assert!(!beta.attachments[0].detachable);

    let gamma = skill(&snapshot, "gamma");
    assert_eq!(gamma.management, ManagementState::Unmanaged);
    assert!(gamma.source.is_none());

    let delta = skill(&snapshot, "delta");
    assert_eq!(delta.management, ManagementState::Unmanaged);
    assert_eq!(
        modes(delta),
        vec![("claude-code".to_string(), AttachmentMode::Copy)]
    );

    let epsilon = skill(&snapshot, "epsilon");
    assert_eq!(epsilon.management, ManagementState::Managed);
    assert_eq!(
        modes(epsilon),
        vec![("claude-code".to_string(), AttachmentMode::Broken)]
    );

    let omega = skill(&snapshot, "omega");
    assert_eq!(omega.management, ManagementState::Unmanaged);
    assert_eq!(
        modes(omega),
        vec![("acme".to_string(), AttachmentMode::Copy)]
    );

    let zeta = skill(&snapshot, "zeta");
    assert!(zeta.description.is_none());
    assert!(
        zeta.warnings
            .iter()
            .any(|warning| warning.contains("SKILL.md")),
        "{:?}",
        zeta.warnings
    );

    assert_eq!(snapshot.anomalies.stale.count, 1);
    assert_eq!(snapshot.anomalies.stale.examples, vec!["ghost"]);
    assert_eq!(snapshot.anomalies.unmanaged.count, 3);
    assert_eq!(snapshot.anomalies.broken.count, 1);
    assert_eq!(
        snapshot.anomalies.broken.examples,
        vec!["epsilon · Claude Code"]
    );
    assert_eq!(
        snapshot.anomalies.modified.count, 0,
        "project baselines are not recorded yet, so unknown is not a local-edit anomaly: {:?}",
        snapshot.anomalies.modified
    );
    assert_eq!(snapshot.anomalies.escape.count, 0);

    let agent_ids: Vec<&str> = snapshot
        .agents
        .iter()
        .map(|agent| agent.id.as_str())
        .collect();
    assert!(agent_ids.contains(&"claude-code"));
    assert!(agent_ids.contains(&"acme"));
    assert!(
        !agent_ids.contains(&"codex"),
        "universal agents share canonical and are not project mounts"
    );
}

#[test]
fn project_detail_uses_project_lock_metadata_and_content_stays_in_scope() {
    let fixture = Fixture::new();
    fixture.write_lock(&fixture.root, &["alpha"]);
    let canonical = fixture.canonical_skill(&fixture.root, "alpha", "project alpha");
    fixture.link(&fixture.root.join(".claude/skills"), "alpha", &canonical);

    let detail = fixture.detail("alpha");
    assert_eq!(detail.id, fixture.id("alpha"));
    assert_eq!(detail.name, "alpha");
    assert_eq!(detail.description.as_deref(), Some("project alpha"));
    assert_eq!(detail.source.as_deref(), Some("acme/alpha"));
    assert_eq!(detail.source_type.as_deref(), Some("github"));
    assert_eq!(detail.reference.as_deref(), Some("main"));
    assert_eq!(detail.skill_path.as_deref(), Some("skills/alpha/SKILL.md"));
    assert!(
        detail.installed_at.is_none() && detail.updated_at.is_none(),
        "project locks carry no timestamps"
    );
    assert_eq!(detail.hash.as_deref(), Some("a".repeat(64).as_str()));
    assert_eq!(detail.management, ManagementState::Managed);
    assert!(detail.canonical_path.is_some());
    assert!(detail.directory_path.is_some());

    let root = fixture
        .model
        .resolve_root(&fixture.scope, &fixture.id("alpha"))
        .unwrap();
    assert_eq!(root, canonical);
    let listing = content::list(&fixture.paths, &root, "").unwrap();
    assert!(listing.entries.iter().any(|entry| entry.name == "SKILL.md"));
    let file = content::read_file(&fixture.paths, &root, "SKILL.md").unwrap();
    assert!(file.content.unwrap().contains("project alpha"));
    assert_eq!(
        content::resolve_open_path(&root, "SKILL.md").unwrap(),
        canonical.join("SKILL.md")
    );

    // Plain names and another project's composite id do not resolve here.
    for raw in [
        "alpha",
        "project:00000000-0000-4000-8000-000000000000/alpha",
    ] {
        let error = fixture.model.detail(&fixture.scope, raw).unwrap_err();
        assert_eq!(error.code, ErrorCode::PathInvalid, "{raw}");
        let error = fixture.model.resolve_root(&fixture.scope, raw).unwrap_err();
        assert_eq!(error.code, ErrorCode::PathInvalid, "{raw}");
    }
}

#[test]
fn composite_ids_are_stable_and_global_ids_are_unchanged() {
    let fixture = Fixture::new();
    fixture.write_lock(&fixture.root, &["alpha"]);
    let project_alpha = fixture.canonical_skill(&fixture.root, "alpha", "project alpha");
    fixture.link(
        &fixture.root.join(".claude/skills"),
        "alpha",
        &project_alpha,
    );

    let global_paths = fixture.paths.clone();
    let global_alpha = global_paths.canonical_skills_dir().join("alpha");
    fs::create_dir_all(&global_alpha).unwrap();
    fs::write(
        global_alpha.join("SKILL.md"),
        "---\nname: alpha\ndescription: global alpha\n---\n",
    )
    .unwrap();
    fs::create_dir_all(global_paths.agents_root()).unwrap();
    fs::write(
        global_paths.lock_path(),
        json!({
            "version": 3,
            "skills": {
                "alpha": {
                    "source": "global/alpha",
                    "sourceType": "github",
                    "sourceUrl": "https://github.com/global/alpha.git",
                    "skillFolderHash": "0".repeat(40),
                    "installedAt": "2026-01-01T00:00:00.000Z",
                    "updatedAt": "2026-02-01T00:00:00.000Z"
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    let first = fixture.snapshot();
    let second = fixture.snapshot();
    let id = fixture.id("alpha");
    assert_eq!(skill(&first, "alpha").id, id);
    assert_eq!(skill(&second, "alpha").id, id);
    assert_eq!(fixture.detail("alpha").id, id);

    let global = fixture.model.snapshot(&ScopeId::global()).unwrap();
    assert_eq!(skill(&global, "alpha").id, "alpha");
    assert_eq!(
        fixture
            .model
            .detail(&ScopeId::global(), "alpha")
            .unwrap()
            .name,
        "alpha"
    );
    assert_eq!(
        fixture
            .model
            .resolve_root(&ScopeId::global(), "alpha")
            .unwrap(),
        global_alpha
    );

    let (other_scope, other_root) = fixture.second_project("other");
    fixture.write_lock(&other_root, &["alpha"]);
    fixture.canonical_skill(&other_root, "alpha", "other alpha");
    let other_id = format!("{}/alpha", other_scope.as_string());
    assert_ne!(other_id, id);
    assert_eq!(
        fixture.model.detail(&other_scope, &other_id).unwrap().name,
        "alpha"
    );
    let error = fixture.model.detail(&other_scope, &id).unwrap_err();
    assert_eq!(error.code, ErrorCode::PathInvalid);
}

#[test]
fn escaping_canonical_and_agent_paths_are_anomalies_and_content_refuses_them() {
    let fixture = Fixture::new();
    let outside = fixture.tmp.path().join("outside");
    fs::create_dir_all(outside.join("alpha")).unwrap();
    fs::write(
        outside.join("alpha/SKILL.md"),
        "---\nname: alpha\ndescription: outside\n---\n",
    )
    .unwrap();
    fs::create_dir_all(fixture.root.join(".agents/skills")).unwrap();
    symlink(
        outside.join("alpha"),
        fixture.root.join(".agents/skills/alpha"),
    )
    .unwrap();
    fixture.write_lock(&fixture.root, &["alpha"]);

    // An agent project dir that is a symlink outside the root must not be
    // scanned at all.
    fs::create_dir_all(outside.join("agent-skills/evil")).unwrap();
    fs::write(
        outside.join("agent-skills/evil/SKILL.md"),
        "---\nname: evil\n---\n",
    )
    .unwrap();
    fs::create_dir_all(fixture.root.join(".claude")).unwrap();
    symlink(
        outside.join("agent-skills"),
        fixture.root.join(".claude/skills"),
    )
    .unwrap();

    let snapshot = fixture.snapshot();
    let alpha = skill(&snapshot, "alpha");
    assert_eq!(alpha.management, ManagementState::Managed);
    assert!(
        alpha.description.is_none(),
        "SKILL.md outside the project must not be read"
    );
    assert!(
        alpha
            .warnings
            .iter()
            .any(|warning| warning.contains("项目根之外")),
        "{:?}",
        alpha.warnings
    );
    assert!(!snapshot.skills.iter().any(|skill| skill.name == "evil"));
    assert!(
        snapshot.anomalies.escape.count >= 2,
        "{:?}",
        snapshot.anomalies
    );
    assert!(
        snapshot
            .warnings
            .iter()
            .any(|warning| warning.contains("逃逸")),
        "{:?}",
        snapshot.warnings
    );

    let error = fixture
        .model
        .resolve_root(&fixture.scope, &fixture.id("alpha"))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::PathOutsideAllowedRoot);
}

#[test]
fn project_directory_facts_preserve_attachment_install_and_scan_policies() {
    for case in [
        "missing",
        "inside-parent",
        "inside-parent-missing",
        "outside-parent",
        "outside-parent-missing",
        "broken-parent",
        "file-parent",
        "inside-leaf",
        "outside-leaf",
        "broken-leaf",
    ] {
        let fixture = Fixture::new();
        let canonical = fixture.canonical_skill(&fixture.root, "alpha", "inside");
        fixture.write_lock(&fixture.root, &["alpha", "beta"]);
        let outside = fixture.tmp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        let inside = fixture.root.join("linked-parent");
        fs::create_dir_all(&inside).unwrap();
        let parent = fixture.root.join(".claude");
        let dir = parent.join("skills");
        match case {
            "missing" => {}
            "inside-parent" | "inside-parent-missing" => {
                if case == "inside-parent" {
                    fixture.agent_copy(&inside, "skills", "beta", "inside link");
                }
                symlink(&inside, &parent).unwrap();
            }
            "outside-parent" | "outside-parent-missing" => {
                if case == "outside-parent" {
                    fixture.agent_copy(&outside, "skills", "beta", "outside");
                }
                symlink(&outside, &parent).unwrap();
            }
            "broken-parent" => symlink(outside.join("missing"), &parent).unwrap(),
            "file-parent" => fs::write(&parent, "file").unwrap(),
            "inside-leaf" | "outside-leaf" | "broken-leaf" => {
                fs::create_dir_all(&parent).unwrap();
                let target = match case {
                    "inside-leaf" => fixture
                        .agent_copy(&inside, "skills", "beta", "inside link")
                        .parent()
                        .unwrap()
                        .to_path_buf(),
                    "outside-leaf" => fixture
                        .agent_copy(&outside, "skills", "beta", "outside")
                        .parent()
                        .unwrap()
                        .to_path_buf(),
                    _ => outside.join("missing"),
                };
                symlink(target, &dir).unwrap();
            }
            _ => unreachable!(),
        }
        let agent = AgentDir {
            id: "claude-code".into(),
            display_name: "Claude Code".into(),
            universal: false,
            skills_dir: dir,
        };
        let located = LocatedSkill {
            name: "alpha".into(),
            management: ManagementState::Managed,
            canonical_path: Some(canonical.clone()),
            primary_path: Some(canonical.clone()),
            attachments: vec![],
            lock_entry: None,
        };
        let context = TargetContext::project(&fixture.root);
        let attach = attachment_target::resolve_attach(
            &fixture.root.join(".agents/skills"),
            &located,
            std::slice::from_ref(&agent),
            &agent.id,
            context,
        );
        let install = attachment_target::resolve_install_link(&canonical, &agent, "alpha", context);
        let (offered, scanned, attach_error, install_error) = match case {
            "missing" | "inside-parent-missing" => (true, false, None, None),
            "inside-parent" => (true, true, None, None),
            "inside-leaf" => (
                true,
                true,
                Some(ErrorCode::PathInvalid),
                Some(ErrorCode::AgentAttachFailed),
            ),
            "outside-parent" | "outside-parent-missing" | "outside-leaf" => (
                false,
                false,
                Some(ErrorCode::PathOutsideAllowedRoot),
                Some(ErrorCode::PathOutsideAllowedRoot),
            ),
            _ => (
                false,
                false,
                Some(ErrorCode::PathInvalid),
                Some(ErrorCode::PathInvalid),
            ),
        };
        assert_eq!(
            attach.as_ref().err().map(|error| error.code),
            attach_error,
            "attach {case}"
        );
        assert_eq!(
            install.as_ref().err().map(|error| error.code),
            install_error,
            "install {case}"
        );
        if case == "missing" || case == "inside-parent-missing" {
            assert!(matches!(
                attach.unwrap().directory_state,
                AgentDirectoryState::Missing(_)
            ));
            assert!(matches!(
                install.unwrap().directory_state,
                AgentDirectoryState::Missing(_)
            ));
        }
        let mut attached = located.clone();
        attached.attachments.push(AttachmentLocation {
            agent_id: agent.id.clone(),
            agent_name: agent.display_name.clone(),
            path: agent.skills_dir.join("alpha"),
            mode: AttachmentMode::Link,
        });
        let detach = attachment_target::resolve_detach(
            &fixture.root.join(".agents/skills"),
            &attached,
            std::slice::from_ref(&agent),
            &agent.id,
            context,
        );
        let detach_error = if case == "missing" || case == "inside-parent-missing" {
            Some(ErrorCode::PathInvalid)
        } else {
            attach_error
        };
        assert_eq!(
            detach.err().map(|error| error.code),
            detach_error,
            "detach {case}"
        );

        let project = fixture
            .registry
            .lookup_available(fixture.scope.project_id().unwrap())
            .unwrap()
            .project;
        let layout = ScopeLayout::project(&fixture.paths, project);
        assert_eq!(
            layout
                .install_targets(&[agent])
                .iter()
                .any(|agent| agent.id == "claude-code"),
            offered,
            "targets {case}"
        );
        assert_eq!(
            layout.agents.iter().any(|agent| agent.id == "claude-code"),
            scanned,
            "layout {case}"
        );
        let snapshot = fixture.snapshot();
        assert_eq!(
            snapshot.skills.iter().any(|skill| skill.name == "beta"),
            scanned,
            "scan {case}"
        );
        let store = BaselineStore::for_scope(&fixture.paths, &fixture.scope);
        assert_eq!(store.load("beta").is_some(), scanned, "baseline {case}");
        if scanned {
            assert_eq!(
                fixture.detail("beta").description.as_deref(),
                Some("inside link")
            );
            let root = fixture
                .model
                .resolve_root(&fixture.scope, &fixture.id("beta"))
                .unwrap();
            assert!(root.starts_with(&fixture.root));
        } else if case == "outside-parent" || case == "outside-leaf" {
            assert!(snapshot.anomalies.escape.count > 0, "escape {case}");
        }
    }
}

#[test]
fn canonical_store_parent_symlink_outside_is_not_scanned_or_fingerprinted() {
    let fixture = Fixture::new();
    let outside = fixture.tmp.path().join("outside");
    fixture.agent_copy(&outside, "skills", "alpha", "outside");
    symlink(&outside, fixture.root.join(".agents")).unwrap();
    fixture.write_lock(&fixture.root, &["alpha"]);
    let snapshot = fixture.snapshot();
    assert!(snapshot.skills.is_empty());
    assert!(
        snapshot
            .anomalies
            .escape
            .examples
            .iter()
            .any(|label| label == "canonical store")
    );
    assert!(
        snapshot
            .warnings
            .iter()
            .any(|warning| warning.contains("canonical store 符号链接逃逸"))
    );
    assert!(
        BaselineStore::for_scope(&fixture.paths, &fixture.scope)
            .load("alpha")
            .is_none()
    );
}

#[test]
fn escaping_skill_leaves_keep_anomalies_without_creating_or_replacing_baselines() {
    for agent_leaf in [false, true] {
        for seeded in [false, true] {
            let fixture = Fixture::new();
            let inside = fixture.canonical_skill(&fixture.root, "beta", "valid control");
            let outside = fixture.tmp.path().join("outside/alpha");
            fs::create_dir_all(&outside).unwrap();
            fs::write(
                outside.join("SKILL.md"),
                "---\nname: alpha\ndescription: outside\n---\n",
            )
            .unwrap();
            fs::write(outside.join("secret.txt"), "external contents").unwrap();
            let dir = fixture.root.join(if agent_leaf {
                ".claude/skills"
            } else {
                ".agents/skills"
            });
            fixture.link(&dir, "alpha", &outside);
            fixture.write_lock(&fixture.root, &["alpha", "beta"]);
            let store = BaselineStore::for_scope(&fixture.paths, &fixture.scope);
            let old_baseline = if seeded {
                store.record_from_dir("alpha", &inside, true).unwrap();
                Some(fs::read(store.baseline_path("alpha")).unwrap())
            } else {
                None
            };

            // Same-name records in Global and another Project stay independent.
            let (other_scope, other_root) = fixture.second_project("other");
            let other_skill = fixture.canonical_skill(&other_root, "alpha", "other");
            let other_store = BaselineStore::for_scope(&fixture.paths, &other_scope);
            other_store
                .record_from_dir("alpha", &other_skill, true)
                .unwrap();
            let other_baseline = fs::read(other_store.baseline_path("alpha")).unwrap();
            let global_store = BaselineStore::new(&fixture.paths);
            let global_skill = fixture.canonical_skill(fixture.paths.home(), "alpha", "global");
            global_store
                .record_from_dir("alpha", &global_skill, true)
                .unwrap();
            let global_baseline = fs::read(global_store.baseline_path("alpha")).unwrap();

            let snapshot = fixture.snapshot();
            let alpha = skill(&snapshot, "alpha");
            assert_eq!(
                alpha.management,
                if agent_leaf {
                    ManagementState::CopyOnly
                } else {
                    ManagementState::Managed
                }
            );
            assert_eq!(alpha.local_modification, LocalModification::Unknown);
            assert!(alpha.description.is_none());
            assert!(
                alpha
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("项目根之外"))
            );
            assert!(snapshot.anomalies.escape.count > 0);
            assert!(fixture.detail("alpha").description.is_none());
            assert_eq!(
                fixture
                    .model
                    .resolve_root(&fixture.scope, &fixture.id("alpha"))
                    .unwrap_err()
                    .code,
                ErrorCode::PathOutsideAllowedRoot
            );
            assert_eq!(
                fs::read(store.baseline_path("alpha")).ok(),
                old_baseline,
                "agent_leaf={agent_leaf}, seeded={seeded}"
            );
            assert!(
                store.load("beta").is_some(),
                "valid paths still initialize baselines"
            );
            assert_eq!(
                fs::read(other_store.baseline_path("alpha")).unwrap(),
                other_baseline
            );
            assert_eq!(
                fs::read(global_store.baseline_path("alpha")).unwrap(),
                global_baseline
            );
        }
    }
}

#[test]
fn scope_errors_are_isolated_and_unavailable_or_forgotten_roots_revoke_reads() {
    let fixture = Fixture::new();
    // Global stays healthy throughout.
    let global_alpha = fixture.paths.canonical_skills_dir().join("alpha");
    fs::create_dir_all(&global_alpha).unwrap();
    fs::write(
        global_alpha.join("SKILL.md"),
        "---\nname: alpha\ndescription: global\n---\n",
    )
    .unwrap();
    assert!(
        fixture
            .model
            .snapshot(&ScopeId::global())
            .unwrap()
            .skills
            .iter()
            .any(|skill| skill.name == "alpha")
    );

    fixture.write_lock(&fixture.root, &["alpha"]);
    fixture.canonical_skill(&fixture.root, "alpha", "project alpha");
    assert!(fixture.model.snapshot(&fixture.scope).is_ok());

    // Corrupt lock: only this project errors.
    fixture.write_lock_raw(&fixture.root, "{not json");
    let error = fixture.model.snapshot(&fixture.scope).unwrap_err();
    assert_eq!(error.code, ErrorCode::LockCorrupt);
    let error = fixture
        .model
        .detail(&fixture.scope, &fixture.id("alpha"))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::LockCorrupt);
    assert!(
        !fixture
            .model
            .snapshot(&ScopeId::global())
            .unwrap()
            .skills
            .is_empty()
    );

    // Unsupported version: project-specific error code, still isolated.
    fixture.write_lock_raw(&fixture.root, r#"{"version":3,"skills":{}}"#);
    let error = fixture.model.snapshot(&fixture.scope).unwrap_err();
    assert_eq!(error.code, ErrorCode::ProjectLockUnsupported);
    assert!(fixture.model.snapshot(&ScopeId::global()).is_ok());

    // Another project with a valid lock is unaffected.
    let (other_scope, other_root) = fixture.second_project("other");
    fixture.write_lock(&other_root, &["beta"]);
    fixture.canonical_skill(&other_root, "beta", "other beta");
    assert!(
        fixture
            .model
            .snapshot(&other_scope)
            .unwrap()
            .skills
            .iter()
            .any(|skill| skill.name == "beta")
    );

    // Remove the root: a distinct unavailable state, not a lock error.
    fs::remove_dir_all(&fixture.root).unwrap();
    let error = fixture.model.snapshot(&fixture.scope).unwrap_err();
    assert_eq!(error.code, ErrorCode::ProjectRootUnavailable);
    assert!(error.message.contains("app"));
    let list = fixture.registry.list();
    let entry = list
        .projects
        .iter()
        .find(|entry| entry.project.scope() == fixture.scope)
        .unwrap();
    assert!(!entry.available);

    // Forget revokes reads immediately; the other project and global survive.
    fixture
        .registry
        .forget(&fixture.scope.project_id().unwrap().to_string())
        .unwrap();
    let error = fixture.model.snapshot(&fixture.scope).unwrap_err();
    assert_eq!(error.code, ErrorCode::ProjectNotFound);
    let error = fixture
        .model
        .detail(&fixture.scope, &fixture.id("alpha"))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ProjectNotFound);
    assert!(fixture.model.snapshot(&ScopeId::global()).is_ok());
    assert!(fixture.model.snapshot(&other_scope).is_ok());
}

#[test]
fn canonical_store_symlink_outside_the_project_is_an_escape() {
    let fixture = Fixture::new();
    let outside = fixture.tmp.path().join("outside-store");
    fs::create_dir_all(outside.join("alpha")).unwrap();
    fs::write(
        outside.join("alpha/SKILL.md"),
        "---\nname: alpha\ndescription: outside\n---\n",
    )
    .unwrap();
    fs::create_dir_all(fixture.root.join(".agents")).unwrap();
    symlink(&outside, fixture.root.join(".agents/skills")).unwrap();
    fixture.write_lock(&fixture.root, &["alpha"]);

    let snapshot = fixture.snapshot();
    assert!(
        snapshot.skills.is_empty(),
        "an escaping canonical store must not be scanned: {:?}",
        snapshot.skills
    );
    assert_eq!(snapshot.anomalies.stale.examples, vec!["alpha"]);
    assert_eq!(snapshot.anomalies.escape.count, 1);
    assert!(
        snapshot
            .warnings
            .iter()
            .any(|warning| warning.contains("canonical store") && warning.contains("逃逸")),
        "{:?}",
        snapshot.warnings
    );
    let error = fixture
        .model
        .detail(&fixture.scope, &fixture.id("alpha"))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::SkillNotFound);
    let error = fixture
        .model
        .resolve_root(&fixture.scope, &fixture.id("alpha"))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::SkillNotFound);
}

#[test]
fn unreadable_lock_and_unreadable_root_are_scope_states() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    fixture.write_lock(&fixture.root, &["alpha"]);
    fixture.canonical_skill(&fixture.root, "alpha", "alpha");

    let lock_path = fixture.root.join("skills-lock.json");
    fs::set_permissions(&lock_path, fs::Permissions::from_mode(0o000)).unwrap();
    let result = fixture.model.snapshot(&fixture.scope);
    fs::set_permissions(&lock_path, fs::Permissions::from_mode(0o644)).unwrap();
    if fs::read_to_string(&lock_path).is_err() {
        let error = result.unwrap_err();
        assert_eq!(error.code, ErrorCode::LockReadFailed);
        // Other scopes are unaffected.
        assert!(fixture.model.snapshot(&ScopeId::global()).is_ok());
    }

    fs::set_permissions(&fixture.root, fs::Permissions::from_mode(0o000)).unwrap();
    let result = fixture.model.snapshot(&fixture.scope);
    let unreadable = fs::read_dir(&fixture.root).is_err();
    fs::set_permissions(&fixture.root, fs::Permissions::from_mode(0o755)).unwrap();
    if unreadable {
        let error = result.unwrap_err();
        assert_eq!(error.code, ErrorCode::ProjectRootUnavailable);
        assert!(fixture.model.snapshot(&ScopeId::global()).is_ok());
    }
}

#[test]
fn project_cache_follows_lock_and_canonical_changes_without_global_refresh() {
    let fixture = Fixture::new();
    fixture.write_lock(&fixture.root, &["alpha"]);
    fixture.canonical_skill(&fixture.root, "alpha", "alpha");
    let snapshot = fixture.snapshot();
    assert_eq!(snapshot.skills.len(), 1);
    assert_eq!(
        fixture.detail("alpha").description.as_deref(),
        Some("alpha")
    );

    // A lock rewrite must invalidate the scope cache on the next detail read.
    let beta = fixture.canonical_skill(&fixture.root, "beta", "beta");
    fixture.write_lock(&fixture.root, &["alpha", "beta"]);
    let detail = fixture.detail("beta");
    assert_eq!(detail.description.as_deref(), Some("beta"));
    let expected = fixture.paths.display(&beta);
    assert_eq!(detail.canonical_path.as_deref(), Some(expected.as_str()));
    assert!(beta.is_dir());

    // get_inventory remains the refresh point for topology changes.
    let before = fixture.snapshot();
    assert!(!before.skills.iter().any(|skill| skill.name == "gamma"));
    let gamma = fixture.canonical_skill(&fixture.root, "gamma", "gamma");
    fixture.write_lock(&fixture.root, &["alpha", "beta", "gamma"]);
    let after = fixture.snapshot();
    assert!(after.skills.iter().any(|skill| skill.name == "gamma"));
    assert!(gamma.is_dir());
}
