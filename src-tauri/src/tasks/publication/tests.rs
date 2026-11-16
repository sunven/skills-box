use super::*;
use crate::{
    marketplace::{Coordinate, records::Installation},
    projects::ProjectRegistry,
    scope::ScopeId,
};
use std::{cell::Cell, path::PathBuf};

/// A marketplace plan that knows nothing about SkillHub: it exercises the
/// publication contract on its own.
struct FakePlan {
    layout: ScopeLayout,
    targets: Vec<InstallTarget>,
    name: String,
    record: Installation,
    previous: Option<Installation>,
    previous_paths: Option<crate::tasks::uninstall::UninstallPlan>,
    checks: Cell<u32>,
    fail_check_at: Option<u32>,
}

impl MarketplaceInstallation for FakePlan {
    fn scope(&self) -> &ScopeId {
        &self.layout.scope
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn targets(&self) -> &[InstallTarget] {
        &self.targets
    }
    fn previous(&self) -> Option<&Installation> {
        self.previous.as_ref()
    }
    fn previous_paths(&self) -> Option<&crate::tasks::uninstall::UninstallPlan> {
        self.previous_paths.as_ref()
    }
    fn check(&self, _: &AppPaths, _: &ScopeLayout) -> Result<(), AppError> {
        let checks = self.checks.get() + 1;
        self.checks.set(checks);
        if self.fail_check_at == Some(checks) {
            return Err(AppError::cli_command_changed());
        }
        Ok(())
    }
    fn installation(&self, content_hash: String) -> Installation {
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
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let paths = AppPaths::new(root.join("home"), root.join("data"));
        paths.ensure_dirs().unwrap();
        fs::create_dir_all(paths.home().join(".claude/skills")).unwrap();
        let registry = ProjectRegistry::new(paths.clone());
        let layout = ScopeLayout::resolve_detected(&paths, &registry, &ScopeId::Global).unwrap();
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
            layout: layout.clone(),
            targets,
            name: record.name.clone(),
            record,
            previous: None,
            previous_paths: None,
            checks: Cell::new(0),
            fail_check_at: None,
        };
        Self { tmp, paths, plan }
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

    let canonical = fixture.plan.layout.canonical_dir.join("fake-skill");
    assert_eq!(
        fs::read_to_string(canonical.join("notes.txt")).unwrap(),
        "second source"
    );
    let records = MarketplaceRecords::read(&fixture.plan.layout.lock_path).unwrap();
    let record = &records.skills["fake-skill"];
    assert_eq!(record.version, "3.1.4");
    assert_eq!(
        record.content_hash,
        fingerprint_folder(&canonical).unwrap().content_hash
    );
    assert!(
        BaselineStore::for_scope(&fixture.paths, &fixture.plan.layout.scope)
            .load("fake-skill")
            .unwrap()
            .verified
    );
    assert!(
        fixture.plan.targets[0]
            .skills_dir
            .join("fake-skill")
            .is_symlink()
    );
    assert!(
        fixture.plan.checks.get() >= 2,
        "publication must revalidate after preparing the candidate"
    );
    assert!(
        !fixture.plan.layout.lock_path.exists(),
        "marketplace publication must not write the Git lock"
    );
}

#[test]
fn publication_record_race_keeps_the_existing_record_baseline_and_attachments() {
    let fixture = Fixture::new();
    let mut records = MarketplaceRecords::default();
    records.skills.insert(
        "fake-skill".into(),
        Installation {
            version: "0.9.0".into(),
            content_hash: "seeded-hash".into(),
            ..fixture.plan.record.clone()
        },
    );
    records.save(&fixture.plan.layout.lock_path).unwrap();
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
    let records = MarketplaceRecords::read(&fixture.plan.layout.lock_path).unwrap();
    assert_eq!(records.skills["fake-skill"].version, "0.9.0");
    assert!(
        BaselineStore::for_scope(&fixture.paths, &fixture.plan.layout.scope)
            .load("fake-skill")
            .is_none()
    );
    assert!(
        !fixture.plan.targets[0]
            .skills_dir
            .join("fake-skill")
            .exists()
    );
    assert!(
        fixture
            .plan
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
    let canonical = fixture.plan.layout.canonical_dir.join("fake-skill");
    let installed = MarketplaceRecords::read(&fixture.plan.layout.lock_path)
        .unwrap()
        .skills["fake-skill"]
        .clone();

    fixture.plan.previous = Some(installed.clone());
    fixture.plan.previous_paths = Some(
        crate::tasks::uninstall::plan_uninstall(&fixture.paths, &fixture.plan.layout, "fake-skill")
            .unwrap(),
    );
    fixture.plan.fail_check_at = Some(2);
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
        crate::error::ErrorCode::CliCommandChanged
    );
    assert_eq!(
        fs::read_to_string(canonical.join("notes.txt")).unwrap(),
        "first"
    );
    let records = MarketplaceRecords::read(&fixture.plan.layout.lock_path).unwrap();
    assert_eq!(records.skills["fake-skill"].version, "3.1.4");
    assert_eq!(
        BaselineStore::for_scope(&fixture.paths, &fixture.plan.layout.scope)
            .load("fake-skill")
            .unwrap()
            .content_hash,
        installed.content_hash
    );
    assert!(
        fixture.plan.targets[0]
            .skills_dir
            .join("fake-skill")
            .is_symlink()
    );
}
