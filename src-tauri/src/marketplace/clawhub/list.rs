//! ClawHub catalog list parsing and pagination.

use super::client;
use crate::error::AppError;
use crate::marketplace::{Coordinate, MarketplaceSource};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

const PAGE_LIMIT: u32 = 50;
const MAX_ITEMS: usize = 100;
const MAX_CURSOR: usize = 4096;
pub(super) const MAX_NAME: usize = 400;
pub(super) const MAX_TEXT: usize = 20_000;
const MAX_TOPICS: usize = 50;
const MAX_TOPIC: usize = 100;
const MAX_VERSION: usize = 200;

/// Only the documented catalog sorts are exposed. Upstream accepts aliases
/// (`default`, `stars`, `installs`, ...) but the product contract does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Sort {
    Recommended,
    Updated,
    Downloads,
}

impl Sort {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Recommended => "recommended",
            Self::Updated => "updated",
            Self::Downloads => "downloads",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    pub downloads: Option<u64>,
    pub installs: Option<u64>,
    pub stars: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub coordinate: Coordinate,
    pub name: Option<String>,
    pub description: Option<String>,
    pub metrics: Metrics,
    pub topics: Vec<String>,
    pub official: Option<bool>,
    pub latest_version: Option<String>,
    pub official_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub skills: Vec<Skill>,
    pub next_cursor: Option<String>,
}

pub async fn list(sort: Sort, cursor: Option<String>) -> Result<Listing, AppError> {
    let cursor = validate_cursor(cursor)?;
    let mut query = vec![
        ("limit", PAGE_LIMIT.to_string()),
        ("sort", sort.as_str().to_string()),
    ];
    if let Some(cursor) = &cursor {
        query.push(("cursor", cursor.clone()));
    }
    let key = format!("{}|{}", sort.as_str(), cursor.as_deref().unwrap_or(""));
    parse_listing(client::get(&key, &query).await?)
}

fn validate_cursor(cursor: Option<String>) -> Result<Option<String>, AppError> {
    match cursor {
        None => Ok(None),
        Some(cursor) => {
            let cursor = cursor.trim();
            if cursor.is_empty()
                || cursor.len() > MAX_CURSOR
                || cursor.chars().any(char::is_control)
            {
                return Err(AppError::clawhub_unavailable("无效的目录游标"));
            }
            Ok(Some(cursor.to_string()))
        }
    }
}

fn parse_listing(value: Value) -> Result<Listing, AppError> {
    let object = value
        .as_object()
        .ok_or_else(|| AppError::clawhub_unavailable("目录响应不是对象"))?;
    let rows = object
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::clawhub_unavailable("目录响应缺少 items"))?;
    if rows.len() > MAX_ITEMS {
        return Err(AppError::clawhub_unavailable("目录响应超出单次条目限制"));
    }
    let mut skills = Vec::with_capacity(rows.len());
    let mut seen = BTreeSet::new();
    for row in rows {
        let owner = required_component(row, "ownerHandle")?;
        let slug = required_component(row, "slug")?;
        let coordinate = Coordinate {
            source: MarketplaceSource::Clawhub,
            namespace: owner,
            slug,
        };
        coordinate
            .validate()
            .map_err(|error| AppError::clawhub_unavailable(error.message))?;
        if !seen.insert(coordinate.canonical()) {
            return Err(AppError::clawhub_unavailable("目录响应包含重复条目"));
        }
        let name = optional_text(row, "displayName", MAX_NAME)?;
        let description = optional_text(row, "description", MAX_TEXT)?
            .or(optional_text(row, "summary", MAX_TEXT)?);
        let latest_version = latest_version(row)?;
        skills.push(Skill {
            official_url: coordinate.url(),
            coordinate,
            name,
            description,
            metrics: metrics(row)?,
            topics: topics(row)?,
            official: optional_bool(row, "official")?.or(optional_bool(row, "isOfficial")?),
            latest_version,
        });
    }
    let next_cursor = match object.get("nextCursor") {
        None | Some(Value::Null) => None,
        Some(Value::String(cursor)) => {
            if cursor.len() > MAX_CURSOR || cursor.chars().any(char::is_control) {
                return Err(AppError::clawhub_unavailable("目录响应游标无效"));
            }
            let cursor = cursor.trim();
            (!cursor.is_empty()).then(|| cursor.to_string())
        }
        Some(_) => return Err(AppError::clawhub_unavailable("目录响应游标无效")),
    };
    Ok(Listing {
        skills,
        next_cursor,
    })
}

pub(super) fn required_component(row: &Value, key: &str) -> Result<String, AppError> {
    row.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= MAX_VERSION)
        .map(str::to_string)
        .ok_or_else(|| AppError::clawhub_unavailable(format!("目录条目缺少有效 {key}")))
}

/// Optional display text. Absent/null/blank strings stay missing so the UI
/// never has to invent a value. Embedded newlines are allowed (markdown
/// descriptions); NUL is not.
pub(super) fn optional_text(
    row: &Value,
    key: &str,
    max: usize,
) -> Result<Option<String>, AppError> {
    match row.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value))
            if value.len() <= max && !value.trim().is_empty() && !value.contains('\0') =>
        {
            Ok(Some(value.clone()))
        }
        Some(_) => Err(AppError::clawhub_unavailable(format!(
            "目录条目 {key} 无效或超长"
        ))),
    }
}

pub(super) fn optional_bool(row: &Value, key: &str) -> Result<Option<bool>, AppError> {
    match row.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(AppError::clawhub_unavailable(format!(
            "目录条目 {key} 无效"
        ))),
    }
}

pub(super) fn metrics(row: &Value) -> Result<Metrics, AppError> {
    let stats = match row.get("stats") {
        None | Some(Value::Null) => return Ok(Metrics::default()),
        Some(stats) => stats
            .as_object()
            .ok_or_else(|| AppError::clawhub_unavailable("目录条目 stats 无效"))?,
    };
    let count = |key: &str| match stats.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| AppError::clawhub_unavailable(format!("目录条目 stats.{key} 无效"))),
    };
    Ok(Metrics {
        downloads: count("downloads")?,
        installs: count("installs")?,
        stars: count("stars")?,
    })
}

pub(super) fn topics(row: &Value) -> Result<Vec<String>, AppError> {
    let values = match row.get("topics") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(values)) => values,
        Some(_) => return Err(AppError::clawhub_unavailable("目录条目 topics 无效")),
    };
    if values.len() > MAX_TOPICS {
        return Err(AppError::clawhub_unavailable(
            "目录条目 topics 超出数量限制",
        ));
    }
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|topic| !topic.trim().is_empty() && topic.len() <= MAX_TOPIC)
                .map(str::to_string)
                .ok_or_else(|| AppError::clawhub_unavailable("目录条目 topics 包含无效标签"))
        })
        .collect()
}

/// `latestVersion.version`, falling back to the `tags.latest` pointer. GitHub-
/// backed entries commonly have `latestVersion: null`; neither value is
/// invented when both are absent.
fn latest_version(row: &Value) -> Result<Option<String>, AppError> {
    let version = match row.get("latestVersion") {
        None | Some(Value::Null) => None,
        Some(latest) => {
            let latest = latest
                .as_object()
                .ok_or_else(|| AppError::clawhub_unavailable("目录条目 latestVersion 无效"))?;
            match latest.get("version") {
                None | Some(Value::Null) => None,
                Some(Value::String(version))
                    if version.len() <= MAX_VERSION && !version.trim().is_empty() =>
                {
                    Some(version.clone())
                }
                Some(_) => {
                    return Err(AppError::clawhub_unavailable(
                        "目录条目 latestVersion.version 无效",
                    ));
                }
            }
        }
    };
    let tag = match row.get("tags") {
        None | Some(Value::Null) => None,
        Some(tags) => {
            let tags = tags
                .as_object()
                .ok_or_else(|| AppError::clawhub_unavailable("目录条目 tags 无效"))?;
            match tags.get("latest") {
                None | Some(Value::Null) => None,
                Some(Value::String(version))
                    if version.len() <= MAX_VERSION && !version.trim().is_empty() =>
                {
                    Some(version.clone())
                }
                Some(_) => {
                    return Err(AppError::clawhub_unavailable("目录条目 tags.latest 无效"));
                }
            }
        }
    };
    Ok(version.or(tag))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_live_shaped_items_and_keeps_missing_fields_missing() {
        let listing = parse_listing(json!({
            "items": [
                {
                    "ownerHandle": "steipete",
                    "slug": "weather",
                    "displayName": "Weather",
                    "summary": "Forecasts for any city.",
                    "description": null,
                    "topics": ["weather", "api"],
                    "tags": {"latest": "1.2.0"},
                    "stats": {"comments": 7, "downloads": 1200, "installs": 34, "stars": 5, "versions": 3},
                    "createdAt": 1767632598365_i64,
                    "updatedAt": 1792672926676_i64,
                    "latestVersion": {"version": "1.2.0", "createdAt": 1792672635457_i64, "changelog": "fixes"},
                    "metadata": null
                },
                {
                    "ownerHandle": "aws",
                    "slug": "agents-build",
                    "displayName": null,
                    "summary": null,
                    "description": null,
                    "topics": null,
                    "tags": {},
                    "stats": {},
                    "latestVersion": null
                }
            ],
            "nextCursor": "cursor-1"
        }))
        .unwrap();

        assert_eq!(listing.skills.len(), 2);
        assert_eq!(listing.next_cursor.as_deref(), Some("cursor-1"));
        let full = &listing.skills[0];
        assert_eq!(full.coordinate.source, MarketplaceSource::Clawhub);
        assert_eq!(full.coordinate.canonical(), "@steipete/weather");
        assert_eq!(
            full.official_url,
            "https://clawhub.ai/steipete/skills/weather"
        );
        assert_eq!(full.name.as_deref(), Some("Weather"));
        assert_eq!(full.description.as_deref(), Some("Forecasts for any city."));
        assert_eq!(
            full.metrics,
            Metrics {
                downloads: Some(1200),
                installs: Some(34),
                stars: Some(5),
            }
        );
        assert_eq!(full.topics, ["weather", "api"]);
        assert_eq!(full.official, None);
        assert_eq!(full.latest_version.as_deref(), Some("1.2.0"));

        let sparse = &listing.skills[1];
        assert_eq!(sparse.coordinate.canonical(), "@aws/agents-build");
        assert!(sparse.name.is_none());
        assert!(sparse.description.is_none());
        assert_eq!(sparse.metrics, Metrics::default());
        assert!(sparse.topics.is_empty());
        assert!(sparse.official.is_none());
        assert!(sparse.latest_version.is_none());
    }

    #[test]
    fn prefers_description_accepts_official_flags_and_keeps_real_zeroes() {
        let listing = parse_listing(json!({
            "items": [{
                "ownerHandle": "openclaw",
                "slug": "browser",
                "displayName": "Browser",
                "summary": "short",
                "description": "longer description",
                "official": true,
                "stats": {"downloads": 0, "installs": 0, "stars": 0}
            }],
            "nextCursor": null
        }))
        .unwrap();
        let skill = &listing.skills[0];
        assert_eq!(skill.description.as_deref(), Some("longer description"));
        assert_eq!(skill.official, Some(true));
        assert_eq!(
            skill.metrics,
            Metrics {
                downloads: Some(0),
                installs: Some(0),
                stars: Some(0),
            }
        );
        assert!(listing.next_cursor.is_none());

        let alias = parse_listing(json!({
            "items": [{"ownerHandle": "openclaw", "slug": "browser", "isOfficial": false}],
            "nextCursor": ""
        }))
        .unwrap();
        assert_eq!(alias.skills[0].official, Some(false));
        assert!(alias.next_cursor.is_none());
    }

    #[test]
    fn accepts_an_empty_catalog_page() {
        let listing = parse_listing(json!({"items": [], "nextCursor": null})).unwrap();
        assert!(listing.skills.is_empty());
        assert!(listing.next_cursor.is_none());
    }

    #[test]
    fn rejects_malformed_pages_duplicate_identities_and_oversized_values() {
        for bad in [
            json!({"nextCursor": null}),
            json!({"items": {}, "nextCursor": null}),
            json!({"items": [], "nextCursor": 1}),
            json!({"items": [{"slug": "x"}], "nextCursor": null}),
            json!({"items": [{"ownerHandle": "", "slug": "x"}], "nextCursor": null}),
            json!({"items": [{"ownerHandle": "a b", "slug": "x"}], "nextCursor": null}),
            json!({"items": [{"ownerHandle": "a", "slug": "../x"}], "nextCursor": null}),
            json!({"items": [
                {"ownerHandle": "a", "slug": "x"},
                {"ownerHandle": "a", "slug": "x"}
            ], "nextCursor": null}),
            json!({"items": [{"ownerHandle": "a", "slug": "x", "stats": {"downloads": -1}}], "nextCursor": null}),
            json!({"items": [{"ownerHandle": "a", "slug": "x", "stats": {"downloads": "many"}}], "nextCursor": null}),
            json!({"items": [{"ownerHandle": "a", "slug": "x", "stats": []}], "nextCursor": null}),
            json!({"items": [{"ownerHandle": "a", "slug": "x", "topics": "weather"}], "nextCursor": null}),
            json!({"items": [{"ownerHandle": "a", "slug": "x", "topics": [1]}], "nextCursor": null}),
            json!({"items": [{"ownerHandle": "a", "slug": "x", "displayName": 7}], "nextCursor": null}),
            json!({"items": [{"ownerHandle": "a", "slug": "x", "latestVersion": "1.0.0"}], "nextCursor": null}),
            json!({"items": [{"ownerHandle": "a", "slug": "x", "official": "yes"}], "nextCursor": null}),
            json!({"items": [{"ownerHandle": "a", "slug": "x", "tags": "latest"}], "nextCursor": null}),
        ] {
            assert!(parse_listing(bad.clone()).is_err(), "{bad}");
        }
        let oversized: Vec<Value> = (0..=MAX_ITEMS)
            .map(|index| json!({"ownerHandle": "a", "slug": format!("s{index}")}))
            .collect();
        assert!(parse_listing(json!({"items": oversized, "nextCursor": null})).is_err());
        assert!(
            parse_listing(json!({"items": [], "nextCursor": "x".repeat(MAX_CURSOR + 1)})).is_err()
        );
        assert!(
            parse_listing(json!({"items": [{
                "ownerHandle": "a",
                "slug": "x",
                "displayName": "x".repeat(MAX_NAME + 1)
            }], "nextCursor": null}))
            .is_err()
        );
    }

    #[test]
    fn exposes_only_the_documented_sorts() {
        for sort in [Sort::Recommended, Sort::Updated, Sort::Downloads] {
            assert_eq!(
                serde_json::to_string(&sort).unwrap(),
                format!("\"{}\"", sort.as_str())
            );
        }
        assert!(serde_json::from_str::<Sort>("\"trending\"").is_err());
        assert!(serde_json::from_str::<Sort>("\"stars\"").is_err());
        assert!(serde_json::from_str::<Sort>("\"default\"").is_err());
    }

    #[test]
    fn validates_cursors_before_building_a_query() {
        assert_eq!(validate_cursor(None).unwrap(), None);
        assert_eq!(
            validate_cursor(Some("cursor-1".into())).unwrap().as_deref(),
            Some("cursor-1")
        );
        assert!(validate_cursor(Some("".into())).is_err());
        assert!(validate_cursor(Some("   ".into())).is_err());
        assert!(validate_cursor(Some("bad\ncursor".into())).is_err());
        assert!(validate_cursor(Some("x".repeat(MAX_CURSOR + 1))).is_err());
    }

    #[tokio::test]
    #[ignore = "read-only live ClawHub catalog; requires network"]
    async fn clawhub_live_catalog_lists_each_documented_sort() {
        for sort in [Sort::Recommended, Sort::Updated, Sort::Downloads] {
            let first = list(sort, None).await.unwrap();
            assert!(!first.skills.is_empty(), "{sort:?}");
            let identities: BTreeSet<String> = first
                .skills
                .iter()
                .map(|skill| skill.coordinate.canonical())
                .collect();
            assert_eq!(identities.len(), first.skills.len());
            assert!(
                first
                    .skills
                    .iter()
                    .all(|skill| skill.coordinate.source == MarketplaceSource::Clawhub)
            );
            if let Some(cursor) = first.next_cursor.clone() {
                let second = list(sort, Some(cursor)).await.unwrap();
                assert!(
                    second
                        .skills
                        .iter()
                        .all(|skill| !identities.contains(&skill.coordinate.canonical()))
                );
            }
        }
    }
}
