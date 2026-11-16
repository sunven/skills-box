//! Marketplace adapters and app-owned, scope-local installation records.
pub mod clawhub;
pub mod files;
pub mod provider;
pub mod records;
pub mod runtime;
pub mod skills_sh;
pub mod updates;

use crate::error::AppError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MarketplaceSource {
    Skillhub,
    Clawhub,
}
impl MarketplaceSource {
    /// Wire/record value used as the projected `sourceType`.
    pub fn key(self) -> &'static str {
        match self {
            Self::Skillhub => "skillhub",
            Self::Clawhub => "clawhub",
        }
    }
    /// Parse a projected `sourceType` back to its marketplace source.
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "skillhub" => Some(Self::Skillhub),
            "clawhub" => Some(Self::Clawhub),
            _ => None,
        }
    }
    /// User-facing source label; never derive this from a match at call sites.
    pub fn label(self) -> &'static str {
        match self {
            Self::Skillhub => "SkillHub",
            Self::Clawhub => "ClawHub",
        }
    }
    /// Canonical public listing URL for one coordinate on this source.
    pub fn listing_url(self, namespace: &str, slug: &str) -> String {
        match self {
            Self::Skillhub => format!("https://skillhub.cn/skills/{namespace}/{slug}"),
            Self::Clawhub => format!("https://clawhub.ai/{namespace}/skills/{slug}"),
        }
    }
}
/// Display label for a projected marketplace `sourceType`.
pub fn source_label(source_type: &str) -> Option<&'static str> {
    MarketplaceSource::from_key(source_type).map(MarketplaceSource::label)
}
/// Native marketplace sources are app-owned records (`skillhub`/`clawhub`),
/// plus expert-pack workflows (`skillhub-pack`). Git, unmanaged and empty
/// source types are not native.
pub fn is_native_source_type(source_type: &str) -> bool {
    source_type == "skillhub-pack" || MarketplaceSource::from_key(source_type).is_some()
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
    /// Parse the `@owner/slug` display form for a known source. Existing
    /// SkillHub callers keep the source-less default; ClawHub records must
    /// pass their own source instead of inheriting SkillHub.
    ///
    /// Warning: this default is a trap for source-aware code. Any ClawHub or
    /// future-source coordinate must use [`Self::from_canonical_for`]; a bare
    /// `@owner/slug` string carries no source.
    pub fn from_canonical(raw: &str) -> Result<Self, AppError> {
        Self::from_canonical_for(MarketplaceSource::Skillhub, raw)
    }
    pub fn from_canonical_for(source: MarketplaceSource, raw: &str) -> Result<Self, AppError> {
        let (namespace, slug) = raw
            .strip_prefix('@')
            .and_then(|s| s.split_once('/'))
            .ok_or_else(|| AppError::marketplace_unavailable("缺少完整市场坐标"))?;
        let value = Self {
            source,
            namespace: namespace.into(),
            slug: slug.into(),
        };
        value.validate()?;
        Ok(value)
    }
    pub fn url(&self) -> String {
        self.source.listing_url(&self.namespace, &self.slug)
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
/// Marketplace installation references are the pinned release (SkillHub,
/// ClawHub archive) or the resolved commit (ClawHub GitHub-backed).
pub fn validate_reference(source: MarketplaceSource, reference: &str) -> Result<(), AppError> {
    validate_version(reference)?;
    if source == MarketplaceSource::Clawhub && !is_commit(reference) && !is_semver(reference) {
        return Err(AppError::marketplace_unavailable(
            "ClawHub 安装引用必须是 semver 版本或 40 位提交哈希",
        ));
    }
    Ok(())
}
pub fn is_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn is_semver(value: &str) -> bool {
    let (core, build) = value
        .split_once('+')
        .map_or((value, None), |(a, b)| (a, Some(b)));
    let (core, prerelease) = core
        .split_once('-')
        .map_or((core, None), |(a, b)| (a, Some(b)));
    let identifiers = |raw: &str| {
        !raw.is_empty()
            && raw.split('.').all(|part| {
                !part.is_empty() && part.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
            })
    };
    let components: Vec<&str> = core.split('.').collect();
    components.len() == 3
        && components
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|c| c.is_ascii_digit()))
        && prerelease.is_none_or(|raw| identifiers(raw))
        && build.is_none_or(|raw| identifiers(raw))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marketplace_sources_round_trip_keys_labels_and_native_classification() {
        for source in [MarketplaceSource::Skillhub, MarketplaceSource::Clawhub] {
            assert_eq!(MarketplaceSource::from_key(source.key()), Some(source));
            assert!(is_native_source_type(source.key()));
        }
        assert_eq!(
            source_label("clawhub"),
            Some(MarketplaceSource::Clawhub.label())
        );
        assert_eq!(source_label("github"), None);
        assert!(is_native_source_type("skillhub-pack"));
        for source in ["github", "git", "unmanaged", ""] {
            assert!(!is_native_source_type(source), "{source}");
        }
    }

    #[test]
    fn marketplace_sources_keep_skillhub_wire_format_and_add_clawhub() {
        assert_eq!(
            serde_json::to_string(&MarketplaceSource::Skillhub).unwrap(),
            "\"skillhub\""
        );
        assert_eq!(
            serde_json::to_string(&MarketplaceSource::Clawhub).unwrap(),
            "\"clawhub\""
        );
        assert!(serde_json::from_str::<MarketplaceSource>("\"unknown\"").is_err());
    }

    #[test]
    fn coordinates_parse_display_and_link_per_source() {
        let skillhub = Coordinate::from_canonical("@owner/slug").unwrap();
        assert_eq!(skillhub.source, MarketplaceSource::Skillhub);
        assert_eq!(skillhub.canonical(), "@owner/slug");
        assert_eq!(skillhub.url(), "https://skillhub.cn/skills/owner/slug");

        let clawhub =
            Coordinate::from_canonical_for(MarketplaceSource::Clawhub, "@owner/slug").unwrap();
        assert_eq!(clawhub.source, MarketplaceSource::Clawhub);
        assert_eq!(clawhub.canonical(), "@owner/slug");
        assert_eq!(clawhub.url(), "https://clawhub.ai/owner/skills/slug");
        assert!(Coordinate::from_canonical_for(MarketplaceSource::Clawhub, "owner/slug").is_err());
    }

    #[test]
    fn clawhub_references_require_semver_or_pinned_commit() {
        for valid in [
            "1.0.0",
            "2026.11.10-beta.1",
            "1.0.0-rc.1+build.2",
            "b4416dddac9b6a0cc5412136e6dbf8f07ffdb31c",
        ] {
            assert!(
                validate_reference(MarketplaceSource::Clawhub, valid).is_ok(),
                "{valid}"
            );
        }
        for invalid in [
            "latest",
            "1.0",
            "main",
            "b4416ddd",
            "1.0.0 ",
            "b4416dddac9b6a0cc5412136e6dbf8f07ffdb31C",
        ] {
            assert!(
                validate_reference(MarketplaceSource::Clawhub, invalid).is_err(),
                "{invalid}"
            );
        }
        // SkillHub keeps accepting its existing version labels.
        assert!(validate_reference(MarketplaceSource::Skillhub, "release-2026.11").is_ok());
    }
}
