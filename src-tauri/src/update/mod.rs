//! Read-only update checking.
//!
//! `check_updates` never runs the skills CLI. It reads lock v3, groups entries
//! by `(source, ref)`, fetches each repository's recursive Git tree once, and
//! compares the lock's 40-char `skillFolderHash` with the folder tree SHA
//! computed from that response. Network failures are isolated per source and
//! keep the previous successful result; they never become "current".

pub mod cache;
pub mod github;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use futures::StreamExt;
use serde::Serialize;

use crate::error::AppError;
use crate::inventory::UpdateState;
use crate::lock::{LockEntry, LockFile};
use crate::paths::AppPaths;
use cache::{CachedSourceTree, UpdateCache, is_fresh};
use github::{GitHubTreeClient, RemoteTree, ReqwestGitHubClient};

pub const UPDATE_CHECK_CACHE_TTL_MS: u64 = 6 * 60 * 60 * 1000;
pub const UPDATE_CHECK_MAX_SOURCES: usize = 20;
pub const UPDATE_CHECK_CONCURRENCY: usize = 4;
pub const UPDATE_CHECK_DEADLINE_MS: u64 = 30_000;

pub trait Clock: Send + Sync {
    fn now_epoch_ms(&self) -> u64;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_epoch_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0)
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

pub struct UpdateService {
    paths: AppPaths,
    client: Arc<dyn GitHubTreeClient>,
    clock: Arc<dyn Clock>,
    cache: UpdateCache,
    config: UpdateCheckConfig,
}

impl UpdateService {
    pub fn new(paths: AppPaths) -> Self {
        Self {
            cache: UpdateCache::new(&paths),
            paths,
            client: Arc::new(ReqwestGitHubClient::new()),
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
        let now = self.clock.now_epoch_ms();
        let selected = skill_ids.map(|ids| ids.into_iter().collect::<BTreeSet<_>>());
        if selected.as_ref().is_some_and(BTreeSet::is_empty) {
            return Ok(UpdateCheckReport {
                skills: Vec::new(),
                sources: Vec::new(),
                checked_at_epoch_ms: now,
                last_success_at_epoch_ms: None,
                skipped_source_count: 0,
                cached_source_count: 0,
                warnings: Vec::new(),
            });
        }

        let (mut skill_results, groups) = self.collect_targets(&lock, selected.as_ref());
        let mut cached_groups = Vec::new();
        let mut network_groups = Vec::new();
        for group in groups.into_values() {
            match self.cache.load(&group.source, group.reference.as_deref()) {
                Some(cached)
                    if is_fresh(cached.fetched_at_epoch_ms, now, self.config.cache_ttl_ms) =>
                {
                    cached_groups.push((group, cached));
                }
                cached => network_groups.push((group, cached)),
            }
        }

        let mut source_results = Vec::new();
        let mut warnings = Vec::new();

        for (group, cached) in cached_groups {
            let outcome = self.cached_outcome(group, &cached);
            warnings.extend(outcome.warnings);
            source_results.push(outcome.source);
            skill_results.extend(outcome.skills);
        }

        let max_sources = self.config.max_sources;
        let mut to_check = Vec::new();
        let mut skipped = Vec::new();
        for (index, group) in network_groups.into_iter().enumerate() {
            if index < max_sources {
                to_check.push(group);
            } else {
                skipped.push(group);
            }
        }

        for (group, cached) in skipped {
            let outcome = self.skipped_outcome(group, cached.as_ref());
            warnings.extend(outcome.warnings);
            source_results.push(outcome.source);
            skill_results.extend(outcome.skills);
        }

        let outcomes = self.run_network(to_check).await;
        for outcome in outcomes {
            warnings.extend(outcome.warnings);
            source_results.push(outcome.source);
            skill_results.extend(outcome.skills);
        }

        skill_results.sort_by(|left, right| left.name.cmp(&right.name));
        source_results.sort_by(|left, right| {
            left.source
                .to_ascii_lowercase()
                .cmp(&right.source.to_ascii_lowercase())
                .then_with(|| left.reference.cmp(&right.reference))
        });

        let skipped_source_count = source_results
            .iter()
            .filter(|source| source.status == UpdateSourceStatus::Skipped)
            .count();
        if skipped_source_count > 0 {
            warnings.push(format!(
                "本次检查达到来源上限（{max_sources} 个），有 {skipped_source_count} 个来源尚未检查。"
            ));
        }
        let cached_source_count = source_results
            .iter()
            .filter(|source| source.status == UpdateSourceStatus::Cached)
            .count();
        let last_success_at_epoch_ms = source_results
            .iter()
            .filter_map(|source| source.last_success_at_epoch_ms)
            .max();
        warnings.dedup();

        Ok(UpdateCheckReport {
            skills: skill_results,
            sources: source_results,
            checked_at_epoch_ms: now,
            last_success_at_epoch_ms,
            skipped_source_count,
            cached_source_count,
            warnings,
        })
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
                        source: target.source.to_ascii_lowercase(),
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

    async fn run_network(
        &self,
        groups: Vec<(Group, Option<CachedSourceTree>)>,
    ) -> Vec<GroupOutcome> {
        if groups.is_empty() {
            return Vec::new();
        }
        let pending: BTreeMap<GroupKey, (Group, Option<CachedSourceTree>)> = groups
            .iter()
            .cloned()
            .map(|(group, cached)| (group.key(), (group, cached)))
            .collect();
        let mut completed = BTreeSet::new();
        let mut outcomes = Vec::new();
        let deadline = tokio::time::Instant::now()
            + std::time::Duration::from_millis(self.config.deadline_ms.max(1));
        let concurrency = self.config.concurrency.max(1);
        let mut stream = futures::stream::iter(groups.into_iter().map(|(group, cached)| {
            let key = group.key();
            async move { (key, self.network_outcome(group, cached).await) }
        }))
        .buffer_unordered(concurrency);

        while let Ok(Some((key, outcome))) = tokio::time::timeout_at(deadline, stream.next()).await
        {
            completed.insert(key);
            outcomes.push(outcome);
        }

        for (key, (group, cached)) in pending {
            if !completed.contains(&key) {
                outcomes.push(self.timeout_outcome(group, cached.as_ref()));
            }
        }
        outcomes
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
                    key: group.key(),
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
                    key: group.key(),
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
            key: group.key(),
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

    fn skipped_outcome(&self, group: Group, cached: Option<&CachedSourceTree>) -> GroupOutcome {
        let note = format!(
            "本次检查达到来源上限（{} 个），该来源尚未检查。",
            self.config.max_sources
        );
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
                    cached: false,
                    last_success_at_epoch_ms,
                    previous_state: previous,
                    error: None,
                    note: Some(note.clone()),
                }
            })
            .collect();
        GroupOutcome {
            key: group.key(),
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
                    cached: false,
                    last_success_at_epoch_ms,
                    previous_state: previous,
                    error: Some(error.clone()),
                    note: last_success_at_epoch_ms.map(|_| "已保留上次成功检查结果。".to_string()),
                }
            })
            .collect();
        GroupOutcome {
            key: group.key(),
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
    fn key(&self) -> GroupKey {
        GroupKey {
            source: self.source.to_ascii_lowercase(),
            reference: self.reference.clone(),
        }
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

struct GroupOutcome {
    #[allow(dead_code)]
    key: GroupKey,
    source: UpdateSourceResult,
    skills: Vec<UpdateSkillResult>,
    warnings: Vec<String>,
}

struct Evaluation {
    state: UpdateState,
    remote_hash: Option<String>,
    note: Option<String>,
}

fn checkable_target(entry: &LockEntry) -> Result<Target, String> {
    if !entry.source_type.eq_ignore_ascii_case("github") {
        return Err("仅支持公开 GitHub 来源，无法检查更新。".to_string());
    }
    let hash = entry.skill_folder_hash.trim();
    if hash.is_empty() {
        return Err("未记录版本指纹，无法检查更新。".to_string());
    }
    if !crate::baseline::is_git_tree_hash(hash) {
        if hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
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
    let Some(source) = normalized_github_source(entry) else {
        return Err("来源不是公开 GitHub 仓库，无法检查更新。".to_string());
    };
    let reference = entry
        .reference
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("HEAD"))
        .map(str::to_string);
    Ok(Target {
        name: entry.name.clone(),
        source,
        reference,
        skill_path,
        lock_hash: hash.to_string(),
    })
}

fn normalized_github_source(entry: &LockEntry) -> Option<String> {
    for candidate in [entry.source.as_str(), entry.source_url.as_str()] {
        if let Ok(spec) = crate::tasks::source::validate_source(candidate) {
            return Some(spec.normalized);
        }
    }
    None
}

fn uncheckable_result(entry: &LockEntry, note: String) -> UpdateSkillResult {
    UpdateSkillResult {
        name: entry.name.clone(),
        source: (!entry.source.is_empty()).then(|| entry.source.clone()),
        reference: entry.reference.clone(),
        lock_hash: (!entry.skill_folder_hash.is_empty()).then(|| entry.skill_folder_hash.clone()),
        remote_hash: None,
        state: UpdateState::Unknown,
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
    use crate::error::codes;
    use crate::process::fake::ScriptedExecutor;
    use crate::update::github::GitHubFetchError;
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
        assert_eq!(offline.code, codes::NETWORK_OFFLINE);
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
            codes::NETWORK_OFFLINE
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
            codes::NETWORK_RATE_LIMITED
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
            codes::NETWORK_RATE_LIMITED
        );
        assert_eq!(
            result_of(&report, "beta").error.as_ref().unwrap().code,
            codes::NETWORK_ERROR
        );
        assert_eq!(
            result_of(&report, "gamma").error.as_ref().unwrap().code,
            codes::NETWORK_ERROR
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
        ]);
        let client = Arc::new(FakeGitHub::default());
        let service = fixture.service(
            Arc::clone(&client),
            Arc::new(TestClock::new(1_000)),
            UpdateCheckConfig::default(),
        );

        let report = service.check(None).await.unwrap();
        for name in ["local", "legacy", "empty", "no-path"] {
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
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("来源上限"))
        );
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
            codes::NETWORK_ERROR
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
