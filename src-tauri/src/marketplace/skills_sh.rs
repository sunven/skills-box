//! Anonymous skills.sh catalog and browsing snapshots. Installation remains Git-owned.
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeSet, time::Duration};

const API: &str = "https://www.skills.sh";
const MAX_RESPONSE: usize = 16 * 1024 * 1024;
const MAX_TEXT: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum View {
    AllTime,
    Trending,
    Hot,
}
impl View {
    fn path(self) -> &'static str {
        match self {
            Self::AllTime => "all-time",
            Self::Trending => "trending",
            Self::Hot => "hot",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Coordinate {
    pub source: String,
    pub skill_id: String,
}
impl Coordinate {
    fn validate(&self) -> Result<(), AppError> {
        if self.source.len() > 400
            || self.skill_id.len() > 200
            || !component(&self.skill_id)
            || !self.source.split('/').all(component)
        {
            return Err(AppError::skills_sh_unavailable("无效的 skills.sh 条目标识"));
        }
        Ok(())
    }
    fn git_source(&self) -> Option<String> {
        let (owner, _) = self.source.split_once('/')?;
        // A domain (including a pathful well-known provider) is never a GitHub owner.
        if !owner
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        {
            return None;
        }
        let source = crate::tasks::source::validate_source(&self.source).ok()?;
        (source.kind == crate::tasks::source::SourceKind::GitHub).then_some(source.normalized)
    }
    fn id(&self) -> String {
        format!("{}/{}", self.source, self.skill_id)
    }
    fn url(&self) -> String {
        let prefix = if self.git_source().is_some() {
            ""
        } else {
            "site/"
        };
        format!("{API}/{prefix}{}", self.id())
    }
}
fn component(value: &str) -> bool {
    !value.is_empty()
        && !matches!(value, "." | "..")
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub coordinate: Coordinate,
    pub name: String,
    pub installs: Option<u64>,
    pub git_source: Option<String>,
    pub official_url: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub skills: Vec<Skill>,
    pub total: u64,
    pub page: u32,
    pub has_more: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct File {
    pub path: String,
    pub contents: Option<String>,
    pub size_bytes: usize,
}
#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub files: Option<Vec<File>>,
    pub hash: Option<String>,
}

async fn read(path: &str, query: &[(&str, String)]) -> Result<Option<Value>, AppError> {
    let client = reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .user_agent("SkillsBox/0.1 skills.sh-marketplace")
        .build()
        .map_err(AppError::skills_sh_unavailable)?;
    let mut response = client
        .get(format!("{API}{path}"))
        .query(query)
        .send()
        .await
        .map_err(AppError::skills_sh_unavailable)?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(AppError::skills_sh_unavailable(format!(
            "HTTP {}",
            response.status()
        )));
    }
    if response
        .content_length()
        .is_some_and(|size| size > MAX_RESPONSE as u64)
    {
        return Err(AppError::skills_sh_unavailable("响应超过 16 MB 限制"));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(AppError::skills_sh_unavailable)?
    {
        if bytes.len() + chunk.len() > MAX_RESPONSE {
            return Err(AppError::skills_sh_unavailable("响应超过 16 MB 限制"));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(AppError::skills_sh_unavailable)
}

pub async fn list(keyword: &str, view: View, page: u32) -> Result<Listing, AppError> {
    let keyword = keyword.trim();
    if page > 100_000
        || keyword.chars().count() > 400
        || (!keyword.is_empty() && (keyword.chars().count() < 2 || page != 0))
    {
        return Err(AppError::skills_sh_unavailable(
            "搜索至少需要两个字符，且不支持搜索结果翻页",
        ));
    }
    let search = !keyword.is_empty();
    let value = if search {
        read(
            "/api/search",
            &[("q", keyword.into()), ("limit", "200".into())],
        )
        .await?
    } else {
        read(&format!("/api/skills/{}/{page}", view.path()), &[]).await?
    }
    .ok_or_else(|| AppError::skills_sh_unavailable("目录接口不存在"))?;
    parse_listing(value, search, page)
}

fn parse_listing(value: Value, search: bool, page: u32) -> Result<Listing, AppError> {
    let rows = value["skills"]
        .as_array()
        .ok_or_else(|| AppError::skills_sh_unavailable("目录响应缺少 skills"))?;
    if rows.len() > 200 {
        return Err(AppError::skills_sh_unavailable("目录响应超出单次条目限制"));
    }
    let mut skills = Vec::new();
    let mut seen = BTreeSet::new();
    for row in rows {
        let coordinate: Coordinate =
            serde_json::from_value(row.clone()).map_err(AppError::skills_sh_unavailable)?;
        coordinate.validate()?;
        if row
            .get("id")
            .is_some_and(|id| id.as_str() != Some(&coordinate.id()))
        {
            return Err(AppError::skills_sh_unavailable("返回的条目身份不一致"));
        }
        let name = row["name"]
            .as_str()
            .filter(|name| !name.trim().is_empty() && name.len() <= 400)
            .ok_or_else(|| AppError::skills_sh_unavailable("条目缺少有效名称"))?;
        if seen.insert(coordinate.id()) {
            skills.push(Skill {
                git_source: coordinate.git_source(),
                official_url: coordinate.url(),
                coordinate,
                name: name.into(),
                installs: row["installs"].as_u64(),
            });
        }
    }
    let total = if search {
        skills.len() as u64
    } else {
        value["total"]
            .as_u64()
            .ok_or_else(|| AppError::skills_sh_unavailable("榜单响应缺少 total"))?
    };
    let has_more = if search {
        false
    } else {
        if value["page"].as_u64() != Some(page as u64) {
            return Err(AppError::skills_sh_unavailable("榜单响应页码不一致"));
        }
        value["hasMore"]
            .as_bool()
            .ok_or_else(|| AppError::skills_sh_unavailable("榜单响应缺少 hasMore"))?
    };
    Ok(Listing {
        skills,
        total,
        page,
        has_more,
    })
}

pub async fn snapshot(coordinate: &Coordinate) -> Result<Snapshot, AppError> {
    coordinate.validate()?;
    if coordinate.git_source().is_none() {
        return Ok(Snapshot {
            files: None,
            hash: None,
        });
    }
    let Some(value) = read(&format!("/api/download/{}", coordinate.id()), &[]).await? else {
        return Ok(Snapshot {
            files: None,
            hash: None,
        });
    };
    parse_snapshot(value)
}

fn parse_snapshot(value: Value) -> Result<Snapshot, AppError> {
    let hash = value["hash"].as_str().map(String::from);
    if value.get("files").is_some_and(Value::is_null) {
        return Ok(Snapshot { files: None, hash });
    }
    let rows = value["files"]
        .as_array()
        .filter(|files| files.len() <= 1000)
        .ok_or_else(|| AppError::skills_sh_unavailable("快照文件列表无效或超过 1000 项"))?;
    let mut seen = BTreeSet::new();
    let mut files = Vec::new();
    for row in rows {
        let path = row["path"]
            .as_str()
            .filter(|path| {
                !path.is_empty()
                    && path.len() <= 1024
                    && !path.contains(['\\', '\0'])
                    && !path.contains(':')
                    && !path.chars().any(char::is_control)
                    && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
            })
            .ok_or_else(|| AppError::skills_sh_unavailable("快照包含无效文件路径"))?;
        if !seen.insert(path.to_string()) {
            return Err(AppError::skills_sh_unavailable("快照包含重复文件"));
        }
        let content = row["contents"]
            .as_str()
            .ok_or_else(|| AppError::skills_sh_unavailable("快照缺少文件文本"))?;
        files.push(File {
            path: path.into(),
            size_bytes: content.len(),
            contents: (content.len() <= MAX_TEXT && !content.contains('\0'))
                .then(|| content.into()),
        });
    }
    for file in &files {
        for (index, _) in file.path.match_indices('/') {
            if seen.contains(&file.path[..index]) {
                return Err(AppError::skills_sh_unavailable("快照文件与目录路径冲突"));
            }
        }
    }
    Ok(Snapshot {
        files: Some(files),
        hash,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn catalog_preserves_identity_and_separates_domain_sources() {
        let listing = parse_listing(
            json!({"skills": [
            {"source":"owner/repo", "skillId":"url-slug", "name":"actual-name", "installs":0},
            {"source":"open.feishu.cn", "skillId":"lark-doc", "name":"Lark"},
            {"source":"open.feishu.cn/path", "skillId":"nested", "name":"Nested"}
        ], "total":500, "hasMore":true, "page":2}),
            false,
            2,
        )
        .unwrap();
        assert_eq!(listing.skills[0].git_source.as_deref(), Some("owner/repo"));
        assert_eq!(listing.skills[0].name, "actual-name");
        assert_eq!(listing.skills[0].installs, Some(0));
        assert!(
            listing.skills[1..]
                .iter()
                .all(|skill| skill.git_source.is_none())
        );
        assert!(
            listing.skills[1]
                .official_url
                .contains("/site/open.feishu.cn/")
        );
        assert!(listing.has_more);
        assert!(parse_listing(json!({"skills":[]}), false, 0).is_err());
        assert!(
            parse_listing(
                json!({"skills":[{"source":"a/b","skillId":"x","name":"x","id":"other/x"}]}),
                true,
                0
            )
            .is_err()
        );
    }

    #[test]
    fn snapshots_reject_ambiguous_or_unsafe_files_and_bound_preview_text() {
        for path in [
            "../secret",
            "/absolute",
            "a//b",
            "a/./b",
            "a\\b",
            "C:secret",
        ] {
            assert!(parse_snapshot(json!({"files":[{"path":path,"contents":"x"}]})).is_err());
        }
        assert!(
            parse_snapshot(
                json!({"files":[{"path":"x","contents":"a"},{"path":"x","contents":"b"}]})
            )
            .is_err()
        );
        let snap = parse_snapshot(json!({"files":[{"path":"large.txt","contents":"a".repeat(MAX_TEXT+1)}],"hash":"snapshot"})).unwrap();
        assert!(snap.files.unwrap()[0].contents.is_none());
        assert!(
            parse_snapshot(json!({"files":null}))
                .unwrap()
                .files
                .is_none()
        );
        assert!(parse_snapshot(json!({"error":"unavailable"})).is_err());
        assert!(
            parse_snapshot(
                json!({"files":[{"path":"a/b","contents":"x"},{"path":"a","contents":"y"}]})
            )
            .is_err()
        );
    }

    #[tokio::test]
    #[ignore = "read-only live skills.sh contract; requires network"]
    async fn skills_sh_live_catalog_and_snapshot() {
        for view in [View::AllTime, View::Trending, View::Hot] {
            assert!(!list("", view, 0).await.unwrap().skills.is_empty());
        }
        let listing = list("find-skills", View::AllTime, 0).await.unwrap();
        assert!(!listing.skills.is_empty());
        let content = snapshot(&Coordinate {
            source: "vercel-labs/skills".into(),
            skill_id: "find-skills".into(),
        })
        .await
        .unwrap();
        assert!(
            content
                .files
                .unwrap()
                .iter()
                .any(|file| file.path == "SKILL.md")
        );
    }
}
