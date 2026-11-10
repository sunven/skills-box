//! Anonymous official SkillHub catalog. No CLI dependency or ambient source configuration.
use super::{Coordinate, MarketplaceSource, original_name, validate_version};
use crate::error::AppError;
use serde::Serialize;
use serde_json::Value;
use std::{
    io::{Cursor, Read},
    time::Duration,
};

const API: &str = "https://api.skillhub.cn";
const MAX_PACKAGE: usize = 32 * 1024 * 1024;
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub skills: Vec<Summary>,
    pub total: u64,
    pub page: u32,
    pub page_size: u32,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub coordinate: Coordinate,
    pub name: String,
    pub publisher: String,
    pub description: String,
    pub version: String,
    pub official_url: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detail {
    #[serde(flatten)]
    pub summary: Summary,
    pub markdown: String,
    pub original_name: Option<String>,
    pub install_error: Option<AppError>,
    pub requirements: Vec<String>,
}
fn client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .https_only(true)
        .user_agent("SkillsBox/0.1 SkillHub-marketplace")
        .build()
        .map_err(AppError::marketplace_unavailable)
}
fn text<'a>(v: &'a Value, keys: &[&str]) -> &'a str {
    keys.iter()
        .find_map(|key| v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()))
        .unwrap_or("")
}
fn coordinate(v: &Value) -> Result<Coordinate, AppError> {
    let ns = &v["namespace"];
    let value = Coordinate {
        source: MarketplaceSource::Skillhub,
        namespace: text(ns, &["handle"]).into(),
        slug: text(ns, &["publicSlug"]).into(),
    };
    value.validate()?;
    if text(ns, &["canonicalName"]) != value.canonical() {
        return Err(AppError::marketplace_unavailable("平台坐标不一致"));
    }
    Ok(value)
}
pub async fn list(keyword: &str, page: u32) -> Result<Listing, AppError> {
    if page == 0 || page > 100_000 || keyword.len() > 400 {
        return Err(AppError::marketplace_unavailable("无效的分页或关键词"));
    }
    let value: Value = client()?
        .get(format!("{API}/api/skills"))
        .query(&[
            ("page", page.to_string()),
            ("pageSize", "20".into()),
            ("keyword", keyword.trim().into()),
        ])
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(AppError::marketplace_unavailable)?
        .json()
        .await
        .map_err(AppError::marketplace_unavailable)?;
    if value["code"] != 0 {
        return Err(AppError::marketplace_unavailable(text(
            &value,
            &["message"],
        )));
    }
    let data = &value["data"];
    let skills = data["skills"]
        .as_array()
        .ok_or_else(|| AppError::marketplace_unavailable("目录响应缺少 skills"))?
        .iter()
        .map(|v| {
            let c = coordinate(v)?;
            Ok(Summary {
                official_url: c.url(),
                coordinate: c,
                name: text(v, &["name", "slug"]).into(),
                publisher: text(v, &["ownerName"]).into(),
                description: text(v, &["description_zh", "description"]).into(),
                version: text(v, &["version"]).into(),
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    Ok(Listing {
        skills,
        total: data["total"]
            .as_u64()
            .ok_or_else(|| AppError::marketplace_unavailable("目录响应缺少 total"))?,
        page,
        page_size: 20,
    })
}
async fn metadata(c: &Coordinate) -> Result<Value, AppError> {
    c.validate()?;
    let value: Value = client()?
        .get(format!("{API}/api/v1/skills/{}", c.slug))
        .query(&[("namespace", &c.namespace)])
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(AppError::marketplace_unavailable)?
        .json()
        .await
        .map_err(AppError::marketplace_unavailable)?;
    if coordinate(&value)? != *c {
        return Err(AppError::marketplace_unavailable("返回了其他市场坐标"));
    }
    Ok(value)
}
pub async fn latest(c: &Coordinate) -> Result<String, AppError> {
    let v = metadata(c).await?;
    let version = text(&v["latestVersion"], &["version"]);
    validate_version(version)?;
    Ok(version.into())
}
pub async fn detail(c: &Coordinate) -> Result<Detail, AppError> {
    let v = metadata(c).await?;
    let skill = &v["skill"];
    let version = text(&v["latestVersion"], &["version"]);
    validate_version(version)?;
    let markdown = release_markdown(c, version).await?;
    let (name, install_error) = match original_name(&markdown) {
        Ok(name) => (Some(name), None),
        Err(e) => (None, Some(e)),
    };
    let requirements = if skill["labels"]["requires_api_key"] == "true" {
        vec!["运行此技能需要 API Key；请按技能说明配置。".into()]
    } else {
        vec![]
    };
    Ok(Detail {
        summary: Summary {
            coordinate: c.clone(),
            official_url: c.url(),
            name: text(skill, &["displayName", "slug"]).into(),
            description: text(skill, &["summary_zh", "summary"]).into(),
            publisher: {
                let publisher = text(&v["publisher"], &["name"]);
                if publisher.is_empty() {
                    text(&v["owner"], &["displayName", "handle"]).into()
                } else {
                    publisher.into()
                }
            },
            version: version.into(),
        },
        markdown,
        original_name: name,
        install_error,
        requirements,
    })
}
pub async fn release_markdown(c: &Coordinate, version: &str) -> Result<String, AppError> {
    c.validate()?;
    validate_version(version)?;
    let mut response = client()?
        .get(format!("{API}/api/v1/download"))
        .query(&[
            ("slug", c.slug.as_str()),
            ("namespace", c.namespace.as_str()),
            ("version", version),
        ])
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(AppError::marketplace_unavailable)?;
    if response
        .content_length()
        .is_some_and(|n| n > MAX_PACKAGE as u64)
    {
        return Err(AppError::marketplace_unavailable(
            "技能包超过 32 MB 预览限制",
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(AppError::marketplace_unavailable)?
    {
        if bytes.len() + chunk.len() > MAX_PACKAGE {
            return Err(AppError::marketplace_unavailable(
                "技能包超过 32 MB 预览限制",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    tokio::task::spawn_blocking(move || markdown_from_zip(&bytes))
        .await
        .map_err(AppError::marketplace_unavailable)?
}
pub(crate) fn markdown_from_zip(bytes: &[u8]) -> Result<String, AppError> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(AppError::marketplace_unavailable)?;
    let file = archive
        .by_name("SKILL.md")
        .map_err(|_| AppError::marketplace_unavailable("技能包根目录缺少 SKILL.md"))?;
    if file.is_symlink() || file.size() > 2 * 1024 * 1024 {
        return Err(AppError::marketplace_unavailable(
            "SKILL.md 类型或大小不受支持",
        ));
    }
    let mut text = String::new();
    file.take(2 * 1024 * 1024 + 1)
        .read_to_string(&mut text)
        .map_err(AppError::marketplace_unavailable)?;
    if text.len() > 2 * 1024 * 1024 {
        return Err(AppError::marketplace_unavailable("SKILL.md 超过大小限制"));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[ignore = "read-only official API smoke test; requires network"]
    async fn skillhub_live_catalog_and_exact_release_preview() {
        let catalog = list("", 1).await.unwrap();
        assert!(!catalog.skills.is_empty());
        let c = Coordinate::from_canonical("@tencent-adm/ima-skills").unwrap();
        let detail = detail(&c).await.unwrap();
        assert_eq!(detail.summary.coordinate, c);
        assert!(detail.original_name.is_some(), "{:?}", detail.install_error);
        assert!(!detail.markdown.is_empty());
        assert!(
            release_markdown(&c, "nonexistent-skills-box-test-release")
                .await
                .is_err()
        );
    }
    #[test]
    fn marketplace_identity_cannot_be_guessed_from_display_name() {
        assert!(coordinate(&serde_json::json!({"slug":"skill","name":"name"})).is_err());
        assert!(coordinate(&serde_json::json!({"namespace":{"handle":"a","publicSlug":"b","canonicalName":"@other/b"}})).is_err());
        for invalid in [
            "slug",
            "@a/../escape",
            "@a/--force",
            "@a/b@latest",
            "@a/b?host=evil",
        ] {
            assert!(Coordinate::from_canonical(invalid).is_err());
        }
        assert!(original_name("---\ndescription: no name\n---").is_err());
        assert!(original_name("---\nname: .hidden\n---").is_err());
        assert_eq!(
            original_name("---\nname: original-name\n---").unwrap(),
            "original-name"
        );
    }
}
