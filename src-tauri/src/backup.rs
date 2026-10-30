//! Pre-operation backup snapshots.
//!
//! Every update (and later uninstall/detach) writes a snapshot under app data
//! `backups/<timestamp>-<action>-<skill>/` before touching the disk:
//!
//! - `skill-lock.json` — a byte copy of the strict v3 lock (or
//!   `lock-absent.txt` when a fresh machine has no lock yet);
//! - `canonical/` — the shared skill folder;
//! - `attachments/<agent-id>/` — every real copy attachment (symlinks and
//!   native/shared mounts are recorded in the manifest but not copied);
//! - `attachment-manifest.json` — action, skill, agents, canonical/lock
//!   sources and the mode/backup location of every physical attachment.
//!
//! A backup failure is fatal for the operation (`backup.failed`) and partial
//! directories are removed. Nothing here writes the lock or skill folders.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::time::now_ms;

use serde::Serialize;

use crate::error::AppError;
use crate::inventory::AttachmentMode;
use crate::paths::AppPaths;
use crate::security::safe_component;

pub const BACKUP_MANIFEST_FILE: &str = "attachment-manifest.json";
pub const BACKUP_LOCK_FILE: &str = "skill-lock.json";
pub const BACKUP_PROJECT_LOCK_FILE: &str = "skills-lock.json";
pub const BACKUP_LOCK_ABSENT_FILE: &str = "lock-absent.txt";
pub const BACKUP_CANONICAL_DIR: &str = "canonical";
pub const BACKUP_ATTACHMENTS_DIR: &str = "attachments";
pub const BACKUP_MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupAction {
    Update,
    Uninstall,
    Detach,
}

impl BackupAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Update => "update",
            Self::Uninstall => "uninstall",
            Self::Detach => "detach",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupAttachment {
    pub agent_id: String,
    pub agent_name: String,
    pub path: PathBuf,
    pub mode: AttachmentMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestAttachment {
    pub agent_id: String,
    pub agent_name: String,
    pub path: String,
    pub mode: AttachmentMode,
    pub backed_up: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupSnapshot {
    pub action: BackupAction,
    pub scope: crate::scope::ScopeId,
    pub skill: String,
    pub path: PathBuf,
    pub lock_present: bool,
    pub canonical_present: bool,
    pub agents: Vec<String>,
    pub attachments: Vec<ManifestAttachment>,
}

#[derive(Debug, Clone)]
pub struct BackupStore {
    root: PathBuf,
    lock_path: PathBuf,
    lock_file_name: &'static str,
    scope: crate::scope::ScopeId,
}

impl BackupStore {
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            root: paths.backups_dir(),
            lock_path: paths.lock_path(),
            lock_file_name: BACKUP_LOCK_FILE,
            scope: crate::scope::ScopeId::Global,
        }
    }

    /// Scope-isolated snapshots. Global keeps the historical flat directory;
    /// project snapshots live under `backups/project-<id>/` and back up that
    /// project's own `skills-lock.json`.
    pub fn for_layout(paths: &AppPaths, layout: &crate::scope_layout::ScopeLayout) -> Self {
        match layout.scope.project_id() {
            None => Self::new(paths),
            Some(id) => Self {
                root: paths.backups_dir().join(format!("project-{id}")),
                lock_path: layout.lock_path.clone(),
                lock_file_name: BACKUP_PROJECT_LOCK_FILE,
                scope: layout.scope.clone(),
            },
        }
    }

    pub fn from_paths(root: impl Into<PathBuf>, lock_path: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            lock_path: lock_path.into(),
            lock_file_name: BACKUP_LOCK_FILE,
            scope: crate::scope::ScopeId::Global,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Create a complete snapshot. Any failure aborts the operation and cleans
    /// up a partially written backup directory.
    pub fn create(
        &self,
        action: BackupAction,
        skill: &str,
        canonical: Option<&Path>,
        attachments: &[BackupAttachment],
        agents: &[String],
    ) -> Result<BackupSnapshot, AppError> {
        if skill.trim().is_empty() {
            return Err(AppError::backup_failed("备份缺少 skill 标识。"));
        }
        fs::create_dir_all(&self.root).map_err(|error| {
            AppError::backup_failed(format!("无法创建备份目录 {}：{error}", self.root.display()))
        })?;

        let directory = self.unique_directory(action, skill);
        fs::create_dir_all(&directory).map_err(|error| {
            AppError::backup_failed(format!("无法创建备份 {}：{error}", directory.display()))
        })?;

        match self.write_snapshot(&directory, action, skill, canonical, attachments, agents) {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => {
                let _ = fs::remove_dir_all(&directory);
                Err(error)
            }
        }
    }

    fn write_snapshot(
        &self,
        directory: &Path,
        action: BackupAction,
        skill: &str,
        canonical: Option<&Path>,
        attachments: &[BackupAttachment],
        agents: &[String],
    ) -> Result<BackupSnapshot, AppError> {
        let lock_present = match fs::copy(&self.lock_path, directory.join(self.lock_file_name)) {
            Ok(_) => true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::write(
                    directory.join(BACKUP_LOCK_ABSENT_FILE),
                    b"skill lock did not exist before this operation\n",
                )
                .map_err(|error| io_backup_error(directory, error))?;
                false
            }
            Err(error) => {
                return Err(AppError::backup_failed(format!(
                    "无法备份 lock {}：{error}",
                    self.lock_path.display()
                )));
            }
        };

        let canonical_present = match canonical {
            Some(source) => {
                ensure_directory(source)?;
                copy_tree(source, &directory.join(BACKUP_CANONICAL_DIR))
                    .map_err(|error| io_backup_error(source, error))?;
                true
            }
            None => false,
        };

        let mut manifest_attachments = Vec::with_capacity(attachments.len());
        for attachment in attachments {
            let destination = directory
                .join(BACKUP_ATTACHMENTS_DIR)
                .join(safe_component(&attachment.agent_id));
            let backup_path = match attachment.mode {
                AttachmentMode::Copy => {
                    ensure_directory(&attachment.path)?;
                    copy_tree(&attachment.path, &destination)
                        .map_err(|error| io_backup_error(&attachment.path, error))?;
                    Some(
                        format!(
                            "{}/{}/{}",
                            BACKUP_ATTACHMENTS_DIR,
                            safe_component(&attachment.agent_id),
                            ""
                        )
                        .trim_end_matches('/')
                        .to_string(),
                    )
                }
                _ => None,
            };
            manifest_attachments.push(ManifestAttachment {
                agent_id: attachment.agent_id.clone(),
                agent_name: attachment.agent_name.clone(),
                path: attachment.path.to_string_lossy().into_owned(),
                mode: attachment.mode,
                backed_up: backup_path.is_some(),
                backup_path,
            });
        }

        let manifest = Manifest {
            version: BACKUP_MANIFEST_VERSION,
            action,
            scope: self.scope.as_string(),
            skill: skill.to_string(),
            created_at_epoch_ms: now_ms(),
            agents: agents.to_vec(),
            lock_source: self.lock_path.to_string_lossy().into_owned(),
            lock_present,
            canonical_source: canonical.map(|path| path.to_string_lossy().into_owned()),
            canonical_present,
            attachments: manifest_attachments.clone(),
        };
        let serialized = serde_json::to_string_pretty(&manifest).map_err(|error| {
            AppError::backup_failed(format!("无法序列化备份 manifest：{error}"))
        })?;
        fs::write(directory.join(BACKUP_MANIFEST_FILE), serialized)
            .map_err(|error| io_backup_error(directory, error))?;

        Ok(BackupSnapshot {
            action,
            scope: self.scope.clone(),
            skill: skill.to_string(),
            path: directory.to_path_buf(),
            lock_present,
            canonical_present,
            agents: agents.to_vec(),
            attachments: manifest_attachments,
        })
    }

    fn unique_directory(&self, action: BackupAction, skill: &str) -> PathBuf {
        let stamp = now_ms();
        let base = format!("{stamp}-{}-{}", action.as_str(), safe_component(skill));
        let mut candidate = self.root.join(&base);
        let mut suffix = 1;
        while candidate.exists() {
            candidate = self.root.join(format!("{base}-{suffix}"));
            suffix += 1;
        }
        candidate
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    version: u32,
    action: BackupAction,
    scope: String,
    skill: String,
    created_at_epoch_ms: u64,
    agents: Vec<String>,
    lock_source: String,
    lock_present: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    canonical_source: Option<String>,
    canonical_present: bool,
    attachments: Vec<ManifestAttachment>,
}

fn ensure_directory(path: &Path) -> Result<(), AppError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        AppError::backup_failed(format!("无法读取待备份目录 {}：{error}", path.display()))
    })?;
    let is_dir = fs::metadata(path)
        .map(|metadata| metadata.is_dir())
        .unwrap_or(false)
        || metadata.file_type().is_symlink();
    if !is_dir {
        return Err(AppError::backup_failed(format!(
            "待备份路径不是目录：{}",
            path.display()
        )));
    }
    Ok(())
}

/// Recursive copy that preserves symlinks as symlinks and file permissions.
/// The root itself may be a symlink to a directory (link installs); its
/// contents are read through the link.
fn copy_tree(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(destination)?;
    let mut entries = fs::read_dir(source)?.collect::<Result<Vec<_>, io::Error>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let file_type = entry.file_type()?;
        let target = destination.join(entry.file_name());
        if file_type.is_symlink() {
            let link = fs::read_link(entry.path())?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(link, target)?;
        } else if file_type.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

fn io_backup_error(path: &Path, error: io::Error) -> AppError {
    AppError::backup_failed(format!("{}：{error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use std::os::unix::fs::symlink;

    struct Fixture {
        tmp: tempfile::TempDir,
        paths: AppPaths,
        canonical: PathBuf,
        qoder: PathBuf,
        claude: PathBuf,
        gemini: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path().join("home");
            let paths = AppPaths::new(&home, tmp.path().join("app-data"));
            let canonical = paths.canonical_skills_dir().join("alpha");
            fs::create_dir_all(canonical.join("references")).unwrap();
            fs::write(canonical.join("SKILL.md"), "# alpha v1\n").unwrap();
            fs::write(canonical.join("references/guide.md"), "guide\n").unwrap();
            symlink(
                canonical.join("references/guide.md"),
                canonical.join("references/guide-link.md"),
            )
            .unwrap();
            fs::create_dir_all(paths.agents_root()).unwrap();
            fs::write(paths.lock_path(), r#"{"version":3,"skills":{"alpha":{}}}"#).unwrap();

            let qoder = home.join(".qoder/skills");
            let claude = home.join(".claude/skills");
            let gemini = home.join(".gemini/skills");
            fs::create_dir_all(&qoder).unwrap();
            fs::create_dir_all(&claude).unwrap();
            fs::create_dir_all(&gemini).unwrap();
            symlink(&canonical, qoder.join("alpha")).unwrap();
            fs::create_dir_all(claude.join("alpha")).unwrap();
            fs::write(claude.join("alpha/SKILL.md"), "# alpha copy\n").unwrap();
            fs::create_dir_all(gemini.join("alpha")).unwrap();
            fs::write(gemini.join("alpha/SKILL.md"), "# alpha gemini\n").unwrap();

            Self {
                tmp,
                paths,
                canonical,
                qoder,
                claude,
                gemini,
            }
        }

        fn attachments(&self) -> Vec<BackupAttachment> {
            vec![
                BackupAttachment {
                    agent_id: "qoder".to_string(),
                    agent_name: "Qoder".to_string(),
                    path: self.qoder.join("alpha"),
                    mode: AttachmentMode::Link,
                },
                BackupAttachment {
                    agent_id: "claude-code".to_string(),
                    agent_name: "Claude Code".to_string(),
                    path: self.claude.join("alpha"),
                    mode: AttachmentMode::Copy,
                },
                BackupAttachment {
                    agent_id: "gemini-cli".to_string(),
                    agent_name: "Gemini CLI".to_string(),
                    path: self.canonical.clone(),
                    mode: AttachmentMode::Native,
                },
                BackupAttachment {
                    agent_id: "broken".to_string(),
                    agent_name: "Broken".to_string(),
                    path: self.gemini.join("missing"),
                    mode: AttachmentMode::Broken,
                },
            ]
        }
    }

    #[test]
    fn creates_lock_canonical_copy_attachments_and_manifest() {
        let fixture = Fixture::new();
        let store = BackupStore::new(&fixture.paths);
        let snapshot = store
            .create(
                BackupAction::Update,
                "alpha",
                Some(&fixture.canonical),
                &fixture.attachments(),
                &["qoder".to_string(), "universal".to_string()],
            )
            .unwrap();

        let name = snapshot.path.file_name().unwrap().to_string_lossy();
        let (stamp, action, skill) = name
            .split_once("-update-")
            .map(|(stamp, rest)| (stamp, "update", rest))
            .expect("timestamp-action-skill layout");
        assert!(stamp.chars().all(|character| character.is_ascii_digit()));
        assert_eq!(action, "update");
        assert_eq!(skill, "alpha");
        assert_eq!(snapshot.path.parent().unwrap(), fixture.paths.backups_dir());
        assert!(snapshot.lock_present);
        assert!(snapshot.canonical_present);
        assert_eq!(
            snapshot.agents,
            vec!["qoder".to_string(), "universal".to_string()]
        );

        assert_eq!(
            fs::read_to_string(snapshot.path.join(BACKUP_LOCK_FILE)).unwrap(),
            fs::read_to_string(fixture.paths.lock_path()).unwrap()
        );
        assert_eq!(
            fs::read_to_string(snapshot.path.join("canonical/SKILL.md")).unwrap(),
            "# alpha v1\n"
        );
        assert_eq!(
            fs::read_to_string(snapshot.path.join("canonical/references/guide.md")).unwrap(),
            "guide\n"
        );
        assert!(
            snapshot
                .path
                .join("canonical/references/guide-link.md")
                .is_symlink()
        );
        assert_eq!(
            fs::read_to_string(snapshot.path.join("attachments/claude-code/SKILL.md")).unwrap(),
            "# alpha copy\n"
        );
        assert!(!snapshot.path.join("attachments/qoder").exists());
        assert!(!snapshot.path.join("attachments/gemini-cli").exists());

        let manifest: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(snapshot.path.join(BACKUP_MANIFEST_FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["version"], 1);
        assert_eq!(manifest["action"], "update");
        assert_eq!(manifest["skill"], "alpha");
        assert_eq!(manifest["lockPresent"], true);
        assert_eq!(manifest["canonicalPresent"], true);
        assert_eq!(manifest["agents"][1], "universal");
        let attachments = manifest["attachments"].as_array().unwrap();
        assert_eq!(attachments.len(), 4);
        let claude = attachments
            .iter()
            .find(|entry| entry["agentId"] == "claude-code")
            .unwrap();
        assert_eq!(claude["mode"], "copy");
        assert_eq!(claude["backedUp"], true);
        assert_eq!(claude["backupPath"], "attachments/claude-code");
        let qoder = attachments
            .iter()
            .find(|entry| entry["agentId"] == "qoder")
            .unwrap();
        assert_eq!(qoder["mode"], "link");
        assert_eq!(qoder["backedUp"], false);
        assert!(qoder.get("backupPath").is_none());
    }

    #[test]
    fn missing_lock_is_recorded_and_not_an_error() {
        let fixture = Fixture::new();
        fs::remove_file(fixture.paths.lock_path()).unwrap();
        let snapshot = BackupStore::new(&fixture.paths)
            .create(
                BackupAction::Update,
                "alpha",
                Some(&fixture.canonical),
                &[],
                &["universal".to_string()],
            )
            .unwrap();
        assert!(!snapshot.lock_present);
        assert!(snapshot.path.join(BACKUP_LOCK_ABSENT_FILE).is_file());
        assert!(!snapshot.path.join(BACKUP_LOCK_FILE).exists());
    }

    #[test]
    fn backup_failure_maps_to_backup_failed_and_cleans_up() {
        let fixture = Fixture::new();
        // Make the backups root impossible to create as a directory.
        fs::create_dir_all(fixture.paths.app_data()).unwrap();
        fs::write(fixture.paths.backups_dir(), b"not a directory").unwrap();
        let error = BackupStore::new(&fixture.paths)
            .create(
                BackupAction::Update,
                "alpha",
                Some(&fixture.canonical),
                &[],
                &["universal".to_string()],
            )
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::BackupFailed);
        assert!(error.message.contains("备份"));

        // A missing canonical source is also fatal.
        let fixture = Fixture::new();
        let missing = fixture.tmp.path().join("missing-skill");
        let error = BackupStore::new(&fixture.paths)
            .create(BackupAction::Update, "alpha", Some(&missing), &[], &[])
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::BackupFailed);
        let remaining: Vec<_> = fs::read_dir(fixture.paths.backups_dir()).unwrap().collect();
        assert!(
            remaining.is_empty(),
            "partial backup directories must be removed"
        );
    }

    #[test]
    fn skill_names_cannot_escape_the_backups_root() {
        let fixture = Fixture::new();
        let snapshot = BackupStore::new(&fixture.paths)
            .create(
                BackupAction::Update,
                "../../evil/skill",
                Some(&fixture.canonical),
                &[],
                &[],
            )
            .unwrap();
        assert_eq!(snapshot.path.parent().unwrap(), fixture.paths.backups_dir());
    }

    #[test]
    fn project_snapshots_are_scope_isolated_and_back_up_the_project_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        let root = tmp.path().join("projects/app");
        let canonical = root.join(".agents/skills/alpha");
        fs::create_dir_all(&canonical).unwrap();
        fs::write(canonical.join("SKILL.md"), "# project alpha\n").unwrap();
        fs::write(
            root.join("skills-lock.json"),
            r#"{"version":1,"skills":{"alpha":{"source":"acme/skills","sourceType":"github","computedHash":"abc"}}}"#,
        )
        .unwrap();
        let copy = root.join(".claude/skills/alpha");
        fs::create_dir_all(&copy).unwrap();
        fs::write(copy.join("SKILL.md"), "# project copy\n").unwrap();

        let project = crate::projects::Project {
            id: uuid::Uuid::new_v4(),
            root: root.clone(),
            display_name: "app".to_string(),
            added_at: 1,
            last_opened_at: 1,
        };
        let scope = project.scope();
        let layout = crate::scope_layout::ScopeLayout::project(&paths, project);
        let snapshot = BackupStore::for_layout(&paths, &layout)
            .create(
                BackupAction::Uninstall,
                "alpha",
                Some(&canonical),
                &[BackupAttachment {
                    agent_id: "claude-code".to_string(),
                    agent_name: "Claude Code".to_string(),
                    path: copy,
                    mode: AttachmentMode::Copy,
                }],
                &["claude-code".to_string(), "universal".to_string()],
            )
            .unwrap();

        assert_eq!(snapshot.scope, scope);
        assert_eq!(
            snapshot.path.parent().unwrap(),
            paths
                .backups_dir()
                .join(format!("project-{}", scope.project_id().unwrap()))
        );
        assert!(snapshot.path.join(BACKUP_PROJECT_LOCK_FILE).is_file());
        assert!(!snapshot.path.join(BACKUP_LOCK_FILE).exists());
        assert_eq!(
            fs::read_to_string(snapshot.path.join("canonical/SKILL.md")).unwrap(),
            "# project alpha\n"
        );
        assert_eq!(
            fs::read_to_string(snapshot.path.join("attachments/claude-code/SKILL.md")).unwrap(),
            "# project copy\n"
        );
        let manifest: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(snapshot.path.join(BACKUP_MANIFEST_FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["scope"], serde_json::json!(scope.as_string()));
        assert_eq!(
            manifest["lockSource"],
            serde_json::json!(layout.lock_path.to_string_lossy())
        );
        assert!(!paths.lock_path().exists(), "global lock is never read");
    }

    #[test]
    fn duplicate_timestamps_get_a_unique_suffix() {
        let fixture = Fixture::new();
        let store = BackupStore::new(&fixture.paths);
        // Hold the first directory open so the next attempt cannot reuse it.
        let first = store
            .create(
                BackupAction::Update,
                "alpha",
                Some(&fixture.canonical),
                &[],
                &[],
            )
            .unwrap();
        let existing = first.path.join("marker");
        fs::write(&existing, b"x").unwrap();
        let second = store
            .create(
                BackupAction::Update,
                "alpha",
                Some(&fixture.canonical),
                &[],
                &[],
            )
            .unwrap();
        assert_ne!(first.path, second.path);
        assert!(first.path.exists());
    }
}
