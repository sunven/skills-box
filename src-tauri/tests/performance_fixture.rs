//! 100-skill inventory fixture and performance budget.
//!
//! The fixture mirrors the acceptance matrix's scale case: 100 skills across
//! 20 sources, mounted into four detected agent directories (two mounts per
//! skill), with 20 copy-only skills that have no canonical folder and 80
//! managed symlink skills. It is generated in a temp HOME and removed
//! afterwards; the real HOME is never read.
//!
//! The spec thresholds (cold < 1.5 s, warm < 500 ms, detail < 150 ms) are for
//! a release `.app`. This test runs in the debug profile, so passing here is
//! a conservative lower bound; the measured numbers are printed for the
//! acceptance record.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use skills_box_lib::inventory::{self, ManagementState};
use skills_box_lib::paths::AppPaths;
use skills_box_lib::projects::ProjectRegistry;
use skills_box_lib::read_model::ReadModel;

const SKILLS: usize = 100;
const SOURCES: usize = 20;
/// Every fourth skill (indices 0, 5, 10, ...) is copy-only.
const COPY_EVERY: usize = 5;

const AGENTS: &[(&str, &str)] = &[
    ("claude-code", ".claude"),
    ("qoder", ".qoder"),
    ("gemini-cli", ".gemini"),
    ("cursor", ".cursor"),
];

/// Project scans only non-universal catalog dirs; cursor/gemini are universal
/// and read `<root>/.agents/skills` directly.
const PROJECT_AGENTS: &[(&str, &str)] = &[
    ("claude-code", ".claude"),
    ("qoder", ".qoder"),
    ("augment", ".augment"),
    ("continue", ".continue"),
];

fn skill_name(index: usize) -> String {
    format!("perf-skill-{index:03}")
}

fn source_name(index: usize) -> String {
    format!("acme/source-{:02}", index % SOURCES)
}

fn write_skill(dir: &Path, name: &str, index: usize) {
    fs::create_dir_all(dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!(
            "---\nname: {name}\ndescription: Performance fixture skill {index} for the inventory scan.\n---\n\n# {name}\n\n{} lines of body text.\n{}\n",
            index % 17 + 1,
            "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(8)
        ),
    )
    .unwrap();
}

fn build_fixture(paths: &AppPaths) -> Vec<String> {
    let canonical = paths.canonical_skills_dir();
    fs::create_dir_all(&canonical).unwrap();
    let mut names = Vec::with_capacity(SKILLS);
    let mut lock_skills = serde_json::Map::new();

    for (_, home_dir) in AGENTS {
        fs::create_dir_all(paths.home().join(home_dir).join("skills")).unwrap();
    }

    for index in 0..SKILLS {
        let name = skill_name(index);
        names.push(name.clone());
        let source = source_name(index);

        if index % COPY_EVERY == 0 {
            // Copy-only: no canonical folder, real files in two agent dirs.
            for slot in 0..2 {
                let (_, home_dir) = AGENTS[(index + slot) % AGENTS.len()];
                write_skill(
                    &paths.home().join(home_dir).join("skills").join(&name),
                    &name,
                    index,
                );
            }
        } else {
            let target = canonical.join(&name);
            write_skill(&target, &name, index);
            for slot in 0..2 {
                let (_, home_dir) = AGENTS[(index + slot) % AGENTS.len()];
                let link = paths.home().join(home_dir).join("skills").join(&name);
                if link.symlink_metadata().is_err() {
                    symlink(&target, &link).unwrap();
                }
            }
        }

        lock_skills.insert(
            name.clone(),
            json!({
                "source": source,
                "sourceType": "github",
                "sourceUrl": format!("https://github.com/{source}.git"),
                "skillPath": format!("skills/{name}/SKILL.md"),
                "skillFolderHash": format!("{index:040x}"),
                "installedAt": "2026-01-02T03:04:05.000Z",
                "updatedAt": "2026-02-03T04:05:06.000Z"
            }),
        );
    }

    let lock = json!({
        "version": 3,
        "skills": lock_skills,
        "dismissed": {}
    });
    fs::create_dir_all(paths.lock_path().parent().unwrap()).unwrap();
    fs::write(
        paths.lock_path(),
        serde_json::to_string_pretty(&lock).unwrap(),
    )
    .unwrap();
    names
}

fn median(samples: &[Duration]) -> Duration {
    let mut values: Vec<Duration> = samples.to_vec();
    values.sort();
    values[values.len() / 2]
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

#[test]
fn one_hundred_skill_fixture_meets_the_inventory_budget() {
    let temp = tempfile::tempdir().unwrap();
    let home: PathBuf = temp.path().join("home");
    let app_data = temp.path().join("app-data");
    let paths = AppPaths::new(&home, &app_data);
    paths.ensure_dirs().unwrap();
    let names = build_fixture(&paths);

    let cold_started = Instant::now();
    let snapshot = inventory::read(&paths).unwrap();
    let cold = cold_started.elapsed();

    assert_eq!(
        snapshot.skills.len(),
        SKILLS,
        "fixture must expose 100 rows"
    );
    let managed = snapshot
        .skills
        .iter()
        .filter(|skill| skill.management == ManagementState::Managed)
        .count();
    let copy_only = snapshot
        .skills
        .iter()
        .filter(|skill| skill.management == ManagementState::CopyOnly)
        .count();
    assert_eq!(managed, 80, "managed symlink skills");
    assert_eq!(copy_only, 20, "copy-only skills");
    let detected_agents = snapshot
        .agents
        .iter()
        .filter(|agent| agent.detected)
        .count();
    assert_eq!(
        detected_agents,
        AGENTS.len(),
        "four fixture agents must be detected"
    );
    assert_eq!(
        snapshot.anomalies.stale.count, 0,
        "every lock entry has content: {:?}",
        snapshot.anomalies
    );
    assert_eq!(snapshot.anomalies.broken.count, 0);
    assert_eq!(snapshot.anomalies.unmanaged.count, 0);
    // Synthetic lock hashes cannot match real git tree SHAs, so the first
    // scan legitimately reports "local modification unknown" for these
    // fixtures. They still exercise the anomaly grouping path; the fixture is
    // intentionally not used to assert modification semantics.
    println!(
        "fixture anomalies: total={} modified={}",
        snapshot.anomalies.total, snapshot.anomalies.modified.count
    );

    let mut warm_samples = Vec::new();
    for _ in 0..5 {
        let started = Instant::now();
        let warm = inventory::read(&paths).unwrap();
        warm_samples.push(started.elapsed());
        assert_eq!(warm.skills.len(), SKILLS);
    }
    let warm = median(&warm_samples);

    let mut detail_samples = Vec::new();
    for _ in 0..5 {
        let skill_id = &names[1];
        let started = Instant::now();
        let detail = skills_box_lib::detail::read(&paths, skill_id).unwrap();
        detail_samples.push(started.elapsed());
        assert_eq!(detail.name, *skill_id);
    }
    let detail = median(&detail_samples);

    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    println!(
        "100-skill fixture ({profile}): cold={:.1}ms warm_median={:.1}ms detail_median={:.1}ms managed={managed} copy_only={copy_only} agents={}",
        millis(cold),
        millis(warm),
        millis(detail),
        snapshot.agents.len()
    );

    assert!(
        cold < Duration::from_millis(1500),
        "cold inventory {:.1}ms exceeds the 1.5s budget",
        millis(cold)
    );
    assert!(
        warm < Duration::from_millis(500),
        "warm inventory {:.1}ms exceeds the 500ms budget",
        millis(warm)
    );
    assert!(
        detail < Duration::from_millis(150),
        "detail read {:.1}ms exceeds the 150ms budget",
        millis(detail)
    );
}

/// 100 project skills with a v1 `skills-lock.json` under a temp project root.
/// Catalog agent directories are the four fixture agents mapped into the
/// project; the project root itself is registered through the real registry.
fn build_project_fixture(root: &Path) -> Vec<String> {
    fs::create_dir_all(root.join(".agents/skills")).unwrap();
    let mut names = Vec::with_capacity(SKILLS);
    let mut lock_skills = serde_json::Map::new();

    for (_, agent_dir) in PROJECT_AGENTS {
        fs::create_dir_all(root.join(agent_dir).join("skills")).unwrap();
    }

    for index in 0..SKILLS {
        let name = skill_name(index);
        names.push(name.clone());
        let source = source_name(index);

        if index % COPY_EVERY == 0 {
            // Copy-only in the project: no canonical folder, real files in
            // two agent project dirs.
            for slot in 0..2 {
                let (_, agent_dir) = PROJECT_AGENTS[(index + slot) % PROJECT_AGENTS.len()];
                write_skill(
                    &root.join(agent_dir).join("skills").join(&name),
                    &name,
                    index,
                );
            }
        } else {
            let target = root.join(".agents/skills").join(&name);
            write_skill(&target, &name, index);
            for slot in 0..2 {
                let (_, agent_dir) = PROJECT_AGENTS[(index + slot) % PROJECT_AGENTS.len()];
                let link = root.join(agent_dir).join("skills").join(&name);
                if link.symlink_metadata().is_err() {
                    symlink(&target, &link).unwrap();
                }
            }
        }

        lock_skills.insert(
            name.clone(),
            json!({
                "source": source,
                "sourceType": "github",
                "sourceUrl": format!("https://github.com/{source}.git"),
                "skillPath": format!("skills/{name}/SKILL.md"),
                "computedHash": format!("{:064x}", index),
            }),
        );
    }

    let lock = json!({"version": 1, "skills": lock_skills});
    fs::write(
        root.join("skills-lock.json"),
        serde_json::to_string_pretty(&lock).unwrap(),
    )
    .unwrap();
    names
}

#[test]
fn one_hundred_project_skills_meet_the_project_inventory_budget() {
    let temp = tempfile::tempdir().unwrap();
    let home: PathBuf = temp.path().join("home");
    fs::create_dir_all(&home).unwrap();
    let app_data = temp.path().join("app-data");
    let paths = AppPaths::new(&home, &app_data);
    paths.ensure_dirs().unwrap();
    let root = temp.path().join("projects/app");
    fs::create_dir_all(&root).unwrap();
    let names = build_project_fixture(&root);

    let registry = Arc::new(ProjectRegistry::with_clock(
        paths.clone(),
        Arc::new(|| 1_000),
    ));
    let entry = registry.add(root.to_str().unwrap()).unwrap();
    let scope = entry.project.scope();
    let model = ReadModel::with_projects(paths.clone(), Arc::clone(&registry));

    let cold_started = Instant::now();
    let snapshot = model.snapshot(&scope).unwrap();
    let cold = cold_started.elapsed();

    assert_eq!(snapshot.skills.len(), SKILLS, "fixture exposes 100 rows");
    let managed = snapshot
        .skills
        .iter()
        .filter(|skill| skill.management == ManagementState::Managed)
        .count();
    let copy_only = snapshot
        .skills
        .iter()
        .filter(|skill| skill.management == ManagementState::CopyOnly)
        .count();
    assert_eq!(managed, 80, "managed canonical project skills");
    assert_eq!(copy_only, 20, "copy-only project skills");
    let detected_agents = snapshot
        .agents
        .iter()
        .filter(|agent| agent.detected)
        .count();
    assert_eq!(
        detected_agents,
        PROJECT_AGENTS.len(),
        "four project agent directories must be scanned"
    );

    let mut warm_samples = Vec::new();
    for _ in 0..5 {
        let started = Instant::now();
        let warm = model.snapshot(&scope).unwrap();
        warm_samples.push(started.elapsed());
        assert_eq!(warm.skills.len(), SKILLS);
    }
    let warm = median(&warm_samples);

    let mut detail_samples = Vec::new();
    for _ in 0..5 {
        let skill_id = scope.skill_id(&names[1]);
        let started = Instant::now();
        let detail = model.detail(&scope, &skill_id).unwrap();
        detail_samples.push(started.elapsed());
        assert_eq!(detail.name, names[1]);
    }
    let detail = median(&detail_samples);

    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    println!(
        "100-project-skill fixture ({profile}): cold={:.1}ms warm_median={:.1}ms detail_median={:.1}ms managed={managed} copy_only={copy_only} agents={}",
        millis(cold),
        millis(warm),
        millis(detail),
        snapshot.agents.len()
    );

    assert!(
        cold < Duration::from_millis(1500),
        "cold project inventory {:.1}ms exceeds the 1.5s budget",
        millis(cold)
    );
    assert!(
        warm < Duration::from_millis(500),
        "warm project inventory {:.1}ms exceeds the 500ms budget",
        millis(warm)
    );
    assert!(
        detail < Duration::from_millis(150),
        "project detail read {:.1}ms exceeds the 150ms budget",
        millis(detail)
    );
}
