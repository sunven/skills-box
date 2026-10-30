//! Source archives for project content fingerprints.
//!
//! Project update checks compare a SHA-256 content hash, so they need the
//! remote files, not the Git tree metadata the global check uses. For each
//! `(source, ref)` group the checker resolves the latest commit, then downloads
//! the codeload archive once per commit. Archives are size- and time-capped and
//! cached by commit under app data `cache/archives/`.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::cache::is_fresh;
use super::github::{GITHUB_API_BASE, GitHubFetchError};
use crate::paths::AppPaths;

/// Hard caps from spec §9/§10.4: one archive download per source per commit,
/// with bounded memory and a bounded wait.
pub const ARCHIVE_MAX_BYTES: u64 = 64 * 1024 * 1024;
pub const ARCHIVE_MAX_UNCOMPRESSED_BYTES: u64 = 256 * 1024 * 1024;
pub const ARCHIVE_MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
pub const ARCHIVE_MAX_FILES: usize = 20_000;
pub const ARCHIVE_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60);
pub const ARCHIVE_CACHE_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveLimits {
    pub max_compressed_bytes: u64,
    pub max_uncompressed_bytes: u64,
    pub max_file_bytes: u64,
    pub max_files: usize,
}

impl Default for ArchiveLimits {
    fn default() -> Self {
        Self {
            max_compressed_bytes: ARCHIVE_MAX_BYTES,
            max_uncompressed_bytes: ARCHIVE_MAX_UNCOMPRESSED_BYTES,
            max_file_bytes: ARCHIVE_MAX_FILE_BYTES,
            max_files: ARCHIVE_MAX_FILES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveError {
    TooLarge { detail: String },
    TooManyFiles { limit: usize },
    Invalid { detail: String },
    Fetch(GitHubFetchError),
}

impl ArchiveError {
    pub fn to_app_error(&self) -> crate::error::AppError {
        match self {
            Self::Fetch(error) => error.to_app_error(),
            Self::TooLarge { detail } => crate::error::AppError::network_error(format!(
                "来源归档超过大小上限，已跳过该来源：{detail}"
            )),
            Self::TooManyFiles { limit } => crate::error::AppError::network_error(format!(
                "来源归档文件数超过上限（{limit} 个），已跳过该来源。"
            )),
            Self::Invalid { detail } => {
                crate::error::AppError::network_error(format!("来源归档无法解析：{detail}"))
            }
        }
    }
}

impl From<GitHubFetchError> for ArchiveError {
    fn from(error: GitHubFetchError) -> Self {
        Self::Fetch(error)
    }
}

/// Files of one commit, keyed by repository-relative path with `/`
/// separators. Directory entries, symlinks and installer exclusions are
/// dropped while parsing, so the map is exactly what the CLI hashes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceArchive {
    pub files: BTreeMap<String, Vec<u8>>,
}

impl SourceArchive {
    /// Parse a GitHub codeload `tar.gz` (a single root directory named
    /// `<repo>-<sha>/`). Caps apply while reading so a hostile archive cannot
    /// exhaust memory.
    pub fn from_tar_gz(raw: &[u8], limits: ArchiveLimits) -> Result<Self, ArchiveError> {
        let mut decoder = flate2::read::GzDecoder::new(raw);
        let mut archive = tar::Archive::new(&mut decoder);
        let mut files = BTreeMap::new();
        let mut total: u64 = 0;
        for entry in archive.entries().map_err(|error| ArchiveError::Invalid {
            detail: error.to_string(),
        })? {
            let mut entry = entry.map_err(|error| ArchiveError::Invalid {
                detail: error.to_string(),
            })?;
            if !entry.header().entry_type().is_file() {
                continue;
            }
            let path = entry.path().map_err(|error| ArchiveError::Invalid {
                detail: error.to_string(),
            })?;
            let Some(relative) = strip_archive_root(&path) else {
                continue;
            };
            if is_installer_exclusion(&relative) {
                continue;
            }
            if files.len() >= limits.max_files {
                return Err(ArchiveError::TooManyFiles {
                    limit: limits.max_files,
                });
            }
            let size = entry.size();
            if size > limits.max_file_bytes {
                return Err(ArchiveError::TooLarge {
                    detail: format!("{relative} 为 {size} 字节"),
                });
            }
            total = total.saturating_add(size);
            if total > limits.max_uncompressed_bytes {
                return Err(ArchiveError::TooLarge {
                    detail: format!("解压后超过 {} 字节", limits.max_uncompressed_bytes),
                });
            }
            let mut content = Vec::with_capacity(size as usize);
            entry
                .read_to_end(&mut content)
                .map_err(|error| ArchiveError::Invalid {
                    detail: error.to_string(),
                })?;
            files.insert(relative, content);
        }
        Ok(Self { files })
    }

    /// Files below `folder` with the folder prefix removed, ready for
    /// `content_hash::hash_entries`.
    pub fn files_under(&self, folder: &str) -> Vec<(String, Vec<u8>)> {
        if folder.is_empty() {
            // `skillPath` at the repository root: the skill folder is the
            // repository root, so every file is relative to it.
            return self
                .files
                .iter()
                .map(|(path, content)| (path.clone(), content.clone()))
                .collect();
        }
        let prefix = format!("{folder}/");
        self.files
            .iter()
            .filter_map(|(path, content)| {
                path.strip_prefix(&prefix)
                    .map(|relative| (relative.to_string(), content.clone()))
            })
            .collect()
    }

    pub fn contains_file(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    pub fn contains_folder(&self, folder: &str) -> bool {
        let prefix = format!("{folder}/");
        self.files.keys().any(|path| path.starts_with(&prefix))
    }
}

fn strip_archive_root(path: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(part) => parts.push(part.to_string_lossy().to_string()),
            std::path::Component::CurDir => {}
            _ => return None,
        }
    }
    // The first component is the codeload root directory (`<repo>-<sha>`).
    if parts.len() < 2 {
        return None;
    }
    let relative = parts[1..].join("/");
    if relative.split('/').any(|part| part == "..") {
        return None;
    }
    Some(relative)
}

fn is_installer_exclusion(path: &str) -> bool {
    path.split('/')
        .any(|part| part == ".git" || part == "node_modules")
}

/// Resolve a commit and download the matching archive. One implementation is
/// used in production; tests provide a scripted fake.
#[async_trait]
pub trait ArchiveClient: Send + Sync {
    /// Latest commit SHA for `source` at the lock's ref (default branch when
    /// no ref is pinned). Must be a 40-character Git SHA.
    async fn latest_commit(
        &self,
        source: &str,
        reference: Option<&str>,
    ) -> Result<String, GitHubFetchError>;

    /// Raw `tar.gz` bytes. Implementations enforce the compressed size cap.
    async fn download_archive(
        &self,
        source: &str,
        commit: &str,
    ) -> Result<Vec<u8>, GitHubFetchError>;
}

#[derive(Debug, Clone)]
pub struct ReqwestArchiveClient {
    client: reqwest::Client,
}

impl ReqwestArchiveClient {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(ARCHIVE_DOWNLOAD_TIMEOUT)
            .user_agent(concat!("skills-box/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        Self { client }
    }

    pub fn with_client(client: reqwest::Client) -> Self {
        Self { client }
    }
}

impl Default for ReqwestArchiveClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ArchiveClient for ReqwestArchiveClient {
    async fn latest_commit(
        &self,
        source: &str,
        reference: Option<&str>,
    ) -> Result<String, GitHubFetchError> {
        let url = commit_url(source, reference)?;
        let response = self.client.get(url).send().await.map_err(map_reqwest)?;
        let status = response.status();
        if !status.is_success() {
            return Err(http_status_error(status.as_u16()));
        }
        let body = response.text().await.map_err(map_reqwest)?;
        parse_commit_response(&body)
    }

    async fn download_archive(
        &self,
        source: &str,
        commit: &str,
    ) -> Result<Vec<u8>, GitHubFetchError> {
        let url = archive_url(source, commit)?;
        let mut response = self.client.get(url).send().await.map_err(map_reqwest)?;
        let status = response.status();
        if !status.is_success() {
            return Err(http_status_error(status.as_u16()));
        }
        if let Some(length) = response.content_length()
            && length > ARCHIVE_MAX_BYTES
        {
            return Err(GitHubFetchError::http(
                413,
                format!("来源归档为 {length} 字节，超过上限"),
            ));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(map_reqwest)? {
            if bytes.len() as u64 + chunk.len() as u64 > ARCHIVE_MAX_BYTES {
                return Err(GitHubFetchError::http(
                    413,
                    format!("来源归档超过 {ARCHIVE_MAX_BYTES} 字节上限"),
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
}

fn map_reqwest(error: reqwest::Error) -> GitHubFetchError {
    if error.is_timeout() {
        GitHubFetchError::offline(format!("归档请求超时：{error}"))
    } else if error.is_connect() {
        GitHubFetchError::offline(error.to_string())
    } else {
        GitHubFetchError::parse(error.to_string())
    }
}

fn http_status_error(status: u16) -> GitHubFetchError {
    if status == 404 {
        GitHubFetchError::http(404, "仓库或提交不存在")
    } else if status == 429 {
        GitHubFetchError::rate_limited("GitHub 返回 429")
    } else {
        GitHubFetchError::http(status, "")
    }
}

pub fn commit_url(source: &str, reference: Option<&str>) -> Result<reqwest::Url, GitHubFetchError> {
    let (owner, repo) = super::github::validate_repo(source)?;
    let reference = reference
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("HEAD");
    let mut url = reqwest::Url::parse(GITHUB_API_BASE)
        .map_err(|error| GitHubFetchError::http(0, error.to_string()))?;
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| GitHubFetchError::http(0, "无法构造 GitHub API URL"))?;
        segments
            .push("repos")
            .push(&owner)
            .push(&repo)
            .push("commits")
            .push(reference);
    }
    Ok(url)
}

pub fn archive_url(source: &str, commit: &str) -> Result<reqwest::Url, GitHubFetchError> {
    let (owner, repo) = super::github::validate_repo(source)?;
    if !crate::git_tree::is_git_tree_hash(commit) {
        return Err(GitHubFetchError::parse("归档 commit 不是 40 位 Git SHA"));
    }
    reqwest::Url::parse(&format!(
        "https://codeload.github.com/{owner}/{repo}/tar.gz/{commit}"
    ))
    .map_err(|error| GitHubFetchError::http(0, error.to_string()))
}

/// Parse `GET /repos/{owner}/{repo}/commits/{ref}`: only the SHA is needed.
pub fn parse_commit_response(raw: &str) -> Result<String, GitHubFetchError> {
    #[derive(Deserialize)]
    struct CommitResponse {
        sha: String,
    }
    let response: CommitResponse = serde_json::from_str(raw)
        .map_err(|error| GitHubFetchError::parse(format!("commit 响应无法解析：{error}")))?;
    if !crate::git_tree::is_git_tree_hash(&response.sha) {
        return Err(GitHubFetchError::parse("commit 响应缺少有效的 SHA"));
    }
    Ok(response.sha)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArchiveMetadata {
    version: u32,
    commit: String,
    fetched_at_epoch_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedArchive {
    pub archive: SourceArchive,
    pub fetched_at_epoch_ms: u64,
}

/// Commit-keyed archive cache. Archives are immutable for a commit, so a cache
/// hit avoids the download; the stored timestamp only drives the six-hour
/// freshness policy (and "last successful check" reporting).
#[derive(Debug, Clone)]
pub struct CommitArchiveCache {
    dir: PathBuf,
}

impl CommitArchiveCache {
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            dir: paths.cache_dir().join("archives"),
        }
    }

    pub fn from_dir(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn archive_path(&self, commit: &str) -> PathBuf {
        self.dir.join(format!("{commit}.tar.gz"))
    }

    fn metadata_path(&self, commit: &str) -> PathBuf {
        self.dir.join(format!("{commit}.json"))
    }

    /// Fresh-or-stale load; callers decide freshness. A commit that is not a
    /// 40-char Git SHA never resolves to a path inside the cache.
    pub fn load(&self, commit: &str, limits: ArchiveLimits) -> Option<CachedArchive> {
        if !crate::git_tree::is_git_tree_hash(commit) {
            return None;
        }
        let metadata: ArchiveMetadata =
            serde_json::from_str(&fs::read_to_string(self.metadata_path(commit)).ok()?).ok()?;
        if metadata.version != ARCHIVE_CACHE_VERSION || metadata.commit != commit {
            return None;
        }
        let raw = fs::read(self.archive_path(commit)).ok()?;
        let archive = SourceArchive::from_tar_gz(&raw, limits).ok()?;
        Some(CachedArchive {
            archive,
            fetched_at_epoch_ms: metadata.fetched_at_epoch_ms,
        })
    }

    pub fn is_fresh(&self, cached: &CachedArchive, now_epoch_ms: u64, ttl_ms: u64) -> bool {
        is_fresh(cached.fetched_at_epoch_ms, now_epoch_ms, ttl_ms)
    }

    pub fn store(&self, commit: &str, raw: &[u8], fetched_at_epoch_ms: u64) -> io::Result<()> {
        if !crate::git_tree::is_git_tree_hash(commit) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "commit 不是 40 位 Git SHA",
            ));
        }
        fs::create_dir_all(&self.dir)?;
        let temp_archive = self
            .dir
            .join(format!(".tmp-{}-{commit}.tar.gz", std::process::id()));
        fs::write(&temp_archive, raw)?;
        if let Err(error) = fs::rename(&temp_archive, self.archive_path(commit)) {
            let _ = fs::remove_file(&temp_archive);
            return Err(error);
        }
        let metadata = ArchiveMetadata {
            version: ARCHIVE_CACHE_VERSION,
            commit: commit.to_string(),
            fetched_at_epoch_ms,
        };
        let serialized = serde_json::to_vec(&metadata)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let temp_metadata = self
            .dir
            .join(format!(".tmp-{}-{commit}.json", std::process::id()));
        fs::write(&temp_metadata, serialized)?;
        if let Err(error) = fs::rename(&temp_metadata, self.metadata_path(commit)) {
            let _ = fs::remove_file(&temp_metadata);
            return Err(error);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write;

    fn tar_gz(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (path, content) in entries {
            if path.ends_with('/') {
                builder.append_dir(path, ".").unwrap();
                continue;
            }
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, path, *content).unwrap();
        }
        let tar = builder.into_inner().unwrap();
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&tar).unwrap();
        encoder.finish().unwrap()
    }

    const COMMIT: &str = "1aee802d9139d4df84d017de44fb1b37a7b614e9";

    #[test]
    fn parses_a_codeload_archive_strips_the_root_and_installer_exclusions() {
        let raw = tar_gz(&[
            ("acme-skills-abc123/SKILL.md", b"hello"),
            ("acme-skills-abc123/references/guide.md", b"guide"),
            ("acme-skills-abc123/.git/config", b"[core]"),
            ("acme-skills-abc123/node_modules/dep/index.js", b"x"),
            ("acme-skills-abc123/empty/", b""),
        ]);
        let archive = SourceArchive::from_tar_gz(&raw, ArchiveLimits::default()).unwrap();
        assert_eq!(
            archive.files.keys().cloned().collect::<Vec<_>>(),
            vec!["SKILL.md", "references/guide.md"]
        );
        assert_eq!(archive.files["SKILL.md"], b"hello");
        assert_eq!(
            archive.files_under("references"),
            vec![("guide.md".to_string(), b"guide".to_vec())]
        );
        assert!(archive.contains_file("SKILL.md"));
        assert!(archive.contains_folder("references"));
        assert!(!archive.contains_folder("empty"));
    }

    #[test]
    fn rejects_archives_over_file_count_and_size_caps() {
        let raw = tar_gz(&[("root/a.txt", b"aaaa"), ("root/b.txt", b"bbbb")]);
        let limits = ArchiveLimits {
            max_files: 1,
            ..ArchiveLimits::default()
        };
        assert_eq!(
            SourceArchive::from_tar_gz(&raw, limits).unwrap_err(),
            ArchiveError::TooManyFiles { limit: 1 }
        );

        let limits = ArchiveLimits {
            max_uncompressed_bytes: 5,
            ..ArchiveLimits::default()
        };
        assert!(matches!(
            SourceArchive::from_tar_gz(&raw, limits).unwrap_err(),
            ArchiveError::TooLarge { .. }
        ));

        let limits = ArchiveLimits {
            max_file_bytes: 3,
            ..ArchiveLimits::default()
        };
        assert!(matches!(
            SourceArchive::from_tar_gz(&raw, limits).unwrap_err(),
            ArchiveError::TooLarge { .. }
        ));
    }

    #[test]
    fn empty_and_corrupt_archives_are_invalid() {
        assert!(SourceArchive::from_tar_gz(b"", ArchiveLimits::default()).is_err());
        assert!(matches!(
            SourceArchive::from_tar_gz(b"not gzip", ArchiveLimits::default()).unwrap_err(),
            ArchiveError::Invalid { .. }
        ));
    }

    #[test]
    fn builds_commit_and_codeload_urls_from_a_normalized_source() {
        let url = commit_url("https://github.com/Acme/Skills.git", Some("main")).unwrap();
        assert_eq!(
            url.as_str(),
            "https://api.github.com/repos/Acme/Skills/commits/main"
        );
        let url = commit_url("acme/skills", None).unwrap();
        assert_eq!(
            url.as_str(),
            "https://api.github.com/repos/acme/skills/commits/HEAD"
        );
        let url = archive_url("acme/skills", COMMIT).unwrap();
        assert_eq!(
            url.as_str(),
            format!("https://codeload.github.com/acme/skills/tar.gz/{COMMIT}")
        );
        assert!(archive_url("acme/skills", "not-a-sha").is_err());
        assert!(commit_url("../evil", None).is_err());
    }

    #[test]
    fn parses_only_40_hex_commit_responses() {
        assert_eq!(
            parse_commit_response(&format!(r#"{{"sha":"{COMMIT}"}}"#)).unwrap(),
            COMMIT
        );
        assert!(parse_commit_response(r#"{"sha":"abc"}"#).is_err());
        assert!(parse_commit_response("{not json").is_err());
    }

    #[test]
    fn caches_archives_by_commit_and_rejects_corrupt_or_foreign_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = CommitArchiveCache::from_dir(tmp.path().join("archives"));
        let raw = tar_gz(&[("root/SKILL.md", b"hello")]);

        cache.store(COMMIT, &raw, 1_000).unwrap();
        let cached = cache.load(COMMIT, ArchiveLimits::default()).unwrap();
        assert_eq!(cached.fetched_at_epoch_ms, 1_000);
        assert_eq!(cached.archive.files["SKILL.md"], b"hello");
        assert!(cache.is_fresh(&cached, 1_000, 10));
        assert!(!cache.is_fresh(&cached, 1_010, 10));

        let other = "0123456789abcdef0123456789abcdef01234567";
        assert!(cache.load(other, ArchiveLimits::default()).is_none());
        assert!(cache.store("../escape", &raw, 1_000).is_err());
        assert!(cache.load("../escape", ArchiveLimits::default()).is_none());

        fs::write(cache.archive_path(COMMIT), b"broken").unwrap();
        assert!(cache.load(COMMIT, ArchiveLimits::default()).is_none());
    }

    #[test]
    fn maps_archive_failures_to_typed_network_errors() {
        let error = ArchiveError::TooLarge {
            detail: "x".to_string(),
        }
        .to_app_error();
        assert_eq!(error.code, crate::error::ErrorCode::NetworkError);
        let error = ArchiveError::from(GitHubFetchError::offline("offline")).to_app_error();
        assert_eq!(error.code, crate::error::ErrorCode::NetworkOffline);
    }
}
