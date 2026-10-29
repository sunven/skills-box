//! Read-only skill content browsing.
//!
//! The frontend sends a server-resolved skill id plus a relative path. Every
//! path is validated lexically, canonicalized, and checked against the skill
//! root before a directory is listed, a file is read, or a file is opened.

use serde::Serialize;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

use crate::detail::SKILL_MD_LIMIT_BYTES;
use crate::error::AppError;
use crate::paths::AppPaths;

pub const CONTENT_MAX_ENTRIES: usize = 500;
pub const CONTENT_MAX_SCAN: usize = 2_000;
pub const CONTENT_MAX_DEPTH: usize = 12;
pub const CONTENT_TEXT_LIMIT_BYTES: u64 = SKILL_MD_LIMIT_BYTES;
pub const CONTENT_IMAGE_LIMIT_BYTES: u64 = 5 * 1024 * 1024;

const HIDDEN_NAMES: [&str; 3] = [".git", "node_modules", ".DS_Store"];
const IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "svg"];
const BINARY_SNIFF_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillEntryKind {
    File,
    Dir,
    Broken,
    Outside,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDirEntry {
    pub name: String,
    /// Relative to the skill root, using `/` separators.
    pub path: String,
    pub kind: SkillEntryKind,
    pub symlink: bool,
    pub size_bytes: Option<u64>,
    pub markdown: bool,
    pub expandable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDirListing {
    pub path: String,
    pub display_path: String,
    pub entries: Vec<SkillDirEntry>,
    pub truncated: bool,
    pub max_entries: usize,
    pub max_depth: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillFileStatus {
    Ok,
    TooLarge,
    Binary,
    Unreadable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillFileKind {
    Markdown,
    Text,
    Image,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillFileContent {
    pub path: String,
    pub display_path: String,
    pub status: SkillFileStatus,
    pub kind: SkillFileKind,
    pub content: Option<String>,
    pub data_url: Option<String>,
    pub size_bytes: Option<u64>,
    pub limit_bytes: u64,
    pub error: Option<String>,
}

pub fn list(paths: &AppPaths, root: &Path, relative: &str) -> Result<SkillDirListing, AppError> {
    let root_canonical = canonical_root(root)?;
    let segments = validate_relative_path(relative)?;
    let directory = resolve_segments(root, &root_canonical, &segments)?;
    let metadata = fs::metadata(&directory)
        .map_err(|error| AppError::path_invalid(format!("无法读取目录信息：{error}")))?;
    if !metadata.is_dir() {
        return Err(AppError::path_invalid("目标不是目录。"));
    }

    let depth = segments.len();
    let prefix = segments.join("/");
    let mut entries = Vec::new();
    let mut truncated = false;

    let read_dir = fs::read_dir(&directory)
        .map_err(|error| AppError::path_invalid(format!("无法列出目录：{error}")))?;
    for (scanned, item) in read_dir.enumerate() {
        if scanned >= CONTENT_MAX_SCAN {
            truncated = true;
            break;
        }
        let item = match item {
            Ok(item) => item,
            Err(_) => continue,
        };
        let name = match item.file_name().into_string() {
            Ok(name) => name,
            Err(_) => continue,
        };
        if HIDDEN_NAMES.contains(&name.as_str()) {
            continue;
        }
        let file_type = match item.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };
        let symlink = file_type.is_symlink();
        let (kind, size_bytes) = if symlink {
            match fs::metadata(item.path()) {
                Ok(target) => {
                    let inside = fs::canonicalize(item.path())
                        .map(|path| path.starts_with(&root_canonical))
                        .unwrap_or(false);
                    if !inside {
                        (SkillEntryKind::Outside, None)
                    } else if target.is_dir() {
                        (SkillEntryKind::Dir, None)
                    } else if target.is_file() {
                        (SkillEntryKind::File, Some(target.len()))
                    } else {
                        continue;
                    }
                }
                Err(_) => (SkillEntryKind::Broken, None),
            }
        } else if file_type.is_dir() {
            (SkillEntryKind::Dir, None)
        } else if file_type.is_file() {
            (
                SkillEntryKind::File,
                item.metadata().ok().map(|meta| meta.len()),
            )
        } else {
            continue;
        };

        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        entries.push(SkillDirEntry {
            markdown: kind == SkillEntryKind::File && is_markdown(&name),
            expandable: kind == SkillEntryKind::Dir && depth < CONTENT_MAX_DEPTH,
            name,
            path,
            kind,
            symlink,
            size_bytes,
        });
    }

    entries.sort_by(|left, right| {
        rank(left.kind)
            .cmp(&rank(right.kind))
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.name.cmp(&right.name))
    });
    if entries.len() > CONTENT_MAX_ENTRIES {
        entries.truncate(CONTENT_MAX_ENTRIES);
        truncated = true;
    }

    let display_root = paths.display(root);
    let display_path = if prefix.is_empty() {
        display_root
    } else {
        format!("{display_root}/{prefix}")
    };
    Ok(SkillDirListing {
        path: prefix,
        display_path,
        entries,
        truncated,
        max_entries: CONTENT_MAX_ENTRIES,
        max_depth: CONTENT_MAX_DEPTH,
    })
}

pub fn read_file(
    paths: &AppPaths,
    root: &Path,
    relative: &str,
) -> Result<SkillFileContent, AppError> {
    let segments = validate_relative_path(relative)?;
    if segments.is_empty() {
        return Err(AppError::path_invalid("必须指定 skill 内的文件。"));
    }
    let root_canonical = canonical_root(root)?;
    let path = resolve_segments(root, &root_canonical, &segments)?;
    let display_path = format!("{}/{}", paths.display(root), segments.join("/"));
    let name = segments.last().expect("non-empty segments");
    let kind = file_kind(name);
    let limit = limit_for(kind);

    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) => {
            return Ok(file_value(
                relative,
                display_path,
                SkillFileStatus::Unreadable,
                kind,
                None,
                None,
                None,
                limit,
                Some(error.to_string()),
            ));
        }
    };
    if !metadata.is_file() {
        return Err(AppError::path_invalid("目标不是普通文件。"));
    }
    if metadata.len() > limit {
        return Ok(file_value(
            relative,
            display_path,
            SkillFileStatus::TooLarge,
            kind,
            None,
            None,
            Some(metadata.len()),
            limit,
            None,
        ));
    }

    let file = match File::open(&path) {
        Ok(file) => file,
        Err(error) => {
            return Ok(file_value(
                relative,
                display_path,
                SkillFileStatus::Unreadable,
                kind,
                None,
                None,
                Some(metadata.len()),
                limit,
                Some(error.to_string()),
            ));
        }
    };
    let mut bytes = Vec::new();
    if let Err(error) = file.take(limit + 1).read_to_end(&mut bytes) {
        return Ok(file_value(
            relative,
            display_path,
            SkillFileStatus::Unreadable,
            kind,
            None,
            None,
            Some(metadata.len()),
            limit,
            Some(error.to_string()),
        ));
    }
    if bytes.len() as u64 > limit {
        return Ok(file_value(
            relative,
            display_path,
            SkillFileStatus::TooLarge,
            kind,
            None,
            None,
            Some(bytes.len() as u64),
            limit,
            None,
        ));
    }

    match kind {
        SkillFileKind::Image => {
            let mime = image_mime(name).expect("image kind implies a supported extension");
            let data_url = format!("data:{mime};base64,{}", BASE64.encode(&bytes));
            Ok(file_value(
                relative,
                display_path,
                SkillFileStatus::Ok,
                kind,
                None,
                Some(data_url),
                Some(metadata.len()),
                limit,
                None,
            ))
        }
        SkillFileKind::Markdown | SkillFileKind::Text => {
            let size_bytes = Some(metadata.len());
            if bytes.iter().take(BINARY_SNIFF_BYTES).any(|byte| *byte == 0) {
                return Ok(file_value(
                    relative,
                    display_path,
                    SkillFileStatus::Binary,
                    kind,
                    None,
                    None,
                    size_bytes,
                    limit,
                    None,
                ));
            }
            match String::from_utf8(bytes) {
                Ok(content) => Ok(file_value(
                    relative,
                    display_path,
                    SkillFileStatus::Ok,
                    kind,
                    Some(content),
                    None,
                    size_bytes,
                    limit,
                    None,
                )),
                Err(_) => Ok(file_value(
                    relative,
                    display_path,
                    SkillFileStatus::Binary,
                    kind,
                    None,
                    None,
                    size_bytes,
                    limit,
                    None,
                )),
            }
        }
    }
}

/// Resolve and guard one file for `open_skill_file`. The returned path is
/// canonical and is guaranteed to point at a regular file inside the skill.
pub fn resolve_open_path(root: &Path, relative: &str) -> Result<PathBuf, AppError> {
    let segments = validate_relative_path(relative)?;
    if segments.is_empty() {
        return Err(AppError::path_invalid("必须指定 skill 内的文件。"));
    }
    let root_canonical = canonical_root(root)?;
    let path = resolve_segments(root, &root_canonical, &segments)?;
    let metadata = fs::metadata(&path)
        .map_err(|error| AppError::path_invalid(format!("无法读取文件信息：{error}")))?;
    if !metadata.is_file() {
        return Err(AppError::path_invalid("目标不是普通文件。"));
    }
    Ok(path)
}

fn canonical_root(root: &Path) -> Result<PathBuf, AppError> {
    fs::canonicalize(root)
        .map_err(|error| AppError::path_invalid(format!("无法解析 skill 目录：{error}")))
}

fn resolve_segments(
    root: &Path,
    root_canonical: &Path,
    segments: &[String],
) -> Result<PathBuf, AppError> {
    let mut candidate = root.to_path_buf();
    for segment in segments {
        candidate.push(segment);
    }
    let resolved = fs::canonicalize(&candidate).map_err(|error| {
        AppError::path_invalid(format!("无法解析内容路径 {}：{error}", candidate.display()))
    })?;
    if !resolved.starts_with(root_canonical) {
        return Err(AppError::path_outside_allowed_root(format!(
            "{} -> {}",
            candidate.display(),
            resolved.display()
        )));
    }
    Ok(resolved)
}

fn validate_relative_path(raw: &str) -> Result<Vec<String>, AppError> {
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    if raw.len() > 4096
        || raw.trim() != raw
        || raw.starts_with('/')
        || raw.contains('\\')
        || raw.contains('\0')
    {
        return Err(AppError::path_invalid(
            "内容路径必须是 skill 内的相对路径。",
        ));
    }
    let mut segments = Vec::new();
    for segment in raw.split('/') {
        if segment.is_empty()
            || segment == "."
            || segment == ".."
            || segment.len() > 255
            || HIDDEN_NAMES.contains(&segment)
        {
            return Err(AppError::path_invalid(format!(
                "内容路径包含不允许的片段：{segment:?}"
            )));
        }
        segments.push(segment.to_string());
    }
    if segments.len() > CONTENT_MAX_DEPTH {
        return Err(AppError::path_invalid(format!(
            "目录层级超过上限（{CONTENT_MAX_DEPTH} 层）。"
        )));
    }
    Ok(segments)
}

fn rank(kind: SkillEntryKind) -> u8 {
    match kind {
        SkillEntryKind::Dir => 0,
        SkillEntryKind::File => 1,
        SkillEntryKind::Broken => 2,
        SkillEntryKind::Outside => 3,
    }
}

fn extension_lower(name: &str) -> Option<String> {
    Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
}

fn is_markdown(name: &str) -> bool {
    matches!(extension_lower(name).as_deref(), Some("md" | "markdown"))
}

fn file_kind(name: &str) -> SkillFileKind {
    match extension_lower(name).as_deref() {
        Some("md" | "markdown") => SkillFileKind::Markdown,
        Some(extension) if IMAGE_EXTENSIONS.contains(&extension) => SkillFileKind::Image,
        _ => SkillFileKind::Text,
    }
}

fn image_mime(name: &str) -> Option<&'static str> {
    match extension_lower(name).as_deref() {
        Some("png") => Some("image/png"),
        Some("jpg" | "jpeg") => Some("image/jpeg"),
        Some("gif") => Some("image/gif"),
        Some("webp") => Some("image/webp"),
        Some("svg") => Some("image/svg+xml"),
        _ => None,
    }
}

fn limit_for(kind: SkillFileKind) -> u64 {
    match kind {
        SkillFileKind::Image => CONTENT_IMAGE_LIMIT_BYTES,
        SkillFileKind::Markdown | SkillFileKind::Text => CONTENT_TEXT_LIMIT_BYTES,
    }
}

#[allow(clippy::too_many_arguments)]
fn file_value(
    path: &str,
    display_path: String,
    status: SkillFileStatus,
    kind: SkillFileKind,
    content: Option<String>,
    data_url: Option<String>,
    size_bytes: Option<u64>,
    limit_bytes: u64,
    error: Option<String>,
) -> SkillFileContent {
    SkillFileContent {
        path: path.to_string(),
        display_path,
        status,
        kind,
        content,
        data_url,
        size_bytes,
        limit_bytes,
        error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use std::os::unix::fs::{PermissionsExt, symlink};

    struct Fixture {
        _tmp: tempfile::TempDir,
        paths: AppPaths,
        canonical: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path().join("home");
            fs::create_dir_all(home.join(".agents/skills")).unwrap();
            let canonical = home.join(".agents/skills");
            let paths = AppPaths::new(&home, tmp.path().join("app-data"));
            Self {
                _tmp: tmp,
                paths,
                canonical,
            }
        }

        fn write_lock(&self, entries: &[(&str, String)]) {
            let mut skills = serde_json::Map::new();
            for (name, hash) in entries {
                skills.insert(
                    (*name).to_string(),
                    serde_json::json!({
                        "source": "acme/skills",
                        "sourceType": "github",
                        "sourceUrl": "https://github.com/acme/skills.git",
                        "ref": "main",
                        "skillPath": format!("skills/{name}/SKILL.md"),
                        "skillFolderHash": hash,
                        "installedAt": "2026-01-01T00:00:00.000Z",
                        "updatedAt": "2026-02-03T04:05:06.000Z"
                    }),
                );
            }
            let lock = serde_json::json!({"version": 3, "skills": skills});
            fs::create_dir_all(self.paths.agents_root()).unwrap();
            fs::write(self.paths.lock_path(), lock.to_string()).unwrap();
        }

        fn canonical_skill(&self, name: &str) -> PathBuf {
            let dir = self.canonical.join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: {name} 的描述\n---\n# {name}\n"),
            )
            .unwrap();
            dir
        }

        fn managed_skill(&self, name: &str) -> PathBuf {
            let dir = self.canonical_skill(name);
            self.write_lock(&[(name, "0".repeat(40))]);
            dir
        }

        fn link(&self, root: &Path, name: &str, target: &Path) {
            symlink(target, root.join(name)).unwrap();
        }

        fn root(&self, name: &str) -> PathBuf {
            self.canonical.join(name)
        }

        fn list(&self, name: &str, relative: &str) -> Result<SkillDirListing, AppError> {
            list(&self.paths, &self.root(name), relative)
        }

        fn read(&self, name: &str, relative: &str) -> Result<SkillFileContent, AppError> {
            read_file(&self.paths, &self.root(name), relative)
        }
    }

    #[test]
    fn lists_directories_first_and_skips_hidden_entries() {
        let fixture = Fixture::new();
        let dir = fixture.managed_skill("alpha");
        fs::create_dir_all(dir.join("references")).unwrap();
        fs::create_dir_all(dir.join("assets")).unwrap();
        fs::create_dir_all(dir.join(".git")).unwrap();
        fs::create_dir_all(dir.join("node_modules")).unwrap();
        fs::write(dir.join(".gitignore"), "target\n").unwrap();
        fs::write(dir.join(".DS_Store"), "x").unwrap();
        fs::write(dir.join("run.sh"), "#!/bin/sh\n").unwrap();
        fs::write(dir.join("references/api.md"), "# API\n").unwrap();

        let listing = fixture.list("alpha", "").unwrap();
        let names = listing
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec!["assets", "references", ".gitignore", "run.sh", "SKILL.md"]
        );
        assert!(!names.contains(&".git"));
        assert!(!names.contains(&"node_modules"));
        assert!(!names.contains(&".DS_Store"));
        assert_eq!(listing.path, "");
        assert_eq!(listing.display_path, "~/.agents/skills/alpha");
        assert!(!listing.truncated);
        assert_eq!(listing.max_entries, CONTENT_MAX_ENTRIES);
        assert_eq!(listing.max_depth, CONTENT_MAX_DEPTH);

        let assets = listing
            .entries
            .iter()
            .find(|entry| entry.name == "assets")
            .unwrap();
        assert_eq!(assets.kind, SkillEntryKind::Dir);
        assert!(assets.expandable);
        assert_eq!(assets.size_bytes, None);
        assert!(!assets.symlink);

        let run = listing
            .entries
            .iter()
            .find(|entry| entry.name == "run.sh")
            .unwrap();
        assert_eq!(run.kind, SkillEntryKind::File);
        assert_eq!(run.path, "run.sh");
        assert_eq!(run.size_bytes, Some(10));
        assert!(!run.markdown);

        let skill_md = listing
            .entries
            .iter()
            .find(|entry| entry.name == "SKILL.md")
            .unwrap();
        assert!(skill_md.markdown);
        assert!(!skill_md.expandable);
    }

    #[test]
    fn nested_listings_use_relative_paths_and_markdown_flags() {
        let fixture = Fixture::new();
        let dir = fixture.managed_skill("alpha");
        fs::create_dir_all(dir.join("references/nested")).unwrap();
        fs::write(dir.join("references/api.md"), "# API\n").unwrap();
        fs::write(dir.join("references/notes.txt"), "notes\n").unwrap();

        let listing = fixture.list("alpha", "references").unwrap();
        assert_eq!(listing.path, "references");
        assert_eq!(listing.display_path, "~/.agents/skills/alpha/references");
        let nested = listing
            .entries
            .iter()
            .find(|entry| entry.name == "nested")
            .unwrap();
        assert_eq!(nested.path, "references/nested");
        assert!(nested.expandable);
        let api = listing
            .entries
            .iter()
            .find(|entry| entry.name == "api.md")
            .unwrap();
        assert_eq!(api.path, "references/api.md");
        assert!(api.markdown);

        let nested_listing = fixture.list("alpha", "references/nested").unwrap();
        assert_eq!(nested_listing.path, "references/nested");
        assert_eq!(
            nested_listing.display_path,
            "~/.agents/skills/alpha/references/nested"
        );
    }

    #[test]
    fn rejects_traversal_absolute_and_hidden_segments() {
        let fixture = Fixture::new();
        fixture.managed_skill("alpha");
        for relative in [
            "../alpha",
            "/etc",
            "a//b",
            "a/./b",
            "a/../b",
            "a\\b",
            "a\0b",
            ".git/config",
            "node_modules/pkg/index.js",
            ".DS_Store",
            " spaced",
        ] {
            let error = fixture.list("alpha", relative).unwrap_err();
            assert_eq!(error.code, ErrorCode::PathInvalid, "{relative:?}");
        }
        let too_deep = (0..=CONTENT_MAX_DEPTH)
            .map(|index| format!("d{index}"))
            .collect::<Vec<_>>()
            .join("/");
        let error = fixture.list("alpha", &too_deep).unwrap_err();
        assert_eq!(error.code, ErrorCode::PathInvalid);
        assert!(error.detail.as_deref().unwrap_or("").contains("目录层级"));
    }

    #[test]
    fn truncates_large_directories() {
        let fixture = Fixture::new();
        let dir = fixture.managed_skill("alpha");
        for index in 0..CONTENT_MAX_ENTRIES + 5 {
            fs::write(dir.join(format!("file-{index:04}.txt")), "x").unwrap();
        }
        let listing = fixture.list("alpha", "").unwrap();
        assert_eq!(listing.entries.len(), CONTENT_MAX_ENTRIES);
        assert!(listing.truncated);
    }

    #[test]
    fn symlinks_inside_outside_and_broken_are_classified() {
        let fixture = Fixture::new();
        let dir = fixture.managed_skill("alpha");
        fs::create_dir_all(dir.join("references")).unwrap();
        fs::write(dir.join("references/api.md"), "# API\n").unwrap();
        fixture.link(&dir, "inside.md", &dir.join("references/api.md"));
        fixture.link(&dir, "inside-dir", &dir.join("references"));
        let outside = fixture._tmp.path().join("outside.txt");
        fs::write(&outside, "secret").unwrap();
        fixture.link(&dir, "outside.txt", &outside);
        fixture.link(&dir, "broken.txt", &dir.join("missing.txt"));

        let listing = fixture.list("alpha", "").unwrap();
        let inside = listing
            .entries
            .iter()
            .find(|entry| entry.name == "inside.md")
            .unwrap();
        assert_eq!(inside.kind, SkillEntryKind::File);
        assert!(inside.symlink);
        assert_eq!(inside.size_bytes, Some(6));
        let inside_dir = listing
            .entries
            .iter()
            .find(|entry| entry.name == "inside-dir")
            .unwrap();
        assert_eq!(inside_dir.kind, SkillEntryKind::Dir);
        assert!(inside_dir.expandable);
        let outside_entry = listing
            .entries
            .iter()
            .find(|entry| entry.name == "outside.txt")
            .unwrap();
        assert_eq!(outside_entry.kind, SkillEntryKind::Outside);
        let broken = listing
            .entries
            .iter()
            .find(|entry| entry.name == "broken.txt")
            .unwrap();
        assert_eq!(broken.kind, SkillEntryKind::Broken);

        assert_eq!(
            fixture.read("alpha", "inside.md").unwrap().status,
            SkillFileStatus::Ok
        );
        assert_eq!(
            fixture.list("alpha", "inside-dir").unwrap().path,
            "inside-dir"
        );
        assert_eq!(
            fixture.read("alpha", "outside.txt").unwrap_err().code,
            ErrorCode::PathOutsideAllowedRoot
        );
        assert_eq!(
            fixture.read("alpha", "broken.txt").unwrap_err().code,
            ErrorCode::PathInvalid
        );
    }

    #[test]
    fn reads_text_markdown_and_reports_binary_and_oversized_files() {
        let fixture = Fixture::new();
        let dir = fixture.managed_skill("alpha");
        fs::write(dir.join("notes.txt"), "第一行\n第二行\n").unwrap();
        fs::write(dir.join("blob.bin"), b"abc\0def").unwrap();
        fs::write(dir.join("invalid.txt"), [0xff, 0xfe, 0xfd]).unwrap();
        let huge = File::create(dir.join("huge.md")).unwrap();
        huge.set_len(CONTENT_TEXT_LIMIT_BYTES + 1).unwrap();

        let md = fixture.read("alpha", "SKILL.md").unwrap();
        assert_eq!(md.status, SkillFileStatus::Ok);
        assert_eq!(md.kind, SkillFileKind::Markdown);
        assert!(md.content.unwrap().contains("# alpha"));
        assert_eq!(md.limit_bytes, CONTENT_TEXT_LIMIT_BYTES);

        let text = fixture.read("alpha", "notes.txt").unwrap();
        assert_eq!(text.status, SkillFileStatus::Ok);
        assert_eq!(text.kind, SkillFileKind::Text);
        assert_eq!(text.content.as_deref(), Some("第一行\n第二行\n"));

        let binary = fixture.read("alpha", "blob.bin").unwrap();
        assert_eq!(binary.status, SkillFileStatus::Binary);
        assert!(binary.content.is_none());

        let invalid = fixture.read("alpha", "invalid.txt").unwrap();
        assert_eq!(invalid.status, SkillFileStatus::Binary);

        let oversized = fixture.read("alpha", "huge.md").unwrap();
        assert_eq!(oversized.status, SkillFileStatus::TooLarge);
        assert_eq!(oversized.size_bytes, Some(CONTENT_TEXT_LIMIT_BYTES + 1));
        assert!(oversized.content.is_none());
    }

    #[test]
    fn reads_images_as_data_urls_and_caps_them() {
        let fixture = Fixture::new();
        let dir = fixture.managed_skill("alpha");
        fs::write(dir.join("pixel.png"), [0x89, b'P', b'N', b'G']).unwrap();
        fs::write(
            dir.join("icon.svg"),
            "<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
        )
        .unwrap();
        let huge = File::create(dir.join("huge.png")).unwrap();
        huge.set_len(CONTENT_IMAGE_LIMIT_BYTES + 1).unwrap();

        let png = fixture.read("alpha", "pixel.png").unwrap();
        assert_eq!(png.status, SkillFileStatus::Ok);
        assert_eq!(png.kind, SkillFileKind::Image);
        assert!(
            png.data_url
                .as_deref()
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
        assert!(png.content.is_none());

        let svg = fixture.read("alpha", "icon.svg").unwrap();
        assert_eq!(svg.kind, SkillFileKind::Image);
        assert!(
            svg.data_url
                .as_deref()
                .unwrap()
                .starts_with("data:image/svg+xml;base64,")
        );

        let oversized = fixture.read("alpha", "huge.png").unwrap();
        assert_eq!(oversized.status, SkillFileStatus::TooLarge);
        assert_eq!(oversized.limit_bytes, CONTENT_IMAGE_LIMIT_BYTES);
    }

    #[test]
    fn rejects_directories_and_the_skill_root_as_files() {
        let fixture = Fixture::new();
        let dir = fixture.managed_skill("alpha");
        fs::create_dir_all(dir.join("references")).unwrap();

        assert_eq!(
            fixture.read("alpha", "").unwrap_err().code,
            ErrorCode::PathInvalid
        );
        assert_eq!(
            fixture.read("alpha", "references").unwrap_err().code,
            ErrorCode::PathInvalid
        );
    }

    #[test]
    fn unreadable_files_report_a_readable_status() {
        let fixture = Fixture::new();
        let dir = fixture.managed_skill("alpha");
        let secret = dir.join("secret.txt");
        fs::write(&secret, "secret\n").unwrap();
        fs::set_permissions(&secret, fs::Permissions::from_mode(0o000)).unwrap();
        let result = fixture.read("alpha", "secret.txt").unwrap();
        fs::set_permissions(&secret, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(result.status, SkillFileStatus::Unreadable);
        assert!(result.error.is_some());
    }

    #[test]
    fn open_path_resolves_files_and_rejects_directories_and_traversal() {
        let fixture = Fixture::new();
        let dir = fixture.managed_skill("alpha");
        fs::create_dir_all(dir.join("references")).unwrap();
        fs::write(dir.join("references/api.md"), "# API\n").unwrap();

        let opened = resolve_open_path(&fixture.root("alpha"), "references/api.md").unwrap();
        assert_eq!(
            opened,
            fs::canonicalize(dir.join("references/api.md")).unwrap()
        );

        for relative in ["", "references", "../alpha", ".git/config"] {
            let error = resolve_open_path(&fixture.root("alpha"), relative).unwrap_err();
            assert_eq!(error.code, ErrorCode::PathInvalid, "{relative:?}");
        }
    }

    #[test]
    fn unmanaged_skills_are_browsable() {
        let fixture = Fixture::new();
        let dir = fixture.canonical_skill("gamma");
        fs::write(dir.join("notes.txt"), "notes\n").unwrap();
        let listing = fixture.list("gamma", "").unwrap();
        assert!(
            listing
                .entries
                .iter()
                .any(|entry| entry.name == "notes.txt")
        );
        assert_eq!(
            fixture.read("gamma", "notes.txt").unwrap().status,
            SkillFileStatus::Ok
        );
    }
}
