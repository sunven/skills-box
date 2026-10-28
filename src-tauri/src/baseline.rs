//! Baseline fingerprint store for local-modification detection.
//!
//! The GUI records a content fingerprint of each managed skill folder under
//! app data `baselines/`. Later scans recompute the fingerprint and compare it
//! with the recorded baseline:
//!
//! - a verified baseline (recorded after install/update or when the 40-char
//!   lock hash matched) decides clean vs modified locally;
//! - an unverified baseline (recorded on a first scan whose lock hash could not
//!   be compared) stays `unknown` until a verified baseline is recorded, so a
//!   single first-scan warning is not silently forgotten;
//! - without a usable baseline, a matching 40-char Git tree hash proves
//!   `clean`; anything else stays `unknown` (`本地改动?`).
//!
//! Fingerprints deliberately ignore installer-excluded entries (`metadata.json`
//! files, `.git`, `__pycache__`, `__pypackages__` directories) and noise that
//! would otherwise create false positives (`.DS_Store`, `node_modules`). All
//! writes stay inside app data; skill folders are never modified.

use std::fs::{self, File};
use std::io::{self, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest, Sha256};

use crate::error::AppError;
use crate::paths::AppPaths;

pub const BASELINE_VERSION: u32 = 1;
pub const FINGERPRINT_NAMESPACE: &[u8] = b"skills-box-folder-fingerprint-v1\0";

/// Result of comparing a skill folder with its baseline and lock hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalModification {
    Clean,
    Modified,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Baseline {
    pub version: u32,
    pub name: String,
    pub path: String,
    pub content_hash: String,
    pub file_count: u64,
    pub total_bytes: u64,
    /// True when the baseline was recorded from a known-good state: an install
    /// or update, or a first scan whose Git tree hash matched the lock.
    pub verified: bool,
    pub recorded_at_epoch_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderFingerprint {
    pub content_hash: String,
    pub file_count: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalModificationAssessment {
    pub state: LocalModification,
    pub detail: Option<String>,
    pub baseline_created: bool,
    pub baseline_updated: bool,
}

#[derive(Debug, Clone)]
pub struct BaselineStore {
    dir: PathBuf,
}

impl BaselineStore {
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            dir: paths.baselines_dir(),
        }
    }

    pub fn from_dir(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn baseline_path(&self, name: &str) -> PathBuf {
        self.dir.join(baseline_file_name(name))
    }

    /// Read a baseline. Missing, malformed, or foreign-version files are
    /// treated as absent: a broken baseline must never fail inventory reads.
    pub fn load(&self, name: &str) -> Option<Baseline> {
        let raw = fs::read_to_string(self.baseline_path(name)).ok()?;
        let baseline: Baseline = serde_json::from_str(&raw).ok()?;
        if baseline.version != BASELINE_VERSION || baseline.name != name {
            return None;
        }
        Some(baseline)
    }

    /// Record a fingerprint. `verified` must only be true when the caller knows
    /// the folder matches the lock (install/update or a hash-checked first
    /// scan). This is the reusable entry point for install/update tickets.
    pub fn record(
        &self,
        name: &str,
        path: &Path,
        fingerprint: &FolderFingerprint,
        verified: bool,
    ) -> Result<Baseline, AppError> {
        fs::create_dir_all(&self.dir).map_err(|error| {
            AppError::new("baseline.write_failed", "无法创建基线指纹目录。")
                .detail(error.to_string())
                .hint("请检查应用数据目录的权限。")
        })?;

        let baseline = Baseline {
            version: BASELINE_VERSION,
            name: name.to_string(),
            path: path.to_string_lossy().into_owned(),
            content_hash: fingerprint.content_hash.clone(),
            file_count: fingerprint.file_count,
            total_bytes: fingerprint.total_bytes,
            verified,
            recorded_at_epoch_ms: epoch_ms(),
        };
        let serialized = serde_json::to_string_pretty(&baseline).map_err(|error| {
            AppError::new("baseline.write_failed", "无法序列化基线指纹。").detail(error.to_string())
        })?;

        let destination = self.baseline_path(name);
        let temp = self
            .dir
            .join(format!(".tmp-{}-{}", std::process::id(), epoch_nanos()));
        fs::write(&temp, serialized).map_err(|error| {
            let _ = fs::remove_file(&temp);
            AppError::new("baseline.write_failed", "无法写入基线指纹。")
                .detail(error.to_string())
                .hint("请检查应用数据目录的权限与磁盘空间。")
        })?;
        if let Err(error) = fs::rename(&temp, &destination) {
            let _ = fs::remove_file(&temp);
            return Err(AppError::new("baseline.write_failed", "无法保存基线指纹。")
                .detail(error.to_string())
                .hint("请检查应用数据目录的权限与磁盘空间。"));
        }
        Ok(baseline)
    }

    pub fn record_from_dir(
        &self,
        name: &str,
        path: &Path,
        verified: bool,
    ) -> Result<Baseline, AppError> {
        let fingerprint = fingerprint_folder(path).map_err(|error| {
            AppError::new("skill.read_failed", "无法计算 skill 目录指纹。")
                .detail(error.to_string())
        })?;
        self.record(name, path, &fingerprint, verified)
    }

    pub fn clear(&self, name: &str) -> io::Result<()> {
        match fs::remove_file(self.baseline_path(name)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Compare a skill folder with its baseline and lock hash, recording a
    /// baseline on first scan. All filesystem/read failures degrade to
    /// `unknown` rather than failing the inventory.
    pub fn assess(
        &self,
        name: &str,
        path: Option<&Path>,
        lock_hash: Option<&str>,
    ) -> LocalModificationAssessment {
        let Some(path) = path else {
            return LocalModificationAssessment {
                state: LocalModification::Unknown,
                detail: Some("没有可读取的 skill 目录。".to_string()),
                baseline_created: false,
                baseline_updated: false,
            };
        };
        let fingerprint = match fingerprint_folder(path) {
            Ok(fingerprint) => fingerprint,
            Err(error) => {
                return LocalModificationAssessment {
                    state: LocalModification::Unknown,
                    detail: Some(format!("无法计算本地指纹：{error}")),
                    baseline_created: false,
                    baseline_updated: false,
                };
            }
        };

        let path_key = path.to_string_lossy();
        let existing = self.load(name).filter(|baseline| baseline.path == path_key);

        let lock_matches = compare_lock_hash(path, lock_hash);
        let mut baseline_created = false;
        let mut baseline_updated = false;

        let state = if let Some(baseline) = existing {
            let unchanged = baseline.content_hash == fingerprint.content_hash;
            match lock_matches {
                Some(true) => {
                    // Current content is provably the installed content. This
                    // also promotes an unverified first-scan baseline.
                    if (!baseline.verified || !unchanged)
                        && self.record(name, path, &fingerprint, true).is_ok()
                    {
                        baseline_updated = true;
                    }
                    LocalModification::Clean
                }
                _ if unchanged => {
                    if baseline.verified {
                        LocalModification::Clean
                    } else {
                        LocalModification::Unknown
                    }
                }
                _ => LocalModification::Modified,
            }
        } else {
            let verified = lock_matches == Some(true);
            if self.record(name, path, &fingerprint, verified).is_ok() {
                baseline_created = true;
            }
            if verified {
                LocalModification::Clean
            } else {
                LocalModification::Unknown
            }
        };

        let detail = match state {
            LocalModification::Clean => None,
            LocalModification::Modified => Some("当前文件与基线指纹不一致。".to_string()),
            LocalModification::Unknown => {
                let reason = match lock_hash {
                    None => "没有 lock 条目可比对",
                    Some("") => "lock 未记录可比较的 hash",
                    Some(hash) if is_git_tree_hash(hash) => "lock hash 与当前目录不一致",
                    Some(_) => "lock hash 不是 40 位 Git tree hash，无法可靠比对",
                };
                Some(format!(
                    "{reason}；首次扫描已记录基线但尚未校验，可能是安装器排除项造成的误报，请确认本地改动。"
                ))
            }
        };

        LocalModificationAssessment {
            state,
            detail,
            baseline_created,
            baseline_updated,
        }
    }
}

pub fn fingerprint_folder(root: &Path) -> io::Result<FolderFingerprint> {
    let mut context = FingerprintContext {
        hasher: Sha256::new(),
        file_count: 0,
        total_bytes: 0,
    };
    context.hasher.update(FINGERPRINT_NAMESPACE);
    fingerprint_dir(root, Path::new(""), &mut context)?;
    Ok(FolderFingerprint {
        content_hash: hex(&context.hasher.finalize()),
        file_count: context.file_count,
        total_bytes: context.total_bytes,
    })
}

struct FingerprintContext {
    hasher: Sha256,
    file_count: u64,
    total_bytes: u64,
}

fn fingerprint_dir(
    dir: &Path,
    relative: &Path,
    context: &mut FingerprintContext,
) -> io::Result<()> {
    for entry in read_sorted(dir)? {
        let name = entry.file_name();
        let file_type = entry.file_type()?;
        let name = name.to_string_lossy();
        if ignored_entry(&name, file_type.is_dir()) {
            continue;
        }
        let child_relative = relative.join(name.as_ref());
        let child = entry.path();
        if file_type.is_symlink() {
            let target = fs::read_link(&child)?;
            let target = target.to_string_lossy();
            update_path(&mut context.hasher, &child_relative);
            context.hasher.update(b"l");
            update_blob(&mut context.hasher, target.as_bytes());
            context.file_count += 1;
            context.total_bytes += target.len() as u64;
        } else if file_type.is_dir() {
            fingerprint_dir(&child, &child_relative, context)?;
        } else if file_type.is_file() {
            let metadata = entry.metadata()?;
            let mut file = File::open(&child)?;
            let mut digest = Sha256::new();
            let mut buffer = [0u8; 64 * 1024];
            loop {
                let read = file.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                digest.update(&buffer[..read]);
            }
            update_path(&mut context.hasher, &child_relative);
            context.hasher.update([executable_marker(&metadata)]);
            context.hasher.update(digest.finalize());
            context.file_count += 1;
            context.total_bytes += metadata.len();
        }
    }
    Ok(())
}

fn update_path(hasher: &mut Sha256, relative: &Path) {
    let bytes = relative.to_string_lossy();
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes.as_bytes());
}

fn update_blob(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn executable_marker(metadata: &fs::Metadata) -> u8 {
    if metadata.permissions().mode() & 0o111 != 0 {
        b'x'
    } else {
        b'f'
    }
}

/// Compute the Git tree SHA for a folder, mirroring `git add -A -f` on the
/// installed copy and skipping installer-excluded entries. Used only as a
/// first-scan heuristic against a 40-char lock hash.
pub fn git_tree_hash(root: &Path) -> io::Result<String> {
    let hash = hash_git_tree(root)?.unwrap_or_else(empty_tree_hash);
    Ok(hex(&hash))
}

fn empty_tree_hash() -> [u8; 20] {
    git_object_hash(b"tree", &[])
}

type GitTreeRow = (Vec<u8>, Vec<u8>, &'static str, [u8; 20]);

fn hash_git_tree(dir: &Path) -> io::Result<Option<[u8; 20]>> {
    let mut rows: Vec<GitTreeRow> = Vec::new();
    for entry in read_sorted(dir)? {
        let name_os = entry.file_name();
        let name = name_os.to_string_lossy().into_owned();
        let file_type = entry.file_type()?;
        if ignored_entry(&name, file_type.is_dir()) {
            continue;
        }
        let child = entry.path();
        let name_bytes = name.as_bytes().to_vec();
        if file_type.is_symlink() {
            let target = fs::read_link(&child)?;
            let target = target.to_string_lossy();
            let hash = git_object_hash(b"blob", target.as_bytes());
            let mut key = name_bytes.clone();
            key.push(0);
            rows.push((key, name_bytes, "120000", hash));
        } else if file_type.is_dir() {
            if let Some(hash) = hash_git_tree(&child)? {
                let mut key = name_bytes.clone();
                key.push(b'/');
                rows.push((key, name_bytes, "40000", hash));
            }
        } else if file_type.is_file() {
            let metadata = entry.metadata()?;
            let contents = fs::read(&child)?;
            let hash = git_object_hash(b"blob", &contents);
            let mode = if metadata.permissions().mode() & 0o111 != 0 {
                "100755"
            } else {
                "100644"
            };
            let mut key = name_bytes.clone();
            key.push(0);
            rows.push((key, name_bytes, mode, hash));
        }
    }

    if rows.is_empty() {
        return Ok(None);
    }
    rows.sort_by(|left, right| left.0.cmp(&right.0));

    let mut body = Vec::new();
    for (_, name, mode, hash) in rows {
        body.extend_from_slice(mode.as_bytes());
        body.push(b' ');
        body.extend_from_slice(&name);
        body.push(0);
        body.extend_from_slice(&hash);
    }
    Ok(Some(git_object_hash(b"tree", &body)))
}

fn git_object_hash(kind: &[u8], body: &[u8]) -> [u8; 20] {
    let mut hasher = Sha1::new();
    hasher.update(kind);
    hasher.update(b" ");
    hasher.update(body.len().to_string().as_bytes());
    hasher.update([0]);
    hasher.update(body);
    hasher.finalize().into()
}

fn compare_lock_hash(path: &Path, lock_hash: Option<&str>) -> Option<bool> {
    let hash = lock_hash?;
    if !is_git_tree_hash(hash) {
        return None;
    }
    git_tree_hash(path)
        .ok()
        .map(|local| local.eq_ignore_ascii_case(hash))
}

pub fn is_git_tree_hash(hash: &str) -> bool {
    hash.len() == 40 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn read_sorted(dir: &Path) -> io::Result<Vec<fs::DirEntry>> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, io::Error>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    Ok(entries)
}

/// Entries that must not contribute to fingerprints: installer exclusions,
/// VCS/generated directories, and Finder noise.
fn ignored_entry(name: &str, is_dir: bool) -> bool {
    if is_dir {
        matches!(
            name,
            ".git" | "node_modules" | "__pycache__" | "__pypackages__"
        )
    } else {
        matches!(name, "metadata.json" | ".DS_Store")
    }
}

fn baseline_file_name(name: &str) -> String {
    let mut safe: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    if safe.is_empty() {
        safe.push_str("skill");
    }
    let digest = Sha256::digest(name.as_bytes());
    format!("{safe}-{}", hex(&digest[..6]))
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn epoch_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::AppPaths;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    fn fixture() -> (tempfile::TempDir, BaselineStore, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let store = BaselineStore::from_dir(tmp.path().join("app-data/baselines"));
        let skill = tmp.path().join("home/.agents/skills/alpha");
        write(&skill.join("SKILL.md"), "---\nname: alpha\n---\n# Alpha\n");
        write(&skill.join("references/guide.md"), "guide\n");
        (tmp, store, skill)
    }

    #[test]
    fn fingerprint_is_stable_and_ignores_installer_exclusions_and_noise() {
        let (_tmp, _store, skill) = fixture();
        let first = fingerprint_folder(&skill).unwrap();
        let second = fingerprint_folder(&skill).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.file_count, 2);

        write(&skill.join("metadata.json"), "{\"generated\":true}");
        write(&skill.join(".DS_Store"), "noise");
        write(&skill.join("__pycache__/x.pyc"), "cache");
        write(&skill.join("node_modules/pkg/index.js"), "dep");
        assert_eq!(fingerprint_folder(&skill).unwrap(), first);

        write(&skill.join("references/guide.md"), "changed\n");
        assert_ne!(fingerprint_folder(&skill).unwrap(), first);

        write(&skill.join("extra.txt"), "extra");
        assert_ne!(fingerprint_folder(&skill).unwrap(), first);
    }

    #[test]
    fn fingerprint_notices_executable_bit_changes() {
        use std::os::unix::fs::PermissionsExt;
        let (_tmp, _store, skill) = fixture();
        let before = fingerprint_folder(&skill).unwrap();
        let script = skill.join("scripts/run.sh");
        write(&script, "#!/bin/sh\n");
        let with_script = fingerprint_folder(&skill).unwrap();
        assert_ne!(before, with_script);
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        assert_ne!(fingerprint_folder(&skill).unwrap(), with_script);
    }

    #[test]
    fn git_tree_hash_matches_a_known_git_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let skill = tmp.path().join("skill");
        write(
            &skill.join("SKILL.md"),
            "---\nname: alpha\ndescription: test\n---\n# Alpha\n",
        );
        write(&skill.join("sub/note.txt"), "hello\n");
        // Generated with `git add -A -f . && git write-tree` on the same tree.
        assert_eq!(
            git_tree_hash(&skill).unwrap(),
            "abbb9ef3b9eb76f085759d18cec6322ebe34be97"
        );
        // Installer exclusions and noise do not affect the Git tree.
        write(&skill.join("metadata.json"), "{}");
        write(&skill.join(".DS_Store"), "noise");
        assert_eq!(
            git_tree_hash(&skill).unwrap(),
            "abbb9ef3b9eb76f085759d18cec6322ebe34be97"
        );
    }

    #[test]
    fn git_tree_hash_handles_empty_folders_and_empty_trees() {
        let tmp = tempfile::tempdir().unwrap();
        let empty = tmp.path().join("empty");
        fs::create_dir_all(&empty).unwrap();
        assert_eq!(
            git_tree_hash(&empty).unwrap(),
            "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
        );

        let skill = tmp.path().join("skill");
        write(&skill.join("SKILL.md"), "x");
        fs::create_dir_all(skill.join("empty-dir")).unwrap();
        let before = git_tree_hash(&skill).unwrap();
        write(&skill.join("metadata.json"), "{}");
        assert_eq!(git_tree_hash(&skill).unwrap(), before);
    }

    #[test]
    fn git_tree_hash_uses_git_entry_order_for_prefix_names() {
        let tmp = tempfile::tempdir().unwrap();
        let skill = tmp.path().join("skill");
        write(&skill.join("a"), "x");
        write(&skill.join("a.b"), "y");
        // Generated with `git add -A -f . && git write-tree` on the same tree.
        assert_eq!(
            git_tree_hash(&skill).unwrap(),
            "846cef4236dfea2453c3513485f972921fedb43d"
        );
    }

    #[test]
    fn first_scan_with_matching_git_hash_is_clean_and_records_verified_baseline() {
        let (_tmp, store, skill) = fixture();
        let hash = git_tree_hash(&skill).unwrap();
        let assessment = store.assess("alpha", Some(&skill), Some(&hash));
        assert_eq!(assessment.state, LocalModification::Clean);
        assert!(assessment.baseline_created);
        let baseline = store.load("alpha").unwrap();
        assert!(baseline.verified);
        assert_eq!(baseline.path, skill.to_string_lossy());
    }

    #[test]
    fn first_scan_without_a_comparable_lock_hash_is_unknown() {
        let (_tmp, store, skill) = fixture();
        let assessment = store.assess("alpha", Some(&skill), None);
        assert_eq!(assessment.state, LocalModification::Unknown);
        assert!(assessment.detail.as_deref().unwrap().contains("本地改动"));
        let baseline = store.load("alpha").unwrap();
        assert!(!baseline.verified);

        // The warning survives later scans while the content is unchanged.
        let again = store.assess("alpha", Some(&skill), None);
        assert_eq!(again.state, LocalModification::Unknown);
    }

    #[test]
    fn first_scan_with_mismatching_git_hash_is_unknown_not_modified() {
        let (_tmp, store, skill) = fixture();
        let assessment = store.assess(
            "alpha",
            Some(&skill),
            Some("0000000000000000000000000000000000000000"),
        );
        assert_eq!(assessment.state, LocalModification::Unknown);
        assert!(!store.load("alpha").unwrap().verified);
    }

    #[test]
    fn first_scan_with_empty_or_sha256_lock_hashes_is_unknown() {
        let sha256 = "a".repeat(64);
        for hash in ["", sha256.as_str()] {
            let (_tmp, store, skill) = fixture();
            let assessment = store.assess("alpha", Some(&skill), Some(hash));
            assert_eq!(
                assessment.state,
                LocalModification::Unknown,
                "hash {hash:?}"
            );
            assert!(assessment.detail.as_deref().unwrap().contains("hash"));
            assert!(!store.load("alpha").unwrap().verified);
        }
    }

    #[test]
    fn a_modified_unverified_baseline_reports_modified() {
        let (_tmp, store, skill) = fixture();
        store.assess("alpha", Some(&skill), None);
        write(&skill.join("SKILL.md"), "edited after first scan\n");
        let assessment = store.assess("alpha", Some(&skill), None);
        assert_eq!(assessment.state, LocalModification::Modified);
    }

    #[test]
    fn unmanaged_like_assessment_records_a_baseline_for_later_adoption() {
        let (_tmp, store, skill) = fixture();
        let assessment = store.assess("alpha", Some(&skill), None);
        assert_eq!(assessment.state, LocalModification::Unknown);
        assert!(assessment.baseline_created);
        let hash = git_tree_hash(&skill).unwrap();
        let adopted = store.assess("alpha", Some(&skill), Some(&hash));
        assert_eq!(adopted.state, LocalModification::Clean);
        assert!(store.load("alpha").unwrap().verified);
    }

    #[test]
    fn verified_baseline_detects_local_edits_and_returns_to_clean_when_restored() {
        let (_tmp, store, skill) = fixture();
        let hash = git_tree_hash(&skill).unwrap();
        store.assess("alpha", Some(&skill), Some(&hash));
        let original = fs::read_to_string(skill.join("SKILL.md")).unwrap();

        write(&skill.join("SKILL.md"), "edited\n");
        let modified = store.assess("alpha", Some(&skill), Some(&hash));
        assert_eq!(modified.state, LocalModification::Modified);
        assert!(modified.detail.as_deref().unwrap().contains("基线"));

        write(&skill.join("SKILL.md"), &original);
        let clean = store.assess("alpha", Some(&skill), Some(&hash));
        assert_eq!(clean.state, LocalModification::Clean);
    }

    #[test]
    fn unverified_baseline_becomes_clean_once_the_lock_hash_matches() {
        let (_tmp, store, skill) = fixture();
        store.assess("alpha", Some(&skill), None);
        let hash = git_tree_hash(&skill).unwrap();
        let assessment = store.assess("alpha", Some(&skill), Some(&hash));
        assert_eq!(assessment.state, LocalModification::Clean);
        assert!(store.load("alpha").unwrap().verified);
    }

    #[test]
    fn baseline_for_a_different_path_is_not_reused() {
        let (tmp, store, skill) = fixture();
        store.assess("alpha", Some(&skill), None);
        let other = tmp.path().join("other/alpha");
        write(&other.join("SKILL.md"), "other\n");
        let assessment = store.assess("alpha", Some(&other), None);
        assert_eq!(assessment.state, LocalModification::Unknown);
        assert_eq!(
            store.load("alpha").unwrap().path,
            other.to_string_lossy().to_string()
        );
    }

    #[test]
    fn explicit_verified_record_is_reusable_for_install_and_update() {
        let (_tmp, store, skill) = fixture();
        store.record_from_dir("alpha", &skill, true).unwrap();
        let baseline = store.load("alpha").unwrap();
        assert!(baseline.verified);
        assert!(baseline.file_count > 0);
        assert_eq!(baseline.version, BASELINE_VERSION);
    }

    #[test]
    fn corrupt_baseline_files_are_ignored_and_replaced() {
        let (tmp, store, skill) = fixture();
        fs::create_dir_all(store.dir()).unwrap();
        fs::write(store.baseline_path("alpha"), "{not json").unwrap();
        assert!(store.load("alpha").is_none());
        let assessment = store.assess("alpha", Some(&skill), None);
        assert_eq!(assessment.state, LocalModification::Unknown);
        assert!(store.load("alpha").is_some());
        assert!(tmp.path().exists());
    }

    #[test]
    fn baseline_file_names_are_contained_even_for_traversal_like_names() {
        let store = BaselineStore::from_dir("/tmp/baselines");
        for name in ["alpha", "../evil", "ce:review", "a/b"] {
            let path = store.baseline_path(name);
            assert_eq!(path.parent().unwrap(), Path::new("/tmp/baselines"));
        }
        assert_ne!(store.baseline_path("a/b"), store.baseline_path("a_b"));
    }

    #[test]
    fn assess_without_path_is_unknown() {
        let store = BaselineStore::from_dir("/tmp/unused");
        let assessment = store.assess("alpha", None, None);
        assert_eq!(assessment.state, LocalModification::Unknown);
    }

    #[test]
    fn app_paths_store_uses_the_baselines_directory() {
        let (tmp, _store, _skill) = fixture();
        let paths = AppPaths::new(tmp.path().join("home"), tmp.path().join("app-data"));
        assert_eq!(BaselineStore::new(&paths).dir(), paths.baselines_dir());
    }
}
