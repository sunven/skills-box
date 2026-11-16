//! ClawHub keyword search.
//!
//! `/api/v1/search` is the only catalog endpoint that exposes `install.kind`
//! (`clawhub`, `github`, `skills-sh`); skills.sh alias entries are excluded
//! from the ClawHub source. Upstream order is preserved and there is no cursor
//! pagination, so the response reports how many rows were returned and the
//! requested upstream limit.

use super::{client, list};
use crate::error::AppError;
use crate::marketplace::{Coordinate, MarketplaceSource};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;

const SEARCH_PATH: &str = "/api/v1/search";
/// Single request cap. Search has no cursor, so the UI must state this limit.
pub const SEARCH_LIMIT: u32 = 50;
const MAX_RESULTS: usize = 100;
const MAX_QUERY_CHARS: usize = 200;
const MAX_KIND: usize = 40;
const MAX_VERSION: usize = 200;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Search {
    pub skills: Vec<list::Skill>,
    /// Rows returned to the UI after alias and non-skill filtering.
    pub returned: usize,
    /// Rows the upstream returned before filtering.
    pub upstream_returned: usize,
    /// The `limit` requested from the upstream endpoint.
    pub limit: u32,
    /// skills.sh aliases excluded from this source.
    pub filtered_aliases: usize,
    /// Entries whose install kind is neither native ClawHub nor a skills.sh
    /// alias; excluded so the source stays skills-only.
    pub filtered_others: usize,
}

pub async fn search(query: &str) -> Result<Search, AppError> {
    let query = validate_query(query)?;
    let key = format!("search|{query}");
    let value = client::get_at(
        SEARCH_PATH,
        &key,
        &[("q", query), ("limit", SEARCH_LIMIT.to_string())],
    )
    .await?;
    parse_search(value)
}

/// Search terms are trimmed and bounded. Control characters would end up in
/// the upstream query string and the session cache key, so they are rejected.
fn validate_query(query: &str) -> Result<String, AppError> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Err(AppError::clawhub_invalid_query("搜索关键词不能为空"));
    }
    if trimmed.chars().count() > MAX_QUERY_CHARS {
        return Err(AppError::clawhub_invalid_query(format!(
            "搜索关键词超过 {MAX_QUERY_CHARS} 个字符"
        )));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(AppError::clawhub_invalid_query("搜索关键词包含无效字符"));
    }
    Ok(trimmed.to_string())
}

fn parse_search(value: Value) -> Result<Search, AppError> {
    let object = value
        .as_object()
        .ok_or_else(|| AppError::clawhub_unavailable("搜索响应不是对象"))?;
    let rows = object
        .get("results")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::clawhub_unavailable("搜索响应缺少 results"))?;
    if rows.len() > MAX_RESULTS {
        return Err(AppError::clawhub_unavailable("搜索响应超出单次条目限制"));
    }
    let mut skills = Vec::with_capacity(rows.len());
    let mut seen = BTreeSet::new();
    let mut filtered_aliases = 0;
    let mut filtered_others = 0;
    for row in rows {
        let kind = row
            .get("install")
            .and_then(Value::as_object)
            .and_then(|install| install.get("kind"))
            .and_then(Value::as_str)
            .filter(|kind| !kind.is_empty() && kind.len() <= MAX_KIND)
            .ok_or_else(|| AppError::clawhub_unavailable("搜索结果缺少有效 install.kind"))?;
        match kind {
            // ClawHub indexes skills.sh entries as installable aliases. They
            // belong to the skills.sh source, not this one.
            "skills-sh" => {
                filtered_aliases += 1;
                continue;
            }
            // `clawhub` is native; `github` is the declared kind for
            // GitHub-backed registry entries. Both must carry a native skill.
            "clawhub" | "github" => {}
            // Neither a native skill nor a skills.sh alias. The response has
            // identified a non-skill/unsupported entity, so it is excluded.
            _ => {
                filtered_others += 1;
                continue;
            }
        }
        let native_value = row
            .get("native")
            .and_then(Value::as_object)
            .and_then(|native| native.get("skill"))
            .filter(|skill| skill.is_object())
            .ok_or_else(|| AppError::clawhub_unavailable("搜索结果缺少 native.skill"))?;
        let native = native_value.as_object().expect("checked object");
        let owner = list::required_component(row, "ownerHandle")?;
        let slug = list::required_component(row, "slug")?;
        if let Some(native_slug) = native.get("slug").and_then(Value::as_str) {
            if native_slug != slug {
                return Err(AppError::clawhub_unavailable(
                    "搜索结果 native.skill 与条目身份不一致",
                ));
            }
        }
        let coordinate = Coordinate {
            source: MarketplaceSource::Clawhub,
            namespace: owner,
            slug,
        };
        coordinate
            .validate()
            .map_err(|error| AppError::clawhub_unavailable(error.message))?;
        if !seen.insert(coordinate.canonical()) {
            return Err(AppError::clawhub_unavailable("搜索响应包含重复条目"));
        }
        let name = list::optional_text(row, "displayName", list::MAX_NAME)?.or(
            list::optional_text(native_value, "displayName", list::MAX_NAME)?,
        );
        let description = list::optional_text(row, "summary", list::MAX_TEXT)?.or(
            list::optional_text(native_value, "summary", list::MAX_TEXT)?,
        );
        let mut metrics = list::metrics(native_value)?;
        if metrics.downloads.is_none() {
            metrics.downloads = optional_count(row, "downloads")?;
        }
        skills.push(list::Skill {
            official_url: coordinate.url(),
            coordinate,
            name,
            description,
            metrics,
            topics: list::topics(native_value)?,
            official: list::optional_bool(row, "official")?,
            latest_version: optional_version(row)?,
        });
    }
    Ok(Search {
        returned: skills.len(),
        skills,
        upstream_returned: rows.len(),
        limit: SEARCH_LIMIT,
        filtered_aliases,
        filtered_others,
    })
}

/// Top-level `downloads` is the only aggregate search exposes outside
/// `native.skill.stats`; absent/null stays missing rather than becoming zero.
fn optional_count(row: &Value, key: &str) -> Result<Option<u64>, AppError> {
    match row.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| AppError::clawhub_unavailable(format!("搜索结果 {key} 无效"))),
    }
}

/// Top-level `version` is currently null for native results; it is shown only
/// when upstream provides a real string and never derived from internal tags.
fn optional_version(row: &Value) -> Result<Option<String>, AppError> {
    match row.get("version") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(version))
            if version.len() <= MAX_VERSION && !version.trim().is_empty() =>
        {
            Ok(Some(version.clone()))
        }
        Some(_) => Err(AppError::clawhub_unavailable("搜索结果 version 无效")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn native(owner: &str, slug: &str, display: &str) -> Value {
        json!({
            "ownerHandle": owner,
            "slug": slug,
            "displayName": display,
            "summary": format!("{display} summary"),
            "official": false,
            "downloads": 7,
            "version": null,
            "install": {"kind": "clawhub", "reference": format!("{owner}/{slug}")},
            "source": "clawhub",
            "native": {
                "ownerHandle": owner,
                "skill": {
                    "slug": slug,
                    "displayName": display,
                    "summary": format!("{display} summary"),
                    "topics": ["tools"],
                    "stats": {"downloads": 7, "installs": 2, "stars": 1}
                },
                "version": null
            }
        })
    }

    fn alias(owner: &str, slug: &str) -> Value {
        json!({
            "ownerHandle": owner,
            "slug": slug,
            "install": {
                "kind": "skills-sh",
                "reference": format!("skills-sh:{owner}/repo/{slug}"),
                "sourceUrl": format!("https://www.skills.sh/{owner}/repo/{slug}")
            },
            "source": "skills-sh",
            "native": null
        })
    }

    #[test]
    fn preserves_upstream_order_and_filters_skills_sh_aliases() {
        let search = parse_search(json!({
            "results": [
                native("steipete", "weather", "Weather"),
                alias("anthropics", "debug"),
                native("aws", "agents-build", "Agents Build")
            ]
        }))
        .unwrap();

        assert_eq!(
            search
                .skills
                .iter()
                .map(|skill| skill.coordinate.canonical())
                .collect::<Vec<_>>(),
            ["@steipete/weather", "@aws/agents-build"]
        );
        assert_eq!(search.returned, 2);
        assert_eq!(search.upstream_returned, 3);
        assert_eq!(search.limit, SEARCH_LIMIT);
        assert_eq!(search.filtered_aliases, 1);
        assert_eq!(search.filtered_others, 0);

        let first = &search.skills[0];
        assert_eq!(first.coordinate.source, MarketplaceSource::Clawhub);
        assert_eq!(first.name.as_deref(), Some("Weather"));
        assert_eq!(first.description.as_deref(), Some("Weather summary"));
        assert_eq!(first.official, Some(false));
        assert_eq!(first.latest_version, None);
        assert_eq!(first.topics, ["tools"]);
        assert_eq!(
            first.metrics,
            list::Metrics {
                downloads: Some(7),
                installs: Some(2),
                stars: Some(1),
            }
        );
        assert_eq!(
            first.official_url,
            "https://clawhub.ai/steipete/skills/weather"
        );
    }

    #[test]
    fn filters_other_install_kinds_and_alias_rows_without_identities() {
        let search = parse_search(json!({
            "results": [
                {"install": {"kind": "skills-sh"}},
                {"install": {"kind": "plugin"}, "ownerHandle": "a", "slug": "b"},
                native("a", "native", "Native")
            ]
        }))
        .unwrap();
        assert_eq!(search.returned, 1);
        assert_eq!(search.upstream_returned, 3);
        assert_eq!(search.filtered_aliases, 1);
        assert_eq!(search.filtered_others, 1);
        assert_eq!(search.skills[0].coordinate.canonical(), "@a/native");
    }

    #[test]
    fn keeps_missing_fields_missing_and_accepts_zero_metrics() {
        let search = parse_search(json!({
            "results": [{
                "ownerHandle": "openclaw",
                "slug": "browser",
                "displayName": null,
                "summary": null,
                "official": null,
                "version": "2.1.0",
                "install": {"kind": "clawhub"},
                "native": {"skill": {"slug": "browser", "stats": {
                    "downloads": 0, "installs": 0, "stars": 0
                }}}
            }]
        }))
        .unwrap();
        let skill = &search.skills[0];
        assert!(skill.name.is_none());
        assert!(skill.description.is_none());
        assert!(skill.official.is_none());
        assert_eq!(skill.latest_version.as_deref(), Some("2.1.0"));
        assert_eq!(
            skill.metrics,
            list::Metrics {
                downloads: Some(0),
                installs: Some(0),
                stars: Some(0),
            }
        );
        assert!(skill.topics.is_empty());
    }

    #[test]
    fn rejects_malformed_search_payloads_and_duplicate_identities() {
        for bad in [
            json!(null),
            json!({}),
            json!({"results": {}}),
            json!({"results": [{"slug": "x"}]}),
            json!({"results": [{"install": {"kind": ""}, "ownerHandle": "a", "slug": "b"}]}),
            json!({"results": [{"install": {"kind": 7}, "ownerHandle": "a", "slug": "b"}]}),
            json!({"results": [{"install": {"kind": "clawhub"}, "ownerHandle": "a", "slug": "b", "native": null}]}),
            json!({"results": [{"install": {"kind": "clawhub"}, "ownerHandle": "a", "slug": "b", "native": {"skill": {"slug": "other"}}}]}),
            json!({"results": [{"install": {"kind": "clawhub"}, "slug": "b", "native": {"skill": {}}}]}),
            json!({"results": [
                {"install": {"kind": "clawhub"}, "ownerHandle": "a", "slug": "b", "native": {"skill": {}}},
                {"install": {"kind": "clawhub"}, "ownerHandle": "a", "slug": "b", "native": {"skill": {}}}
            ]}),
            json!({"results": [{"install": {"kind": "clawhub"}, "ownerHandle": "a", "slug": "b", "displayName": 7, "native": {"skill": {}}}]}),
            json!({"results": [{"install": {"kind": "clawhub"}, "ownerHandle": "a", "slug": "b", "native": {"skill": {"stats": {"downloads": "many"}}}}]}),
            json!({"results": [{"install": {"kind": "clawhub"}, "ownerHandle": "a", "slug": "b", "native": {"skill": {"topics": "weather"}}}]}),
            json!({"results": [{"install": {"kind": "clawhub"}, "ownerHandle": "a", "slug": "b", "official": "yes", "native": {"skill": {}}}]}),
            json!({"results": [{"install": {"kind": "clawhub"}, "ownerHandle": "a", "slug": "b", "version": 3, "native": {"skill": {}}}]}),
        ] {
            assert!(parse_search(bad.clone()).is_err(), "{bad}");
        }
        let oversized: Vec<Value> = (0..=MAX_RESULTS)
            .map(|index| alias("owner", &format!("s{index}")))
            .collect();
        assert!(parse_search(json!({"results": oversized})).is_err());
    }

    #[test]
    fn validates_queries_before_building_a_request() {
        assert_eq!(validate_query("  weather  ").unwrap(), "weather");
        assert_eq!(validate_query("local weather").unwrap(), "local weather");
        assert_eq!(
            validate_query(&"技".repeat(MAX_QUERY_CHARS))
                .unwrap()
                .chars()
                .count(),
            MAX_QUERY_CHARS
        );
        for bad in [
            String::new(),
            "   ".into(),
            "\n".into(),
            format!("bad\u{7}query"),
            "技".repeat(MAX_QUERY_CHARS + 1),
        ] {
            let error = validate_query(&bad).unwrap_err();
            assert_eq!(error.code.as_str(), "marketplace.unavailable", "{bad:?}");
            assert!(!error.retryable, "{bad:?}");
        }
    }

    #[tokio::test]
    #[ignore = "read-only live ClawHub search; requires network"]
    async fn clawhub_live_search_maps_fields_and_filters_aliases() {
        let native = search("weather").await.unwrap();
        assert!(native.returned > 0);
        assert_eq!(native.limit, SEARCH_LIMIT);
        assert!(native.upstream_returned >= native.returned);
        assert!(native.skills.iter().all(|skill| skill.coordinate.source
            == MarketplaceSource::Clawhub
            && skill.official_url.starts_with("https://clawhub.ai/")));

        let mixed = search("knowledge-work").await.unwrap();
        assert!(mixed.filtered_aliases > 0);
        assert!(mixed.returned < mixed.upstream_returned);
    }
}
