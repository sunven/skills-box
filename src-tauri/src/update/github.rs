//! GitHub Trees API access and recursive-tree folder hash computation.
//!
//! One recursive tree request per `(source, ref)` covers every skill in that
//! repository. The lock's 40-char `skillFolderHash` is a Git tree object SHA-1;
//! it is compared with the tree SHA computed from the recursive entries using
//! Git tree object semantics. 64-char/empty values are never compared.

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub const GITHUB_API_BASE: &str = "https://api.github.com";
pub const GITHUB_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
pub const EMPTY_TREE_SHA: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTreeEntry {
    /// `blob`, `tree`, or `commit` (submodule).
    #[serde(rename = "type")]
    pub kind: String,
    pub sha: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

impl RemoteTreeEntry {
    pub fn new(kind: impl Into<String>, sha: impl Into<String>, mode: Option<&str>) -> Self {
        Self {
            kind: kind.into(),
            sha: sha.into(),
            mode: mode.map(str::to_string),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTree {
    pub root_sha: String,
    #[serde(default)]
    pub truncated: bool,
    pub entries: BTreeMap<String, RemoteTreeEntry>,
}

impl RemoteTree {
    pub fn new(
        root_sha: impl Into<String>,
        truncated: bool,
        entries: BTreeMap<String, RemoteTreeEntry>,
    ) -> Self {
        Self {
            root_sha: root_sha.into(),
            truncated,
            entries,
        }
    }

    pub fn entry(&self, path: &str) -> Option<&RemoteTreeEntry> {
        self.entries.get(path)
    }

    pub fn has_tree(&self, folder: &str) -> bool {
        self.entries
            .get(folder)
            .is_some_and(|entry| entry.kind == "tree")
    }

    /// Git tree object SHA for a folder, computed from the recursive entries.
    /// The empty string addresses the repository root. Returns `None` when the
    /// folder is absent, the response is truncated, or any entry is malformed.
    pub fn folder_tree_sha(&self, folder: &str) -> Option<String> {
        if self.truncated {
            return None;
        }
        let folder = if folder.is_empty() {
            String::new()
        } else {
            normalize_tree_path(folder)?
        };
        if !folder.is_empty() && !self.has_tree(&folder) {
            return None;
        }

        let mut rows: Vec<GitTreeRow> = Vec::new();
        for (path, entry) in &self.entries {
            let Some(name) = direct_child_name(path, &folder) else {
                continue;
            };
            let mode = git_mode(&entry.kind, entry.mode.as_deref())?;
            let sha = decode_sha(&entry.sha)?;
            let mut key = name.clone().into_bytes();
            key.push(if entry.kind == "tree" { b'/' } else { 0 });
            rows.push((key, name.into_bytes(), mode, sha));
        }

        if rows.is_empty() {
            return Some(EMPTY_TREE_SHA.to_string());
        }
        rows.sort_by(|left, right| left.0.cmp(&right.0));

        let mut body = Vec::new();
        for (_, name, mode, sha) in rows {
            body.extend_from_slice(mode.as_bytes());
            body.push(b' ');
            body.extend_from_slice(&name);
            body.push(0);
            body.extend_from_slice(&sha);
        }
        Some(git_object_hash(b"tree", &body))
    }
}

type GitTreeRow = (Vec<u8>, Vec<u8>, &'static str, [u8; 20]);

/// Normalize a lock `skillPath`/tree path: forward slashes, no `.`/`..`
/// segments, no leading/trailing slashes.
pub fn normalize_tree_path(raw: &str) -> Option<String> {
    let raw = raw.trim().replace('\\', "/");
    if raw.ends_with('/') {
        return None;
    }
    let mut parts = Vec::new();
    for part in raw.split('/') {
        match part {
            "" | "." => continue,
            ".." => return None,
            _ => parts.push(part),
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

fn direct_child_name(path: &str, folder: &str) -> Option<String> {
    if folder.is_empty() {
        if path.is_empty() || path.contains('/') {
            return None;
        }
        return Some(path.to_string());
    }
    let rest = path.strip_prefix(folder)?.strip_prefix('/')?;
    if rest.is_empty() || rest.contains('/') {
        return None;
    }
    Some(rest.to_string())
}

fn git_mode(kind: &str, mode: Option<&str>) -> Option<&'static str> {
    match kind {
        "tree" => Some("40000"),
        "commit" => Some("160000"),
        "blob" => match mode {
            Some("100755") => Some("100755"),
            Some("120000") => Some("120000"),
            _ => Some("100644"),
        },
        _ => None,
    }
}

fn decode_sha(value: &str) -> Option<[u8; 20]> {
    if value.len() != 40 {
        return None;
    }
    let mut output = [0u8; 20];
    for (index, byte) in output.iter_mut().enumerate() {
        let start = index * 2;
        *byte = u8::from_str_radix(value.get(start..start + 2)?, 16).ok()?;
    }
    Some(output)
}

fn git_object_hash(kind: &[u8], body: &[u8]) -> String {
    use sha1::{Digest, Sha1};

    let mut hasher = Sha1::new();
    hasher.update(kind);
    hasher.update(b" ");
    hasher.update(body.len().to_string().as_bytes());
    hasher.update([0]);
    hasher.update(body);
    let digest = hasher.finalize();
    let mut output = String::with_capacity(40);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitHubFetchError {
    Offline { detail: String },
    RateLimited { detail: String },
    Http { status: u16, detail: String },
    Parse { detail: String },
}

impl GitHubFetchError {
    pub fn offline(detail: impl Into<String>) -> Self {
        Self::Offline {
            detail: detail.into(),
        }
    }

    pub fn rate_limited(detail: impl Into<String>) -> Self {
        Self::RateLimited {
            detail: detail.into(),
        }
    }

    pub fn http(status: u16, detail: impl Into<String>) -> Self {
        Self::Http {
            status,
            detail: detail.into(),
        }
    }

    pub fn parse(detail: impl Into<String>) -> Self {
        Self::Parse {
            detail: detail.into(),
        }
    }

    pub fn to_app_error(&self) -> crate::error::AppError {
        match self {
            Self::Offline { detail } => crate::error::AppError::network_offline(detail),
            Self::RateLimited { detail } => crate::error::AppError::network_rate_limited(detail),
            Self::Http {
                status: 404,
                detail,
            } => {
                let suffix = if detail.is_empty() {
                    String::new()
                } else {
                    format!("：{detail}")
                };
                crate::error::AppError::network_error(format!(
                    "仓库不存在或不可公开访问（HTTP 404）{suffix}"
                ))
            }
            Self::Http { status, detail } => {
                let suffix = if detail.is_empty() {
                    String::new()
                } else {
                    format!("：{detail}")
                };
                crate::error::AppError::network_error(format!("HTTP {status}{suffix}"))
            }
            Self::Parse { detail } => crate::error::AppError::network_error(detail),
        }
    }
}

#[async_trait]
pub trait GitHubTreeClient: Send + Sync {
    async fn fetch_tree(
        &self,
        source: &str,
        reference: Option<&str>,
    ) -> Result<RemoteTree, GitHubFetchError>;
}

fn validate_repo(source: &str) -> Result<(String, String), GitHubFetchError> {
    let normalized = crate::tasks::source::validate_source(source)
        .map_err(|error| GitHubFetchError::http(400, error.message))?
        .normalized;
    let Some((owner, repo)) = normalized.split_once('/') else {
        return Err(GitHubFetchError::http(400, "来源不是 owner/repo"));
    };
    Ok((owner.to_string(), repo.to_string()))
}

/// Build the recursive Trees API URL. The ref is encoded as one path segment,
/// so branch names containing `/` stay inside `{ref}`.
pub fn tree_url(source: &str, reference: Option<&str>) -> Result<reqwest::Url, GitHubFetchError> {
    let (owner, repo) = validate_repo(source)?;
    let mut url = reqwest::Url::parse(GITHUB_API_BASE)
        .map_err(|error| GitHubFetchError::http(0, error.to_string()))?;
    let reference = reference
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("HEAD");
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| GitHubFetchError::http(0, "无法构造 GitHub API URL"))?;
        segments
            .push("repos")
            .push(&owner)
            .push(&repo)
            .push("git")
            .push("trees")
            .push(reference);
    }
    url.query_pairs_mut().append_pair("recursive", "1");
    Ok(url)
}

/// Parse a `GET /git/trees/{ref}?recursive=1` response body.
pub fn parse_tree_response(raw: &str) -> Result<RemoteTree, GitHubFetchError> {
    #[derive(Deserialize)]
    struct TreeResponse {
        sha: String,
        #[serde(default)]
        tree: Vec<TreeResponseEntry>,
        #[serde(default)]
        truncated: bool,
    }

    #[derive(Deserialize)]
    struct TreeResponseEntry {
        path: String,
        #[serde(rename = "type")]
        kind: String,
        sha: String,
        #[serde(default)]
        mode: Option<String>,
    }

    let response: TreeResponse = serde_json::from_str(raw)
        .map_err(|error| GitHubFetchError::parse(format!("GitHub tree 响应无法解析：{error}")))?;
    if !crate::baseline::is_git_tree_hash(&response.sha) {
        return Err(GitHubFetchError::parse("GitHub tree 响应缺少有效的根 SHA"));
    }

    let mut entries = BTreeMap::new();
    for entry in response.tree {
        let Some(path) = normalize_tree_path(&entry.path) else {
            return Err(GitHubFetchError::parse(format!(
                "GitHub tree 包含无效路径：{}",
                entry.path
            )));
        };
        entries.insert(
            path,
            RemoteTreeEntry {
                kind: entry.kind,
                sha: entry.sha,
                mode: entry.mode,
            },
        );
    }

    Ok(RemoteTree::new(response.sha, response.truncated, entries))
}

/// Production GitHub client: one unauthenticated recursive Trees request per
/// source with a hard timeout. Public repositories only (v1 scope).
#[derive(Debug, Clone)]
pub struct ReqwestGitHubClient {
    client: reqwest::Client,
}

impl ReqwestGitHubClient {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(GITHUB_REQUEST_TIMEOUT)
            .user_agent(concat!("skills-box/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        Self { client }
    }

    pub fn with_client(client: reqwest::Client) -> Self {
        Self { client }
    }
}

impl Default for ReqwestGitHubClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl GitHubTreeClient for ReqwestGitHubClient {
    async fn fetch_tree(
        &self,
        source: &str,
        reference: Option<&str>,
    ) -> Result<RemoteTree, GitHubFetchError> {
        let url = tree_url(source, reference)?;
        let response = self
            .client
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .map_err(map_request_error)?;

        let status = response.status();
        if !status.is_success() {
            let remaining = header_value(&response, "x-ratelimit-remaining");
            let retry_after = header_value(&response, "retry-after");
            let reset = header_value(&response, "x-ratelimit-reset");
            let body = response.text().await.unwrap_or_default();
            let mut error = classify_http_error(
                status.as_u16(),
                remaining.as_deref(),
                retry_after.as_deref(),
                &body,
            );
            if let GitHubFetchError::RateLimited { detail } = &mut error
                && let Some(reset) = reset
            {
                detail.push_str(&format!("；x-ratelimit-reset={reset}"));
            }
            return Err(error);
        }

        let body = response.text().await.map_err(|error| {
            GitHubFetchError::offline(format!("读取 GitHub tree 响应失败：{error}"))
        })?;
        parse_tree_response(&body)
    }
}

fn header_value(response: &reqwest::Response, name: &str) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

fn map_request_error(error: reqwest::Error) -> GitHubFetchError {
    if error.is_connect() || error.is_timeout() {
        GitHubFetchError::offline(error.to_string())
    } else {
        GitHubFetchError::http(0, error.to_string())
    }
}

fn classify_http_error(
    status: u16,
    rate_limit_remaining: Option<&str>,
    retry_after: Option<&str>,
    body: &str,
) -> GitHubFetchError {
    let limited = status == 429
        || status == 403 && (rate_limit_remaining == Some("0") || retry_after.is_some());
    let body = truncate_detail(body);
    if limited {
        let detail = if body.is_empty() {
            format!("HTTP {status}")
        } else {
            format!("HTTP {status}：{body}")
        };
        return GitHubFetchError::rate_limited(detail);
    }
    GitHubFetchError::http(status, body)
}

fn truncate_detail(body: &str) -> String {
    let trimmed = body.trim();
    let mut output: String = trimmed.chars().take(300).collect();
    if trimmed.chars().count() > 300 {
        output.push('…');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "1aee802d9139d4df84d017de44fb1b37a7b614e9";
    const SKILLS_TREE: &str = "12b71a9df5501f6747107c154020a4026ad6d014";
    const ALPHA_TREE: &str = "55ce0eaec6cf25cf0b4752dd1048448eabb1eb31";
    const SUB_TREE: &str = "640ba63b141ba35b527fee65e78140148c8d0a5b";
    const SKILL_MD: &str = "882a458e7b47d67722f6aeccb51f16b1842f1a19";
    const REFERENCES: &str = "7e2b6439aebf0bb975796f691b3b227d0af43bb5";
    const NOTE: &str = "ce013625030ba8dba906f756967f9e9ca394464a";

    /// Fixture generated with `git add -A && git write-tree` and
    /// `git ls-tree -r -t` for:
    /// SKILL.md, sub/note.txt, skills/alpha/{SKILL.md,references.md}.
    fn fixture_tree() -> RemoteTree {
        let mut entries = BTreeMap::new();
        entries.insert(
            "SKILL.md".to_string(),
            RemoteTreeEntry::new("blob", SKILL_MD, Some("100644")),
        );
        entries.insert(
            "skills".to_string(),
            RemoteTreeEntry::new("tree", SKILLS_TREE, Some("040000")),
        );
        entries.insert(
            "skills/alpha".to_string(),
            RemoteTreeEntry::new("tree", ALPHA_TREE, Some("040000")),
        );
        entries.insert(
            "skills/alpha/SKILL.md".to_string(),
            RemoteTreeEntry::new("blob", SKILL_MD, Some("100644")),
        );
        entries.insert(
            "skills/alpha/references.md".to_string(),
            RemoteTreeEntry::new("blob", REFERENCES, Some("100644")),
        );
        entries.insert(
            "sub".to_string(),
            RemoteTreeEntry::new("tree", SUB_TREE, Some("040000")),
        );
        entries.insert(
            "sub/note.txt".to_string(),
            RemoteTreeEntry::new("blob", NOTE, Some("100644")),
        );
        RemoteTree::new(ROOT, false, entries)
    }

    #[test]
    fn computes_known_tree_shas_from_recursive_entries() {
        let tree = fixture_tree();
        assert_eq!(tree.folder_tree_sha("").as_deref(), Some(ROOT));
        assert_eq!(tree.folder_tree_sha("skills").as_deref(), Some(SKILLS_TREE));
        assert_eq!(
            tree.folder_tree_sha("skills/alpha").as_deref(),
            Some(ALPHA_TREE)
        );
        assert_eq!(tree.folder_tree_sha("sub").as_deref(), Some(SUB_TREE));
    }

    #[test]
    fn returns_none_for_missing_folders_and_truncated_trees() {
        let tree = fixture_tree();
        assert_eq!(tree.folder_tree_sha("missing"), None);

        let mut truncated = tree.clone();
        truncated.truncated = true;
        assert_eq!(truncated.folder_tree_sha("skills/alpha"), None);
    }

    #[test]
    fn normalizes_tree_paths_and_rejects_traversal() {
        assert_eq!(
            normalize_tree_path(" ./skills/alpha/SKILL.md ").as_deref(),
            Some("skills/alpha/SKILL.md")
        );
        assert_eq!(
            normalize_tree_path("skills//alpha/SKILL.md").as_deref(),
            Some("skills/alpha/SKILL.md")
        );
        for invalid in ["", ".", "..", "../secret", "skills/../secret", "skills/"] {
            assert_eq!(normalize_tree_path(invalid), None, "{invalid:?}");
        }
    }

    #[test]
    fn empty_tree_hash_matches_git() {
        let tree = RemoteTree::new(EMPTY_TREE_SHA, false, BTreeMap::new());
        assert_eq!(tree.folder_tree_sha("").as_deref(), Some(EMPTY_TREE_SHA));
    }

    #[test]
    fn builds_tree_api_urls_and_encodes_refs() {
        assert_eq!(
            tree_url("acme/skills", None).unwrap().as_str(),
            "https://api.github.com/repos/acme/skills/git/trees/HEAD?recursive=1"
        );
        assert_eq!(
            tree_url("acme/skills", Some("release/1.2"))
                .unwrap()
                .as_str(),
            "https://api.github.com/repos/acme/skills/git/trees/release%2F1.2?recursive=1"
        );
        assert!(tree_url("not-a-repo", None).is_err());
    }

    #[test]
    fn parses_recursive_tree_payloads_and_preserves_truncation() {
        let raw = r#"{
          "sha": "1aee802d9139d4df84d017de44fb1b37a7b614e9",
          "tree": [
            {"path": "skills/alpha", "type": "tree", "sha": "55ce0eaec6cf25cf0b4752dd1048448eabb1eb31", "mode": "040000"},
            {"path": "skills/alpha/SKILL.md", "type": "blob", "sha": "882a458e7b47d67722f6aeccb51f16b1842f1a19", "mode": "100644"}
          ],
          "truncated": true
        }"#;
        let tree = parse_tree_response(raw).unwrap();
        assert!(tree.truncated);
        assert_eq!(tree.root_sha, ROOT);
        assert_eq!(tree.entry("skills/alpha/SKILL.md").unwrap().kind, "blob");
        assert!(tree.has_tree("skills/alpha"));

        assert!(parse_tree_response("{not json").is_err());
        assert!(parse_tree_response(r#"{"sha":"","tree":[]}"#).is_err());
    }

    #[test]
    fn classifies_rate_limits_separately_from_other_http_failures() {
        assert!(matches!(
            classify_http_error(429, None, None, "slow down"),
            GitHubFetchError::RateLimited { .. }
        ));
        assert!(matches!(
            classify_http_error(403, Some("0"), None, ""),
            GitHubFetchError::RateLimited { .. }
        ));
        assert!(matches!(
            classify_http_error(403, None, Some("30"), ""),
            GitHubFetchError::RateLimited { .. }
        ));
        assert!(matches!(
            classify_http_error(403, Some("10"), None, "forbidden"),
            GitHubFetchError::Http { status: 403, .. }
        ));
        assert!(matches!(
            classify_http_error(404, None, None, "Not Found"),
            GitHubFetchError::Http { status: 404, .. }
        ));
        assert!(matches!(
            classify_http_error(500, None, None, "boom"),
            GitHubFetchError::Http { status: 500, .. }
        ));
    }
}
