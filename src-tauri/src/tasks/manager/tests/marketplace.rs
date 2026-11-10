use super::*;
use crate::cli::preview::{CliCommandRequest, CommandPlan};
use crate::marketplace::{
    Coordinate,
    provider::{Detail, Summary},
    records::{Installation, Records},
    runtime::Runtime,
};
use crate::scope::ScopeId;
use crate::tasks::manager::marketplace::apply_pinned_updates;
use crate::tasks::marketplace::{Plan, StagingDirectory};

struct Fixture {
    _tmp: tempfile::TempDir,
    paths: AppPaths,
    manager: TaskManager,
}
impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(tmp.path().join("home"), tmp.path().join("data"));
        paths.ensure_dirs().unwrap();
        for name in ["git-skill", "market-skill"] {
            let dir = paths.canonical_skills_dir().join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("SKILL.md"), format!("---\nname: {name}\n---\n")).unwrap();
            BaselineStore::new(&paths)
                .record_from_dir(name, &dir, true)
                .unwrap();
        }
        write_pre_lock(&paths, &["git-skill"]);
        let record = Installation {
            name: "market-skill".into(),
            coordinate: Coordinate::from_canonical("@publisher/market-skill").unwrap(),
            version: "1.0.0".into(),
            content_hash: "baseline".into(),
            installed_at: "now".into(),
            updated_at: "now".into(),
        };
        let mut records = Records::default();
        records.skills.insert(record.name.clone(), record);
        records.save(&paths.lock_path()).unwrap();
        // A private dummy executable plus scripted executor never probes user programs.
        fs::create_dir_all(paths.home().join(".local/bin")).unwrap();
        fs::write(paths.home().join(".local/bin/skillhub"), "dummy").unwrap();
        let executor =
            ScriptedExecutor::new().expect_any(ScriptedResponse::ok("skillhub 2025.1.0"));
        let manager = TaskManager::new(
            Arc::new(FakeRuntime::ok()),
            Arc::new(executor),
            Arc::new(FakeConflicts::default()),
            paths.clone(),
            TaskManagerConfig::default(),
        );
        Self {
            _tmp: tmp,
            paths,
            manager,
        }
    }
}

#[tokio::test]
async fn unavailable_marketplace_cli_only_skips_its_rows_and_keeps_the_confirmed_skip() {
    let f = Fixture::new();
    let names = vec!["git-skill".into(), "market-skill".into()];
    let preview = f
        .manager
        .preview_cli_commands(
            ScopeId::Global,
            CliCommandRequest::Update {
                skills: names.clone(),
            },
        )
        .await
        .unwrap();
    assert!(preview.marketplace_cli_required);
    assert!(
        preview
            .entries
            .iter()
            .find(|e| e.name == "git-skill")
            .unwrap()
            .command
            .is_some()
    );
    assert!(
        preview
            .entries
            .iter()
            .find(|e| e.name == "market-skill")
            .unwrap()
            .command
            .is_none()
    );
    let layout = f.manager.scope_layout(&ScopeId::Global).unwrap();
    let mut replanned = update_task::plan_updates(&f.paths, &layout, &names).unwrap();
    apply_pinned_updates(
        &f.paths,
        &layout,
        &mut replanned,
        f.manager.pinned_updates(&Some(preview.id.clone())),
    )
    .unwrap();
    assert_eq!(replanned.items.len(), 1);
    assert_eq!(replanned.skipped[0].name, "market-skill");
    assert_eq!(
        replanned
            .command_preview(&layout, FakeRuntime::ok().result.as_ref().ok())
            .id,
        preview.id
    );
}

#[test]
fn pinned_marketplace_update_retains_the_confirmed_release_without_git_runtime() {
    let f = Fixture::new();
    let layout = f.manager.scope_layout(&ScopeId::Global).unwrap();
    let names = vec!["market-skill".into()];
    let mut pinned = update_task::plan_updates(&f.paths, &layout, &names).unwrap();
    let item = &pinned.items[0];
    let record = item.marketplace_record.as_ref().unwrap().clone();
    let plan = Plan {
        detail: Detail {
            summary: Summary {
                coordinate: record.coordinate.clone(),
                name: item.name.clone(),
                publisher: "publisher".into(),
                description: "description".into(),
                version: "2.0.0".into(),
                official_url: record.coordinate.url(),
            },
            original_name: Some(item.name.clone()),
            markdown: "---\nname: market-skill\n---\nnew release".into(),
            install_error: None,
            requirements: vec![],
        },
        runtime: Runtime {
            program: "/private/skillhub".into(),
            env: BTreeMap::new(),
        },
        stage: StagingDirectory::new().unwrap(),
        layout: layout.clone(),
        targets: item.targets.clone(),
        previous: Some(record),
        previous_paths: Some(
            uninstall_task::plan_uninstall(&f.paths, &layout, "market-skill").unwrap(),
        ),
        previous_fingerprint: Some(
            crate::baseline::fingerprint_folder(&item.canonical)
                .unwrap()
                .content_hash,
        ),
        created_at: crate::time::now_ms(),
    };
    pinned.items[0].marketplace_plan = Some(plan);
    let preview = pinned.command_preview(&layout, None);
    let mut replanned = update_task::plan_updates(&f.paths, &layout, &names).unwrap();
    apply_pinned_updates(
        &f.paths,
        &layout,
        &mut replanned,
        Some((ScopeId::Global, pinned)),
    )
    .unwrap();
    assert!(!replanned.needs_cli());
    let current = replanned.command_preview(&layout, None);
    assert_eq!(current.id, preview.id);
    assert_eq!(current.marketplace_versions["market-skill"], "2.0.0");
    assert!(
        current.entries[0]
            .command
            .as_ref()
            .unwrap()
            .contains("market-skill@2.0.0 --namespace publisher")
    );
    fs::write(
        replanned.items[0].canonical.join("SKILL.md"),
        "changed after confirmation",
    )
    .unwrap();
    assert!(
        replanned.items[0]
            .marketplace_plan
            .as_ref()
            .unwrap()
            .check(&f.paths, &layout)
            .is_err()
    );
}
