//! Marketplace adapters and app-owned, scope-local installation records.
pub mod provider;
pub mod records;
pub mod runtime;
pub mod updates;

use crate::error::AppError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MarketplaceSource {
    Skillhub,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Coordinate {
    pub source: MarketplaceSource,
    pub namespace: String,
    pub slug: String,
}
impl Coordinate {
    pub fn validate(&self) -> Result<(), AppError> {
        for value in [&self.namespace, &self.slug] {
            if value.is_empty()
                || value.len() > 200
                || !value
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
                || value.starts_with('.')
                || value.starts_with('-')
            {
                return Err(AppError::marketplace_unavailable("无效的市场坐标"));
            }
        }
        Ok(())
    }
    pub fn canonical(&self) -> String {
        format!("@{}/{}", self.namespace, self.slug)
    }
    pub fn from_canonical(raw: &str) -> Result<Self, AppError> {
        let (namespace, slug) = raw
            .strip_prefix('@')
            .and_then(|s| s.split_once('/'))
            .ok_or_else(|| AppError::marketplace_unavailable("缺少完整市场坐标"))?;
        let value = Self {
            source: MarketplaceSource::Skillhub,
            namespace: namespace.into(),
            slug: slug.into(),
        };
        value.validate()?;
        Ok(value)
    }
    pub fn url(&self) -> String {
        format!(
            "https://skillhub.cn/skills/{}/{}",
            self.namespace, self.slug
        )
    }
}
pub fn validate_version(version: &str) -> Result<(), AppError> {
    if version.is_empty()
        || version.len() > 200
        || !version
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.+".contains(&c))
        || version == "latest"
    {
        return Err(AppError::marketplace_unavailable(
            "平台未提供可固定的有效版本",
        ));
    }
    Ok(())
}
pub fn original_name(markdown: &str) -> Result<String, AppError> {
    let name = crate::inventory::frontmatter::parse(markdown)
        .map_err(AppError::marketplace_unavailable)?
        .and_then(|f| f.name)
        .ok_or_else(|| AppError::marketplace_unavailable("SKILL.md 缺少原始 name，无法安装"))?;
    crate::tasks::install::validate_skill_name(&name)?;
    if name.starts_with('.') || name.trim() != name {
        return Err(AppError::skill_invalid_name(
            "原始技能名不能是隐藏目录或带首尾空白。",
        ));
    }
    Ok(name)
}
