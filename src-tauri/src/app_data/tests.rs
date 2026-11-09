use super::*;
use crate::{
    backup::{BackupAction, BackupStore},
    settings::SettingsService,
    tasks::{TaskAction, TaskOverall, TaskResult, TaskSkillResult, TaskSkillStatus},
};
use std::os::unix::fs::symlink;

fn fixture() -> (tempfile::TempDir, AppPaths, DataService) {
    let tmp = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(tmp.path().join("home"), tmp.path().join("data"));
    paths.ensure_dirs().unwrap();
    (tmp, paths, DataService::default())
}
fn backup(paths: &AppPaths, name: &str) -> PathBuf {
    BackupStore::new(paths)
        .create(BackupAction::Uninstall, name, None, &[], &[])
        .unwrap()
        .path
}
fn cache(paths: &AppPaths, name: &str) -> PathBuf {
    let path = paths.cache_dir().join(format!("update-tree-{name}.json"));
    fs::write(&path, b"cached tree").unwrap();
    path
}
fn marker(paths: &AppPaths, snapshot: &Path) {
    MarkerStore::new(paths)
        .begin(&crate::interrupted::InterruptedMutation {
            task_id: "interrupted".into(),
            kind: crate::interrupted::MutationKind::Uninstall,
            scope: crate::scope::ScopeId::Global,
            lock_path: None,
            skills: vec!["alpha".into()],
            started_at_epoch_ms: 1,
            snapshot_paths: vec![snapshot.to_string_lossy().into()],
        })
        .unwrap();
}
#[test]
fn reports_categories_and_deletes_only_confirmed_app_files() {
    let (_tmp, paths, service) = fixture();
    let old = backup(&paths, "alpha");
    let tree = cache(&paths, "one");
    let history = paths.app_data().join("discovery.json");
    fs::write(&history, b"read and ignored decisions").unwrap();
    fs::write(paths.app_data().join("unknown.json"), b"unknown").unwrap();
    fs::create_dir_all(paths.canonical_skills_dir()).unwrap();
    let skill = paths.canonical_skills_dir().join("original.txt");
    fs::write(&skill, b"user skill").unwrap();
    let report = service.report(&paths).unwrap();
    assert!(report.total_bytes > report.cleanable_bytes);
    assert_eq!(report.backups.len(), 1);
    let preview = service
        .preview(
            &paths,
            Selection {
                cache: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(preview.items.len(), 1);
    let later = cache(&paths, "later");
    let result = service.clean(&paths, &preview.id).unwrap();
    assert!(result.items.iter().all(|i| i.error.is_none()));
    assert_eq!(result.freed_bytes, 11);
    assert!(!tree.exists());
    for retained in [old, history, skill, later] {
        assert!(retained.exists());
    }
    assert!(service.clean(&paths, &preview.id).is_err());
}
#[test]
fn backup_folder_metadata_is_other_data_and_cannot_be_opened_as_a_snapshot() {
    let (_tmp, paths, service) = fixture();
    let snapshot = backup(&paths, "alpha");
    let project = paths
        .backups_dir()
        .join(format!("project-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&project).unwrap();
    let files = [
        paths.backups_dir().join(".DS_Store"),
        project.join(".DS_Store"),
        paths.backups_dir().join("notes.txt"),
    ];
    for path in &files {
        fs::write(path, b"metadata").unwrap();
    }
    let report = service.report(&paths).unwrap();
    assert_eq!(
        report.backups.len(),
        1,
        "folder metadata must not appear as protected backups"
    );
    assert_eq!(report.backups[0].name, "alpha");
    assert_eq!(
        service
            .backup_directory(&paths, &report.backups[0].id)
            .unwrap(),
        snapshot
    );
    let other = report
        .categories
        .iter()
        .find(|usage| usage.category == Category::Other)
        .unwrap();
    assert_eq!(other.bytes, 24);
    assert_eq!(other.cleanable_bytes, 0);
    for path in &files {
        let relative = path
            .strip_prefix(paths.app_data())
            .unwrap()
            .to_string_lossy();
        let id = format!("{:x}", Sha256::digest(relative.as_bytes()));
        assert!(service.backup_directory(&paths, &id).is_err());
    }
    let preview = service
        .preview(
            &paths,
            Selection {
                all_backups: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(preview.items.len(), 1);
    service.clean(&paths, &preview.id).unwrap();
    assert!(service.report(&paths).unwrap().backups.is_empty());
    assert!(files.iter().all(|path| path.exists()));
}

#[test]
fn preview_rejects_changed_files_without_deleting_other_confirmed_entries() {
    let (_tmp, paths, service) = fixture();
    let a = cache(&paths, "a");
    let b = cache(&paths, "b");
    let preview = service
        .preview(
            &paths,
            Selection {
                cache: true,
                ..Default::default()
            },
        )
        .unwrap();
    fs::write(&b, b"changed content").unwrap();
    assert!(service.clean(&paths, &preview.id).is_err());
    assert!(a.exists() && b.exists());
}
#[test]
fn excludes_protected_snapshots_and_rechecks_new_references() {
    let (_tmp, paths, service) = fixture();
    let a = backup(&paths, "alpha");
    let b = backup(&paths, "beta");
    let preview = service
        .preview(
            &paths,
            Selection {
                all_backups: true,
                ..Default::default()
            },
        )
        .unwrap();
    marker(&paths, &a);
    assert!(service.clean(&paths, &preview.id).is_err());
    let report = service.report(&paths).unwrap();
    assert_eq!(
        report
            .backups
            .iter()
            .filter(|b| b.protected_reason.is_some())
            .count(),
        1
    );
    let preview = service
        .preview(
            &paths,
            Selection {
                all_backups: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(preview.items.len(), 1);
    service.clean(&paths, &preview.id).unwrap();
    assert!(a.exists());
    assert!(!b.exists());
    fs::write(paths.interrupted_marker_path(), b"corrupt marker").unwrap();
    assert!(
        service.report(&paths).unwrap().backups[0]
            .protected_reason
            .is_some()
    );
}
#[test]
fn symbolic_links_are_not_followed_for_statistics_or_deletion() {
    let (tmp, paths, service) = fixture();
    let outside = tmp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    let secret = outside.join("keep");
    fs::write(&secret, vec![7u8; 100_000]).unwrap();
    symlink(&outside, paths.cache_dir().join("archives")).unwrap();
    let report = service.report(&paths).unwrap();
    assert!(report.total_bytes < 100_000);
    assert_eq!(report.cleanable_bytes, 0);
    assert!(!report.warnings.is_empty());
    assert!(
        service
            .preview(
                &paths,
                Selection {
                    cache: true,
                    ..Default::default()
                }
            )
            .is_err()
    );
    fs::remove_file(paths.cache_dir().join("archives")).unwrap();
    fs::create_dir(paths.cache_dir().join("archives")).unwrap();
    symlink(&outside, paths.cache_dir().join("archives/link")).unwrap();
    let preview = service
        .preview(
            &paths,
            Selection {
                cache: true,
                ..Default::default()
            },
        )
        .unwrap();
    service.clean(&paths, &preview.id).unwrap();
    assert!(secret.exists());
}
#[test]
fn replacement_of_a_category_directory_with_a_link_invalidates_cleanup() {
    let (tmp, paths, service) = fixture();
    let tree = cache(&paths, "test");
    let preview = service
        .preview(
            &paths,
            Selection {
                cache: true,
                ..Default::default()
            },
        )
        .unwrap();
    let moved = tmp.path().join("moved");
    fs::rename(paths.cache_dir(), &moved).unwrap();
    symlink(&moved, paths.cache_dir()).unwrap();
    assert!(service.clean(&paths, &preview.id).is_err());
    assert!(moved.join(tree.file_name().unwrap()).exists());
    assert!(checked_remove(&paths, &paths.app_data().join("../moved/file")).is_err());
}
#[test]
fn corrupt_manifests_and_unknown_files_are_counted_but_never_swept() {
    let (_tmp, paths, service) = fixture();
    let a = backup(&paths, "alpha");
    fs::write(a.join(crate::backup::BACKUP_MANIFEST_FILE), b"bad").unwrap();
    fs::write(paths.logs_dir().join("notes.txt"), b"keep").unwrap();
    let report = service.report(&paths).unwrap();
    assert!(report.backups[0].protected_reason.is_some());
    assert_eq!(report.cleanable_bytes, 0);
    assert!(
        service
            .preview(
                &paths,
                Selection {
                    all_backups: true,
                    logs: true,
                    ..Default::default()
                }
            )
            .is_err()
    );
    assert!(
        service
            .preview(
                &paths,
                Selection {
                    backup_ids: vec!["../../outside".into()],
                    ..Default::default()
                }
            )
            .is_err()
    );
}
#[test]
fn cache_activity_and_cleanup_exclude_each_other_and_release_on_drop() {
    let (_tmp, paths, _service) = fixture();
    let work = paths.data_coordination.activity().unwrap();
    assert!(paths.clone().data_coordination.cleanup().is_err());
    drop(work);
    let cleanup = paths.data_coordination.cleanup().unwrap();
    assert!(paths.data_coordination.activity().is_err());
    assert!(paths.data_coordination.cleanup().is_err());
    drop(cleanup);
    assert!(paths.data_coordination.activity().is_ok());
}
#[test]
fn preferences_preserve_credentials_unknown_fields_and_defaults_on_restart() {
    let (_tmp, paths, _service) = fixture();
    fs::write(
        paths.settings_path(),
        r#"{"version":1,"githubToken":"dummy-secret","futureField":42}"#,
    )
    .unwrap();
    let settings = SettingsService::new(paths.clone(), None);
    assert_eq!(
        settings.data_preferences().unwrap(),
        DataPreferences::default()
    );
    settings
        .save_data_preferences(DataPreferences {
            retain_backups: false,
            operation_logging: false,
        })
        .unwrap();
    settings.save("replacement-secret").unwrap();
    let restarted = SettingsService::new(paths.clone(), None);
    assert!(!restarted.data_preferences().unwrap().retain_backups);
    assert_eq!(
        restarted.credentials().unwrap().token.as_deref(),
        Some("replacement-secret")
    );
    restarted.clear().unwrap();
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(paths.settings_path()).unwrap()).unwrap();
    assert_eq!(value["futureField"], 42);
    assert_eq!(value["dataManagement"]["operationLogging"], false);
}
#[test]
fn active_operation_captures_preferences_while_next_operation_sees_new_values() {
    let (_tmp, paths, _service) = fixture();
    let settings = SettingsService::new(paths.clone(), None);
    let active = paths
        .data_coordination
        .operation(settings.data_preferences().unwrap());
    settings
        .save_data_preferences(DataPreferences {
            retain_backups: false,
            operation_logging: false,
        })
        .unwrap();
    assert!(paths.data_preferences().retain_backups);
    drop(active);
    assert!(!paths.data_preferences().retain_backups);
}
fn outcome(paths: &AppPaths, good: &Path, bad: &Path) -> TaskResult {
    TaskResult {
        action: TaskAction::Update,
        scope: crate::scope::ScopeId::Global,
        overall: TaskOverall::Partial,
        skills: vec![
            TaskSkillResult {
                name: "good".into(),
                status: TaskSkillStatus::Updated,
                error: None,
                backup_path: Some(paths.display(good)),
            },
            TaskSkillResult {
                name: "bad".into(),
                status: TaskSkillStatus::Failed,
                error: None,
                backup_path: Some(paths.display(bad)),
            },
        ],
        warnings: vec![],
        backup_path: Some(paths.display(good)),
        log_path: None,
        started_at: 1,
        finished_at_epoch_ms: 2,
        preview: None,
        batch: None,
    }
}
#[test]
fn retention_cleans_only_successful_snapshots_and_removes_stale_result_links() {
    let (_tmp, paths, _service) = fixture();
    let good = backup(&paths, "good");
    let bad = backup(&paths, "bad");
    let old = backup(&paths, "old");
    SettingsService::new(paths.clone(), None)
        .save_data_preferences(DataPreferences {
            retain_backups: false,
            operation_logging: true,
        })
        .unwrap();
    let mut result = outcome(&paths, &good, &bad);
    finish_successful_backups(&paths, &mut result, &[]);
    assert!(!good.exists());
    assert!(bad.exists() && old.exists());
    assert!(result.skills[0].backup_path.is_none());
    assert_eq!(result.backup_path, Some(paths.display(&bad)));
}
#[test]
fn cancellation_shared_failure_and_marker_protection_all_retain_snapshots() {
    let (_tmp, paths, _service) = fixture();
    let good = backup(&paths, "good");
    let bad = backup(&paths, "bad");
    SettingsService::new(paths.clone(), None)
        .save_data_preferences(DataPreferences {
            retain_backups: false,
            operation_logging: true,
        })
        .unwrap();
    let mut cancelled = outcome(&paths, &good, &bad);
    cancelled.overall = TaskOverall::Cancelled;
    finish_successful_backups(&paths, &mut cancelled, &[]);
    assert!(good.exists());
    let mut shared = outcome(&paths, &good, &good);
    finish_successful_backups(&paths, &mut shared, &[]);
    assert!(good.exists());
    marker(&paths, &good);
    let mut protected = outcome(&paths, &good, &bad);
    finish_successful_backups(&paths, &mut protected, &[]);
    assert!(good.exists());
}
#[test]
fn task_level_install_backup_is_removed_only_after_full_success() {
    let (_tmp, paths, _service) = fixture();
    let snapshot = backup(&paths, "install-task");
    SettingsService::new(paths.clone(), None)
        .save_data_preferences(DataPreferences {
            retain_backups: false,
            operation_logging: false,
        })
        .unwrap();
    let mut result = outcome(&paths, &snapshot, &snapshot);
    result.skills.clear();
    result.log_path = Some("old-log".into());
    finish_successful_backups(&paths, &mut result, &[snapshot.to_string_lossy().into()]);
    assert!(snapshot.exists());
    result.overall = TaskOverall::Success;
    finish_successful_backups(&paths, &mut result, &[snapshot.to_string_lossy().into()]);
    assert!(!snapshot.exists());
    assert!(result.backup_path.is_none() && result.log_path.is_none());
}
