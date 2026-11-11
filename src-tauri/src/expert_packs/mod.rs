//! Scope-local pack snapshots. Skills retain independent ownership records.
pub mod provider;
pub mod records;
pub mod state;
pub mod workflow;

use crate::{
    error::AppError,
    marketplace::{
        Coordinate,
        provider::{Detail, Summary},
    },
};
use serde::{Deserialize, Serialize};

pub fn validate_slug(slug: &str) -> Result<(), AppError> {
    Coordinate::from_canonical(&format!("@skillhub/{slug}")).map(|_| ())
}
pub fn url(slug: &str) -> String {
    format!("https://skillhub.cn/skillsets/{slug}")
}
pub fn is_native_source(source: &str) -> bool {
    matches!(source, "skillhub" | "skillhub-pack")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Member {
    pub reference: String,
    pub coordinate: Coordinate,
    pub name: String,
    pub version: String,
    pub markdown: String,
}
impl Member {
    pub fn detail(&self) -> Detail {
        Detail {
            summary: Summary {
                coordinate: self.coordinate.clone(),
                name: self.reference.clone(),
                publisher: self.coordinate.namespace.clone(),
                description: String::new(),
                version: self.version.clone(),
                official_url: self.coordinate.url(),
                stars: None,
                downloads: None,
            },
            markdown: self.markdown.clone(),
            original_name: Some(self.name.clone()),
            install_error: None,
            requirements: vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Snapshot {
    pub slug: String,
    pub display_name: String,
    pub summary: String,
    pub original_workflow: String,
    pub workflow_name: String,
    pub installed_workflow: String,
    pub members: Vec<Member>,
    pub agents: Vec<String>,
    pub installed_at: u64,
}
impl Snapshot {
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.members
            .iter()
            .map(|m| m.name.as_str())
            .chain(std::iter::once(self.workflow_name.as_str()))
    }
    pub fn validate(&self) -> Result<(), AppError> {
        validate_slug(&self.slug)?;
        if self.members.is_empty()
            || self.members.len() > 100
            || self.agents.is_empty()
            || self.original_workflow.len() > 2 * 1024 * 1024
        {
            return Err(AppError::lock_corrupt(
                "专家包快照缺少成员或目标，或超过大小限制",
            ));
        }
        let mut names = std::collections::BTreeSet::new();
        let mut coordinates = std::collections::BTreeSet::new();
        for member in &self.members {
            member.coordinate.validate()?;
            validate_slug(&member.reference)?;
            crate::marketplace::validate_version(&member.version)?;
            if crate::marketplace::original_name(&member.markdown)? != member.name
                || !coordinates.insert(member.coordinate.canonical())
            {
                return Err(AppError::lock_corrupt("专家包成员身份重复或不一致"));
            }
        }
        for name in self.names() {
            crate::tasks::install::validate_skill_name(name)?;
            if !names.insert(name.to_lowercase()) {
                return Err(AppError::skill_conflict(
                    name,
                    "专家包入口与成员名称重复，或仅大小写不同。",
                ));
            }
        }
        if crate::marketplace::original_name(&self.installed_workflow)? != self.workflow_name {
            return Err(AppError::lock_corrupt("专家包工作流身份不一致"));
        }
        Ok(())
    }
}
