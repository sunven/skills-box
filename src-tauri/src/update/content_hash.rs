//! CLI-identical content fingerprints for legacy and portable project locks.
//!
//! V1 project locks store `computedHash`, the skills CLI's SHA-256 over a skill
//! folder: every regular file (excluding `.git` and `node_modules`), relative
//! paths with `/`, sorted with JavaScript `localeCompare` (the CLI sorts with
//! `a.relativePath.localeCompare(b.relativePath)`), then update SHA-256 with
//! the path bytes followed by the content bytes.
//!
//! `localeCompare` uses ICU collation, not byte order. This module implements
//! the subset that covers ASCII paths (punctuation < digits < letters, case as
//! a tie-break) and refuses non-ASCII paths: hashing them with a different
//! order would produce a false "update available".
//!
//! V2 uses UTF-8 byte order, `path NUL length NUL content` framing, normalized
//! text line endings and the installer's exclusions (skills@1.8.1 skill-hash.ts).

use std::cmp::Ordering;
use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashVersion {
    Legacy,
    Portable,
}

impl HashVersion {
    pub fn for_project_lock(version: u64) -> Self {
        match version {
            1 => Self::Legacy,
            2 => Self::Portable,
            _ => unreachable!("project lock version must be validated before hashing"),
        }
    }

    pub fn folder_hash(self, root: &Path) -> Result<String, FolderHashError> {
        if self == Self::Legacy {
            return folder_hash(root);
        }
        let root = fs::canonicalize(root).map_err(|e| FolderHashError::Io(e.to_string()))?;
        let mut files = Vec::new();
        collect_portable_files(&root, &root, &mut Vec::new(), &mut files)?;
        let entries = files
            .into_iter()
            .map(|(path, full)| {
                fs::read(full)
                    .map(|content| (path, content))
                    .map_err(|e| FolderHashError::Io(e.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.hash_entries(entries)
    }

    pub fn hash_entries(self, entries: Vec<(String, Vec<u8>)>) -> Result<String, FolderHashError> {
        if self == Self::Legacy {
            return hash_entries(entries);
        }
        let mut entries: Vec<_> = entries
            .into_iter()
            .filter_map(|(path, content)| {
                let path = path.replace('\\', "/");
                (!self.excludes_path(&path)).then(|| (path, normalize_line_endings(content)))
            })
            .collect();
        entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        let mut hash = Sha256::new();
        for (path, content) in entries {
            hash.update(path.as_bytes());
            hash.update(b"\0");
            hash.update(content.len().to_string().as_bytes());
            hash.update(b"\0");
            hash.update(content);
        }
        Ok(hex(&hash.finalize()))
    }

    pub(crate) fn excludes_path(self, path: &str) -> bool {
        let mut parts = path.split('/').peekable();
        while let Some(part) = parts.next() {
            let directory = parts.peek().is_some();
            if match self {
                Self::Legacy => directory && matches!(part, ".git" | "node_modules"),
                Self::Portable => {
                    part == "metadata.json"
                        || directory && matches!(part, ".git" | "__pycache__" | "__pypackages__")
                }
            } {
                return true;
            }
        }
        false
    }
}

fn normalize_line_endings(content: Vec<u8>) -> Vec<u8> {
    if content.contains(&0) {
        return content;
    }
    let mut normalized = Vec::with_capacity(content.len());
    let mut bytes = content.into_iter().peekable();
    while let Some(byte) = bytes.next() {
        if byte == b'\r' && bytes.peek() == Some(&b'\n') {
            continue;
        }
        normalized.push(byte);
    }
    normalized
}

fn collect_portable_files(
    root: &Path,
    current: &Path,
    ancestors: &mut Vec<std::path::PathBuf>,
    files: &mut Vec<(String, std::path::PathBuf)>,
) -> Result<(), FolderHashError> {
    let real = fs::canonicalize(current).map_err(|e| FolderHashError::Io(e.to_string()))?;
    if ancestors.contains(&real) {
        return Ok(());
    }
    ancestors.push(real);
    for entry in fs::read_dir(current).map_err(|e| FolderHashError::Io(e.to_string()))? {
        let entry = entry.map_err(|e| FolderHashError::Io(e.to_string()))?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .unwrap()
            .to_str()
            .ok_or_else(|| FolderHashError::UnsupportedPath(path.display().to_string()))?
            .to_string();
        let kind = entry
            .file_type()
            .map_err(|e| FolderHashError::Io(e.to_string()))?;
        let directory = if kind.is_symlink() {
            let Ok(target) = fs::canonicalize(&path) else {
                continue;
            };
            if !target.starts_with(root) {
                continue;
            }
            fs::metadata(target)
                .map_err(|e| FolderHashError::Io(e.to_string()))?
                .is_dir()
        } else if kind.is_dir() || kind.is_file() {
            kind.is_dir()
        } else {
            continue;
        };
        if HashVersion::Portable.excludes_path(&relative)
            || directory && HashVersion::Portable.excludes_path(&format!("{relative}/"))
        {
            continue;
        }
        if directory {
            collect_portable_files(root, &path, ancestors, files)?;
        } else {
            files.push((relative, path));
        }
    }
    ancestors.pop();
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderHashError {
    Io(String),
    /// A path cannot be ordered like the CLI's ICU collation. Comparing it
    /// would be unreliable, so the caller must report "unknown" instead.
    UnsupportedPath(String),
}

impl FolderHashError {
    pub fn detail(&self) -> &str {
        match self {
            Self::Io(detail) | Self::UnsupportedPath(detail) => detail,
        }
    }
}

/// SHA-256 of a skill folder on disk. `root` is the skill directory itself;
/// relative paths never include it.
pub fn folder_hash(root: &Path) -> Result<String, FolderHashError> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    let mut entries = Vec::with_capacity(files.len());
    for (relative_path, full_path) in files {
        let content = fs::read(&full_path).map_err(|error| {
            FolderHashError::Io(format!("无法读取 {}：{error}", full_path.display()))
        })?;
        entries.push((relative_path, content));
    }
    hash_entries(entries)
}

/// SHA-256 over already-extracted files. Paths are relative to the skill
/// folder and use `/` separators.
pub fn hash_entries(mut entries: Vec<(String, Vec<u8>)>) -> Result<String, FolderHashError> {
    entries.retain(|(path, _)| !HashVersion::Legacy.excludes_path(path));
    for (path, _) in &entries {
        if !is_orderable_ascii(path) {
            return Err(FolderHashError::UnsupportedPath(format!(
                "路径包含非 ASCII 字符：{path:?}"
            )));
        }
    }
    entries.sort_by(|left, right| collate_cmp(&left.0, &right.0));
    let mut hasher = Sha256::new();
    for (path, content) in entries {
        hasher.update(path.as_bytes());
        hasher.update(&content);
    }
    Ok(hex(&hasher.finalize()))
}

/// The identity used by every project lock entry: exactly 64 hex characters.
pub fn is_content_hash(raw: &str) -> bool {
    raw.len() == 64 && raw.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn collect_files(
    base: &Path,
    current: &Path,
    results: &mut Vec<(String, std::path::PathBuf)>,
) -> Result<(), FolderHashError> {
    let entries = fs::read_dir(current)
        .map_err(|error| FolderHashError::Io(format!("无法读取 {}：{error}", current.display())))?;
    for entry in entries {
        let entry = entry.map_err(|error| FolderHashError::Io(error.to_string()))?;
        let name = entry.file_name();
        let name = name.to_string_lossy().to_string();
        let full_path = entry.path();
        let metadata = fs::symlink_metadata(&full_path).map_err(|error| {
            FolderHashError::Io(format!("无法读取 {}：{error}", full_path.display()))
        })?;
        let file_type = metadata.file_type();
        if file_type.is_dir() {
            if name == ".git" || name == "node_modules" {
                continue;
            }
            collect_files(base, &full_path, results)?;
        } else if file_type.is_file() {
            let relative = full_path.strip_prefix(base).map_err(|_| {
                FolderHashError::Io(format!("路径不在 skill 目录内：{}", full_path.display()))
            })?;
            let relative = relative
                .components()
                .map(|component| component.as_os_str().to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join("/");
            results.push((relative, full_path));
        }
        // Symlinks are not regular files in Node's `Dirent.isFile`, so the CLI
        // skips them too.
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

/// `true` when every character has a known ICU primary weight in this module.
fn is_orderable_ascii(path: &str) -> bool {
    path.bytes()
        .all(|byte| (0x20..=0x7e).contains(&byte) && primary_weight(byte).is_some())
}

/// Compare two paths the way JavaScript's `localeCompare` does for ASCII:
/// primary weights (punctuation, then digits, then case-folded letters), then
/// case (lowercase before uppercase).
fn collate_cmp(left: &str, right: &str) -> Ordering {
    let left_primary: Vec<u8> = left.bytes().filter_map(primary_weight).collect();
    let right_primary: Vec<u8> = right.bytes().filter_map(primary_weight).collect();
    match left_primary.cmp(&right_primary) {
        Ordering::Equal => {
            let left_case: Vec<bool> = left.bytes().map(is_uppercase).collect();
            let right_case: Vec<bool> = right.bytes().map(is_uppercase).collect();
            left_case.cmp(&right_case)
        }
        other => other,
    }
}

fn is_uppercase(byte: u8) -> bool {
    byte.is_ascii_uppercase()
}

/// ICU collation primary ranks for printable ASCII, derived from
/// `[' '..'~'].sort((a, b) => a.localeCompare(b))` with Node 26 / ICU 78.
/// Letters share the rank of their lowercase form; case is a tie-break.
fn primary_weight(byte: u8) -> Option<u8> {
    let lower = byte.to_ascii_lowercase();
    let rank = match lower {
        b' ' => 0,
        b'_' => 1,
        b'-' => 2,
        b',' => 3,
        b';' => 4,
        b':' => 5,
        b'!' => 6,
        b'?' => 7,
        b'.' => 8,
        b'\'' => 9,
        b'"' => 10,
        b'(' => 11,
        b')' => 12,
        b'[' => 13,
        b']' => 14,
        b'{' => 15,
        b'}' => 16,
        b'@' => 17,
        b'*' => 18,
        b'/' => 19,
        b'\\' => 20,
        b'&' => 21,
        b'#' => 22,
        b'%' => 23,
        b'`' => 24,
        b'^' => 25,
        b'+' => 26,
        b'<' => 27,
        b'=' => 28,
        b'>' => 29,
        b'|' => 30,
        b'~' => 31,
        b'$' => 32,
        b'0'..=b'9' => 33 + (lower - b'0'),
        b'a'..=b'z' => 43 + (lower - b'a'),
        _ => return None,
    };
    Some(rank)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Golden vectors generated with the pinned CLI's exact algorithm
    /// (`skills@1.7.4` `computeSkillFolderHash`) over these fixture files.
    const GOLDEN_FIXTURE: &str = "c2b938685901acfd5e4034a048958b3f3cda94c434443f426e4e262856446697";
    const GOLDEN_COLLATION: &str =
        "039f90c5eca0719c6e308d2632f43868d9e8174444d480771fc23af07f08e9b2";
    const GOLDEN_SINGLE: &str = "3b249edf79dfeb7efa564530ccf0971694cb306a612c4f0bcbb719bcac103311";

    fn write(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    #[test]
    fn matches_cli_golden_vector_for_a_typical_skill_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("alpha");
        write(
            &root.join("SKILL.md"),
            "---\nname: alpha\ndescription: fixture alpha\n---\n\n# Alpha\n",
        );
        write(&root.join("references/guide.md"), "# Guide\n\nSome text.\n");
        write(&root.join("references/nested/deep.txt"), "deep content\n");
        write(&root.join("scripts/run.sh"), "#!/bin/sh\necho run\n");
        write(&root.join("assets/logo.bin"), "not-really-an-image\n");
        // Excluded by the CLI algorithm.
        write(&root.join(".git/config"), "[core]\n");
        write(
            &root.join("node_modules/dep/index.js"),
            "module.exports = 1;\n",
        );

        assert_eq!(folder_hash(&root).unwrap(), GOLDEN_FIXTURE);
    }

    #[test]
    fn matches_cli_golden_vector_for_locale_collation_order() {
        // Path set chosen to separate locale order from byte order:
        // space < _ < - < . < / < letters, lowercase before uppercase.
        let files = [
            ("SKILL.md", "b\n"),
            ("README.md", "readme\n"),
            ("Z.md", "z\n"),
            ("a b.md", "space\n"),
            ("a-b.md", "dash\n"),
            ("a.b.md", "dot\n"),
            ("a/b.md", "slash\n"),
            ("a_b.md", "underscore\n"),
            ("A.md", "upper\n"),
            ("a.md", "lower\n"),
            ("scripts/run.sh", "#!/bin/sh\n"),
        ];
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("collation");
        for (path, content) in files {
            write(&root.join(path), content);
        }
        assert_eq!(folder_hash(&root).unwrap(), GOLDEN_COLLATION);
    }

    #[test]
    fn golden_vectors_match_hash_entries_in_archive_shape() {
        // The single-file vector must hold when files come from an archive
        // instead of disk (paths relative to the skill folder, `/` separators).
        let hash = hash_entries(vec![("SKILL.md".to_string(), b"c\n".to_vec())]).unwrap();
        assert_eq!(hash, GOLDEN_SINGLE);
    }

    #[test]
    fn ignores_empty_directories_and_symlinks_like_the_cli() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("alpha");
        write(&root.join("SKILL.md"), "c\n");
        fs::create_dir_all(root.join("empty/nested")).unwrap();
        std::os::unix::fs::symlink(root.join("SKILL.md"), root.join("link.md")).unwrap();
        assert_eq!(folder_hash(&root).unwrap(), GOLDEN_SINGLE);
    }

    #[test]
    fn refuses_non_ascii_paths_instead_of_hashing_a_different_order() {
        let result = hash_entries(vec![("技能.md".to_string(), b"x".to_vec())]);
        assert!(matches!(result, Err(FolderHashError::UnsupportedPath(_))));
    }

    #[test]
    fn portable_hash_matches_skills_1_8_1_golden_vector_on_disk_and_in_archives() {
        // Generated by the upstream skill-hash.ts hashSkillFiles function.
        const GOLDEN: &str = "5085663b5fef087e834c7c84d936dc8d7d39aee1a1ff9acc81213f8690e28f74";
        let entries: Vec<(String, Vec<u8>)> = vec![
            ("SKILL.md".into(), b"alpha\r\n".to_vec()),
            ("README.md".into(), b"readme\n".to_vec()),
            ("a-lower.md".into(), b"lower\n".to_vec()),
            ("A.md".into(), b"upper\n".to_vec()),
            ("技能.md".into(), "内容\r\n".as_bytes().to_vec()),
            ("assets/logo.bin".into(), vec![0, 13, 10, 255]),
            (
                "node_modules/dep/index.js".into(),
                b"module.exports=1;\n".to_vec(),
            ),
            ("metadata.json".into(), b"excluded".to_vec()),
            ("references/metadata.json/data".into(), b"excluded".to_vec()),
            (".git/config".into(), b"excluded".to_vec()),
            ("__pycache__/a.pyc".into(), b"excluded".to_vec()),
            ("__pypackages__/a.py".into(), b"excluded".to_vec()),
        ];
        let tmp = tempfile::tempdir().unwrap();
        for (path, content) in &entries {
            let full = tmp.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, content).unwrap();
        }
        assert_eq!(
            HashVersion::Portable.folder_hash(tmp.path()).unwrap(),
            GOLDEN
        );
        assert_eq!(HashVersion::Portable.hash_entries(entries).unwrap(), GOLDEN);
    }

    #[test]
    fn portable_framing_distinguishes_path_and_content_boundaries() {
        let a = vec![("a".into(), b"bc".to_vec())];
        let b = vec![("ab".into(), b"c".to_vec())];
        assert_eq!(
            hash_entries(a.clone()).unwrap(),
            hash_entries(b.clone()).unwrap()
        );
        assert_ne!(
            HashVersion::Portable.hash_entries(a).unwrap(),
            HashVersion::Portable.hash_entries(b).unwrap()
        );
    }

    #[test]
    fn portable_folder_hash_follows_internal_links_and_skips_cycles_and_escapes() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("alpha");
        write(&root.join("SKILL.md"), "alpha\n");
        write(&root.join("references/guide.md"), "guide\n");
        write(&tmp.path().join("outside.md"), "outside\n");
        symlink("SKILL.md", root.join("linked.md")).unwrap();
        symlink("references", root.join("alias")).unwrap();
        symlink(".", root.join("cycle")).unwrap();
        symlink("../outside.md", root.join("external.md")).unwrap();
        symlink("absent", root.join("broken.md")).unwrap();
        let expected = HashVersion::Portable
            .hash_entries(vec![
                ("SKILL.md".into(), b"alpha\n".to_vec()),
                ("references/guide.md".into(), b"guide\n".to_vec()),
                ("linked.md".into(), b"alpha\n".to_vec()),
                ("alias/guide.md".into(), b"guide\n".to_vec()),
            ])
            .unwrap();
        assert_eq!(HashVersion::Portable.folder_hash(&root).unwrap(), expected);
    }

    #[test]
    fn recognises_only_64_hex_content_hashes() {
        assert!(is_content_hash(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        ));
        assert!(is_content_hash(
            "0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF"
        ));
        assert!(!is_content_hash("0123456789abcdef0123456789abcdef01234567"));
        assert!(!is_content_hash(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdeg"
        ));
    }
}
