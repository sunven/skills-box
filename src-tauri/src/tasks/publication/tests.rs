use super::*;
use crate::{
    error::ErrorCode,
    marketplace::{Coordinate, records::Installation},
    projects::ProjectRegistry,
    scope::ScopeId,
};
use std::{cell::Cell, path::PathBuf};

/// A marketplace plan that knows nothing about SkillHub: it exercises the
/// publication contract on its own.
struct FakePlan {
    publication: Plan,
    record: Installation,
    checks: Cell<u32>,
    before_check: Option<Box<dyn Fn(u32)>>,
    before_record: Option<Box<dyn Fn()>>,
}

impl MarketplaceInstallation for FakePlan {
    fn publication(&self) -> &Plan {
        &self.publication
    }
    fn check_staged(&self) -> Result<(), AppError> {
        let checks = self.checks.get() + 1;
        self.checks.set(checks);
        if let Some(before_check) = &self.before_check {
            before_check(checks);
        }
        Ok(())
    }
    fn installation(&self, content_hash: String) -> Installation {
        if !content_hash.is_empty()
            && let Some(before_record) = &self.before_record
        {
            before_record();
        }
        Installation {
            content_hash,
            ..self.record.clone()
        }
    }
}

struct Fixture {
    tmp: tempfile::TempDir,
    paths: AppPaths,
    plan: FakePlan,
}

impl Fixture {
    fn new() -> Self {
        Self::scoped(false)
    }

    fn scoped(project: bool) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let paths = AppPaths::new(root.join("home"), root.join("data"));
        paths.ensure_dirs().unwrap();
        fs::create_dir_all(paths.home().join(".claude/skills")).unwrap();
        let registry = ProjectRegistry::new(paths.clone());
        let scope = if project {
            let root = root.join("project");
            fs::create_dir_all(&root).unwrap();
            registry
                .add(root.to_str().unwrap())
                .unwrap()
                .project
                .scope()
        } else {
            ScopeId::Global
        };
        let layout = ScopeLayout::resolve_detected(&paths, &registry, &scope).unwrap();
        let (_, targets) = crate::tasks::install::validate_selected_agents(
            &paths,
            &layout,
            &["claude-code".into()],
        )
        .unwrap();
        let coordinate = Coordinate::from_canonical("@publisher/fake-slug").unwrap();
        let record = Installation {
            name: "fake-skill".into(),
            coordinate,
            version: "3.1.4".into(),
            content_hash: String::new(),
            installed_at: "Mon, 16 Nov 2026 03:00:00 GMT".into(),
            updated_at: "Mon, 16 Nov 2026 03:00:00 GMT".into(),
            diagnostics: None,
        };
        let plan = FakePlan {
            publication: Plan {
                layout,
                targets,
                name: record.name.clone(),
                coordinate: record.coordinate.clone(),
                previous: None,
                previous_paths: None,
                previous_fingerprint: None,
            },
            record,
            checks: Cell::new(0),
            before_check: None,
            before_record: None,
        };
        Self { tmp, paths, plan }
    }

    fn current_layout(&self) -> ScopeLayout {
        ScopeLayout::resolve_detected(
            &self.paths,
            &ProjectRegistry::new(self.paths.clone()),
            &self.plan.publication.layout.scope,
        )
        .unwrap()
    }

    fn check(&self) -> Result<(), AppError> {
        check(&self.paths, &self.current_layout(), &self.plan)
    }

    fn install(&self) {
        let root = stage(self, "first", "---\nname: fake-skill\n---\n", "first");
        publish(
            &self.paths,
            Publication::Marketplace {
                plan: &self.plan,
                root: &root,
            },
        )
        .result
        .unwrap();
    }

    fn prepare_update(&mut self) {
        self.plan.publication.layout = self.current_layout();
        let plan = &mut self.plan.publication;
        plan.previous = MarketplaceRecords::read(&plan.layout.lock_path)
            .unwrap()
            .skills
            .get(&plan.name)
            .cloned();
        plan.previous_paths = Some(
            super::super::uninstall::plan_uninstall(
                &self.paths,
                &plan.layout,
                &plan.layout.scope.skill_id(&plan.name),
            )
            .unwrap(),
        );
        plan.previous_fingerprint = Some(
            fingerprint_folder(&plan.layout.canonical_dir.join(&plan.name))
                .unwrap()
                .content_hash,
        );
        self.plan.checks.set(0);
        self.plan.record.version = "3.2.0".into();
    }
}

fn stage(fixture: &Fixture, directory: &str, markdown: &str, notes: &str) -> PathBuf {
    let root = fixture.tmp.path().join(directory);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("SKILL.md"), markdown).unwrap();
    fs::write(root.join("notes.txt"), notes).unwrap();
    root
}

#[test]
fn install_rejects_occupied_names_stale_records_and_duplicate_coordinates() {
    for project in [false, true] {
        let fixture = Fixture::scoped(project);
        let plan = &fixture.plan.publication;
        fixture.check().unwrap();
        let canonical = plan.layout.canonical_dir.join(&plan.name);
        fs::create_dir_all(&canonical).unwrap();
        fs::write(canonical.join("SKILL.md"), "---\nname: fake-skill\n---\n").unwrap();
        assert_eq!(fixture.check().unwrap_err().code, ErrorCode::SkillConflict);
        fs::remove_dir_all(&canonical).unwrap();

        let mut records = MarketplaceRecords::default();
        let record = Installation {
            content_hash: "recorded-hash".into(),
            ..fixture.plan.record.clone()
        };
        records.skills.insert(plan.name.clone(), record.clone());
        records.save(&plan.layout.lock_path).unwrap();
        assert_eq!(
            fixture.check().unwrap_err().code,
            ErrorCode::CliCommandChanged
        );

        records.skills.clear();
        records.skills.insert(
            "another-name".into(),
            Installation {
                name: "another-name".into(),
                ..record
            },
        );
        records.save(&plan.layout.lock_path).unwrap();
        assert_eq!(fixture.check().unwrap_err().code, ErrorCode::SkillConflict);
        assert!(!canonical.exists());
    }
}

#[test]
fn update_rejects_changed_records_content_attachments_and_physical_paths() {
    for project in [false, true] {
        for change in ["record", "content", "attachment", "canonical"] {
            let mut fixture = Fixture::scoped(project);
            fixture.install();
            fixture.prepare_update();
            fixture.check().unwrap();
            let plan = &fixture.plan.publication;
            let canonical = plan.layout.canonical_dir.join(&plan.name);
            let attachment = plan.targets[0].skills_dir.join(&plan.name);
            match change {
                "record" => {
                    let mut records = MarketplaceRecords::read(&plan.layout.lock_path).unwrap();
                    records.skills.get_mut(&plan.name).unwrap().version = "4.0.0".into();
                    records.save(&plan.layout.lock_path).unwrap();
                }
                "content" => fs::write(canonical.join("notes.txt"), "local edit").unwrap(),
                "attachment" => {
                    fs::remove_file(&attachment).unwrap();
                    crate::backup::copy_tree(&canonical, &attachment).unwrap();
                }
                "canonical" => {
                    let moved = plan.layout.canonical_dir.join("moved");
                    fs::rename(&canonical, &moved).unwrap();
                    std::os::unix::fs::symlink(&moved, &canonical).unwrap();
                }
                _ => unreachable!(),
            }
            let root = stage(&fixture, "second", "---\nname: fake-skill\n---\n", "second");
            let outcome = publish(
                &fixture.paths,
                Publication::Marketplace {
                    plan: &fixture.plan,
                    root: &root,
                },
            );
            assert_eq!(
                outcome.result.unwrap_err().code,
                if change == "record" {
                    ErrorCode::CliCommandChanged
                } else {
                    ErrorCode::BatchTargetChanged
                },
                "project={project}, change={change}"
            );
            assert_eq!(
                fs::read_to_string(canonical.join("notes.txt")).unwrap(),
                if change == "content" {
                    "local edit"
                } else {
                    "first"
                }
            );
            assert_eq!(
                BaselineStore::for_scope(&fixture.paths, &plan.layout.scope)
                    .load(&plan.name)
                    .unwrap()
                    .content_hash,
                plan.previous.as_ref().unwrap().content_hash
            );
            assert_eq!(attachment.is_symlink(), change != "attachment");
            assert_eq!(canonical.is_symlink(), change == "canonical");
        }
    }
}

#[test]
fn relocate_invalidates_publication_even_after_returning_to_the_original_root() {
    let fixture = Fixture::scoped(true);
    fixture.check().unwrap();
    let layout = &fixture.plan.publication.layout;
    let registry = ProjectRegistry::new(fixture.paths.clone());
    let id = layout.project.as_ref().unwrap().id.to_string();
    let other = fixture.tmp.path().join("relocated");
    fs::create_dir_all(&other).unwrap();
    registry.relocate(&id, other.to_str().unwrap()).unwrap();
    assert_eq!(
        fixture.check().unwrap_err().code,
        ErrorCode::BatchTargetChanged
    );
    registry
        .relocate(&id, layout.root.to_str().unwrap())
        .unwrap();
    assert_eq!(fixture.current_layout().root, layout.root);
    assert_eq!(
        fixture.check().unwrap_err().code,
        ErrorCode::BatchTargetChanged
    );
    assert!(!layout.canonical_dir.exists());
    assert!(!other.join(".agents/skills").exists());
}

#[test]
fn common_validation_preserves_error_precedence() {
    let mut fixture = Fixture::scoped(true);
    fixture.install();
    fixture.prepare_update();
    let layout = fixture.current_layout();
    let mut records = MarketplaceRecords::read(&layout.lock_path).unwrap();
    let original = records.clone();
    records.skills.get_mut("fake-skill").unwrap().version = "4.0.0".into();
    records.skills.get_mut("fake-skill").unwrap().coordinate =
        Coordinate::from_canonical("@publisher/replaced").unwrap();
    records.skills.insert(
        "other-name".into(),
        Installation {
            name: "other-name".into(),
            ..original.skills["fake-skill"].clone()
        },
    );
    records.save(&layout.lock_path).unwrap();
    fs::write(layout.lock_path.clone(), "corrupt Git lock").unwrap();
    fs::write(
        layout.canonical_dir.join("fake-skill/notes.txt"),
        "local edit",
    )
    .unwrap();
    ProjectRegistry::new(fixture.paths.clone())
        .relocate(
            &layout.project.as_ref().unwrap().id.to_string(),
            layout.root.to_str().unwrap(),
        )
        .unwrap();

    assert_eq!(fixture.check().unwrap_err().code, ErrorCode::LockCorrupt);
    fs::remove_file(&layout.lock_path).unwrap();
    assert_eq!(fixture.check().unwrap_err().code, ErrorCode::SkillConflict);
    records.skills.remove("other-name");
    records.save(&layout.lock_path).unwrap();
    assert_eq!(
        fixture.check().unwrap_err().code,
        ErrorCode::BatchTargetChanged
    );
    fixture.plan.publication.layout = fixture.current_layout();
    assert_eq!(
        fixture.check().unwrap_err().code,
        ErrorCode::CliCommandChanged
    );
    original.save(&layout.lock_path).unwrap();
    assert_eq!(
        fixture.check().unwrap_err().code,
        ErrorCode::BatchTargetChanged
    );
}

#[test]
fn new_install_requires_detection_but_update_retains_project_attachments() {
    for updating in [false, true] {
        let mut fixture = Fixture::scoped(true);
        if updating {
            fixture.install();
            fixture.prepare_update();
        }
        fs::remove_dir_all(fixture.paths.home().join(".claude")).unwrap();
        if updating {
            fixture.check().unwrap();
            let root = stage(&fixture, "second", "---\nname: fake-skill\n---\n", "second");
            publish(
                &fixture.paths,
                Publication::Marketplace {
                    plan: &fixture.plan,
                    root: &root,
                },
            )
            .result
            .unwrap();
            let attachment = fixture.plan.publication.targets[0]
                .skills_dir
                .join("fake-skill");
            assert!(attachment.is_symlink());
            assert_eq!(
                fs::read_to_string(attachment.join("notes.txt")).unwrap(),
                "second"
            );
        } else {
            assert_eq!(
                fixture.check().unwrap_err().code,
                ErrorCode::AgentNotDetected
            );
        }
    }
}

#[test]
fn install_rejects_redirected_canonical_and_agent_directories() {
    for project in [false, true] {
        for canonical in [false, true] {
            let fixture = Fixture::scoped(project);
            let layout = &fixture.plan.publication.layout;
            let directory = if canonical {
                layout.canonical_dir.clone()
            } else {
                fixture.plan.publication.targets[0].skills_dir.clone()
            };
            let destination = if canonical && project {
                fixture.tmp.path().join("outside-project")
            } else {
                layout.root.join("redirected")
            };
            fs::create_dir_all(&destination).unwrap();
            fs::create_dir_all(directory.parent().unwrap()).unwrap();
            if directory.exists() {
                fs::remove_dir(&directory).unwrap();
            }
            std::os::unix::fs::symlink(&destination, &directory).unwrap();
            assert_eq!(
                fixture.check().unwrap_err().code,
                if canonical && project {
                    ErrorCode::PathOutsideAllowedRoot
                } else {
                    ErrorCode::PathInvalid
                },
                "project={project}, canonical={canonical}"
            );
            assert!(fs::read_dir(destination).unwrap().next().is_none());
        }
    }
}

#[test]
fn source_neutral_publication_publishes_a_fake_marketplace_plan() {
    let fixture = Fixture::new();
    let root = stage(
        &fixture,
        "staged",
        "---\nname: fake-skill\ndescription: test\n---\n# Fake\n",
        "second source",
    );
    let outcome = publish(
        &fixture.paths,
        Publication::Marketplace {
            plan: &fixture.plan,
            root: &root,
        },
    );
    outcome.result.unwrap();
    assert!(outcome.warnings.is_empty());

    let canonical = fixture
        .plan
        .publication
        .layout
        .canonical_dir
        .join("fake-skill");
    assert_eq!(
        fs::read_to_string(canonical.join("notes.txt")).unwrap(),
        "second source"
    );
    let records = MarketplaceRecords::read(&fixture.plan.publication.layout.lock_path).unwrap();
    let record = &records.skills["fake-skill"];
    assert_eq!(record.version, "3.1.4");
    assert_eq!(
        record.content_hash,
        fingerprint_folder(&canonical).unwrap().content_hash
    );
    assert!(
        BaselineStore::for_scope(&fixture.paths, &fixture.plan.publication.layout.scope)
            .load("fake-skill")
            .unwrap()
            .verified
    );
    assert!(
        fixture.plan.publication.targets[0]
            .skills_dir
            .join("fake-skill")
            .is_symlink()
    );
    assert!(
        fixture.plan.checks.get() >= 2,
        "publication must revalidate after preparing the candidate"
    );
    assert!(
        !fixture.plan.publication.layout.lock_path.exists(),
        "marketplace publication must not write the Git lock"
    );
}

#[test]
fn publication_record_race_keeps_the_existing_record_baseline_and_attachments() {
    let mut fixture = Fixture::new();
    let mut records = MarketplaceRecords::default();
    records.skills.insert(
        "fake-skill".into(),
        Installation {
            version: "0.9.0".into(),
            content_hash: "seeded-hash".into(),
            ..fixture.plan.record.clone()
        },
    );
    let lock_path = fixture.plan.publication.layout.lock_path.clone();
    fixture.plan.before_record = Some(Box::new(move || records.save(&lock_path).unwrap()));
    let root = stage(
        &fixture,
        "staged",
        "---\nname: fake-skill\ndescription: test\n---\n# Fake\n",
        "second source",
    );
    let outcome = publish(
        &fixture.paths,
        Publication::Marketplace {
            plan: &fixture.plan,
            root: &root,
        },
    );
    let error = outcome.result.unwrap_err();
    assert_eq!(error.code, crate::error::ErrorCode::VerifyFailed);
    assert!(
        error
            .detail
            .as_deref()
            .unwrap_or_default()
            .contains("安装记录在发布期间变化")
    );
    let records = MarketplaceRecords::read(&fixture.plan.publication.layout.lock_path).unwrap();
    assert_eq!(records.skills["fake-skill"].version, "0.9.0");
    assert!(
        BaselineStore::for_scope(&fixture.paths, &fixture.plan.publication.layout.scope)
            .load("fake-skill")
            .is_none()
    );
    assert!(
        !fixture.plan.publication.targets[0]
            .skills_dir
            .join("fake-skill")
            .exists()
    );
    assert!(
        fixture
            .plan
            .publication
            .layout
            .canonical_dir
            .join("fake-skill/notes.txt")
            .is_file(),
        "replacement happens before the record race check; the caller owns recovery backups"
    );
}

#[test]
fn failed_publication_revalidation_keeps_the_existing_installation() {
    let mut fixture = Fixture::new();
    let first = stage(
        &fixture,
        "first",
        "---\nname: fake-skill\ndescription: test\n---\n# Fake\n",
        "first",
    );
    publish(
        &fixture.paths,
        Publication::Marketplace {
            plan: &fixture.plan,
            root: &first,
        },
    )
    .result
    .unwrap();
    let canonical = fixture
        .plan
        .publication
        .layout
        .canonical_dir
        .join("fake-skill");
    let installed = MarketplaceRecords::read(&fixture.plan.publication.layout.lock_path)
        .unwrap()
        .skills["fake-skill"]
        .clone();

    fixture.plan.publication.previous = Some(installed.clone());
    fixture.plan.publication.previous_paths = Some(
        crate::tasks::uninstall::plan_uninstall(
            &fixture.paths,
            &fixture.plan.publication.layout,
            "fake-skill",
        )
        .unwrap(),
    );
    fixture.plan.publication.previous_fingerprint = Some(installed.content_hash.clone());
    let changed = canonical.join("notes.txt");
    let store = fixture.plan.publication.layout.canonical_dir.clone();
    fixture.plan.before_check = Some(Box::new(move |checks| {
        if checks == 2 {
            assert!(fs::read_dir(&store).unwrap().any(|entry| {
                fs::read_to_string(entry.unwrap().path().join("candidate/notes.txt"))
                    .is_ok_and(|content| content == "second")
            }));
            fs::write(&changed, "concurrent edit").unwrap();
        }
    }));
    fixture.plan.checks.set(0);
    fixture.plan.record.version = "3.2.0".into();
    let second = stage(
        &fixture,
        "second",
        "---\nname: fake-skill\ndescription: test\n---\n# Fake\n\nSecond\n",
        "second",
    );
    let outcome = publish(
        &fixture.paths,
        Publication::Marketplace {
            plan: &fixture.plan,
            root: &second,
        },
    );
    assert_eq!(
        outcome.result.unwrap_err().code,
        crate::error::ErrorCode::BatchTargetChanged
    );
    assert_eq!(
        fs::read_to_string(canonical.join("notes.txt")).unwrap(),
        "concurrent edit"
    );
    let records = MarketplaceRecords::read(&fixture.plan.publication.layout.lock_path).unwrap();
    assert_eq!(records.skills["fake-skill"].version, "3.1.4");
    assert_eq!(
        BaselineStore::for_scope(&fixture.paths, &fixture.plan.publication.layout.scope)
            .load("fake-skill")
            .unwrap()
            .content_hash,
        installed.content_hash
    );
    assert!(
        fixture.plan.publication.targets[0]
            .skills_dir
            .join("fake-skill")
            .is_symlink()
    );
}

#[test]
fn marketplace_update_replaces_universal_copies_with_verified_attachments() {
    for source in [
        crate::marketplace::MarketplaceSource::Skillhub,
        crate::marketplace::MarketplaceSource::Clawhub,
    ] {
        let mut fixture = Fixture::new();
        fixture.plan.record.coordinate.source = source;
        fixture.plan.publication.coordinate.source = source;
        let first = stage(
            &fixture,
            "first",
            "---\nname: fake-skill\ndescription: test\n---\n",
            "first",
        );
        publish(
            &fixture.paths,
            Publication::Marketplace {
                plan: &fixture.plan,
                root: &first,
            },
        )
        .result
        .unwrap();
        let gemini = fixture.paths.home().join(".gemini/skills/fake-skill");
        fs::create_dir_all(&gemini).unwrap();
        fs::write(gemini.join("SKILL.md"), "old copy").unwrap();
        fs::write(gemini.join("notes.txt"), "local notes").unwrap();
        fixture.plan.publication.layout = ScopeLayout::resolve_detected(
            &fixture.paths,
            &ProjectRegistry::new(fixture.paths.clone()),
            &ScopeId::Global,
        )
        .unwrap();
        let update = crate::tasks::update::plan_updates(
            &fixture.paths,
            &fixture.plan.publication.layout,
            &["fake-skill".into()],
        )
        .unwrap()
        .items
        .remove(0);
        assert!(
            update
                .targets
                .iter()
                .any(|target| target.id == "gemini-cli" && target.universal)
        );
        assert!(update.copy_agents.iter().any(|name| name == "Gemini CLI"));
        let snapshot = crate::backup::BackupStore::for_layout(
            &fixture.paths,
            &fixture.plan.publication.layout,
        )
        .create(
            crate::backup::BackupAction::Update,
            &update.name,
            Some(&update.canonical),
            &update.attachments,
            &update.agent_ids,
        )
        .unwrap();
        fixture.plan.publication.previous_fingerprint =
            Some(fingerprint_folder(&update.canonical).unwrap().content_hash);
        fixture.plan.publication.previous = update.marketplace_record;
        fixture.plan.publication.previous_paths = Some(
            crate::tasks::uninstall::plan_uninstall(
                &fixture.paths,
                &fixture.plan.publication.layout,
                "fake-skill",
            )
            .unwrap(),
        );
        fixture.plan.publication.targets = update.targets;
        let second = stage(
            &fixture,
            "second",
            "---\nname: fake-skill\ndescription: test\n---\n",
            "updated",
        );
        publish(
            &fixture.paths,
            Publication::Marketplace {
                plan: &fixture.plan,
                root: &second,
            },
        )
        .result
        .unwrap();
        assert!(gemini.is_symlink());
        assert_eq!(
            fs::read_to_string(gemini.join("notes.txt")).unwrap(),
            "updated"
        );
        assert_eq!(
            fs::read_to_string(snapshot.path.join("attachments/gemini-cli/notes.txt")).unwrap(),
            "local notes"
        );
    }
}
