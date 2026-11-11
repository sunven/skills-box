use crate::{
    error::AppError,
    marketplace::{
        Coordinate,
        provider::{client, coordinate},
    },
};
use serde::Serialize;
use serde_json::Value;
const API: &str = "https://api.skillhub.cn/api/v1";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub slug: String,
    pub display_name: String,
    pub summary: String,
    pub member_count: usize,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reference {
    pub slug: String,
    pub namespace: Option<String>,
    pub coordinate: Option<Coordinate>,
    pub error: Option<AppError>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detail {
    #[serde(flatten)]
    pub summary: Summary,
    pub workflow: String,
    pub members: Vec<Reference>,
    pub install_error: Option<AppError>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub packs: Vec<Summary>,
    pub total: u64,
    pub page: u32,
    pub page_size: u32,
}
fn summary(value: &Value) -> Result<Summary, AppError> {
    let slug = value["slug"]
        .as_str()
        .ok_or_else(|| AppError::marketplace_unavailable("专家包缺少 slug"))?;
    super::validate_slug(slug)?;
    Ok(Summary {
        slug: slug.into(),
        display_name: value["displayName"].as_str().unwrap_or(slug).into(),
        summary: value["summary"].as_str().unwrap_or("").into(),
        member_count: value["skillSlugs"].as_array().map_or(0, Vec::len),
    })
}
pub async fn list(keyword: &str, page: u32) -> Result<Listing, AppError> {
    if page == 0 || page > 100_000 || keyword.len() > 400 {
        return Err(AppError::marketplace_unavailable(
            "无效的专家包分页或关键词",
        ));
    }
    let value: Value = client()?
        .get(format!("{API}/skillsets"))
        .query(&[
            ("page", page.to_string()),
            ("pageSize", "24".into()),
            ("keyword", keyword.trim().into()),
        ])
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(AppError::marketplace_unavailable)?
        .json()
        .await
        .map_err(AppError::marketplace_unavailable)?;
    let values = value["skillSets"]
        .as_array()
        .ok_or_else(|| AppError::marketplace_unavailable("专家包列表格式不受支持"))?;
    Ok(Listing {
        packs: values.iter().map(summary).collect::<Result<_, _>>()?,
        total: value["total"]
            .as_u64()
            .ok_or_else(|| AppError::marketplace_unavailable("专家包列表缺少 total"))?,
        page,
        page_size: 24,
    })
}
async fn resolve(reference: &Reference) -> Result<Coordinate, AppError> {
    super::validate_slug(&reference.slug)?;
    if let Some(namespace) = &reference.namespace {
        return Coordinate::from_canonical(&format!("@{namespace}/{}", reference.slug));
    }
    let value: Value = client()?
        .get(format!("{API}/skills/{}", reference.slug))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(AppError::marketplace_unavailable)?
        .json()
        .await
        .map_err(AppError::marketplace_unavailable)?;
    let resolved = coordinate(&value)?;
    if resolved.slug != reference.slug
        || value["ambiguous"] == true
        || value.get("candidates").is_some()
    {
        return Err(AppError::marketplace_unavailable(
            "成员身份不明确，无法固定完整来源",
        ));
    }
    Ok(resolved)
}
pub async fn detail(slug: &str) -> Result<Detail, AppError> {
    use futures::{StreamExt, stream};
    super::validate_slug(slug)?;
    let value: Value = client()?
        .get(format!("{API}/skillsets/{slug}"))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(AppError::marketplace_unavailable)?
        .json()
        .await
        .map_err(AppError::marketplace_unavailable)?;
    let mut detail = parse(&value)?;
    if detail.summary.slug != slug {
        return Err(AppError::marketplace_unavailable("返回了其他专家包"));
    }
    detail.members = stream::iter(detail.members)
        .map(|mut member| async move {
            match resolve(&member).await {
                Ok(c) => member.coordinate = Some(c),
                Err(e) => member.error = Some(e),
            }
            member
        })
        .buffered(4)
        .collect()
        .await;
    Ok(detail)
}
fn parse(value: &Value) -> Result<Detail, AppError> {
    let summary = summary(value)?;
    let mut detail = Detail {
        summary,
        workflow: value["content"].as_str().unwrap_or("").into(),
        members: vec![],
        install_error: None,
    };
    let parsed = (|| {
        let skills = value["skills"]
            .as_array()
            .ok_or_else(|| AppError::marketplace_unavailable("专家包缺少完整成员定义"))?;
        if skills.is_empty() || skills.len() > 100 {
            return Err(AppError::marketplace_unavailable("专家包成员数量不受支持"));
        }
        for item in skills {
            let slug = item["slug"]
                .as_str()
                .ok_or_else(|| AppError::marketplace_unavailable("成员缺少 slug"))?;
            super::validate_slug(slug)?;
            let namespace = match &item["namespace"] {
                Value::Null => None,
                Value::String(s) => Some(s.clone()),
                _ => {
                    return Err(AppError::marketplace_unavailable(
                        "成员 namespace 格式不受支持",
                    ));
                }
            };
            detail.members.push(Reference {
                slug: slug.into(),
                namespace,
                coordinate: None,
                error: None,
            });
        }
        let declared: Vec<_> = value["skillSlugs"]
            .as_array()
            .ok_or_else(|| AppError::marketplace_unavailable("专家包缺少 skillSlugs"))?
            .iter()
            .filter_map(Value::as_str)
            .collect();
        if declared
            != detail
                .members
                .iter()
                .map(|m| m.slug.as_str())
                .collect::<Vec<_>>()
        {
            return Err(AppError::marketplace_unavailable("专家包成员定义不一致"));
        }
        if value["published"] != 1
            || value["scope"] != 3
            || value.get("soul").is_some_and(|v| !v.is_null())
            || value.get("soulContent").is_some_and(|v| !v.is_null())
        {
            return Err(AppError::marketplace_unavailable(
                "仅支持公开普通专家包，不支持 SOUL 组合包",
            ));
        }
        super::workflow::adapt(
            &detail.summary.slug,
            &detail.summary.display_name,
            &detail.summary.summary,
            &detail.workflow,
            &[],
        )?;
        Ok(())
    })();
    detail.install_error = parsed.err();
    Ok(detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn definitions_remain_readable_when_installation_is_unsupported() {
        let mut value = serde_json::json!({"slug":"pack","displayName":"Pack","summary":"summary", "content":"---\nscene: research\n---\nWorkflow", "published":1,"scope":3,"skillSlugs":["member"],"skills":[{"slug":"member","namespace":null}]});
        assert!(parse(&value).unwrap().install_error.is_none());
        value["skillSlugs"] = serde_json::json!(["other"]);
        let detail = parse(&value).unwrap();
        assert!(detail.install_error.is_some());
        assert!(detail.workflow.contains("Workflow"));
        assert_eq!(detail.members.len(), 1);
        value["scope"] = 1.into();
        assert!(parse(&value).unwrap().install_error.is_some());
    }
    #[tokio::test]
    #[ignore = "read-only official API pack contract; requires network"]
    async fn live_pack_definitions_resolve_complete_member_coordinates() {
        let listing = list("", 1).await.unwrap();
        assert!(!listing.packs.is_empty());
        let detail = detail("hr-resume-screening").await.unwrap();
        assert!(detail.install_error.is_none(), "{:?}", detail.install_error);
        assert!(!detail.members.is_empty());
        assert!(
            detail.members.iter().all(|m| m.coordinate.is_some()),
            "{:?}",
            detail.members
        );
    }
}
