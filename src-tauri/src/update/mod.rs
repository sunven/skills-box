//! Read-only update checking.
//!
//! `check_updates` never runs the skills CLI. It reads lock v3, groups entries
//! by `(source, ref)`, and compares GitHub tree SHAs or generic Git content
//! fingerprints with the recorded `skillFolderHash`. Network failures are isolated per source and
//! keep the previous successful result; they never become "current".

pub mod api;
pub mod archive;
mod attempt;
pub mod cache;
mod content_check;
pub mod content_hash;
pub mod git;
pub mod github;
pub mod project;
pub mod project_checks;
pub mod state;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::Serialize;

use crate::error::AppError;
use crate::inventory::UpdateState;
use crate::lock::{LockEntry, LockFile};
use crate::paths::AppPaths;
use archive::{
    ArchiveClient, ArchiveLimits, CommitArchiveCache, SourceArchiveClient, SourceArchives,
};
use attempt::{GroupExecutor, GroupOutcome, PreparedCheck, PreparedSource, SchedulingLimits};
use cache::{CachedSourceTree, UpdateCache, is_fresh};
use github::{GitHubTreeClient, RemoteTree, ReqwestGitHubClient};
use state::{UpdateCheckState, UpdateCheckStateStore};

pub const UPDATE_CHECK_CACHE_TTL_MS: u64 = 6 * 60 * 60 * 1000;
pub const UPDATE_CHECK_MAX_SOURCES: usize = 20;
pub const UPDATE_CHECK_CONCURRENCY: usize = 4;
pub const UPDATE_CHECK_DEADLINE_MS: u64 = crate::timeouts::READ_TIMEOUT_MS;

pub trait Clock: Send + Sync {
    fn now_epoch_ms(&self) -> u64;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_epoch_ms(&self) -> u64 {
        crate::time::now_ms()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCheckConfig {
    pub cache_ttl_ms: u64,
    pub max_sources: usize,
    pub concurrency: usize,
    pub deadline_ms: u64,
}

impl Default for UpdateCheckConfig {
    fn default() -> Self {
        Self {
            cache_ttl_ms: UPDATE_CHECK_CACHE_TTL_MS,
            max_sources: UPDATE_CHECK_MAX_SOURCES,
            concurrency: UPDATE_CHECK_CONCURRENCY,
            deadline_ms: UPDATE_CHECK_DEADLINE_MS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateSourceStatus {
    Ok,
    Cached,
    Error,
    Skipped,
}

/// Why a skill could not be checked. `NotChecked` means a source limit or a
/// skipped source; `Uncheckable` means the source data itself cannot be
/// compared. The frontend groups skip reasons from this instead of prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateUnknownReason {
    NotChecked,
    Uncheckable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSkillResult {
    pub name: String,
    pub source: Option<String>,
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    pub lock_hash: Option<String>,
    pub remote_hash: Option<String>,
    pub state: UpdateState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unknown_reason: Option<UpdateUnknownReason>,
    pub cached: bool,
    pub last_success_at_epoch_ms: Option<u64>,
    pub previous_state: Option<UpdateState>,
    pub error: Option<AppError>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSourceResult {
    pub source: String,
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    pub status: UpdateSourceStatus,
    pub skill_count: usize,
    pub cached: bool,
    pub truncated: bool,
    pub last_success_at_epoch_ms: Option<u64>,
    pub error: Option<AppError>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckReport {
    pub skills: Vec<UpdateSkillResult>,
    pub sources: Vec<UpdateSourceResult>,
    pub checked_at_epoch_ms: u64,
    pub last_success_at_epoch_ms: Option<u64>,
    pub skipped_source_count: usize,
    pub cached_source_count: usize,
    pub warnings: Vec<String>,
}

impl UpdateCheckReport {
    /// Skills with a newer remote fingerprint. Persisted per scope so sidebar
    /// badges do not depend on a live report.
    pub fn candidate_count(&self) -> usize {
        self.skills
            .iter()
            .filter(|skill| skill.state == UpdateState::Update)
            .count()
    }
}

/// An invalidated check ends normally without publishing a report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum UpdateCheckResponse {
    Applied {
        scope: crate::scope::ScopeId,
        #[serde(rename = "checkRevision", skip_serializing_if = "Option::is_none")]
        check_revision: Option<String>,
        report: UpdateCheckReport,
        #[serde(skip_serializing_if = "Option::is_none")]
        state: Option<UpdateCheckState>,
    },
    Invalidated {
        invalidated: bool,
        scope: crate::scope::ScopeId,
        #[serde(rename = "checkRevision")]
        check_revision: String,
    },
}

pub struct UpdateService {
    paths: AppPaths,
    client: Arc<dyn GitHubTreeClient>,
    archive_client: Arc<dyn ArchiveClient>,
    archive_cache: CommitArchiveCache,
    clock: Arc<dyn Clock>,
    cache: UpdateCache,
    state: Arc<UpdateCheckStateStore>,
    config: UpdateCheckConfig,
}

impl UpdateService {
    pub fn new(paths: AppPaths) -> Self {
        let state = Arc::new(UpdateCheckStateStore::new(&paths));
        Self::new_with_api(paths, state, api::GitHubApiClient::anonymous())
    }

    /// Production wiring: one store shared with the Project check lifecycle.
    pub fn new_with_api(
        paths: AppPaths,
        state: Arc<UpdateCheckStateStore>,
        api: api::GitHubApiClient,
    ) -> Self {
        Self {
            cache: UpdateCache::new(&paths),
            state,
            archive_client: Arc::new(SourceArchiveClient::with_api(paths.clone(), api.clone())),
            archive_cache: CommitArchiveCache::new(&paths),
            paths,
            client: Arc::new(ReqwestGitHubClient::with_api(api)),
            clock: Arc::new(SystemClock),
            config: UpdateCheckConfig::default(),
        }
    }

    pub fn with_dependencies(
        paths: AppPaths,
        client: Arc<dyn GitHubTreeClient>,
        clock: Arc<dyn Clock>,
        config: UpdateCheckConfig,
    ) -> Self {
        Self {
            cache: UpdateCache::new(&paths),
            state: Arc::new(UpdateCheckStateStore::new(&paths)),
            archive_client: Arc::new(SourceArchiveClient::new(paths.clone())),
            archive_cache: CommitArchiveCache::new(&paths),
            paths,
            client,
            clock,
            config,
        }
    }

    pub async fn check(
        &self,
        skill_ids: Option<Vec<String>>,
    ) -> Result<UpdateCheckReport, AppError> {
        let lock = LockFile::read(&self.paths.lock_path())?;
        let full_check = skill_ids.is_none();
        let now = self.clock.now_epoch_ms();
        let selected = skill_ids.map(|ids| ids.into_iter().collect::<BTreeSet<_>>());
        let (skills, groups) = self.collect_targets(&lock, selected.as_ref());
        let sources = groups
            .into_values()
            .map(|group| {
                let cached = if group.is_content() {
                    None
                } else {
                    self.cache.load(&group.source, group.reference.as_deref())
                };
                match cached {
                    Some(cached)
                        if is_fresh(cached.fetched_at_epoch_ms, now, self.config.cache_ttl_ms) =>
                    {
                        PreparedSource::Ready(Box::new(self.cached_outcome(group, &cached)))
                    }
                    cached => PreparedSource::Pending((group, cached)),
                }
            })
            .collect();
        let mut report = attempt::run(
            PreparedCheck {
                checked_at_epoch_ms: now,
                skills,
                sources,
            },
            SchedulingLimits {
                max_sources: self.config.max_sources,
                concurrency: self.config.concurrency,
                deadline_ms: self.config.deadline_ms,
            },
            GlobalGroupExecutor {
                service: self,
                archives: SourceArchives::new(
                    Arc::clone(&self.archive_client),
                    self.archive_cache.clone(),
                    Arc::clone(&self.clock),
                    self.config.cache_ttl_ms,
                    ArchiveLimits::default(),
                ),
            },
        )
        .await;
        if full_check {
            let state = UpdateCheckState::from_report(&report);
            if let Err(error) = self.state.store(&crate::scope::ScopeId::Global, &state) {
                report
                    .warnings
                    .push(format!("无法写入更新检查状态：{error}"));
            }
        }
        Ok(report)
    }

    /// Last persisted full-check summary for one scope, if it exists and is
    /// valid. Version-1 files are read as the global scope.
    pub fn read_state(&self, scope: &crate::scope::ScopeId) -> Option<UpdateCheckState> {
        self.state.load(scope)
    }

    fn collect_targets(
        &self,
        lock: &LockFile,
        selected: Option<&BTreeSet<String>>,
    ) -> (Vec<UpdateSkillResult>, BTreeMap<GroupKey, Group>) {
        let mut base_results = Vec::new();
        let mut groups: BTreeMap<GroupKey, Group> = BTreeMap::new();
        for entry in lock.iter() {
            if selected.is_some_and(|names| !names.contains(&entry.name)) {
                continue;
            }
            match checkable_target(entry) {
                Ok(target) => {
                    let key = GroupKey {
                        source: crate::tasks::source::source_identity(&target.source),
                        reference: target.reference.clone(),
                    };
                    groups
                        .entry(key)
                        .or_insert_with(|| Group {
                            source: target.source.clone(),
                            reference: target.reference.clone(),
                            targets: Vec::new(),
                        })
                        .targets
                        .push(target);
                }
                Err(note) => base_results.push(uncheckable_result(entry, note)),
            }
        }
        (base_results, groups)
    }

    async fn content_outcome(&self, group: Group, archives: &SourceArchives) -> GroupOutcome {
        let acquired = match archives
            .acquire(&group.source, group.reference.as_deref())
            .await
        {
            Ok(acquired) => acquired,
            Err(failure) => {
                let mut outcome = self.timeout_outcome(group, None);
                outcome.source.error = Some(failure.error.clone());
                outcome.source.last_success_at_epoch_ms = failure.previous_fetched_at;
                for skill in &mut outcome.skills {
                    skill.error = Some(failure.error.clone());
                    skill.last_success_at_epoch_ms = failure.previous_fetched_at;
                }
                return outcome;
            }
        };
        let skills = group
            .targets
            .iter()
            .map(|target| {
                let evaluation = content_check::evaluate_target(
                    &content_check::ContentTarget {
                        name: target.name.clone(),
                        skill_path: target.skill_path.clone(),
                        lock_hash: target.lock_hash.clone(),
                    },
                    &acquired.archive,
                );
                UpdateSkillResult {
                    name: target.name.clone(),
                    source: Some(group.source.clone()),
                    reference: group.reference.clone(),
                    lock_hash: Some(target.lock_hash.clone()),
                    remote_hash: evaluation.remote_hash,
                    state: evaluation.state,
                    unknown_reason: (evaluation.state == UpdateState::Unknown)
                        .then_some(UpdateUnknownReason::Uncheckable),
                    cached: acquired.cached,
                    last_success_at_epoch_ms: Some(acquired.fetched_at_epoch_ms),
                    previous_state: None,
                    error: None,
                    note: evaluation.note,
                }
            })
            .collect();
        GroupOutcome {
            source: UpdateSourceResult {
                source: group.source,
                reference: group.reference,
                status: if acquired.cached {
                    UpdateSourceStatus::Cached
                } else {
                    UpdateSourceStatus::Ok
                },
                skill_count: group.targets.len(),
                cached: acquired.cached,
                truncated: false,
                last_success_at_epoch_ms: Some(acquired.fetched_at_epoch_ms),
                error: None,
                note: None,
            },
            skills,
            warnings: Vec::new(),
        }
    }

    async fn network_outcome(
        &self,
        group: Group,
        cached: Option<CachedSourceTree>,
    ) -> GroupOutcome {
        let fetched_at = self.clock.now_epoch_ms();
        match self
            .client
            .fetch_tree(&group.source, group.reference.as_deref())
            .await
        {
            Ok(tree) => {
                let mut warnings = Vec::new();
                if let Err(error) =
                    self.cache
                        .store(&group.source, group.reference.as_deref(), &tree, fetched_at)
                {
                    warnings.push(format!("无法写入更新检查缓存：{error}"));
                }
                let truncated_note = tree
                    .truncated
                    .then(|| "GitHub tree 响应被截断，无法可靠比较。".to_string());
                let skills = group
                    .targets
                    .iter()
                    .map(|target| {
                        success_result(target, &tree, fetched_at, false, truncated_note.clone())
                    })
                    .collect();
                GroupOutcome {
                    source: UpdateSourceResult {
                        source: group.source,
                        reference: group.reference,
                        status: UpdateSourceStatus::Ok,
                        skill_count: group.targets.len(),
                        cached: false,
                        truncated: tree.truncated,
                        last_success_at_epoch_ms: Some(fetched_at),
                        error: None,
                        note: truncated_note,
                    },
                    skills,
                    warnings,
                }
            }
            Err(error) => {
                let app_error = error.to_app_error();
                let previous_checked_at = cached.as_ref().map(|cached| cached.fetched_at_epoch_ms);
                let skills = group
                    .targets
                    .iter()
                    .map(|target| {
                        let previous = cached
                            .as_ref()
                            .map(|cached| evaluate(target, &cached.tree).state);
                        UpdateSkillResult {
                            name: target.name.clone(),
                            source: Some(target.source.clone()),
                            reference: target.reference.clone(),
                            lock_hash: Some(target.lock_hash.clone()),
                            remote_hash: None,
                            state: UpdateState::Error,
                            unknown_reason: None,
                            cached: false,
                            last_success_at_epoch_ms: previous_checked_at,
                            previous_state: previous,
                            error: Some(app_error.clone()),
                            note: previous_checked_at
                                .map(|_| "已保留上次成功检查结果。".to_string()),
                        }
                    })
                    .collect();
                GroupOutcome {
                    source: UpdateSourceResult {
                        source: group.source,
                        reference: group.reference,
                        status: UpdateSourceStatus::Error,
                        skill_count: group.targets.len(),
                        cached: false,
                        truncated: cached.as_ref().is_some_and(|cached| cached.tree.truncated),
                        last_success_at_epoch_ms: previous_checked_at,
                        error: Some(app_error),
                        note: None,
                    },
                    skills,
                    warnings: Vec::new(),
                }
            }
        }
    }

    fn cached_outcome(&self, group: Group, cached: &CachedSourceTree) -> GroupOutcome {
        let truncated_note = cached
            .tree
            .truncated
            .then(|| "GitHub tree 响应被截断，无法可靠比较。".to_string());
        let skills = group
            .targets
            .iter()
            .map(|target| {
                success_result(
                    target,
                    &cached.tree,
                    cached.fetched_at_epoch_ms,
                    true,
                    truncated_note.clone(),
                )
            })
            .collect();
        GroupOutcome {
            source: UpdateSourceResult {
                source: group.source,
                reference: group.reference,
                status: UpdateSourceStatus::Cached,
                skill_count: group.targets.len(),
                cached: true,
                truncated: cached.tree.truncated,
                last_success_at_epoch_ms: Some(cached.fetched_at_epoch_ms),
                error: None,
                note: truncated_note,
            },
            skills,
            warnings: Vec::new(),
        }
    }

    fn skipped_outcome(
        &self,
        group: Group,
        cached: Option<&CachedSourceTree>,
        max_sources: usize,
    ) -> GroupOutcome {
        let note = format!("本次检查达到来源上限（{max_sources} 个），该来源尚未检查。");
        let last_success_at_epoch_ms = cached.map(|cached| cached.fetched_at_epoch_ms);
        let skills = group
            .targets
            .iter()
            .map(|target| {
                let previous = cached
                    .as_ref()
                    .map(|cached| evaluate(target, &cached.tree).state);
                UpdateSkillResult {
                    name: target.name.clone(),
                    source: Some(target.source.clone()),
                    reference: target.reference.clone(),
                    lock_hash: Some(target.lock_hash.clone()),
                    remote_hash: None,
                    state: UpdateState::Unknown,
                    unknown_reason: Some(UpdateUnknownReason::NotChecked),
                    cached: false,
                    last_success_at_epoch_ms,
                    previous_state: previous,
                    error: None,
                    note: Some(note.clone()),
                }
            })
            .collect();
        GroupOutcome {
            source: UpdateSourceResult {
                source: group.source,
                reference: group.reference,
                status: UpdateSourceStatus::Skipped,
                skill_count: group.targets.len(),
                cached: false,
                truncated: cached.is_some_and(|cached| cached.tree.truncated),
                last_success_at_epoch_ms,
                error: None,
                note: Some(note),
            },
            skills,
            warnings: Vec::new(),
        }
    }

    fn timeout_outcome(&self, group: Group, cached: Option<&CachedSourceTree>) -> GroupOutcome {
        let error = AppError::network_error("检查更新超时，已停止本次网络请求。");
        let last_success_at_epoch_ms = cached.map(|cached| cached.fetched_at_epoch_ms);
        let skills = group
            .targets
            .iter()
            .map(|target| {
                let previous = cached
                    .as_ref()
                    .map(|cached| evaluate(target, &cached.tree).state);
                UpdateSkillResult {
                    name: target.name.clone(),
                    source: Some(target.source.clone()),
                    reference: target.reference.clone(),
                    lock_hash: Some(target.lock_hash.clone()),
                    remote_hash: None,
                    state: UpdateState::Error,
                    unknown_reason: None,
                    cached: false,
                    last_success_at_epoch_ms,
                    previous_state: previous,
                    error: Some(error.clone()),
                    note: last_success_at_epoch_ms.map(|_| "已保留上次成功检查结果。".to_string()),
                }
            })
            .collect();
        GroupOutcome {
            source: UpdateSourceResult {
                source: group.source,
                reference: group.reference,
                status: UpdateSourceStatus::Error,
                skill_count: group.targets.len(),
                cached: false,
                truncated: cached.is_some_and(|cached| cached.tree.truncated),
                last_success_at_epoch_ms,
                error: Some(error),
                note: None,
            },
            skills,
            warnings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct GroupKey {
    source: String,
    reference: Option<String>,
}

#[derive(Debug, Clone)]
struct Group {
    source: String,
    reference: Option<String>,
    targets: Vec<Target>,
}

impl Group {
    fn is_content(&self) -> bool {
        self.targets
            .first()
            .is_some_and(|target| content_hash::is_content_hash(&target.lock_hash))
    }
}

#[derive(Debug, Clone)]
struct Target {
    name: String,
    source: String,
    reference: Option<String>,
    skill_path: String,
    lock_hash: String,
}

struct GlobalGroupExecutor<'a> {
    service: &'a UpdateService,
    archives: SourceArchives,
}

#[async_trait::async_trait]
impl GroupExecutor for GlobalGroupExecutor<'_> {
    type Group = (Group, Option<CachedSourceTree>);

    async fn check(&self, (group, cached): &Self::Group) -> GroupOutcome {
        if group.is_content() {
            self.service
                .content_outcome(group.clone(), &self.archives)
                .await
        } else {
            self.service
                .network_outcome(group.clone(), cached.clone())
                .await
        }
    }

    fn skipped(&self, (group, cached): &Self::Group, max_sources: usize) -> GroupOutcome {
        self.service
            .skipped_outcome(group.clone(), cached.as_ref(), max_sources)
    }

    fn timed_out(&self, (group, cached): &Self::Group) -> GroupOutcome {
        self.service.timeout_outcome(group.clone(), cached.as_ref())
    }
}

struct Evaluation {
    state: UpdateState,
    remote_hash: Option<String>,
    note: Option<String>,
}

fn checkable_target(entry: &LockEntry) -> Result<Target, String> {
    if !crate::update_target::supported_type(&entry.source_type) {
        return Err("仅支持公开 HTTPS Git 来源，无法检查更新。".to_string());
    }
    let hash = entry.skill_folder_hash.trim();
    if hash.is_empty() {
        return Err("未记录版本指纹，无法检查更新。".to_string());
    }
    if !entry.source_type.eq_ignore_ascii_case("github") {
        if !content_hash::is_content_hash(hash) {
            return Err("版本指纹不是 64 位 SHA-256 内容指纹，无法比较。".to_string());
        }
    } else if !crate::git_tree::is_git_tree_hash(hash) {
        if content_hash::is_content_hash(hash) {
            return Err(
                "记录的是 64 位内容指纹（SHA-256），无法与 Git tree SHA 比较。".to_string(),
            );
        }
        return Err("版本指纹不是 40 位 Git tree SHA，无法比较。".to_string());
    }
    let Some(skill_path) = entry
        .skill_path
        .as_deref()
        .and_then(github::normalize_tree_path)
    else {
        return Err("lock 未记录有效的 skillPath。".to_string());
    };
    let target = crate::update_target::resolve(entry).map_err(|error| match error {
        crate::update_target::UpdateTargetError::Source { .. } => {
            "来源不是公开 HTTPS Git 仓库，无法检查更新。".to_string()
        }
        crate::update_target::UpdateTargetError::Reference { .. } => {
            "lock ref 不是安全的分支/标签名，无法检查更新。".to_string()
        }
    })?;
    Ok(Target {
        name: target.name,
        source: target.source,
        reference: target.reference,
        skill_path,
        lock_hash: hash.to_string(),
    })
}

fn uncheckable_result(entry: &LockEntry, note: String) -> UpdateSkillResult {
    UpdateSkillResult {
        name: entry.name.clone(),
        source: (!entry.source.is_empty()).then(|| entry.source.clone()),
        reference: entry.reference.clone(),
        lock_hash: (!entry.skill_folder_hash.is_empty()).then(|| entry.skill_folder_hash.clone()),
        remote_hash: None,
        state: UpdateState::Unknown,
        unknown_reason: Some(UpdateUnknownReason::Uncheckable),
        cached: false,
        last_success_at_epoch_ms: None,
        previous_state: None,
        error: None,
        note: Some(note),
    }
}

fn evaluate(target: &Target, tree: &RemoteTree) -> Evaluation {
    if tree.truncated {
        return Evaluation {
            state: UpdateState::Unknown,
            remote_hash: None,
            note: Some("GitHub tree 响应被截断，无法可靠比较。".to_string()),
        };
    }
    let Some(entry) = tree.entry(&target.skill_path) else {
        return Evaluation {
            state: UpdateState::PathMissing,
            remote_hash: None,
            note: Some("来源中已找不到记录的 skillPath，可能已被删除或移动。".to_string()),
        };
    };
    let folder = if entry.kind == "tree" {
        target.skill_path.clone()
    } else {
        parent_path(&target.skill_path)
    };
    match tree.folder_tree_sha(&folder) {
        Some(remote_hash) => {
            let state = if remote_hash.eq_ignore_ascii_case(&target.lock_hash) {
                UpdateState::Current
            } else {
                UpdateState::Update
            };
            Evaluation {
                state,
                remote_hash: Some(remote_hash),
                note: None,
            }
        }
        None if !folder.is_empty() && !tree.has_tree(&folder) => Evaluation {
            state: UpdateState::PathMissing,
            remote_hash: None,
            note: Some("来源中已找不到 skill 文件夹，可能已被删除或移动。".to_string()),
        },
        None => Evaluation {
            state: UpdateState::Unknown,
            remote_hash: None,
            note: Some("无法计算目标文件夹的 tree SHA，无法比较。".to_string()),
        },
    }
}

fn success_result(
    target: &Target,
    tree: &RemoteTree,
    checked_at_epoch_ms: u64,
    cached: bool,
    extra_note: Option<String>,
) -> UpdateSkillResult {
    let evaluation = evaluate(target, tree);
    UpdateSkillResult {
        name: target.name.clone(),
        source: Some(target.source.clone()),
        reference: target.reference.clone(),
        lock_hash: Some(target.lock_hash.clone()),
        remote_hash: evaluation.remote_hash,
        state: evaluation.state,
        unknown_reason: (evaluation.state == UpdateState::Unknown)
            .then_some(UpdateUnknownReason::Uncheckable),
        cached,
        last_success_at_epoch_ms: Some(checked_at_epoch_ms),
        previous_state: None,
        error: None,
        note: evaluation.note.or(extra_note),
    }
}

fn parent_path(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use crate::process::fake::ScriptedExecutor;
    use crate::update::github::GitHubFetchError;
    use crate::update::state::{UPDATE_CHECK_STATE_ENTRY_VERSION, UpdateCheckOutcome};
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::time::Duration;

    const ROOT: &str = "1aee802d9139d4df84d017de44fb1b37a7b614e9";
    const SKILLS_TREE: &str = "12b71a9df5501f6747107c154020a4026ad6d014";
    const ALPHA_TREE: &str = "55ce0eaec6cf25cf0b4752dd1048448eabb1eb31";
    const SUB_TREE: &str = "640ba63b141ba35b527fee65e78140148c8d0a5b";
    const SKILL_MD: &str = "882a458e7b47d67722f6aeccb51f16b1842f1a19";
    const REFERENCES: &str = "7e2b6439aebf0bb975796f691b3b227d0af43bb5";
    const NOTE: &str = "ce013625030ba8dba906f756967f9e9ca394464a";
    const ZERO_HASH: &str = "0000000000000000000000000000000000000000";
    const SHA256_HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn fixture_tree() -> RemoteTree {
        use github::RemoteTreeEntry;
        let mut entries = BTreeMap::new();
        entries.insert(
            "SKILL.md".to_string(),
            RemoteTreeEntry::new("blob", SKILL_MD, Some("100644")),
        );
        entries.insert(
            "skills".to_string(),
            RemoteTreeEntry::new("tree", SKILLS_TREE, Some("040000")),
        );
        entries.insert(
            "skills/alpha".to_string(),
            RemoteTreeEntry::new("tree", ALPHA_TREE, Some("040000")),
        );
        entries.insert(
            "skills/alpha/SKILL.md".to_string(),
            RemoteTreeEntry::new("blob", SKILL_MD, Some("100644")),
        );
        entries.insert(
            "skills/alpha/references.md".to_string(),
            RemoteTreeEntry::new("blob", REFERENCES, Some("100644")),
        );
        entries.insert(
            "sub".to_string(),
            RemoteTreeEntry::new("tree", SUB_TREE, Some("040000")),
        );
        entries.insert(
            "sub/note.txt".to_string(),
            RemoteTreeEntry::new("blob", NOTE, Some("100644")),
        );
        RemoteTree::new(ROOT, false, entries)
    }

    struct TestClock(AtomicU64);

    impl TestClock {
        fn new(value: u64) -> Self {
            Self(AtomicU64::new(value))
        }

        fn set(&self, value: u64) {
            self.0.store(value, Ordering::SeqCst);
        }
    }

    impl Clock for TestClock {
        fn now_epoch_ms(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
    }

    type TreeResult = Result<RemoteTree, GitHubFetchError>;
    type SourceKey = (String, Option<String>);

    #[derive(Default)]
    struct FakeGitHub {
        trees: Mutex<HashMap<SourceKey, TreeResult>>,
        calls: Mutex<Vec<SourceKey>>,
        delay: Duration,
        active: AtomicUsize,
        max_active: AtomicUsize,
    }

    impl FakeGitHub {
        fn with_tree(self, source: &str, reference: Option<&str>, tree: RemoteTree) -> Self {
            self.trees.lock().unwrap().insert(
                (source.to_string(), reference.map(str::to_string)),
                Ok(tree),
            );
            self
        }

        fn with_error(
            self,
            source: &str,
            reference: Option<&str>,
            error: GitHubFetchError,
        ) -> Self {
            self.trees.lock().unwrap().insert(
                (source.to_string(), reference.map(str::to_string)),
                Err(error),
            );
            self
        }

        fn with_delay(mut self, delay: Duration) -> Self {
            self.delay = delay;
            self
        }

        fn set_tree(&self, source: &str, reference: Option<&str>, tree: RemoteTree) {
            self.trees.lock().unwrap().insert(
                (source.to_string(), reference.map(str::to_string)),
                Ok(tree),
            );
        }

        fn set_error(&self, source: &str, reference: Option<&str>, error: GitHubFetchError) {
            self.trees.lock().unwrap().insert(
                (source.to_string(), reference.map(str::to_string)),
                Err(error),
            );
        }

        fn calls(&self) -> Vec<SourceKey> {
            self.calls.lock().unwrap().clone()
        }

        fn reset_calls(&self) {
            self.calls.lock().unwrap().clear();
        }

        fn max_active(&self) -> usize {
            self.max_active.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl GitHubTreeClient for FakeGitHub {
        async fn fetch_tree(
            &self,
            source: &str,
            reference: Option<&str>,
        ) -> Result<RemoteTree, GitHubFetchError> {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(active, Ordering::SeqCst);
            self.calls
                .lock()
                .unwrap()
                .push((source.to_string(), reference.map(str::to_string)));
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            self.active.fetch_sub(1, Ordering::SeqCst);
            self.trees
                .lock()
                .unwrap()
                .get(&(source.to_string(), reference.map(str::to_string)))
                .cloned()
                .unwrap_or_else(|| Err(GitHubFetchError::http(404, "unscripted source")))
        }
    }

    struct Fixture {
        _tmp: tempfile::TempDir,
        paths: AppPaths,
    }

    impl Fixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path().join("home");
            std::fs::create_dir_all(home.join(".agents")).unwrap();
            let paths = AppPaths::new(home, tmp.path().join("app-data"));
            Self { _tmp: tmp, paths }
        }

        fn write_entries(&self, entries: &[LockSpec]) {
            let mut skills = serde_json::Map::new();
            for spec in entries {
                skills.insert(spec.name.to_string(), spec.to_json());
            }
            let lock = serde_json::json!({"version": 3, "skills": skills});
            std::fs::write(self.paths.lock_path(), lock.to_string()).unwrap();
        }

        fn service(
            &self,
            client: Arc<FakeGitHub>,
            clock: Arc<TestClock>,
            config: UpdateCheckConfig,
        ) -> UpdateService {
            UpdateService::with_dependencies(self.paths.clone(), client, clock, config)
        }
    }

    struct ContentClient {
        commit_calls: AtomicUsize,
        download_calls: AtomicUsize,
        fail: std::sync::atomic::AtomicBool,
        delay: Duration,
    }

    impl Default for ContentClient {
        fn default() -> Self {
            Self {
                commit_calls: AtomicUsize::new(0),
                download_calls: AtomicUsize::new(0),
                fail: std::sync::atomic::AtomicBool::new(false),
                delay: Duration::ZERO,
            }
        }
    }

    #[async_trait::async_trait]
    impl ArchiveClient for ContentClient {
        fn cache_namespace(&self, _source: &str) -> &'static str {
            "git"
        }
        async fn latest_commit(
            &self,
            _source: &str,
            _reference: Option<&str>,
        ) -> Result<String, GitHubFetchError> {
            self.commit_calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(self.delay).await;
            if self.fail.load(Ordering::SeqCst) {
                Err(GitHubFetchError::git("upstream unavailable"))
            } else {
                Ok("0123456789abcdef0123456789abcdef01234567".into())
            }
        }
        async fn download_archive(
            &self,
            _source: &str,
            _commit: &str,
        ) -> Result<Vec<u8>, GitHubFetchError> {
            self.download_calls.fetch_add(1, Ordering::SeqCst);
            let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
                Vec::new(),
                flate2::Compression::default(),
            ));
            for (path, content) in [
                ("root/current/SKILL.md", b"current".as_slice()),
                ("root/changed/SKILL.md", b"changed".as_slice()),
            ] {
                let mut header = tar::Header::new_gnu();
                header.set_size(content.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                tar.append_data(&mut header, path, content).unwrap();
            }
            Ok(tar.into_inner().unwrap().finish().unwrap())
        }
    }

    fn git_entry(source: &str, path: &str, hash: &str) -> serde_json::Value {
        serde_json::json!({ "source": source, "sourceUrl": source, "sourceType": "git", "skillPath": path, "skillFolderHash": hash, "installedAt": "2026-01-01T00:00:00.000Z", "updatedAt": "2026-01-01T00:00:00.000Z" })
    }

    #[tokio::test]
    async fn mixed_github_and_git_sources_check_cache_and_isolate_failures() {
        let fixture = Fixture::new();
        let source = "https://cnb.cool/cnb/skills/cnb-skill.git";
        let current_hash =
            content_hash::hash_entries(vec![("SKILL.md".into(), b"current".to_vec())]).unwrap();
        std::fs::write(fixture.paths.lock_path(), serde_json::json!({"version": 3, "skills": {
            "github": LockSpec::github("github", "acme/skills", None, "skills/alpha/SKILL.md", ALPHA_TREE).to_json(),
            "current": git_entry(source, "current/SKILL.md", &current_hash),
            "changed": git_entry(source, "changed/SKILL.md", &current_hash),
            "missing": git_entry(source, "missing/SKILL.md", &current_hash),
        }}).to_string()).unwrap();
        let github = Arc::new(FakeGitHub::default().with_tree("acme/skills", None, fixture_tree()));
        let archive = Arc::new(ContentClient::default());
        let mut service = fixture.service(
            github.clone(),
            Arc::new(TestClock::new(1_000)),
            UpdateCheckConfig::default(),
        );
        service.archive_client = archive.clone();
        let report = service.check(None).await.unwrap();
        assert_eq!(
            report
                .skills
                .iter()
                .find(|s| s.name == "current")
                .unwrap()
                .state,
            UpdateState::Current
        );
        assert_eq!(
            report
                .skills
                .iter()
                .find(|s| s.name == "changed")
                .unwrap()
                .state,
            UpdateState::Update
        );
        assert_eq!(
            report
                .skills
                .iter()
                .find(|s| s.name == "missing")
                .unwrap()
                .state,
            UpdateState::PathMissing
        );
        assert_eq!(report.sources.len(), 2);
        let cached = service.check(None).await.unwrap();
        assert_eq!(cached.cached_source_count, 2);
        assert_eq!(archive.download_calls.load(Ordering::SeqCst), 1);
        archive.fail.store(true, Ordering::SeqCst);
        let failed = service.check(None).await.unwrap();
        assert_eq!(
            failed
                .skills
                .iter()
                .find(|s| s.name == "current")
                .unwrap()
                .state,
            UpdateState::Error
        );
        assert_eq!(
            failed
                .skills
                .iter()
                .find(|s| s.name == "github")
                .unwrap()
                .state,
            UpdateState::Current
        );
    }

    #[tokio::test]
    async fn git_paths_keep_case_and_share_the_source_budget_and_deadline() {
        let fixture = Fixture::new();
        let source_a = "https://git.example.com/Group/Skills.git";
        let source_b = "https://git.example.com/group/skills.git";
        std::fs::write(
            fixture.paths.lock_path(),
            serde_json::json!({"version": 3, "skills": {
                "a": git_entry(source_a, "current/SKILL.md", &"a".repeat(64)),
                "b": git_entry(source_b, "current/SKILL.md", &"a".repeat(64)),
            }})
            .to_string(),
        )
        .unwrap();
        let archive = Arc::new(ContentClient::default());
        let mut service = fixture.service(
            Arc::new(FakeGitHub::default()),
            Arc::new(TestClock::new(1_000)),
            UpdateCheckConfig::default(),
        );
        service.archive_client = archive.clone();
        assert_eq!(service.check(None).await.unwrap().sources.len(), 2);
        service.config.max_sources = 1;
        assert_eq!(service.check(None).await.unwrap().skipped_source_count, 1);
        service.config.deadline_ms = 10;
        service.archive_client = Arc::new(ContentClient {
            delay: Duration::from_millis(100),
            ..ContentClient::default()
        });
        assert_eq!(
            service
                .check(None)
                .await
                .unwrap()
                .sources
                .iter()
                .filter(|s| s.status == UpdateSourceStatus::Error)
                .count(),
            1
        );
    }

    struct LockSpec {
        name: String,
        source: String,
        reference: Option<String>,
        skill_path: String,
        hash: String,
        source_type: String,
    }

    impl LockSpec {
        fn github(
            name: impl Into<String>,
            source: impl Into<String>,
            reference: Option<&str>,
            skill_path: impl Into<String>,
            hash: impl Into<String>,
        ) -> Self {
            Self {
                name: name.into(),
                source: source.into(),
                reference: reference.map(str::to_string),
                skill_path: skill_path.into(),
                hash: hash.into(),
                source_type: "github".to_string(),
            }
        }

        fn non_github(
            name: impl Into<String>,
            source_type: impl Into<String>,
            skill_path: impl Into<String>,
            hash: impl Into<String>,
        ) -> Self {
            Self {
                name: name.into(),
                source: "acme/skills".to_string(),
                reference: None,
                skill_path: skill_path.into(),
                hash: hash.into(),
                source_type: source_type.into(),
            }
        }

        fn to_json(&self) -> serde_json::Value {
            let mut value = serde_json::json!({
                "source": self.source,
                "sourceType": self.source_type,
                "sourceUrl": format!("https://github.com/{}.git", self.source),
                "skillPath": self.skill_path,
                "skillFolderHash": self.hash,
                "installedAt": "2026-01-01T00:00:00.000Z",
                "updatedAt": "2026-02-03T04:05:06.000Z"
            });
            if let Some(reference) = &self.reference {
                value["ref"] = serde_json::Value::String(reference.clone());
            }
            value
        }
    }

    fn state_of(report: &UpdateCheckReport, name: &str) -> UpdateState {
        report
            .skills
            .iter()
            .find(|skill| skill.name == name)
            .unwrap_or_else(|| panic!("missing skill {name}"))
            .state
    }

    fn result_of<'a>(report: &'a UpdateCheckReport, name: &str) -> &'a UpdateSkillResult {
        report
            .skills
            .iter()
            .find(|skill| skill.name == name)
            .unwrap_or_else(|| panic!("missing skill {name}"))
    }

    fn source_of<'a>(report: &'a UpdateCheckReport, source: &str) -> &'a UpdateSourceResult {
        report
            .sources
            .iter()
            .find(|entry| entry.source == source)
            .unwrap_or_else(|| panic!("missing source {source}"))
    }

    #[tokio::test]
    async fn maps_current_update_unknown_path_missing_and_error_states() {
        let fixture = Fixture::new();
        fixture.write_entries(&[
            LockSpec::github(
                "alpha",
                "acme/skills",
                Some("main"),
                "skills/alpha/SKILL.md",
                ALPHA_TREE,
            ),
            LockSpec::github(
                "beta",
                "acme/skills",
                Some("main"),
                "skills/alpha/references.md",
                ALPHA_TREE,
            ),
            LockSpec::github(
                "gamma",
                "acme/skills",
                Some("main"),
                "sub/note.txt",
                ZERO_HASH,
            ),
            LockSpec::github(
                "delta",
                "acme/skills",
                Some("main"),
                "skills/alpha/SKILL.md",
                "",
            ),
            LockSpec::github(
                "epsilon",
                "acme/skills",
                Some("main"),
                "sub/note.txt",
                SHA256_HASH,
            ),
            LockSpec::github(
                "zeta",
                "acme/skills",
                Some("main"),
                "skills/zeta/SKILL.md",
                ZERO_HASH,
            ),
            LockSpec::github(
                "iota",
                "acme/skills",
                Some("main"),
                "skills/alpha/MISSING.md",
                ALPHA_TREE,
            ),
            LockSpec::github("root", "acme/skills", Some("main"), "SKILL.md", ROOT),
            LockSpec::github(
                "eta",
                "offline/repo",
                None,
                "skills/eta/SKILL.md",
                ZERO_HASH,
            ),
        ]);
        let clock = Arc::new(TestClock::new(1_000_000));
        let client = Arc::new(
            FakeGitHub::default()
                .with_tree("acme/skills", Some("main"), fixture_tree())
                .with_error(
                    "offline/repo",
                    None,
                    GitHubFetchError::offline("connection refused"),
                ),
        );
        let service = fixture.service(Arc::clone(&client), clock, UpdateCheckConfig::default());

        let report = service.check(None).await.unwrap();

        assert_eq!(state_of(&report, "alpha"), UpdateState::Current);
        assert_eq!(state_of(&report, "beta"), UpdateState::Current);
        assert_eq!(state_of(&report, "gamma"), UpdateState::Update);
        assert_eq!(state_of(&report, "delta"), UpdateState::Unknown);
        assert_eq!(state_of(&report, "epsilon"), UpdateState::Unknown);
        assert_eq!(state_of(&report, "zeta"), UpdateState::PathMissing);
        assert_eq!(state_of(&report, "iota"), UpdateState::PathMissing);
        assert_eq!(state_of(&report, "root"), UpdateState::Current);
        assert_eq!(state_of(&report, "eta"), UpdateState::Error);

        assert_eq!(
            result_of(&report, "gamma").remote_hash.as_deref(),
            Some(SUB_TREE)
        );
        assert_eq!(result_of(&report, "delta").remote_hash, None);
        assert!(
            result_of(&report, "delta")
                .note
                .as_deref()
                .unwrap()
                .contains("未记录")
        );
        assert!(
            result_of(&report, "epsilon")
                .note
                .as_deref()
                .unwrap()
                .contains("64")
        );
        assert!(
            result_of(&report, "zeta")
                .note
                .as_deref()
                .unwrap()
                .contains("找不到")
        );
        let offline = result_of(&report, "eta").error.as_ref().unwrap();
        assert_eq!(offline.code, ErrorCode::NetworkOffline);
        assert_eq!(result_of(&report, "eta").previous_state, None);
        assert_eq!(result_of(&report, "eta").last_success_at_epoch_ms, None);

        assert_eq!(
            source_of(&report, "acme/skills").status,
            UpdateSourceStatus::Ok
        );
        assert_eq!(source_of(&report, "acme/skills").skill_count, 6);
        assert_eq!(
            source_of(&report, "offline/repo").status,
            UpdateSourceStatus::Error
        );
        assert_eq!(
            source_of(&report, "offline/repo")
                .error
                .as_ref()
                .unwrap()
                .code,
            ErrorCode::NetworkOffline
        );

        // One request per (source, ref), not per skill.
        assert_eq!(client.calls().len(), 2);
        assert_eq!(report.checked_at_epoch_ms, 1_000_000);
        assert_eq!(report.last_success_at_epoch_ms, Some(1_000_000));
        assert_eq!(report.skipped_source_count, 0);
        assert_eq!(report.cached_source_count, 0);
    }

    #[tokio::test]
    async fn groups_by_source_and_ref_not_by_source_alone() {
        let fixture = Fixture::new();
        fixture.write_entries(&[
            LockSpec::github(
                "alpha",
                "acme/skills",
                Some("main"),
                "skills/alpha/SKILL.md",
                ALPHA_TREE,
            ),
            LockSpec::github(
                "beta",
                "acme/skills",
                Some("dev"),
                "skills/alpha/SKILL.md",
                ALPHA_TREE,
            ),
        ]);
        let mut dev_tree = fixture_tree();
        dev_tree.truncated = true;
        let client = Arc::new(
            FakeGitHub::default()
                .with_tree("acme/skills", Some("main"), fixture_tree())
                .with_tree("acme/skills", Some("dev"), dev_tree),
        );
        let service = fixture.service(
            Arc::clone(&client),
            Arc::new(TestClock::new(1_000)),
            UpdateCheckConfig::default(),
        );

        let report = service.check(None).await.unwrap();
        assert_eq!(state_of(&report, "alpha"), UpdateState::Current);
        assert_eq!(state_of(&report, "beta"), UpdateState::Unknown);
        assert_eq!(client.calls().len(), 2);
        let refs: BTreeSet<_> = report
            .sources
            .iter()
            .map(|source| source.reference.clone())
            .collect();
        assert_eq!(
            refs,
            BTreeSet::from([Some("main".to_string()), Some("dev".to_string())])
        );
    }

    #[tokio::test]
    async fn fresh_cache_skips_network_until_the_six_hour_ttl_expires() {
        let fixture = Fixture::new();
        fixture.write_entries(&[LockSpec::github(
            "alpha",
            "acme/skills",
            Some("main"),
            "skills/alpha/SKILL.md",
            ALPHA_TREE,
        )]);
        let clock = Arc::new(TestClock::new(1_000_000));
        let client =
            Arc::new(FakeGitHub::default().with_tree("acme/skills", Some("main"), fixture_tree()));
        let service = fixture.service(
            Arc::clone(&client),
            Arc::clone(&clock),
            UpdateCheckConfig::default(),
        );

        let first = service.check(None).await.unwrap();
        assert_eq!(first.sources[0].status, UpdateSourceStatus::Ok);
        assert!(!first.skills[0].cached);
        assert_eq!(client.calls().len(), 1);

        client.reset_calls();
        clock.set(1_000_000 + UPDATE_CHECK_CACHE_TTL_MS - 1);
        let second = service.check(None).await.unwrap();
        assert!(
            client.calls().is_empty(),
            "fresh cache must not hit the network"
        );
        assert_eq!(second.sources[0].status, UpdateSourceStatus::Cached);
        assert!(second.skills[0].cached);
        assert_eq!(second.skills[0].last_success_at_epoch_ms, Some(1_000_000));
        assert_eq!(second.last_success_at_epoch_ms, Some(1_000_000));
        assert_eq!(second.cached_source_count, 1);
        let state = service.read_state(&crate::scope::ScopeId::Global).unwrap();
        assert_eq!(
            state.last_attempt_at,
            1_000_000 + UPDATE_CHECK_CACHE_TTL_MS - 1
        );
        assert_eq!(state.last_success_at, Some(1_000_000));
        assert_eq!(state.last_outcome, UpdateCheckOutcome::Ok);
        assert_eq!(state.cached_source_count, 1);

        clock.set(1_000_000 + UPDATE_CHECK_CACHE_TTL_MS);
        let third = service.check(None).await.unwrap();
        assert_eq!(client.calls().len(), 1);
        assert_eq!(third.sources[0].status, UpdateSourceStatus::Ok);
        assert_eq!(
            third.skills[0].last_success_at_epoch_ms,
            Some(1_000_000 + UPDATE_CHECK_CACHE_TTL_MS)
        );
    }

    #[tokio::test]
    async fn fresh_tree_cache_does_not_consume_the_source_budget() {
        let fixture = Fixture::new();
        fixture.write_entries(&[
            LockSpec::github(
                "alpha",
                "a/skills",
                None,
                "skills/alpha/SKILL.md",
                ALPHA_TREE,
            ),
            LockSpec::github(
                "beta",
                "b/skills",
                None,
                "skills/alpha/SKILL.md",
                ALPHA_TREE,
            ),
            LockSpec::github(
                "gamma",
                "c/skills",
                None,
                "skills/alpha/SKILL.md",
                ALPHA_TREE,
            ),
        ]);
        let clock = Arc::new(TestClock::new(1_000));
        let client = Arc::new(
            FakeGitHub::default()
                .with_tree("a/skills", None, fixture_tree())
                .with_tree("b/skills", None, fixture_tree()),
        );
        let mut service = fixture.service(
            Arc::clone(&client),
            Arc::clone(&clock),
            UpdateCheckConfig {
                cache_ttl_ms: 1_000,
                ..UpdateCheckConfig::default()
            },
        );
        service.check(Some(vec!["alpha".into()])).await.unwrap();
        service
            .cache
            .store("b/skills", None, &fixture_tree(), 0)
            .unwrap();
        client.reset_calls();
        clock.set(1_100);
        service.config.max_sources = 0;

        let report = service.check(None).await.unwrap();
        assert!(client.calls().is_empty());
        assert_eq!(report.checked_at_epoch_ms, 1_100);
        assert_eq!(report.cached_source_count, 1);
        assert_eq!(report.skipped_source_count, 2);
        assert_eq!(report.last_success_at_epoch_ms, Some(1_000));
        assert_eq!(state_of(&report, "alpha"), UpdateState::Current);
        assert_eq!(
            result_of(&report, "alpha").last_success_at_epoch_ms,
            Some(1_000)
        );
        assert_eq!(
            source_of(&report, "b/skills").status,
            UpdateSourceStatus::Skipped
        );
        assert_eq!(
            result_of(&report, "beta").previous_state,
            Some(UpdateState::Current)
        );
        assert_eq!(result_of(&report, "beta").last_success_at_epoch_ms, Some(0));
        assert_eq!(result_of(&report, "gamma").previous_state, None);

        service.config.max_sources = 1;
        let report = service.check(None).await.unwrap();
        assert_eq!(client.calls(), vec![("b/skills".to_string(), None)]);
        assert_eq!(report.cached_source_count, 1);
        assert_eq!(report.skipped_source_count, 1);
        assert_eq!(
            source_of(&report, "b/skills").status,
            UpdateSourceStatus::Ok
        );
        assert_eq!(
            source_of(&report, "c/skills").status,
            UpdateSourceStatus::Skipped
        );
    }

    #[tokio::test]
    async fn failures_keep_the_previous_result_and_a_retry_recovers() {
        let fixture = Fixture::new();
        fixture.write_entries(&[
            LockSpec::github(
                "alpha",
                "acme/skills",
                Some("main"),
                "skills/alpha/SKILL.md",
                ALPHA_TREE,
            ),
            LockSpec::github(
                "beta",
                "acme/skills",
                Some("main"),
                "sub/note.txt",
                ZERO_HASH,
            ),
        ]);
        let clock = Arc::new(TestClock::new(1_000_000));
        let client =
            Arc::new(FakeGitHub::default().with_tree("acme/skills", Some("main"), fixture_tree()));
        let service = fixture.service(
            Arc::clone(&client),
            Arc::clone(&clock),
            UpdateCheckConfig::default(),
        );

        let first = service.check(None).await.unwrap();
        assert_eq!(state_of(&first, "alpha"), UpdateState::Current);
        assert_eq!(state_of(&first, "beta"), UpdateState::Update);

        clock.set(1_000_000 + UPDATE_CHECK_CACHE_TTL_MS + 1);
        client.set_error(
            "acme/skills",
            Some("main"),
            GitHubFetchError::rate_limited("HTTP 429"),
        );
        let failed = service.check(None).await.unwrap();
        assert_eq!(state_of(&failed, "alpha"), UpdateState::Error);
        assert_eq!(state_of(&failed, "beta"), UpdateState::Error);
        assert_eq!(
            result_of(&failed, "alpha").previous_state,
            Some(UpdateState::Current)
        );
        assert_eq!(
            result_of(&failed, "beta").previous_state,
            Some(UpdateState::Update)
        );
        assert_eq!(
            result_of(&failed, "alpha").last_success_at_epoch_ms,
            Some(1_000_000)
        );
        assert_eq!(
            result_of(&failed, "alpha").error.as_ref().unwrap().code,
            ErrorCode::NetworkRateLimited
        );
        assert_eq!(failed.sources[0].status, UpdateSourceStatus::Error);
        // A failed check must never surface as up to date.
        assert_ne!(state_of(&failed, "alpha"), UpdateState::Current);

        client.set_tree("acme/skills", Some("main"), fixture_tree());
        let recovered = service.check(None).await.unwrap();
        assert_eq!(state_of(&recovered, "alpha"), UpdateState::Current);
        assert_eq!(state_of(&recovered, "beta"), UpdateState::Update);
        assert!(result_of(&recovered, "alpha").error.is_none());
        assert_eq!(
            result_of(&recovered, "alpha").last_success_at_epoch_ms,
            Some(1_000_000 + UPDATE_CHECK_CACHE_TTL_MS + 1)
        );
    }

    #[tokio::test]
    async fn maps_rate_limits_repository_failures_and_parse_errors() {
        let fixture = Fixture::new();
        fixture.write_entries(&[
            LockSpec::github(
                "alpha",
                "limited/repo",
                None,
                "skills/alpha/SKILL.md",
                ALPHA_TREE,
            ),
            LockSpec::github(
                "beta",
                "missing/repo",
                None,
                "skills/beta/SKILL.md",
                ALPHA_TREE,
            ),
            LockSpec::github(
                "gamma",
                "broken/repo",
                None,
                "skills/gamma/SKILL.md",
                ALPHA_TREE,
            ),
        ]);
        let client = Arc::new(
            FakeGitHub::default()
                .with_error(
                    "limited/repo",
                    None,
                    GitHubFetchError::rate_limited("HTTP 429"),
                )
                .with_error(
                    "missing/repo",
                    None,
                    GitHubFetchError::http(404, "Not Found"),
                )
                .with_error("broken/repo", None, GitHubFetchError::parse("bad json")),
        );
        let service = fixture.service(
            client,
            Arc::new(TestClock::new(1_000)),
            UpdateCheckConfig::default(),
        );

        let report = service.check(None).await.unwrap();
        assert_eq!(state_of(&report, "alpha"), UpdateState::Error);
        assert_eq!(state_of(&report, "beta"), UpdateState::Error);
        assert_eq!(state_of(&report, "gamma"), UpdateState::Error);
        assert_eq!(
            result_of(&report, "alpha").error.as_ref().unwrap().code,
            ErrorCode::NetworkRateLimited
        );
        assert_eq!(
            result_of(&report, "beta").error.as_ref().unwrap().code,
            ErrorCode::NetworkError
        );
        assert_eq!(
            result_of(&report, "gamma").error.as_ref().unwrap().code,
            ErrorCode::NetworkError
        );
        assert_eq!(report.last_success_at_epoch_ms, None);
    }

    #[tokio::test]
    async fn non_github_64_char_empty_and_missing_paths_are_unknown_without_requests() {
        let fixture = Fixture::new();
        fixture.write_entries(&[
            LockSpec::non_github("local", "local", "skills/local/SKILL.md", ALPHA_TREE),
            LockSpec::github(
                "legacy",
                "acme/skills",
                None,
                "skills/legacy/SKILL.md",
                SHA256_HASH,
            ),
            LockSpec::github("empty", "acme/skills", None, "skills/empty/SKILL.md", ""),
            LockSpec::github("no-path", "acme/skills", None, "", ALPHA_TREE),
            LockSpec::github(
                "unsafe-ref",
                "acme/skills",
                Some("--all"),
                "skills/unsafe/SKILL.md",
                ALPHA_TREE,
            ),
        ]);
        let client = Arc::new(FakeGitHub::default());
        let service = fixture.service(
            Arc::clone(&client),
            Arc::new(TestClock::new(1_000)),
            UpdateCheckConfig::default(),
        );

        let report = service.check(None).await.unwrap();
        for name in ["local", "legacy", "empty", "no-path", "unsafe-ref"] {
            assert_eq!(state_of(&report, name), UpdateState::Unknown, "{name}");
            assert!(result_of(&report, name).note.is_some(), "{name}");
        }
        assert!(
            result_of(&report, "legacy")
                .note
                .as_deref()
                .unwrap()
                .contains("64")
        );
        assert!(
            result_of(&report, "unsafe-ref")
                .note
                .as_deref()
                .unwrap()
                .contains("ref")
        );
        assert!(client.calls().is_empty());
        assert!(report.sources.is_empty());
    }

    #[tokio::test]
    async fn truncated_trees_are_unknown_not_current_or_missing() {
        let fixture = Fixture::new();
        fixture.write_entries(&[LockSpec::github(
            "alpha",
            "acme/skills",
            Some("main"),
            "skills/alpha/SKILL.md",
            ALPHA_TREE,
        )]);
        let mut tree = fixture_tree();
        tree.truncated = true;
        let clock = Arc::new(TestClock::new(1_000));
        let service = fixture.service(
            Arc::new(FakeGitHub::default().with_tree("acme/skills", Some("main"), tree)),
            clock,
            UpdateCheckConfig::default(),
        );

        let report = service.check(None).await.unwrap();
        let alpha = result_of(&report, "alpha");
        assert_eq!(alpha.state, UpdateState::Unknown);
        assert!(alpha.note.as_deref().unwrap().contains("截断"));
        assert!(report.sources[0].truncated);
    }

    #[tokio::test]
    async fn skill_ids_scope_the_check_to_the_selected_rows() {
        let fixture = Fixture::new();
        fixture.write_entries(&[
            LockSpec::github(
                "alpha",
                "acme/skills",
                Some("main"),
                "skills/alpha/SKILL.md",
                ALPHA_TREE,
            ),
            LockSpec::github(
                "beta",
                "acme/skills",
                Some("main"),
                "sub/note.txt",
                ZERO_HASH,
            ),
            LockSpec::github(
                "gamma",
                "other/repo",
                None,
                "skills/gamma/SKILL.md",
                ALPHA_TREE,
            ),
        ]);
        let client = Arc::new(
            FakeGitHub::default()
                .with_tree("acme/skills", Some("main"), fixture_tree())
                .with_tree("other/repo", None, fixture_tree()),
        );
        let service = fixture.service(
            Arc::clone(&client),
            Arc::new(TestClock::new(1_000)),
            UpdateCheckConfig::default(),
        );

        let report = service
            .check(Some(vec!["alpha".to_string(), "unknown".to_string()]))
            .await
            .unwrap();
        assert_eq!(report.skills.len(), 1);
        assert_eq!(report.skills[0].name, "alpha");
        assert_eq!(report.sources.len(), 1);
        assert_eq!(report.sources[0].skill_count, 1);
        assert_eq!(client.calls().len(), 1);

        client.reset_calls();
        let empty = service.check(Some(Vec::new())).await.unwrap();
        assert!(empty.skills.is_empty());
        assert!(empty.sources.is_empty());
        assert!(client.calls().is_empty());
    }

    #[tokio::test]
    async fn concurrency_is_capped_and_excess_sources_are_reported_as_skipped() {
        let fixture = Fixture::new();
        let mut entries = Vec::new();
        let mut client = FakeGitHub::default().with_delay(Duration::from_millis(15));
        for index in 0..25 {
            let source = format!("acme/repo-{index:02}");
            entries.push(LockSpec::github(
                format!("skill-{index:02}"),
                source.clone(),
                Some("main"),
                "skills/alpha/SKILL.md",
                ALPHA_TREE,
            ));
            client = client.with_tree(&source, Some("main"), fixture_tree());
        }
        fixture.write_entries(&entries);
        let client = Arc::new(client);
        let config = UpdateCheckConfig {
            max_sources: 20,
            concurrency: 4,
            deadline_ms: 30_000,
            ..UpdateCheckConfig::default()
        };
        let service = fixture.service(Arc::clone(&client), Arc::new(TestClock::new(1_000)), config);

        let report = service.check(None).await.unwrap();
        assert_eq!(client.calls().len(), 20);
        assert!(
            client.max_active() <= 4,
            "requests exceeded concurrency cap: {}",
            client.max_active()
        );
        assert!(
            client.max_active() >= 2,
            "expected concurrent requests, saw {}",
            client.max_active()
        );
        assert_eq!(report.skipped_source_count, 5);
        assert_eq!(report.sources.len(), 25);
        let checked: BTreeSet<String> = client
            .calls()
            .into_iter()
            .map(|(source, _)| source)
            .collect();
        for index in 0..20 {
            assert!(checked.contains(&format!("acme/repo-{index:02}")));
        }
        for index in 20..25 {
            assert!(!checked.contains(&format!("acme/repo-{index:02}")));
        }
        let skipped_states: Vec<_> = report
            .skills
            .iter()
            .filter(|skill| skill.state == UpdateState::Unknown)
            .collect();
        assert_eq!(skipped_states.len(), 5);
        assert!(
            skipped_states
                .iter()
                .all(|skill| skill.note.as_deref().unwrap().contains("上限"))
        );
        assert!(
            skipped_states
                .iter()
                .all(|skill| skill.unknown_reason == Some(UpdateUnknownReason::NotChecked))
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("来源上限"))
        );
        let state = service.read_state(&crate::scope::ScopeId::Global).unwrap();
        assert_eq!(state.last_outcome, UpdateCheckOutcome::Partial);
        assert_eq!(state.failed_source_count, 0);
        assert_eq!(state.skipped_source_count, 5);
    }

    #[tokio::test]
    async fn overall_deadline_turns_pending_sources_into_explicit_errors() {
        let fixture = Fixture::new();
        fixture.write_entries(&[LockSpec::github(
            "alpha",
            "slow/repo",
            None,
            "skills/alpha/SKILL.md",
            ALPHA_TREE,
        )]);
        let client = Arc::new(
            FakeGitHub::default()
                .with_delay(Duration::from_millis(50))
                .with_tree("slow/repo", None, fixture_tree()),
        );
        let config = UpdateCheckConfig {
            deadline_ms: 10,
            ..UpdateCheckConfig::default()
        };
        let service = fixture.service(client, Arc::new(TestClock::new(1_000)), config);

        let report = service.check(None).await.unwrap();
        assert_eq!(state_of(&report, "alpha"), UpdateState::Error);
        assert_eq!(
            result_of(&report, "alpha").error.as_ref().unwrap().code,
            ErrorCode::NetworkError
        );
        assert_eq!(report.sources[0].status, UpdateSourceStatus::Error);
    }

    #[tokio::test]
    async fn checking_updates_never_spawns_the_skills_cli() {
        let fixture = Fixture::new();
        fixture.write_entries(&[LockSpec::github(
            "alpha",
            "acme/skills",
            Some("main"),
            "skills/alpha/SKILL.md",
            ALPHA_TREE,
        )]);
        let client =
            Arc::new(FakeGitHub::default().with_tree("acme/skills", Some("main"), fixture_tree()));
        let service = fixture.service(
            client,
            Arc::new(TestClock::new(1_000)),
            UpdateCheckConfig::default(),
        );
        // Any CLI use would appear here; update checking must remain native.
        let executor = ScriptedExecutor::new().expect_any(
            crate::process::fake::ScriptedResponse::spawn_error("must not run"),
        );

        let report = service.check(None).await.unwrap();
        assert_eq!(state_of(&report, "alpha"), UpdateState::Current);
        assert_eq!(executor.call_count(), 0);
    }

    #[tokio::test]
    async fn full_checks_persist_state_and_scoped_checks_do_not() {
        let fixture = Fixture::new();
        fixture.write_entries(&[
            LockSpec::github(
                "alpha",
                "acme/skills",
                Some("main"),
                "skills/alpha/SKILL.md",
                ALPHA_TREE,
            ),
            LockSpec::github(
                "beta",
                "acme/skills",
                Some("main"),
                "sub/note.txt",
                ALPHA_TREE,
            ),
        ]);
        let clock = Arc::new(TestClock::new(1_000_000));
        let client =
            Arc::new(FakeGitHub::default().with_tree("acme/skills", Some("main"), fixture_tree()));
        let service = fixture.service(
            Arc::clone(&client),
            Arc::clone(&clock),
            UpdateCheckConfig::default(),
        );

        let full = service.check(None).await.unwrap();
        let state = service.read_state(&crate::scope::ScopeId::Global).unwrap();
        assert_eq!(state.version, UPDATE_CHECK_STATE_ENTRY_VERSION);
        assert_eq!(state.last_attempt_at, full.checked_at_epoch_ms);
        assert_eq!(state.last_success_at, full.last_success_at_epoch_ms);
        assert_eq!(state.last_outcome, UpdateCheckOutcome::Ok);
        assert_eq!(state.failed_source_count, 0);
        assert_eq!(state.cached_source_count, 0);
        assert_eq!(state.skipped_source_count, 0);
        assert_eq!(state.candidate_count, full.candidate_count());

        clock.set(1_000_000 + UPDATE_CHECK_CACHE_TTL_MS + 1);
        client.set_error(
            "acme/skills",
            Some("main"),
            GitHubFetchError::offline("connection refused"),
        );
        let scoped = service
            .check(Some(vec!["alpha".to_string()]))
            .await
            .unwrap();
        assert_eq!(scoped.skills.len(), 1);
        assert_eq!(
            service.read_state(&crate::scope::ScopeId::Global),
            Some(state)
        );
    }

    #[tokio::test]
    async fn state_marks_partial_and_failed_outcomes() {
        let partial_fixture = Fixture::new();
        partial_fixture.write_entries(&[
            LockSpec::github(
                "alpha",
                "acme/skills",
                Some("main"),
                "skills/alpha/SKILL.md",
                ALPHA_TREE,
            ),
            LockSpec::github(
                "beta",
                "offline/repo",
                None,
                "skills/beta/SKILL.md",
                ALPHA_TREE,
            ),
        ]);
        let client = Arc::new(
            FakeGitHub::default()
                .with_tree("acme/skills", Some("main"), fixture_tree())
                .with_error(
                    "offline/repo",
                    None,
                    GitHubFetchError::offline("connection refused"),
                ),
        );
        let service = partial_fixture.service(
            client,
            Arc::new(TestClock::new(1_000)),
            UpdateCheckConfig::default(),
        );
        let report = service.check(None).await.unwrap();
        assert_eq!(report.last_success_at_epoch_ms, Some(1_000));
        let state = service.read_state(&crate::scope::ScopeId::Global).unwrap();
        assert_eq!(state.last_outcome, UpdateCheckOutcome::Partial);
        assert_eq!(state.failed_source_count, 1);
        assert_eq!(state.last_success_at, Some(1_000));

        let failed_fixture = Fixture::new();
        failed_fixture.write_entries(&[LockSpec::github(
            "alpha",
            "offline/repo",
            None,
            "skills/alpha/SKILL.md",
            ALPHA_TREE,
        )]);
        let client = Arc::new(FakeGitHub::default().with_error(
            "offline/repo",
            None,
            GitHubFetchError::offline("connection refused"),
        ));
        let service = failed_fixture.service(
            client,
            Arc::new(TestClock::new(2_000)),
            UpdateCheckConfig::default(),
        );
        let report = service.check(None).await.unwrap();
        assert_eq!(report.last_success_at_epoch_ms, None);
        let state = service.read_state(&crate::scope::ScopeId::Global).unwrap();
        assert_eq!(state.last_outcome, UpdateCheckOutcome::Failed);
        assert_eq!(state.failed_source_count, 1);
        assert_eq!(state.last_success_at, None);
    }

    #[tokio::test]
    async fn state_write_failure_adds_a_warning_and_keeps_the_report() {
        let fixture = Fixture::new();
        fixture.write_entries(&[LockSpec::github(
            "alpha",
            "acme/skills",
            Some("main"),
            "skills/alpha/SKILL.md",
            ALPHA_TREE,
        )]);
        let state_path = fixture.paths.update_check_state_path();
        std::fs::create_dir_all(&state_path).unwrap();
        std::fs::write(state_path.join("blocker"), "blocked").unwrap();
        let client =
            Arc::new(FakeGitHub::default().with_tree("acme/skills", Some("main"), fixture_tree()));
        let service = fixture.service(
            client,
            Arc::new(TestClock::new(1_000)),
            UpdateCheckConfig::default(),
        );

        let report = service.check(None).await.unwrap();
        assert_eq!(state_of(&report, "alpha"), UpdateState::Current);
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("无法写入更新检查状态"))
        );
        assert_eq!(service.read_state(&crate::scope::ScopeId::Global), None);
    }

    #[tokio::test]
    async fn report_serializes_the_frontend_contract() {
        let fixture = Fixture::new();
        fixture.write_entries(&[
            LockSpec::github(
                "alpha",
                "acme/skills",
                Some("main"),
                "skills/alpha/SKILL.md",
                ALPHA_TREE,
            ),
            LockSpec::github(
                "zeta",
                "acme/skills",
                Some("main"),
                "skills/zeta/SKILL.md",
                ZERO_HASH,
            ),
        ]);
        let clock = Arc::new(TestClock::new(1_000));
        let service = fixture.service(
            Arc::new(FakeGitHub::default().with_tree("acme/skills", Some("main"), fixture_tree())),
            clock,
            UpdateCheckConfig::default(),
        );

        let report = service.check(None).await.unwrap();
        let json = serde_json::to_value(&report).unwrap();
        let alpha = json["skills"]
            .as_array()
            .unwrap()
            .iter()
            .find(|skill| skill["name"] == "alpha")
            .unwrap();
        assert_eq!(alpha["state"], "current");
        assert_eq!(alpha["lockHash"], ALPHA_TREE);
        assert_eq!(alpha["remoteHash"], ALPHA_TREE);
        assert_eq!(alpha["ref"], "main");
        assert_eq!(alpha["cached"], false);
        assert_eq!(alpha["lastSuccessAtEpochMs"], 1_000);
        assert!(alpha["previousState"].is_null());
        assert!(alpha["error"].is_null());
        assert!(alpha["note"].is_null());
        assert_eq!(json["checkedAtEpochMs"], 1_000);
        assert_eq!(json["lastSuccessAtEpochMs"], 1_000);
        assert_eq!(json["skippedSourceCount"], 0);
        assert_eq!(json["cachedSourceCount"], 0);
        assert_eq!(json["sources"][0]["status"], "ok");
        assert_eq!(json["sources"][0]["skillCount"], 2);

        let zeta = json["skills"]
            .as_array()
            .unwrap()
            .iter()
            .find(|skill| skill["name"] == "zeta")
            .unwrap();
        assert_eq!(zeta["state"], "path_missing");
        assert!(zeta["note"].as_str().unwrap().contains("找不到"));
    }
}
