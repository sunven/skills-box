//! Native GitHub-backed ClawHub installation.
//!
//! The resolver pins `repo/path/commit/contentHash`. Installation downloads the
//! codeload archive at that commit, extracts only the skill subpath, reproduces
//! ClawHub's folder content hash exactly, and refuses to publish on mismatch.

use super::package::StagedPackage;
use crate::{
    error::AppError,
    marketplace::{files, is_commit},
};
use icu_collator::{Collator, CollatorPreferences, options::CollatorOptions};
use icu_locale_core::locale;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read, Write},
    time::Duration,
};

const MAX_DOWNLOAD_BYTES: usize = 100 * 1024 * 1024;
const MAX_UNCOMPRESSED_BYTES: u64 = 80 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 75_000;
const MAX_RAW_MARKDOWN_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubReference {
    pub repo: String,
    pub path: String,
    pub commit: String,
    pub content_hash: String,
}

impl GithubReference {
    pub fn label(&self) -> String {
        format!("{}@{}:{}", self.repo, self.commit, self.path)
    }
}

/// Preview-only SKILL.md read at the pinned commit. The full package hash is
/// verified at install time, not during preview.
pub async fn read_skill_markdown(reference: &GithubReference) -> Result<String, AppError> {
    let mut url = reqwest::Url::parse("https://raw.githubusercontent.com")
        .map_err(AppError::clawhub_unavailable)?;
    {
        let (owner, repo) = reference
            .repo
            .split_once('/')
            .ok_or_else(|| AppError::clawhub_unavailable("GitHub 仓库坐标无效"))?;
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| AppError::clawhub_unavailable("GitHub 原文地址无效"))?;
        segments.push(owner).push(repo).push(&reference.commit);
        for segment in reference.path.split('/') {
            segments.push(segment);
        }
        segments.push("SKILL.md");
    }
    let response = client()?
        .get(url)
        .send()
        .await
        .map_err(AppError::clawhub_unavailable)?;
    if !response.status().is_success() {
        return Err(AppError::clawhub_install_unsupported(format!(
            "无法从固定提交读取 SKILL.md（HTTP {}）",
            response.status()
        )));
    }
    if response
        .content_length()
        .is_some_and(|size| size > MAX_RAW_MARKDOWN_BYTES as u64)
    {
        return Err(AppError::clawhub_package_invalid("SKILL.md 超过大小限制"));
    }
    let mut bytes = Vec::new();
    let mut stream = response;
    while let Some(chunk) = stream
        .chunk()
        .await
        .map_err(AppError::clawhub_unavailable)?
    {
        if bytes.len() + chunk.len() > MAX_RAW_MARKDOWN_BYTES {
            return Err(AppError::clawhub_package_invalid("SKILL.md 超过大小限制"));
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes)
        .map_err(|error| AppError::clawhub_package_invalid(format!("SKILL.md 不是 UTF-8：{error}")))
}

/// Download, verify and stage one GitHub-backed skill folder.
pub async fn stage(reference: &GithubReference) -> Result<StagedPackage, AppError> {
    if !is_commit(&reference.commit) {
        return Err(AppError::clawhub_install_unsupported(
            "GitHub-backed 安装提交无效",
        ));
    }
    let bytes = download(reference).await?;
    let reference = reference.clone();
    tokio::task::spawn_blocking(move || extract_and_verify(bytes, &reference))
        .await
        .map_err(|error| AppError::clawhub_package_invalid(format!("解压任务失败：{error}")))?
}

async fn download(reference: &GithubReference) -> Result<Vec<u8>, AppError> {
    let url = format!(
        "https://codeload.github.com/{}/zip/{}",
        reference.repo, reference.commit
    );
    let response = client()?
        .get(url)
        .send()
        .await
        .map_err(AppError::clawhub_unavailable)?;
    if !response.status().is_success() {
        return Err(AppError::clawhub_unavailable(format!(
            "GitHub 归档下载失败（HTTP {}）",
            response.status()
        )));
    }
    if response
        .content_length()
        .is_some_and(|size| size > MAX_DOWNLOAD_BYTES as u64)
    {
        return Err(AppError::clawhub_package_invalid(
            "GitHub 归档超过下载大小限制",
        ));
    }
    let mut bytes = Vec::new();
    let mut stream = response;
    while let Some(chunk) = stream
        .chunk()
        .await
        .map_err(AppError::clawhub_unavailable)?
    {
        if bytes.len() + chunk.len() > MAX_DOWNLOAD_BYTES {
            return Err(AppError::clawhub_package_invalid(
                "GitHub 归档超过下载大小限制",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() < 5
                && attempt
                    .url()
                    .host_str()
                    .is_some_and(|host| host == "codeload.github.com" || host == "github.com")
            {
                attempt.follow()
            } else {
                attempt.error("GitHub 归档地址不可用")
            }
        }))
        .timeout(Duration::from_secs(120))
        .user_agent("SkillsBox/0.1 ClawHub-marketplace")
        .build()
        .map_err(AppError::clawhub_unavailable)
}

#[derive(Debug)]
struct EntryPlan {
    index: usize,
    path: String,
    size: u64,
    exec_bits: u32,
}

fn extract_and_verify(
    bytes: Vec<u8>,
    reference: &GithubReference,
) -> Result<StagedPackage, AppError> {
    let directory = tempfile::Builder::new()
        .prefix("skills-box-clawhub-github-")
        .tempdir()
        .map_err(|error| AppError::clawhub_package_invalid(format!("无法创建暂存目录：{error}")))?;
    let base = fs::canonicalize(directory.path())
        .map_err(|error| AppError::clawhub_package_invalid(format!("暂存目录不可用：{error}")))?;
    let root = base.join("package");
    fs::create_dir(&root)
        .map_err(|error| AppError::clawhub_package_invalid(format!("无法创建暂存目录：{error}")))?;

    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|error| {
        AppError::clawhub_package_invalid(format!("GitHub 归档不是有效 ZIP：{error}"))
    })?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(AppError::clawhub_package_invalid(
            "GitHub 归档条目数超过限制",
        ));
    }
    let mut entries = BTreeMap::new();
    let mut total_uncompressed = 0u64;
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(|error| {
            AppError::clawhub_package_invalid(format!("无法读取 GitHub 归档：{error}"))
        })?;
        if file.is_dir() {
            continue;
        }
        let normalized = normalize_repo_path(&file.name().map_err(|error| {
            AppError::clawhub_package_invalid(format!("GitHub 归档条目名称无效：{error}"))
        })?);
        if normalized.is_empty() {
            return Err(AppError::clawhub_package_invalid("GitHub 归档包含无效路径"));
        }
        if is_mac_junk(&normalized) {
            continue;
        }
        let size = file.size();
        if size > MAX_FILE_BYTES {
            return Err(AppError::clawhub_package_invalid(format!(
                "GitHub 归档文件 {normalized} 超过单文件限制"
            )));
        }
        total_uncompressed = total_uncompressed.saturating_add(size);
        if total_uncompressed > MAX_UNCOMPRESSED_BYTES {
            return Err(AppError::clawhub_package_invalid(
                "GitHub 归档解压后大小超过限制",
            ));
        }
        entries.insert(
            normalized.clone(),
            EntryPlan {
                index,
                path: normalized,
                size,
                exec_bits: file.unix_mode().unwrap_or(0) & 0o111,
            },
        );
    }
    let entries = strip_common_root(entries);
    let root_prefix = if reference.path.is_empty() {
        String::new()
    } else {
        format!("{}/", reference.path)
    };
    let mut folder: Vec<(String, &EntryPlan)> = entries
        .iter()
        .filter_map(|(path, plan)| {
            if root_prefix.is_empty() {
                (!path.contains('/')).then(|| (path.clone(), plan))
            } else {
                path.strip_prefix(&root_prefix)
                    .filter(|relative| !relative.is_empty())
                    .map(|relative| (relative.to_string(), plan))
            }
        })
        .collect();
    if folder.is_empty() {
        return Err(AppError::clawhub_package_invalid(
            "GitHub 归档中缺少技能子路径",
        ));
    }
    let collator = Collator::try_new(
        CollatorPreferences::from(locale!("en-US")),
        CollatorOptions::default(),
    )
    .map_err(|error| AppError::clawhub_package_invalid(format!("排序器不可用：{error}")))?;
    folder.sort_by(|left, right| collator.compare(&left.0, &right.0));

    let mut hashed = Vec::with_capacity(folder.len());
    for (relative, plan) in &folder {
        files::validate_path(relative).map_err(|error| {
            AppError::clawhub_package_invalid(format!(
                "GitHub 技能路径 {} 无效：{}",
                plan.path, error.message
            ))
        })?;
        let mut source = archive.by_index(plan.index).map_err(|error| {
            AppError::clawhub_package_invalid(format!("无法读取 GitHub 归档：{error}"))
        })?;
        let mut content = Vec::with_capacity(plan.size as usize);
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = source.read(&mut buffer).map_err(|error| {
                AppError::clawhub_package_invalid(format!(
                    "读取 GitHub 归档 {} 失败：{error}",
                    plan.path
                ))
            })?;
            if read == 0 {
                break;
            }
            content.extend_from_slice(&buffer[..read]);
            if content.len() as u64 > plan.size {
                return Err(AppError::clawhub_package_invalid(format!(
                    "GitHub 归档 {} 的内容超过声明大小",
                    plan.path
                )));
            }
        }
        if content.len() as u64 != plan.size {
            return Err(AppError::clawhub_package_invalid(format!(
                "GitHub 归档 {} 的大小不一致",
                plan.path
            )));
        }
        hashed.push((
            relative.clone(),
            content.len() as u64,
            hex(&Sha256::digest(&content)),
            content,
            plan.exec_bits,
        ));
    }
    let payload = hashed
        .iter()
        .map(|(path, size, sha, _, _)| format!("{path}\0{size}\0{sha}"))
        .collect::<Vec<_>>()
        .join("\n");
    let actual = hex(&Sha256::digest(payload.as_bytes()));
    if actual != reference.content_hash {
        return Err(AppError::clawhub_package_invalid(
            "GitHub 技能文件夹内容哈希与 ClawHub 解析结果不一致",
        ));
    }
    for (relative, _, _, content, exec_bits) in hashed {
        let destination = root.join(&relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                AppError::clawhub_package_invalid(format!("无法创建技能目录：{error}"))
            })?;
        }
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .map_err(|error| {
                AppError::clawhub_package_invalid(format!("无法写入技能文件：{error}"))
            })?;
        output.write_all(&content).map_err(|error| {
            AppError::clawhub_package_invalid(format!("无法写入技能文件：{error}"))
        })?;
        #[cfg(unix)]
        if exec_bits != 0 {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&destination)
                .map_err(|error| {
                    AppError::clawhub_package_invalid(format!("无法读取技能文件权限：{error}"))
                })?
                .permissions();
            permissions.set_mode(permissions.mode() | exec_bits);
            fs::set_permissions(&destination, permissions).map_err(|error| {
                AppError::clawhub_package_invalid(format!("无法设置技能文件权限：{error}"))
            })?;
        }
    }
    StagedPackage::from_verified(directory, root)
}

fn strip_common_root(entries: BTreeMap<String, EntryPlan>) -> BTreeMap<String, EntryPlan> {
    let Some(first) = entries.keys().next() else {
        return entries;
    };
    let prefix = format!("{}/", first.split('/').next().unwrap_or_default());
    if prefix == "/" || !entries.keys().all(|path| path.starts_with(&prefix)) {
        return entries;
    }
    entries
        .into_iter()
        .map(|(path, plan)| {
            let mut plan = plan;
            plan.path = path[prefix.len()..].to_string();
            (plan.path.clone(), plan)
        })
        .collect()
}

fn normalize_repo_path(raw: &str) -> String {
    if raw.contains('\0') {
        return String::new();
    }
    let replaced = raw.replace('\\', "/");
    let trimmed = replaced.trim();
    let stripped = trimmed
        .strip_prefix("./")
        .map(|value| value.trim_start_matches('/'))
        .unwrap_or(trimmed);
    let stripped = stripped.trim_start_matches('/');
    let segments = stripped
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if segments
        .iter()
        .any(|segment| *segment == "." || *segment == "..")
    {
        return String::new();
    }
    segments.join("/")
}

fn is_mac_junk(path: &str) -> bool {
    let normalized = path
        .trim()
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_ascii_lowercase();
    let segments = normalized
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if segments.is_empty() {
        return false;
    }
    if segments.contains(&"__macosx") {
        return true;
    }
    let basename = segments.last().copied().unwrap_or_default();
    basename == ".ds_store" || basename.starts_with("._")
}

fn hex(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(value, "{byte:02x}");
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::marketplace::{
        Coordinate, MarketplaceSource,
        clawhub::{Detail, detail as fetch_detail},
    };

    #[test]
    fn normalizes_repo_paths_and_skips_mac_junk_like_the_registry() {
        assert_eq!(normalize_repo_path("./a//b\\c"), "a/b/c");
        assert_eq!(normalize_repo_path("/a/b/"), "a/b");
        assert_eq!(normalize_repo_path("a/../b"), "");
        assert_eq!(normalize_repo_path("a/./b"), "");
        assert_eq!(normalize_repo_path("a\0b"), "");
        assert!(is_mac_junk("__MACOSX/a"));
        assert!(is_mac_junk("a/.DS_Store"));
        assert!(is_mac_junk("a/._b"));
        assert!(!is_mac_junk("a/b"));
    }

    #[test]
    fn hash_payload_uses_relative_paths_sizes_and_lowercase_sha() {
        let collator = Collator::try_new(
            CollatorPreferences::from(locale!("en-US")),
            CollatorOptions::default(),
        )
        .unwrap();
        let mut paths = vec!["SKILL.md", "a.txt", "A.txt"];
        paths.sort_by(|left, right| collator.compare(left, right));
        assert_eq!(paths, ["a.txt", "A.txt", "SKILL.md"]);
    }

    #[tokio::test]
    #[ignore = "downloads a pinned GitHub archive and verifies ClawHub contentHash; requires network"]
    async fn clawhub_live_github_backed_stage_matches_the_resolver_hash() {
        let coordinate =
            Coordinate::from_canonical_for(MarketplaceSource::Clawhub, "@aws/agents-build")
                .unwrap();
        let Detail::Native(native) = fetch_detail(&coordinate).await.unwrap() else {
            panic!("agents-build is a native GitHub-backed entry");
        };
        let resolved = native.resolved_reference.expect("resolver reference");
        let reference = GithubReference {
            repo: resolved.repo,
            path: resolved.path,
            commit: resolved.commit,
            content_hash: resolved.content_hash.expect("content hash"),
        };
        let markdown = read_skill_markdown(&reference).await.unwrap();
        assert!(!markdown.trim().is_empty());
        let staged = stage(&reference).await.unwrap();
        assert!(staged.path().join("SKILL.md").is_file());
        assert_eq!(staged.content_hash.len(), 64);
        staged.check().unwrap();
    }
}
