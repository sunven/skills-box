//! Permanent observation and user state, independent of installation locks.
use crate::tasks::parser::ParsedSkill;
use crate::tasks::preview::CATALOG_CONTRACT;
use crate::tasks::{validate_skill_name, validate_source};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub fn name_identity(name: &str) -> String {
    name.to_lowercase()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryId {
    pub source: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSkill {
    #[serde(flatten)]
    pub skill: ParsedSkill,
    pub install_issue: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    pub first_seen_at: u64,
    pub is_new: bool,
    pub processed: bool,
    pub ignored: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRecord {
    pub baseline: bool,
    pub catalog: BTreeMap<String, CatalogSkill>,
    pub history: BTreeMap<String, Observation>,
    pub contract: String,
    pub commit: Option<String>,
    pub valid: bool,
    pub last_attempt_at: Option<u64>,
    pub last_success_at: Option<u64>,
    pub error: Option<String>,
}

impl SourceRecord {
    pub fn reusable(&self, commit: &str) -> bool {
        self.valid
            && self.baseline
            && self.contract == CATALOG_CONTRACT
            && self.commit.as_deref() == Some(commit)
    }
    pub fn publish(&mut self, skills: Vec<ParsedSkill>, commit: String, now: u64) {
        let mut catalog: BTreeMap<String, CatalogSkill> = BTreeMap::new();
        for skill in skills {
            let name = name_identity(&skill.name);
            self.history.entry(name.clone()).or_insert(Observation {
                first_seen_at: now,
                is_new: self.baseline,
                processed: !self.baseline,
                ignored: false,
            });
            if let Some(previous) = catalog.get_mut(&name) {
                previous.install_issue = Some("来源中存在无法按名称区分的重名 skill".into());
            } else {
                let install_issue = validate_skill_name(&skill.name).err().map(|e| e.message);
                catalog.insert(
                    name,
                    CatalogSkill {
                        skill,
                        install_issue,
                    },
                );
            }
        }
        self.catalog = catalog;
        self.baseline = true;
        self.contract = CATALOG_CONTRACT.into();
        self.commit = Some(commit);
        self.valid = true;
        self.last_success_at = Some(now);
        self.error = None;
    }
    pub fn unread(&self, name: &str) -> bool {
        self.catalog.contains_key(name)
            && self
                .history
                .get(name)
                .is_some_and(|h| h.is_new && !h.processed && !h.ignored)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeSnapshot {
    pub revision: String,
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cooldown {
    pub context: String,
    pub until: u64,
    #[serde(skip)]
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct History {
    pub version: u32,
    pub scopes: BTreeMap<String, ScopeSnapshot>,
    pub sources: BTreeMap<String, SourceRecord>,
    pub cooldown: Option<Cooldown>,
}
impl Default for History {
    fn default() -> Self {
        Self {
            version: 1,
            scopes: BTreeMap::new(),
            sources: BTreeMap::new(),
            cooldown: None,
        }
    }
}
impl History {
    pub fn validate(&self) -> bool {
        self.version == 1
            && self.scopes.iter().all(|(id, snapshot)| {
                id.parse::<crate::scope::ScopeId>().is_ok()
                    && !snapshot.revision.is_empty()
                    && snapshot.sources.iter().all(|s| canonical(s))
            })
            && self.sources.iter().all(|(source, record)| {
                canonical(source)
                    && (!record.valid || record.baseline)
                    && record.catalog.iter().all(|(name, skill)| {
                        *name == name_identity(&skill.skill.name)
                            && !name.is_empty()
                            && name.len() <= 4096
                            && record.history.contains_key(name)
                    })
                    && record.history.iter().all(|(name, h)| {
                        !name.is_empty()
                            && name.len() <= 4096
                            && *name == name_identity(name)
                            && (!h.ignored || h.processed)
                    })
                    && record
                        .commit
                        .as_ref()
                        .is_none_or(|s| crate::git_tree::is_git_tree_hash(s))
            })
    }
}
fn canonical(source: &str) -> bool {
    validate_source(source).is_ok_and(|s| s.identity() == source)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn skills(names: &[&str]) -> Vec<ParsedSkill> {
        names
            .iter()
            .map(|n| ParsedSkill {
                name: (*n).into(),
                description: "description".into(),
                group: None,
            })
            .collect()
    }
    #[test]
    fn baseline_new_removal_return_case_changes_and_user_state() {
        let mut record = SourceRecord::default();
        record.publish(skills(&["alpha"]), "a".repeat(40), 1);
        assert!(!record.unread("alpha"));
        record.publish(skills(&["ALPHA", "beta"]), "b".repeat(40), 2);
        assert!(record.unread("beta"));
        assert!(!record.unread("alpha"));
        record.publish(skills(&[]), "c".repeat(40), 3);
        assert!(!record.unread("beta"));
        record.publish(skills(&["beta"]), "d".repeat(40), 4);
        assert!(record.unread("beta"));
        let state = record.history.get_mut("beta").unwrap();
        state.processed = true;
        state.ignored = true;
        record.publish(skills(&["BETA", "renamed"]), "e".repeat(40), 5);
        assert!(!record.unread("beta"));
        assert!(record.history["beta"].ignored);
        assert!(record.unread("renamed"));
        assert_eq!(record.history["beta"].first_seen_at, 2);
    }
    #[test]
    fn empty_success_establishes_baseline_duplicates_and_unsafe_names_are_not_installable() {
        let mut r = SourceRecord::default();
        r.publish(vec![], "a".repeat(40), 1);
        r.publish(skills(&["foo", "FOO", "../bad"]), "b".repeat(40), 2);
        assert_eq!(r.catalog.len(), 2);
        assert!(r.catalog["foo"].install_issue.is_some());
        assert!(r.catalog["../bad"].install_issue.is_some());
        assert!(r.unread("foo"));
    }
}
