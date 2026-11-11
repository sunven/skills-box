//! Anonymous official SkillHub catalog. No CLI dependency or ambient source configuration.
use super::{Coordinate, MarketplaceSource, original_name, validate_version};
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    io::{Cursor, Read},
    time::Duration,
};

const API: &str = "https://api.skillhub.cn";
const MAX_PACKAGE: usize = 32 * 1024 * 1024;
const PAGE_SIZE: u32 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    Score,
    Trending,
    Downloads,
    UpdatedAt,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApiKeyFilter {
    All,
    Required,
    None,
}

impl ApiKeyFilter {
    fn matches(self, labels: &Value) -> bool {
        let required = labels["requires_api_key"] == "true";
        match self {
            Self::All => true,
            Self::Required => required,
            Self::None => !required,
        }
    }

    fn labels(self) -> Option<&'static str> {
        match self {
            Self::All => None,
            Self::Required => Some("requires_api_key:true"),
            Self::None => Some("requires_api_key:false"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Category {
    pub key: String,
    pub name: String,
}

pub async fn categories() -> Result<Vec<Category>, AppError> {
    let value: Value = client()?
        .get(format!("{API}/api/v1/categories"))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(AppError::marketplace_unavailable)?
        .json()
        .await
        .map_err(AppError::marketplace_unavailable)?;
    parse_categories(&value)
}

fn parse_categories(value: &Value) -> Result<Vec<Category>, AppError> {
    let mut items: Vec<_> = value["items"]
        .as_array()
        .ok_or_else(|| AppError::marketplace_unavailable("分类响应缺少 items"))?
        .iter()
        .filter(|item| item["active"] == true)
        .collect();
    items.sort_by_key(|item| item["sortOrder"].as_i64().unwrap_or(i64::MAX));
    items
        .into_iter()
        .map(|item| {
            let key = text(item, &["key"]);
            let name = text(item, &["name"]);
            if key.is_empty() || name.is_empty() {
                return Err(AppError::marketplace_unavailable("分类缺少标识或名称"));
            }
            Ok(Category {
                key: key.into(),
                name: name.into(),
            })
        })
        .collect()
}

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
    pub stars: Option<u64>,
    pub downloads: Option<u64>,
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
pub(crate) fn client() -> Result<reqwest::Client, AppError> {
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
pub(crate) fn coordinate(v: &Value) -> Result<Coordinate, AppError> {
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
fn summary(v: &Value) -> Result<Summary, AppError> {
    let c = coordinate(v)?;
    Ok(Summary {
        official_url: c.url(),
        coordinate: c,
        name: text(v, &["name", "slug"]).into(),
        publisher: text(v, &["ownerName"]).into(),
        description: text(v, &["description_zh", "description"]).into(),
        version: text(v, &["version"]).into(),
        stars: v["stars"].as_u64(),
        downloads: v["downloads"].as_u64(),
    })
}

// SkillHub's trending tab searches and paginates its showcase, preserving rank order.
fn trending_listing(
    value: &Value,
    keyword: &str,
    page: u32,
    category: &str,
    api_key: ApiKeyFilter,
) -> Result<Listing, AppError> {
    let keyword = keyword.trim().to_lowercase();
    let matches: Vec<_> = value["skills"]
        .as_array()
        .ok_or_else(|| AppError::marketplace_unavailable("榜单响应缺少 skills"))?
        .iter()
        .filter(|skill| {
            if !category.is_empty() && skill["category"].as_str() != Some(category) {
                return false;
            }
            if !api_key.matches(&skill["labels"]) {
                return false;
            }
            keyword.is_empty()
                || ["name", "slug", "description", "description_zh", "ownerName"]
                    .iter()
                    .filter_map(|key| skill[*key].as_str())
                    .chain(skill["namespace"]["handle"].as_str())
                    .chain(
                        skill["tags"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str),
                    )
                    .any(|field| field.to_lowercase().contains(&keyword))
        })
        .collect();
    Ok(Listing {
        total: matches.len() as u64,
        skills: matches
            .into_iter()
            .skip(((page - 1) * PAGE_SIZE) as usize)
            .take(PAGE_SIZE as usize)
            .map(summary)
            .collect::<Result<_, _>>()?,
        page,
        page_size: PAGE_SIZE,
    })
}

pub async fn list(
    keyword: &str,
    page: u32,
    sort: Sort,
    category: &str,
    api_key: ApiKeyFilter,
) -> Result<Listing, AppError> {
    if page == 0 || page > 100_000 || keyword.len() > 400 || category.len() > 200 {
        return Err(AppError::marketplace_unavailable("无效的分页或关键词"));
    }
    let client = client()?;
    let request = if sort == Sort::Trending {
        client.get(format!("{API}/api/v1/showcase/trending"))
    } else {
        let sort_by = match sort {
            Sort::Downloads => "downloads",
            Sort::UpdatedAt => "updated_at",
            _ => "score",
        };
        let mut request = client.get(format!("{API}/api/skills")).query(&[
            ("page", page.to_string()),
            ("pageSize", PAGE_SIZE.to_string()),
            ("keyword", keyword.trim().into()),
            ("sortBy", sort_by.into()),
            ("order", "desc".into()),
            ("category", category.into()),
        ]);
        if let Some(labels) = api_key.labels() {
            request = request.query(&[("labels", labels)]);
        }
        request
    };
    let value: Value = request
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(AppError::marketplace_unavailable)?
        .json()
        .await
        .map_err(AppError::marketplace_unavailable)?;
    if sort == Sort::Trending {
        return trending_listing(&value, keyword, page, category, api_key);
    }
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
        .map(summary)
        .collect::<Result<Vec<_>, AppError>>()?;
    Ok(Listing {
        skills,
        total: data["total"]
            .as_u64()
            .ok_or_else(|| AppError::marketplace_unavailable("目录响应缺少 total"))?,
        page,
        page_size: PAGE_SIZE,
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
            stars: skill["stats"]["stars"].as_u64(),
            downloads: skill["stats"]["downloads"].as_u64(),
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
    #[test]
    fn api_key_filter_uses_platform_labels_before_pagination() {
        let skills: Vec<_> = (0..100).map(|i| serde_json::json!({
            "name": format!("Skill {i}"),
            "description": "Code",
            "category": if i % 2 == 0 { "dev-programming" } else { "office-efficiency" },
            "labels": match i % 4 {
                0 | 1 => serde_json::json!({"requires_api_key": "true"}),
                2 => serde_json::json!({"requires_api_key": "false"}),
                _ => serde_json::json!({})
            },
            "namespace": {"handle": "publisher", "publicSlug": format!("skill-{i}"), "canonicalName": format!("@publisher/skill-{i}")}
        })).collect();
        let value = serde_json::json!({"skills": skills});
        let required =
            trending_listing(&value, "code", 2, "dev-programming", ApiKeyFilter::Required).unwrap();
        assert_eq!(required.total, 25);
        assert_eq!(required.skills.len(), 1);
        assert_eq!(required.skills[0].name, "Skill 96");
        let none = trending_listing(&value, "", 2, "", ApiKeyFilter::None).unwrap();
        assert_eq!(none.total, 50);
        assert_eq!(none.skills[0].name, "Skill 50");
        assert_eq!(none.skills[1].name, "Skill 51");
        assert_eq!(
            trending_listing(&value, "", 1, "", ApiKeyFilter::All)
                .unwrap()
                .total,
            100
        );
        assert_eq!(
            trending_listing(&value, "no match", 1, "", ApiKeyFilter::Required)
                .unwrap()
                .total,
            0
        );
    }

    #[tokio::test]
    #[ignore = "read-only official API Key filter smoke test; requires network"]
    async fn skillhub_live_api_key_filtered_catalogs() {
        for sort in [Sort::Score, Sort::Trending] {
            for filter in [ApiKeyFilter::Required, ApiKeyFilter::None] {
                let filtered = list("", 1, sort, "", filter).await.unwrap();
                assert!(!filtered.skills.is_empty());
                assert!(
                    filtered.total
                        < list("", 1, sort, "", ApiKeyFilter::All)
                            .await
                            .unwrap()
                            .total
                );
                let metadata = metadata(&filtered.skills[0].coordinate).await.unwrap();
                assert!(filter.matches(&metadata["skill"]["labels"]));
                let combined = list("", 1, sort, "dev-programming", filter).await.unwrap();
                assert!(combined.total <= filtered.total);
                if let Some(skill) = combined.skills.first() {
                    let observed = super::metadata(&skill.coordinate).await.unwrap();
                    assert!(filter.matches(&observed["skill"]["labels"]));
                    assert_eq!(observed["skill"]["category"], "dev-programming");
                } else {
                    assert_eq!(combined.total, 0);
                }
            }
        }
    }

    #[test]
    fn categories_follow_platform_order_and_exclude_inactive_entries() {
        let value = serde_json::json!({"items": [
            {"key": "dev-programming", "name": "开发编程", "active": true, "sortOrder": 30},
            {"key": "old", "name": "已停用", "active": false, "sortOrder": 0},
            {"key": "office-efficiency", "name": "办公效率", "active": true, "sortOrder": 10}
        ]});
        let result = parse_categories(&value).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].key, "office-efficiency");
        assert_eq!(result[1].name, "开发编程");
        assert!(parse_categories(&serde_json::json!({})).is_err());
        assert!(parse_categories(&serde_json::json!({"items": [{"active": true}]})).is_err());
    }

    #[tokio::test]
    #[ignore = "read-only official category API smoke test; requires network"]
    async fn skillhub_live_categories_and_filtered_catalogs() {
        let categories = categories().await.unwrap();
        assert!(categories.iter().any(|c| c.key == "dev-programming"));
        for sort in [
            Sort::Score,
            Sort::Downloads,
            Sort::UpdatedAt,
            Sort::Trending,
        ] {
            let filtered = list("", 1, sort, "dev-programming", ApiKeyFilter::All)
                .await
                .unwrap();
            assert!(!filtered.skills.is_empty());
            assert!(filtered.skills.len() <= PAGE_SIZE as usize);
            if sort != Sort::Trending {
                assert!(
                    filtered.total
                        < list("", 1, sort, "", ApiKeyFilter::All)
                            .await
                            .unwrap()
                            .total
                );
            }
        }
    }

    #[tokio::test]
    #[ignore = "read-only official API sorting smoke test; requires network"]
    async fn skillhub_live_sorted_catalogs() {
        for sort in [
            Sort::Score,
            Sort::Trending,
            Sort::Downloads,
            Sort::UpdatedAt,
        ] {
            let first = list("", 1, sort, "", ApiKeyFilter::All).await.unwrap();
            assert_eq!(first.skills.len(), PAGE_SIZE as usize);
            assert!(first.total >= PAGE_SIZE as u64);
            assert!(first.skills[0].stars.is_some());
            assert!(first.skills[0].downloads.is_some());
            let second = list("", 2, sort, "", ApiKeyFilter::All).await.unwrap();
            assert_eq!(second.page, 2);
            assert_ne!(first.skills[0].coordinate, second.skills[0].coordinate);
            let searched = list(&first.skills[0].name, 1, sort, "", ApiKeyFilter::All)
                .await
                .unwrap();
            assert!(
                searched
                    .skills
                    .iter()
                    .any(|skill| skill.coordinate == first.skills[0].coordinate)
            );
        }
    }

    #[test]
    fn trending_search_filters_before_pagination_and_preserves_rank() {
        let skills: Vec<_> = (0..65)
            .map(|i| {
                serde_json::json!({
                    "name": format!("Skill {i}"),
                    "description_zh": if i % 2 == 0 { "清理代码" } else { "other" },
                    "category": if i % 2 == 0 { "dev-programming" } else { "office-efficiency" },
                    "tags": ["Refactor"],
                    "stars": if i == 0 { Some(0) } else { None },
                    "downloads": if i == 0 { Some(11_384_000) } else { None },
                    "namespace": {
                        "handle": "publisher",
                        "publicSlug": format!("skill-{i}"),
                        "canonicalName": format!("@publisher/skill-{i}")
                    }
                })
            })
            .collect();
        let value = serde_json::json!({"skills": skills, "total": 100});
        let first = trending_listing(&value, " 清理 ", 1, "", ApiKeyFilter::All).unwrap();
        assert_eq!(first.total, 33);
        assert_eq!(first.skills.len(), 24);
        assert_eq!(first.skills[0].stars, Some(0));
        assert_eq!(first.skills[0].downloads, Some(11_384_000));
        assert_eq!(first.skills[1].stars, None);
        assert_eq!(first.skills[1].downloads, None);
        assert_eq!(first.skills[23].name, "Skill 46");
        let second = trending_listing(&value, "清理", 2, "", ApiKeyFilter::All).unwrap();
        assert_eq!(second.skills.len(), 9);
        assert_eq!(second.skills[0].name, "Skill 48");
        assert_eq!(second.page, 2);
        assert_eq!(second.page_size, 24);
        assert_eq!(
            trending_listing(&value, " REFACTOR ", 1, "", ApiKeyFilter::All)
                .unwrap()
                .total,
            65
        );
        assert_eq!(
            trending_listing(&value, "publisher", 1, "", ApiKeyFilter::All)
                .unwrap()
                .total,
            65
        );
        assert_eq!(
            trending_listing(&value, "missing", 1, "", ApiKeyFilter::All)
                .unwrap()
                .total,
            0
        );
        assert!(
            trending_listing(&value, "", 4, "", ApiKeyFilter::All)
                .unwrap()
                .skills
                .is_empty()
        );
        assert!(trending_listing(&serde_json::json!({}), "", 1, "", ApiKeyFilter::All).is_err());
        let filtered =
            trending_listing(&value, "", 2, "dev-programming", ApiKeyFilter::All).unwrap();
        assert_eq!(filtered.total, 33);
        assert_eq!(filtered.skills.len(), 9);
        assert_eq!(filtered.skills[0].name, "Skill 48");
        assert_eq!(
            trending_listing(&value, "other", 1, "dev-programming", ApiKeyFilter::All)
                .unwrap()
                .total,
            0
        );
        assert_eq!(
            trending_listing(&value, "", 1, "office-efficiency", ApiKeyFilter::All)
                .unwrap()
                .total,
            32
        );
    }

    #[tokio::test]
    #[ignore = "read-only official API smoke test; requires network"]
    async fn skillhub_live_catalog_and_exact_release_preview() {
        let catalog = list("", 1, Sort::Score, "", ApiKeyFilter::All)
            .await
            .unwrap();
        assert!(!catalog.skills.is_empty());
        let c = Coordinate::from_canonical("@tencent-adm/ima-skills").unwrap();
        let detail = detail(&c).await.unwrap();
        assert_eq!(detail.summary.coordinate, c);
        assert!(detail.summary.stars.is_some());
        assert!(detail.summary.downloads.is_some());
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
