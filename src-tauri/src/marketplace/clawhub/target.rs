//! Installation/update target resolution for ClawHub entries.
//!
//! Archive entries resolve to a ClawHub-hosted semver release. GitHub-backed
//! entries resolve to the pinned commit and folder content hash from the
//! anonymous resolver. Alias entries never install through this source.

use super::github::GithubReference;
use crate::{
    error::AppError,
    marketplace::{Coordinate, MarketplaceSource, clawhub, validate_reference},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallTarget {
    Archive(String),
    Github(GithubReference),
}

pub async fn resolve(coordinate: &Coordinate) -> Result<InstallTarget, AppError> {
    if coordinate.source != MarketplaceSource::Clawhub {
        return Err(AppError::clawhub_install_unsupported(
            "安装需要 ClawHub 来源坐标",
        ));
    }
    coordinate.validate()?;
    let detail = match clawhub::detail(coordinate).await? {
        clawhub::Detail::Native(detail) => detail,
        clawhub::Detail::Alias(_) => {
            return Err(AppError::clawhub_install_unsupported(
                "该条目是 skills.sh 别名，不属于 ClawHub 原生技能",
            ));
        }
    };
    if let Some(version) = detail.latest_version.or_else(|| {
        detail
            .versions
            .first()
            .map(|version| version.version.clone())
    }) {
        validate_reference(MarketplaceSource::Clawhub, &version)?;
        return Ok(InstallTarget::Archive(version));
    }
    let reference = detail.resolved_reference.ok_or_else(|| {
        AppError::clawhub_install_unsupported("该条目没有可安装的版本或 GitHub 解析引用")
    })?;
    let content_hash = reference.content_hash.ok_or_else(|| {
        AppError::clawhub_install_unsupported("ClawHub 解析结果缺少文件夹内容哈希")
    })?;
    validate_reference(MarketplaceSource::Clawhub, &reference.commit)?;
    Ok(InstallTarget::Github(GithubReference {
        repo: reference.repo,
        path: reference.path,
        commit: reference.commit,
        content_hash,
    }))
}
