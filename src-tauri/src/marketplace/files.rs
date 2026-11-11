//! Versioned, read-only files from the public marketplace. No installation or local paths.
use super::{Coordinate, provider, validate_version};
use crate::{
    content::{SkillFileContent, SkillFileKind, SkillFileStatus, file_kind, image_mime, limit_for},
    error::AppError,
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::HashSet, time::Duration};

const API: &str = "https://api.skillhub.cn/api/v1/skills";
const MAX_FILES: usize = 2_000;
const MAX_LIST_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub path: String,
    pub size_bytes: u64,
    #[serde(skip_serializing)]
    sha256: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Listing {
    pub coordinate: Coordinate,
    pub version: String,
    pub files: Vec<Entry>,
}

fn allowed_host(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.port_or_known_default() == Some(443)
        && matches!(
            url.host_str(),
            Some(
                "api.skillhub.cn"
                    | "skillhub-1388575217.cos.accelerate.myqcloud.com"
                    | "skillhub-1388575217.cos.ap-guangzhou.myqcloud.com"
            )
        )
}

fn client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .https_only(true)
        .user_agent("SkillsBox/0.1 SkillHub-marketplace")
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() < 5 && allowed_host(attempt.url()) {
                attempt.follow()
            } else {
                attempt.error("文件地址不属于支持的 SkillHub 官方服务")
            }
        }))
        .build()
        .map_err(AppError::marketplace_unavailable)
}

fn validate_path(path: &str) -> Result<(), AppError> {
    if path.is_empty()
        || path.len() > 4096
        || path.trim() != path
        || path
            .chars()
            .any(|c| c.is_control() || matches!(c, '\\' | ':' | '%'))
        || path.split('/').count() > 32
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.len() > 255)
    {
        return Err(AppError::marketplace_unavailable("无效的市场文件相对路径"));
    }
    Ok(())
}

async fn bounded_body(
    mut response: reqwest::Response,
    limit: u64,
) -> Result<Option<Vec<u8>>, AppError> {
    if response.content_length().is_some_and(|size| size > limit) {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(AppError::marketplace_unavailable)?
    {
        if bytes.len() as u64 + chunk.len() as u64 > limit {
            return Ok(None);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(Some(bytes))
}

pub async fn list(coordinate: &Coordinate, version: &str) -> Result<Listing, AppError> {
    coordinate.validate()?;
    validate_version(version)?;
    let response = client()?
        .get(format!("{API}/{}/files", coordinate.slug))
        .query(&[
            ("namespace", coordinate.namespace.as_str()),
            ("version", version),
        ])
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(AppError::marketplace_unavailable)?;
    let bytes = bounded_body(response, MAX_LIST_BYTES)
        .await?
        .ok_or_else(|| AppError::marketplace_unavailable("文件列表超过 2 MiB 浏览上限"))?;
    let value = serde_json::from_slice(&bytes).map_err(AppError::marketplace_unavailable)?;
    parse_listing(coordinate, version, &value)
}

fn parse_listing(
    coordinate: &Coordinate,
    version: &str,
    value: &Value,
) -> Result<Listing, AppError> {
    if provider::coordinate(value)? != *coordinate || value["version"].as_str() != Some(version) {
        return Err(AppError::marketplace_unavailable(
            "文件列表的市场坐标或版本与当前详情不一致",
        ));
    }
    let items = value["files"]
        .as_array()
        .ok_or_else(|| AppError::marketplace_unavailable("平台未提供文件列表"))?;
    if items.len() > MAX_FILES {
        return Err(AppError::marketplace_unavailable(
            "文件数量超过 2,000 项浏览上限",
        ));
    }
    if value["count"].as_u64() != Some(items.len() as u64) {
        return Err(AppError::marketplace_unavailable(
            "平台返回了不完整的文件列表",
        ));
    }
    let mut paths = HashSet::new();
    let mut files = Vec::with_capacity(items.len());
    for item in items {
        let path = item["path"].as_str().unwrap_or("");
        validate_path(path)?;
        if !paths.insert(path) {
            return Err(AppError::marketplace_unavailable("文件列表包含重复路径"));
        }
        let size_bytes = item["size"]
            .as_u64()
            .ok_or_else(|| AppError::marketplace_unavailable("文件缺少有效大小"))?;
        let sha256 = item["sha256"]
            .as_str()
            .filter(|hash| !hash.is_empty())
            .map(str::to_owned);
        if sha256
            .as_ref()
            .is_some_and(|hash| hash.len() != 64 || !hash.bytes().all(|c| c.is_ascii_hexdigit()))
        {
            return Err(AppError::marketplace_unavailable("文件摘要无效"));
        }
        files.push(Entry {
            path: path.into(),
            size_bytes,
            sha256,
        });
    }
    for path in &paths {
        for (index, _) in path.match_indices('/') {
            if paths.contains(&path[..index]) {
                return Err(AppError::marketplace_unavailable("文件与目录路径冲突"));
            }
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Listing {
        coordinate: coordinate.clone(),
        version: version.into(),
        files,
    })
}

pub async fn read(
    coordinate: &Coordinate,
    version: &str,
    path: &str,
) -> Result<SkillFileContent, AppError> {
    validate_path(path)?;
    // Resolve the file on the native side; callers cannot supply URLs, hashes or local paths.
    let listing = list(coordinate, version).await?;
    let entry = listing
        .files
        .iter()
        .find(|entry| entry.path == path)
        .ok_or_else(|| AppError::marketplace_unavailable("此版本中没有该文件"))?;
    let mut result = preview_metadata(entry);
    if entry.size_bytes > result.limit_bytes {
        result.status = SkillFileStatus::TooLarge;
        return Ok(result);
    }
    let response = client()?
        .get(format!("{API}/{}/file", coordinate.slug))
        .query(&[
            ("namespace", coordinate.namespace.as_str()),
            ("version", version),
            ("path", path),
        ])
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(AppError::marketplace_unavailable)?;
    let Some(bytes) = bounded_body(response, result.limit_bytes).await? else {
        result.status = SkillFileStatus::TooLarge;
        return Ok(result);
    };
    decode(entry, bytes)
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
        return Err(AppError::marketplace_unavailable(
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

#[cfg(test)]
mod tests {
    use super::*;
    fn coordinate() -> Coordinate {
        Coordinate::from_canonical("@publisher/example").unwrap()
    }
    fn manifest() -> Value {
        serde_json::json!({
            "namespace": {"handle": "publisher", "publicSlug": "example", "canonicalName": "@publisher/example"},
            "version": "1.0.0", "count": 2,
            "files": [{"path": "scripts/example.py", "size": 3}, {"path": "SKILL.md", "size": 4}]
        })
    }
    #[test]
    fn listing_keeps_versioned_paths_and_rejects_partial_or_wrong_identity() {
        let value = manifest();
        let listing = parse_listing(&coordinate(), "1.0.0", &value).unwrap();
        assert_eq!(listing.files[0].path, "SKILL.md");
        assert_eq!(listing.files[1].size_bytes, 3);
        assert!(parse_listing(&coordinate(), "2.0.0", &value).is_err());
        let mut other = value.clone();
        other["namespace"]["handle"] = "other".into();
        assert!(parse_listing(&coordinate(), "1.0.0", &other).is_err());
        let mut partial = value;
        partial["count"] = 3.into();
        assert!(parse_listing(&coordinate(), "1.0.0", &partial).is_err());
    }
    #[test]
    fn rejects_unsafe_ambiguous_and_excessive_paths() {
        for path in [
            "",
            "/etc/passwd",
            "../secret",
            "a/../b",
            "a//b",
            "a\\b",
            "%2e%2e/secret",
            "a\0b",
            "https://example.com/a",
            "./SKILL.md",
        ] {
            assert!(validate_path(path).is_err(), "{path:?}");
        }
        assert!(validate_path(&"a/".repeat(33)).is_err());
        assert!(validate_path("references/中文 文档.md").is_ok());
        for path in ["SKILL.md", "scripts", "../secret"] {
            let mut value = manifest();
            value["files"][1]["path"] = path.into();
            if path == "SKILL.md" {
                value["files"][0]["path"] = path.into();
            }
            assert!(parse_listing(&coordinate(), "1.0.0", &value).is_err());
        }
        let mut value = manifest();
        value["files"] = vec![serde_json::json!({"path": "a", "size": 1}); MAX_FILES + 1].into();
        assert!(parse_listing(&coordinate(), "1.0.0", &value).is_err());
    }
    #[test]
    fn previews_text_and_images_without_executing_content() {
        let entry = Entry {
            path: "test.md".into(),
            size_bytes: 4,
            sha256: None,
        };
        assert_eq!(
            decode(&entry, b"# Hi".to_vec()).unwrap().content.as_deref(),
            Some("# Hi")
        );
        assert_eq!(
            decode(&entry, vec![0, 1, 2, 3]).unwrap().status,
            SkillFileStatus::Binary
        );
        assert_eq!(
            decode(&entry, vec![255; 4]).unwrap().status,
            SkillFileStatus::Binary
        );
        let image = Entry {
            path: "image.png".into(),
            ..entry
        };
        assert!(
            decode(&image, vec![1, 2, 3, 4])
                .unwrap()
                .data_url
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
    }
    #[test]
    fn verifies_file_length_and_digest() {
        let entry = Entry {
            path: "file.txt".into(),
            size_bytes: 4,
            sha256: Some(format!("{:x}", Sha256::digest(b"text"))),
        };
        assert!(decode(&entry, b"text".to_vec()).is_ok());
        assert!(decode(&entry, b"oops".to_vec()).is_err());
        assert!(decode(&entry, b"short".to_vec()).is_err());
    }
    #[test]
    fn redirects_stay_on_official_https_file_hosts() {
        for url in [
            "http://api.skillhub.cn/a",
            "https://api.skillhub.cn.evil.test/a",
            "https://localhost/a",
            "https://api.skillhub.cn:444/a",
        ] {
            assert!(!allowed_host(&url.parse().unwrap()));
        }
        assert!(allowed_host(
            &"https://skillhub-1388575217.cos.accelerate.myqcloud.com/skills/a"
                .parse()
                .unwrap()
        ));
    }
    #[tokio::test]
    #[ignore = "read-only official versioned file API smoke test; requires network"]
    async fn skillhub_live_versioned_files() {
        let c = Coordinate::from_canonical("@indiv-integrator/dev-expert-pro").unwrap();
        let files = list(&c, "1.1.0").await.unwrap();
        assert!(
            files
                .files
                .iter()
                .any(|entry| entry.path == "scripts/skill_lookup.py")
        );
        let markdown = read(&c, "1.1.0", "THIRD_PARTY_NOTICES.md").await.unwrap();
        assert_eq!(markdown.status, SkillFileStatus::Ok);
        assert!(markdown.content.unwrap().contains("MIT"));
        let code = read(&c, "1.1.0", "scripts/skill_lookup.py").await.unwrap();
        assert_eq!(code.kind, SkillFileKind::Text);
        assert!(code.content.unwrap().contains("import"));
    }
}
