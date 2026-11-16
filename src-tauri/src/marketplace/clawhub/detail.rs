//! ClawHub skill detail, version history and read-only file preview.
//!
//! Everything here is bounded and anonymous. The detail response is parsed
//! strictly, except that upstream `moderation`/`security` scan data is never
//! read at all (spec R6-Q3: the first version does not display scan results).
//! Version content is always requested for one explicit version so the
//! overview and file tree can never mix releases.

use super::{client, list};
use crate::content::{
    SkillFileContent, SkillFileKind, SkillFileStatus, file_kind, image_mime, limit_for,
};
use crate::error::AppError;
use crate::marketplace::{Coordinate, MarketplaceSource, validate_version};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

const MAX_CHANGELOG: usize = 100_000;
const MAX_LICENSE: usize = 200;
/// One version page. The live endpoint returns newest first and paginates with
/// an opaque cursor; the selected-version UI never sorts this list locally.
const MAX_VERSION_ITEMS: usize = 100;
const MAX_VERSION_PAGES: usize = 4;
const MAX_CURSOR: usize = 4096;
const MAX_SEARCH_RESULTS: usize = 100;
const MAX_SEARCH_KIND: usize = 40;
const MAX_ALIAS_REFERENCE: usize = 400;
const MAX_FILES: usize = 2_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionRef {
    pub version: String,
    pub created_at: Option<i64>,
    pub changelog: Option<String>,
}

/// One ClawHub-native entry. `canonical_url` is derived from the coordinate,
/// never from an upstream field, so the backlink is always the documented
/// `https://clawhub.ai/<owner>/skills/<slug>` listing.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeDetail {
    pub coordinate: Coordinate,
    pub name: Option<String>,
    pub description: Option<String>,
    pub topics: Vec<String>,
    pub metrics: list::Metrics,
    pub owner_display_name: Option<String>,
    pub latest_version: Option<String>,
    pub license: Option<String>,
    pub changelog: Option<String>,
    pub canonical_url: String,
    pub versions: Vec<VersionRef>,
    pub versions_truncated: bool,
}

/// A ClawHub index entry whose install kind is `skills-sh`. It belongs to the
/// skills.sh source; this source never offers ClawHub installation for it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AliasDetail {
    pub coordinate: Coordinate,
    pub name: Option<String>,
    pub reference: Option<String>,
    pub source_url: Option<String>,
    pub repository: Option<String>,
    pub canonical_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Detail {
    Native(NativeDetail),
    Alias(AliasDetail),
}

/// Read one ClawHub entry: its identity, public metrics, version list and —
/// when it is a skills.sh alias — the redirect target instead of content.
pub async fn detail(coordinate: &Coordinate) -> Result<Detail, AppError> {
    validate_coordinate(coordinate)?;
    let path = format!("/api/v1/skills/{}", coordinate.slug);
    let key = format!("detail|@{}", coordinate.canonical().trim_start_matches('@'));
    let native = client::get_at_optional(
        &path,
        &key,
        &[("ownerHandle", coordinate.namespace.clone())],
    )
    .await;
    // The public detail response has no `install.kind`, so exactly one
    // exact-mode search call classifies the coordinate. This happens per
    // opened detail, never per list item.
    let alias = classify_alias(coordinate).await;
    match native {
        Ok(Some(value)) => {
            if let Ok(Some(alias)) = alias {
                return Ok(Detail::Alias(alias));
            }
            // A served detail is native even if only the classifier failed.
            let (versions, truncated) = fetch_versions(coordinate).await?;
            Ok(Detail::Native(parse_native(
                coordinate, &value, versions, truncated,
            )?))
        }
        Ok(None) => match alias {
            Ok(Some(alias)) => Ok(Detail::Alias(alias)),
            Ok(None) => Err(AppError::clawhub_detail_missing("上游返回 HTTP 404")),
            Err(error) => Err(error),
        },
        Err(error) => Err(error),
    }
}

/// The files packaged in one explicit version. `license`/`changelog` come
/// from that same version so version switching never mixes metadata.
pub async fn version_files(
    coordinate: &Coordinate,
    version: &str,
) -> Result<VersionFiles, AppError> {
    validate_coordinate(coordinate)?;
    validate_version(version)?;
    let path = format!("/api/v1/skills/{}/versions/{}", coordinate.slug, version);
    let key = format!(
        "version|{}/{}|{version}",
        coordinate.namespace, coordinate.slug
    );
    match client::get_at_optional(
        &path,
        &key,
        &[("ownerHandle", coordinate.namespace.clone())],
    )
    .await?
    {
        Some(value) => parse_version_files(coordinate, version, &value),
        None => Err(AppError::clawhub_version_missing(
            version,
            "上游返回 HTTP 404",
        )),
    }
}

/// Read-only preview of one file from one explicit version. Oversized files
/// return metadata only and are never downloaded.
pub async fn read_file(
    coordinate: &Coordinate,
    version: &str,
    path: &str,
) -> Result<SkillFileContent, AppError> {
    validate_path(path)?;
    let listing = version_files(coordinate, version).await?;
    let entry = listing
        .files
        .iter()
        .find(|entry| entry.path == path)
        .ok_or_else(|| {
            AppError::clawhub_file_missing(path, format!("版本 v{version} 的文件列表中没有该路径"))
        })?;
    let mut result = preview_metadata(entry);
    if entry.size_bytes > result.limit_bytes {
        result.status = SkillFileStatus::TooLarge;
        return Ok(result);
    }
    let query = [
        ("path", path.to_string()),
        ("version", version.to_string()),
        ("ownerHandle", coordinate.namespace.clone()),
    ];
    let body = client::get_bytes_at(
        &format!("/api/v1/skills/{}/file", coordinate.slug),
        &query,
        result.limit_bytes,
    )
    .await?;
    match body {
        client::Payload::NotFound => Err(AppError::clawhub_file_missing(path, "上游返回 HTTP 404")),
        client::Payload::TooLarge => {
            result.status = SkillFileStatus::TooLarge;
            Ok(result)
        }
        client::Payload::Body(bytes) => decode(entry, bytes),
    }
}

fn validate_coordinate(coordinate: &Coordinate) -> Result<(), AppError> {
    if coordinate.source != MarketplaceSource::Clawhub {
        return Err(AppError::clawhub_unavailable(
            "ClawHub 详情需要 ClawHub 来源坐标",
        ));
    }
    coordinate.validate()
}

/// Exactly one bounded, exact-mode search call per opened detail. A search
/// failure never turns a served native detail into an error.
async fn classify_alias(coordinate: &Coordinate) -> Result<Option<AliasDetail>, AppError> {
    let key = format!("alias|{}/{}", coordinate.namespace, coordinate.slug);
    let value = client::get_at_optional(
        "/api/v1/search",
        &key,
        &[
            ("q", coordinate.slug.clone()),
            ("mode", "exact".to_string()),
            ("limit", MAX_SEARCH_RESULTS.to_string()),
        ],
    )
    .await?;
    match value {
        Some(value) => parse_alias(coordinate, &value),
        None => Ok(None),
    }
}

async fn fetch_versions(coordinate: &Coordinate) -> Result<(Vec<VersionRef>, bool), AppError> {
    let path = format!("/api/v1/skills/{}/versions", coordinate.slug);
    let mut versions: Vec<VersionRef> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut cursor: Option<String> = None;
    for page in 0..MAX_VERSION_PAGES {
        let mut query = vec![
            ("limit", MAX_VERSION_ITEMS.to_string()),
            ("ownerHandle", coordinate.namespace.clone()),
        ];
        if let Some(cursor) = &cursor {
            query.push(("cursor", cursor.clone()));
        }
        let key = format!(
            "versions|{}/{}|{}",
            coordinate.namespace,
            coordinate.slug,
            cursor.as_deref().unwrap_or("first")
        );
        let value = client::get_at(&path, &key, &query).await?;
        let (page_versions, next) = parse_version_page(&value)?;
        for version in page_versions {
            if !seen.insert(version.version.clone()) {
                return Err(AppError::clawhub_unavailable("版本列表跨页包含重复版本"));
            }
            versions.push(version);
        }
        match next {
            Some(next) if page + 1 < MAX_VERSION_PAGES => cursor = Some(next),
            Some(_) => return Ok((versions, true)),
            None => return Ok((versions, false)),
        }
    }
    Ok((versions, cursor.is_some()))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub path: String,
    pub size_bytes: u64,
    #[serde(skip_serializing)]
    sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionFiles {
    pub coordinate: Coordinate,
    pub version: String,
    pub license: Option<String>,
    pub changelog: Option<String>,
    pub files: Vec<Entry>,
}

fn preview_metadata(entry: &Entry) -> SkillFileContent {
    let kind = file_kind(&entry.path);
    SkillFileContent {
        path: entry.path.clone(),
        display_path: entry.path.clone(),
        kind,
        status: SkillFileStatus::Ok,
        content: None,
        data_url: None,
        size_bytes: Some(entry.size_bytes),
        limit_bytes: limit_for(kind),
        error: None,
    }
}

fn decode(entry: &Entry, bytes: Vec<u8>) -> Result<SkillFileContent, AppError> {
    let mut result = preview_metadata(entry);
    if bytes.len() as u64 != entry.size_bytes
        || entry
            .sha256
            .as_ref()
            .is_some_and(|hash| !format!("{:x}", Sha256::digest(&bytes)).eq_ignore_ascii_case(hash))
    {
        return Err(AppError::clawhub_unavailable(
            "文件内容与该版本的文件列表不一致，请重试",
        ));
    }
    if result.kind == SkillFileKind::Image {
        let mime = image_mime(&entry.path).expect("image kind has a supported MIME type");
        result.data_url = Some(format!("data:{mime};base64,{}", BASE64.encode(bytes)));
    } else if bytes.contains(&0) {
        result.status = SkillFileStatus::Binary;
    } else if let Ok(text) = String::from_utf8(bytes) {
        result.content = Some(text);
    } else {
        result.status = SkillFileStatus::Binary;
    }
    Ok(result)
}

fn parse_version_files(
    coordinate: &Coordinate,
    version: &str,
    value: &Value,
) -> Result<VersionFiles, AppError> {
    let object = value
        .as_object()
        .ok_or_else(|| AppError::clawhub_unavailable("版本详情响应不是对象"))?;
    let skill = object
        .get("skill")
        .filter(|skill| skill.is_object())
        .ok_or_else(|| AppError::clawhub_unavailable("版本详情响应缺少 skill"))?;
    let slug = skill
        .get("slug")
        .and_then(Value::as_str)
        .filter(|slug| *slug == coordinate.slug)
        .ok_or_else(|| AppError::clawhub_unavailable("版本详情与请求的 ClawHub 标识不一致"))?;
    debug_assert_eq!(slug, coordinate.slug);
    let version_value = object
        .get("version")
        .filter(|version| version.is_object())
        .ok_or_else(|| AppError::clawhub_unavailable("版本详情响应缺少 version"))?;
    if version_value.get("version").and_then(Value::as_str) != Some(version) {
        return Err(AppError::clawhub_unavailable("版本详情与请求的版本不一致"));
    }
    let items = version_value
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::clawhub_unavailable("版本详情响应缺少文件列表"))?;
    if items.len() > MAX_FILES {
        return Err(AppError::clawhub_unavailable("文件数量超过浏览上限"));
    }
    let mut paths = std::collections::BTreeSet::new();
    let mut files = Vec::with_capacity(items.len());
    for item in items {
        let path = item.get("path").and_then(Value::as_str).unwrap_or("");
        validate_path(path)?;
        if !paths.insert(path.to_string()) {
            return Err(AppError::clawhub_unavailable("文件列表包含重复路径"));
        }
        let size_bytes = item
            .get("size")
            .and_then(Value::as_u64)
            .ok_or_else(|| AppError::clawhub_unavailable("文件缺少有效大小"))?;
        let sha256 = match item.get("sha256") {
            None | Some(Value::Null) => None,
            Some(Value::String(hash))
                if hash.len() == 64 && hash.bytes().all(|c| c.is_ascii_hexdigit()) =>
            {
                Some(hash.clone())
            }
            Some(_) => return Err(AppError::clawhub_unavailable("文件摘要无效")),
        };
        files.push(Entry {
            path: path.to_string(),
            size_bytes,
            sha256,
        });
    }
    for path in &paths {
        for (index, _) in path.match_indices('/') {
            if paths.contains(&path[..index]) {
                return Err(AppError::clawhub_unavailable("文件与目录路径冲突"));
            }
        }
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(VersionFiles {
        coordinate: coordinate.clone(),
        version: version.to_string(),
        license: optional_text(version_value, "license", MAX_LICENSE)?,
        changelog: optional_text(version_value, "changelog", MAX_CHANGELOG)?,
        files,
    })
}

/// Remote file paths are literal, relative, slash-separated and bounded. The
/// same rules as the SkillHub versioned file browser apply so previews cannot
/// address anything outside the reported package.
fn validate_path(path: &str) -> Result<(), AppError> {
    if path.is_empty()
        || path.len() > 4096
        || path.trim() != path
        || path
            .chars()
            .any(|c| c.is_control() || matches!(c, '\\' | ':'))
        || path.split('/').count() > 32
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.len() > 255)
    {
        return Err(AppError::clawhub_unavailable("无效的 ClawHub 文件相对路径"));
    }
    Ok(())
}

/// Optional detail text. Unlike the catalog helper, a blank string is a real
/// upstream no-value (live version changelogs are commonly `""`), so it stays
/// missing instead of failing the whole response.
fn optional_text(row: &Value, key: &str, max: usize) -> Result<Option<String>, AppError> {
    match row.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if value.trim().is_empty() => Ok(None),
        _ => list::optional_text(row, key, max),
    }
}

/// Match one exact `owner/slug` row in an exact-mode search response. Only
/// `install.kind: "skills-sh"` rows are aliases; every other kind (or no row)
/// means this ClawHub entry is not a skills.sh alias.
fn parse_alias(coordinate: &Coordinate, value: &Value) -> Result<Option<AliasDetail>, AppError> {
    let object = value
        .as_object()
        .ok_or_else(|| AppError::clawhub_unavailable("别名分类响应不是对象"))?;
    let results = object
        .get("results")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::clawhub_unavailable("别名分类响应缺少 results"))?;
    if results.len() > MAX_SEARCH_RESULTS {
        return Err(AppError::clawhub_unavailable("别名分类响应超出单条上限"));
    }
    for row in results {
        let owner = row
            .get("ownerHandle")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let slug = row.get("slug").and_then(Value::as_str).unwrap_or_default();
        if owner != coordinate.namespace || slug != coordinate.slug {
            continue;
        }
        let install = row
            .get("install")
            .and_then(Value::as_object)
            .ok_or_else(|| AppError::clawhub_unavailable("别名分类条目缺少 install"))?;
        let kind = install
            .get("kind")
            .and_then(Value::as_str)
            .filter(|kind| !kind.is_empty() && kind.len() <= MAX_SEARCH_KIND)
            .ok_or_else(|| AppError::clawhub_unavailable("别名分类条目 install.kind 无效"))?;
        if kind != "skills-sh" {
            return Ok(None);
        }
        let reference = install
            .get("reference")
            .and_then(Value::as_str)
            .filter(|reference| {
                reference.starts_with("skills-sh:")
                    && reference.len() <= MAX_ALIAS_REFERENCE
                    && !reference.chars().any(char::is_control)
            })
            .map(str::to_string);
        let source_url = install
            .get("sourceUrl")
            .and_then(Value::as_str)
            .and_then(skills_sh_url);
        let repository = row
            .get("sourceIdentity")
            .and_then(Value::as_object)
            .and_then(|identity| {
                let owner = identity.get("owner")?.as_str()?;
                let repo = identity.get("repo")?.as_str()?;
                Some(format!("{owner}/{repo}"))
            })
            .filter(|repository| repository.len() <= MAX_ALIAS_REFERENCE);
        return Ok(Some(AliasDetail {
            coordinate: coordinate.clone(),
            name: optional_text(row, "displayName", list::MAX_NAME)?,
            reference,
            source_url,
            repository,
            canonical_url: coordinate.url(),
        }));
    }
    Ok(None)
}

/// Only skills.sh HTTPS links are surfaced; anything else is treated as not
/// provided rather than offering an arbitrary external navigation target.
fn skills_sh_url(raw: &str) -> Option<String> {
    let url = reqwest::Url::parse(raw).ok()?;
    if url.scheme() != "https" {
        return None;
    }
    match url.host_str() {
        Some("skills.sh" | "www.skills.sh") => Some(url.to_string()),
        _ => None,
    }
}

fn parse_version_page(value: &Value) -> Result<(Vec<VersionRef>, Option<String>), AppError> {
    let object = value
        .as_object()
        .ok_or_else(|| AppError::clawhub_unavailable("版本列表响应不是对象"))?;
    let items = object
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::clawhub_unavailable("版本列表响应缺少 items"))?;
    if items.len() > MAX_VERSION_ITEMS {
        return Err(AppError::clawhub_unavailable("版本列表超出单次条目限制"));
    }
    let mut versions = Vec::with_capacity(items.len());
    let mut seen = std::collections::BTreeSet::new();
    for item in items {
        let version = item
            .get("version")
            .and_then(Value::as_str)
            .filter(|version| !version.trim().is_empty() && version.len() <= 200)
            .ok_or_else(|| AppError::clawhub_unavailable("版本条目缺少有效 version"))?;
        validate_version(version)
            .map_err(|_| AppError::clawhub_unavailable("版本条目 version 无效"))?;
        if !seen.insert(version.to_string()) {
            return Err(AppError::clawhub_unavailable("版本列表包含重复版本"));
        }
        let created_at = match item.get("createdAt") {
            None | Some(Value::Null) => None,
            Some(value) => Some(
                value
                    .as_i64()
                    .ok_or_else(|| AppError::clawhub_unavailable("版本条目 createdAt 无效"))?,
            ),
        };
        versions.push(VersionRef {
            version: version.to_string(),
            created_at,
            changelog: optional_text(item, "changelog", MAX_CHANGELOG)?,
        });
    }
    let cursor = match object.get("nextCursor") {
        None | Some(Value::Null) => None,
        Some(Value::String(cursor)) => {
            let cursor = cursor.trim();
            if cursor.len() > MAX_CURSOR || cursor.chars().any(char::is_control) {
                return Err(AppError::clawhub_unavailable("版本列表游标无效"));
            }
            (!cursor.is_empty()).then(|| cursor.to_string())
        }
        Some(_) => return Err(AppError::clawhub_unavailable("版本列表游标无效")),
    };
    Ok((versions, cursor))
}

fn parse_native(
    coordinate: &Coordinate,
    value: &Value,
    versions: Vec<VersionRef>,
    truncated: bool,
) -> Result<NativeDetail, AppError> {
    let object = value
        .as_object()
        .ok_or_else(|| AppError::clawhub_unavailable("详情响应不是对象"))?;
    let skill = object
        .get("skill")
        .filter(|skill| skill.is_object())
        .ok_or_else(|| AppError::clawhub_unavailable("详情响应缺少 skill"))?;
    let slug = skill
        .get("slug")
        .and_then(Value::as_str)
        .filter(|slug| *slug == coordinate.slug)
        .ok_or_else(|| AppError::clawhub_unavailable("详情与请求的 ClawHub 标识不一致"))?;
    debug_assert_eq!(slug, coordinate.slug);
    let owner = object
        .get("owner")
        .filter(|owner| owner.is_object())
        .ok_or_else(|| AppError::clawhub_unavailable("详情响应缺少 owner"))?;
    let handle = owner
        .get("handle")
        .and_then(Value::as_str)
        .filter(|handle| *handle == coordinate.namespace)
        .ok_or_else(|| AppError::clawhub_unavailable("详情与请求的 ClawHub 发布者不一致"))?;
    debug_assert_eq!(handle, coordinate.namespace);

    let latest = match object.get("latestVersion") {
        None | Some(Value::Null) => None,
        Some(latest) if latest.is_object() => Some(latest),
        Some(_) => {
            return Err(AppError::clawhub_unavailable("详情响应 latestVersion 无效"));
        }
    };
    let latest_version = match latest.and_then(|latest| latest.get("version")) {
        None | Some(Value::Null) => None,
        Some(version) => {
            let version = version
                .as_str()
                .filter(|version| !version.trim().is_empty() && version.len() <= 200)
                .ok_or_else(|| {
                    AppError::clawhub_unavailable("详情响应 latestVersion.version 无效")
                })?;
            Some(version.to_string())
        }
    };
    let license = latest
        .map(|latest| optional_text(latest, "license", MAX_LICENSE))
        .transpose()?
        .flatten();
    let changelog = latest
        .map(|latest| optional_text(latest, "changelog", MAX_CHANGELOG))
        .transpose()?
        .flatten();
    // `skill.description` is the full SKILL.md on the live API; `summary` is
    // the short public summary, so the longer body is only a fallback.
    let description = optional_text(skill, "summary", list::MAX_TEXT)?.or(optional_text(
        skill,
        "description",
        list::MAX_TEXT,
    )
    .ok()
    .flatten());

    Ok(NativeDetail {
        coordinate: coordinate.clone(),
        name: optional_text(skill, "displayName", list::MAX_NAME)?,
        description,
        topics: list::topics(skill)?,
        metrics: list::metrics(skill)?,
        owner_display_name: optional_text(owner, "displayName", list::MAX_NAME)?,
        latest_version,
        license,
        changelog,
        canonical_url: coordinate.url(),
        versions,
        versions_truncated: truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn coordinate() -> Coordinate {
        Coordinate::from_canonical_for(MarketplaceSource::Clawhub, "@steipete/weather").unwrap()
    }

    #[test]
    fn previews_text_and_images_and_verifies_the_reported_digest() {
        let text = Entry {
            path: "notes.txt".into(),
            size_bytes: 4,
            sha256: Some("982d9e3eb996f559e633f4d194def3761d909f5a3b647d1a851fead67c32c9d1".into()),
        };
        let preview = decode(&text, b"text".to_vec()).unwrap();
        assert_eq!(preview.status, SkillFileStatus::Ok);
        assert_eq!(preview.kind, SkillFileKind::Text);
        assert_eq!(preview.content.as_deref(), Some("text"));
        assert_eq!(preview.limit_bytes, 1024 * 1024);
        assert!(preview.data_url.is_none());

        let markdown = Entry {
            path: "SKILL.md".into(),
            size_bytes: 4,
            sha256: Some("38c64a17e33e98b7abb8edace0888dffe5918eea28fe4812281fa1ecc0664af4".into()),
        };
        let preview = decode(&markdown, b"# Hi".to_vec()).unwrap();
        assert_eq!(preview.kind, SkillFileKind::Markdown);
        assert_eq!(preview.content.as_deref(), Some("# Hi"));

        let image = Entry {
            path: "assets/preview.png".into(),
            size_bytes: 4,
            sha256: None,
        };
        let preview = decode(&image, vec![0x89, b'P', b'N', b'G']).unwrap();
        assert_eq!(preview.kind, SkillFileKind::Image);
        assert!(
            preview
                .data_url
                .as_deref()
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
        assert_eq!(preview.limit_bytes, 5 * 1024 * 1024);

        for (path, bytes) in [
            ("blob.bin", b"abc\0def".to_vec()),
            ("invalid.txt", vec![0xff, 0xfe, 0xfd]),
        ] {
            let entry = Entry {
                path: path.into(),
                size_bytes: bytes.len() as u64,
                sha256: None,
            };
            assert_eq!(
                decode(&entry, bytes).unwrap().status,
                SkillFileStatus::Binary,
                "{path}"
            );
        }

        assert!(decode(&text, b"oops".to_vec()).is_err());
        assert!(decode(&text, b"short".to_vec()).is_err());
    }

    #[test]
    fn validates_remote_paths_before_building_any_request() {
        for path in [
            "",
            "/etc/passwd",
            "../secret",
            "a/../b",
            "a//b",
            "a\\b",
            "a\0b",
            "./SKILL.md",
            "https://example.com/a",
            " spaced",
        ] {
            assert!(validate_path(path).is_err(), "{path:?}");
        }
        assert!(validate_path(&format!("{}/x", "a".repeat(256))).is_err());
        assert!(validate_path(&"a/".repeat(33)).is_err());
        assert!(validate_path("references/中文 文档.md").is_ok());
        assert!(validate_path("SKILL.md").is_ok());
    }

    #[test]
    fn parses_version_files_and_rejects_identity_path_or_digest_problems() {
        let value = json!({
            "skill": {"slug": "nano-banana-pro", "displayName": "Nano Banana Pro"},
            "version": {
                "version": "1.0.0",
                "createdAt": 1767651987917_i64,
                "changelog": "Initial release",
                "changelogSource": null,
                "license": "MIT-0",
                "files": [
                    {
                        "path": "scripts/generate_image.py",
                        "size": 5613,
                        "sha256": "8ecca97db200cab923716173fed18e48ce7e291b1726c4e6ed79fa88302023e8",
                        "contentType": "text/plain"
                    },
                    {
                        "path": "SKILL.md",
                        "size": 1083,
                        "sha256": "bf123f49b84e6ba8dbd5ef4998bdffe9fc3770797e5fe013f0a31824a8ad1ec7",
                        "contentType": "text/markdown"
                    }
                ],
                "security": {"status": "clean", "scanners": {"skillspector": {"severity": "LOW"}}}
            }
        });
        let nano =
            Coordinate::from_canonical_for(MarketplaceSource::Clawhub, "@steipete/nano-banana-pro")
                .unwrap();
        let listing = parse_version_files(&nano, "1.0.0", &value).unwrap();
        assert_eq!(listing.version, "1.0.0");
        assert_eq!(listing.license.as_deref(), Some("MIT-0"));
        assert_eq!(listing.changelog.as_deref(), Some("Initial release"));
        assert_eq!(listing.files[0].path, "SKILL.md");
        assert_eq!(listing.files[0].size_bytes, 1083);
        assert!(listing.files[0].sha256.is_some());
        assert_eq!(listing.files[1].path, "scripts/generate_image.py");
        assert!(
            !serde_json::to_string(&listing)
                .unwrap()
                .contains("security")
        );

        assert!(parse_version_files(&nano, "2.0.0", &value).is_err());
        assert!(parse_version_files(&coordinate(), "1.0.0", &value).is_err());

        for mutate in 0..12 {
            let mut broken = value.clone();
            match mutate {
                0 => broken["version"]["version"] = json!("2.0.0"),
                1 => broken["skill"]["slug"] = json!("other"),
                2 => broken["version"]["files"] = json!({}),
                3 => broken["version"]["files"][0]["path"] = json!("../escape"),
                4 => broken["version"]["files"][0]["path"] = json!("/etc/passwd"),
                5 => broken["version"]["files"][1]["path"] = json!("scripts/generate_image.py"),
                6 => broken["version"]["files"][1]["path"] = json!("scripts"),
                7 => broken["version"]["files"][0]["size"] = json!("many"),
                8 => broken["version"]["files"][0]["sha256"] = json!("abc"),
                9 => broken["version"]["files"][0]["sha256"] = json!("z".repeat(64)),
                10 => broken["version"]["license"] = json!(7),
                11 => broken["version"]["changelog"] = json!("x".repeat(MAX_CHANGELOG + 1)),
                _ => unreachable!(),
            }
            assert!(
                parse_version_files(&nano, "1.0.0", &broken).is_err(),
                "mutation {mutate}: {broken}"
            );
        }
    }

    #[test]
    fn classifies_exact_skills_sh_aliases_without_matching_other_owners() {
        let coordinate =
            Coordinate::from_canonical_for(MarketplaceSource::Clawhub, "@anthropics/debug")
                .unwrap();
        let alias = parse_alias(
            &coordinate,
            &json!({
                "results": [
                    {
                        "ownerHandle": "sergeypetrakov",
                        "slug": "debug",
                        "displayName": "debug",
                        "install": {"kind": "clawhub", "reference": "sergeypetrakov/debug", "sourceUrl": null},
                        "source": "clawhub"
                    },
                    {
                        "ownerHandle": "anthropics",
                        "slug": "debug",
                        "displayName": "debug",
                        "install": {
                            "kind": "skills-sh",
                            "reference": "skills-sh:anthropics/knowledge-work-plugins/debug",
                            "sourceUrl": "https://www.skills.sh/anthropics/knowledge-work-plugins/debug"
                        },
                        "source": "skills-sh",
                        "sourceIdentity": {
                            "host": null,
                            "id": "anthropics/knowledge-work-plugins/debug",
                            "owner": "anthropics",
                            "repo": "knowledge-work-plugins"
                        }
                    }
                ]
            }),
        )
        .unwrap()
        .expect("alias");
        assert_eq!(alias.coordinate.canonical(), "@anthropics/debug");
        assert_eq!(
            alias.reference.as_deref(),
            Some("skills-sh:anthropics/knowledge-work-plugins/debug")
        );
        assert_eq!(
            alias.source_url.as_deref(),
            Some("https://www.skills.sh/anthropics/knowledge-work-plugins/debug")
        );
        assert_eq!(
            alias.repository.as_deref(),
            Some("anthropics/knowledge-work-plugins")
        );
        assert_eq!(
            alias.canonical_url,
            "https://clawhub.ai/anthropics/skills/debug"
        );

        for not_alias in [
            json!({"results": []}),
            json!({"results": [{"ownerHandle": "anthropics", "slug": "debug", "install": {"kind": "clawhub"}}]}),
            json!({"results": [{"ownerHandle": "other", "slug": "debug", "install": {"kind": "skills-sh", "reference": "skills-sh:a/b/c"}}]}),
            json!({"results": [{"ownerHandle": "anthropics", "slug": "other", "install": {"kind": "skills-sh"}}]}),
        ] {
            assert!(
                parse_alias(&coordinate, &not_alias).unwrap().is_none(),
                "{not_alias}"
            );
        }

        // Optional link fields never fail the classification; a non-skills.sh
        // URL is dropped instead of being opened by the app.
        let bare = parse_alias(
            &coordinate,
            &json!({
                "results": [{
                    "ownerHandle": "anthropics",
                    "slug": "debug",
                    "install": {
                        "kind": "skills-sh",
                        "reference": "skills-sh:anthropics/knowledge-work-plugins/debug",
                        "sourceUrl": "https://evil.example/debug"
                    }
                }]
            }),
        )
        .unwrap()
        .expect("alias");
        assert_eq!(bare.source_url, None);
        assert_eq!(bare.repository, None);

        for bad in [
            json!(null),
            json!({}),
            json!({"results": {}}),
            json!({"results": [{"ownerHandle": "anthropics", "slug": "debug", "install": {"kind": ""}}]}),
            json!({"results": [{"ownerHandle": "anthropics", "slug": "debug", "install": {"kind": 7}}]}),
        ] {
            assert!(parse_alias(&coordinate, &bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn parses_version_pages_and_rejects_malformed_or_unbounded_items() {
        let (versions, cursor) = parse_version_page(&json!({
            "items": [
                {"version": "1.0.1", "createdAt": 1767651987917_i64, "changelog": "fixes", "changelogSource": "commits"},
                {"version": "1.0.0", "createdAt": 1767545394459_i64, "changelog": "", "changelogSource": null}
            ],
            "nextCursor": "cursor-next"
        }))
        .unwrap();
        assert_eq!(cursor.as_deref(), Some("cursor-next"));
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].version, "1.0.1");
        assert_eq!(versions[0].created_at, Some(1767651987917));
        assert_eq!(versions[0].changelog.as_deref(), Some("fixes"));
        assert_eq!(versions[1].changelog, None);

        let (empty, cursor) =
            parse_version_page(&json!({"items": [], "nextCursor": null})).unwrap();
        assert!(empty.is_empty());
        assert_eq!(cursor, None);

        for bad in [
            json!(null),
            json!({}),
            json!({"items": {}}),
            json!({"items": [{}]}),
            json!({"items": [{"version": ""}]}),
            json!({"items": [{"version": "latest"}]}),
            json!({"items": [{"version": "1.0.0/../x"}]}),
            json!({"items": [{"version": 2}]}),
            json!({"items": [{"version": "1.0.0"}, {"version": "1.0.0"}]}),
            json!({"items": [{"version": "1.0.0", "createdAt": "today"}]}),
            json!({"items": [{"version": "1.0.0", "changelog": 7}]}),
            json!({"items": [{"version": "1.0.0", "changelog": "x".repeat(MAX_CHANGELOG + 1)}]}),
            json!({"items": [], "nextCursor": 1}),
            json!({"items": [], "nextCursor": "bad\ncursor"}),
        ] {
            assert!(parse_version_page(&bad).is_err(), "{bad}");
        }
        let too_many: Vec<Value> = (0..=MAX_VERSION_ITEMS)
            .map(|index| json!({"version": format!("1.0.{index}")}))
            .collect();
        assert!(parse_version_page(&json!({"items": too_many, "nextCursor": null})).is_err());
        assert!(
            parse_version_page(&json!({
                "items": [],
                "nextCursor": "x".repeat(MAX_CURSOR + 1)
            }))
            .is_err()
        );
    }

    #[tokio::test]
    #[ignore = "read-only live ClawHub detail/versions/files; requires network"]
    async fn clawhub_live_detail_versions_files_and_alias_contract() {
        let weather =
            Coordinate::from_canonical_for(MarketplaceSource::Clawhub, "@steipete/weather")
                .unwrap();
        let Detail::Native(weather_detail) = detail(&weather).await.unwrap() else {
            panic!("weather is a native ClawHub entry");
        };
        assert_eq!(weather_detail.name.as_deref(), Some("Weather"));
        assert_eq!(weather_detail.latest_version.as_deref(), Some("1.0.0"));
        assert!(!weather_detail.versions.is_empty());
        assert_eq!(weather_detail.versions[0].version, "1.0.0");
        assert_eq!(
            weather_detail.canonical_url,
            "https://clawhub.ai/steipete/skills/weather"
        );

        let version = weather_detail.versions[0].version.clone();
        let files = version_files(&weather, &version).await.unwrap();
        assert_eq!(files.version, version);
        assert!(files.files.iter().any(|file| file.path == "SKILL.md"));
        assert!(files.files.iter().all(|file| file.size_bytes > 0));
        let markdown = read_file(&weather, &version, "SKILL.md").await.unwrap();
        assert_eq!(markdown.status, SkillFileStatus::Ok);
        assert_eq!(markdown.kind, SkillFileKind::Markdown);
        assert!(markdown.content.unwrap().contains("# Weather"));

        let missing_version = version_files(&weather, "9.9.9").await.unwrap_err();
        assert!(missing_version.message.contains("版本 v9.9.9 不可用"));
        let missing_file = read_file(&weather, &version, "nope.txt").await.unwrap_err();
        assert!(missing_file.message.contains("nope.txt"));

        // A ClawHub-indexed skills.sh entry has no native detail; the single
        // exact-mode search call classifies it as an alias.
        let alias = Coordinate::from_canonical_for(MarketplaceSource::Clawhub, "@anthropics/debug")
            .unwrap();
        let Detail::Alias(alias_detail) = detail(&alias).await.unwrap() else {
            panic!("anthropics/debug is indexed from skills.sh");
        };
        assert!(
            alias_detail
                .reference
                .as_deref()
                .unwrap_or_default()
                .starts_with("skills-sh:")
        );
        assert!(
            alias_detail
                .source_url
                .as_deref()
                .unwrap_or_default()
                .starts_with("https://www.skills.sh/")
        );

        // GitHub-backed entries have no ClawHub-hosted versions.
        let github =
            Coordinate::from_canonical_for(MarketplaceSource::Clawhub, "@aws/agents-build")
                .unwrap();
        let Detail::Native(github_detail) = detail(&github).await.unwrap() else {
            panic!("agents-build is native GitHub-backed");
        };
        assert!(github_detail.latest_version.is_none());
        assert!(github_detail.versions.is_empty());
    }

    #[test]
    fn parses_live_detail_and_never_reads_scan_fields() {
        let detail = parse_native(
            &coordinate(),
            &json!({
                "skill": {
                    "slug": "weather",
                    "displayName": "Weather",
                    "summary": "Get current weather and forecasts (no API key required).",
                    "icon": null,
                    "description": "---\nname: weather\n---\n# Weather\n",
                    "topics": ["Current Weather"],
                    "tags": {"latest": "1.0.0"},
                    "stats": {"comments": 6, "downloads": 173979, "installs": 6528, "stars": 431, "versions": 1},
                    "createdAt": 1767545394459_i64,
                    "updatedAt": 1789594554485_i64
                },
                "latestVersion": {"version": "1.0.0", "createdAt": 1767545394459_i64, "changelog": "", "license": null},
                "metadata": {"setup": [], "os": null, "systems": null},
                "owner": {"handle": "steipete", "userId": "s179", "displayName": "Peter Steinberger", "image": "https://example.test/a.png"},
                "moderation": {
                    "isSuspicious": false,
                    "isMalwareBlocked": false,
                    "verdict": "clean",
                    "reasonCodes": ["review.llm_review"],
                    "summary": "Review: review.llm_review",
                    "engineVersion": "v2.4.26",
                    "updatedAt": 1789013612043_i64
                }
            }),
            vec![VersionRef {
                version: "1.0.0".into(),
                created_at: Some(1767545394459),
                changelog: None,
            }],
            false,
        )
        .unwrap();

        assert_eq!(detail.coordinate.source, MarketplaceSource::Clawhub);
        assert_eq!(detail.name.as_deref(), Some("Weather"));
        assert_eq!(
            detail.description.as_deref(),
            Some("Get current weather and forecasts (no API key required).")
        );
        assert_eq!(detail.topics, ["Current Weather"]);
        assert_eq!(
            detail.metrics,
            list::Metrics {
                downloads: Some(173979),
                installs: Some(6528),
                stars: Some(431),
            }
        );
        assert_eq!(
            detail.owner_display_name.as_deref(),
            Some("Peter Steinberger")
        );
        assert_eq!(detail.latest_version.as_deref(), Some("1.0.0"));
        assert_eq!(detail.license, None);
        assert_eq!(detail.changelog, None);
        assert_eq!(
            detail.canonical_url,
            "https://clawhub.ai/steipete/skills/weather"
        );
        assert_eq!(detail.versions.len(), 1);
        assert!(!detail.versions_truncated);

        let serialized = serde_json::to_value(Detail::Native(detail)).unwrap();
        assert_eq!(serialized["kind"], "native");
        let text = serialized.to_string();
        for forbidden in ["moderation", "isSuspicious", "security", "engineVersion"] {
            assert!(!text.contains(forbidden), "{forbidden}");
        }
    }
}
