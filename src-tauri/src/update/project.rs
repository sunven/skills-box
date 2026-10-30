//! Project-scope update checking by content fingerprint.
//!
//! Project locks store `computedHash` (SHA-256, see [`content_hash`]) instead
//! of a Git tree SHA, so the global tree path cannot be reused. Entries are
//! grouped by `(source, ref)`, the latest commit per group is resolved, and
//! the codeload archive for each commit is downloaded once, cached by commit,
//! and hashed per skill folder with the CLI-identical algorithm.
//!
//! Non-GitHub entries, unsafe refs and non-64-hex fingerprints are reported as
//! unsupported rows; source failures stay inside their source and every result
//! is persisted per scope.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use futures::StreamExt;
use tokio::sync::Mutex;

use super::archive::{
    ArchiveClient, ArchiveError, ArchiveLimits, CommitArchiveCache, SourceArchive,
};
use super::cache::is_fresh;
use super::content_hash::{self, FolderHashError};
use super::state::{UpdateCheckState, UpdateCheckStateStore};
use super::{
    Clock, SystemClock, UPDATE_CHECK_CACHE_TTL_MS, UPDATE_CHECK_CONCURRENCY,
    UPDATE_CHECK_DEADLINE_MS, UPDATE_CHECK_MAX_SOURCES, UpdateCheckReport, UpdateSourceResult,
    UpdateSourceStatus, UpdateSkillResult, uncheckable_result,
};
use crate::error::AppError;
use crate::inventory::UpdateState;
use crate::lock::LockEntry;
use crate::paths::AppPaths;
use crate::project_lock::ProjectLockFile;
use crate::scope::ScopeId;
use crate::update_target::UpdateTarget;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectCheckConfig {
    pub cache_ttl_ms: u64,
    pub max_sources: usize,
    pub concurrency: usize,
    pub deadline_ms: u64,
    pub limits: ArchiveLimits,
}

impl Default for ProjectCheckConfig {
    fn default() -> Self {
        Self {
            cache_ttl_ms: UPDATE_CHECK_CACHE_TTL_MS,
            max_sources: UPDATE_CHECK_MAX_SOURCES,
            concurrency: UPDATE_CHECK_CONCURRENCY,
            deadline_ms: UPDATE_CHECK_DEADLINE_MS,
            limits: ArchiveLimits::default(),
        }
    }
}

pub struct ProjectUpdateService {
    state: UpdateCheckStateStore,
    client: Arc<dyn ArchiveClient>,
    cache: CommitArchiveCache,
    clock: Arc<dyn Clock>,
    config: ProjectCheckConfig,
}

impl ProjectUpdateService {
    pub fn new(paths: AppPaths) -> Self {
        Self::with_dependencies(
            paths,
            Arc::new(super::archive::ReqwestArchiveClient::new()),
            Arc::new(SystemClock),
            ProjectCheckConfig::default(),
        )
    }

    pub fn with_dependencies(
        paths: AppPaths,
        client: Arc<dyn ArchiveClient>,
        clock: Arc<dyn Clock>,
        config: ProjectCheckConfig,
    ) -> Self {
        Self {
            state: UpdateCheckStateStore::new(&paths),
            cache: CommitArchiveCache::new(&paths),
            client,
            clock,
            config,
        }
    }

    pub fn read_state(&self, scope: &ScopeId) -> Option<UpdateCheckState> {
        self.state.load(scope)
    }

    /// Read-only check of one project root. A missing lock is an empty report
    /// (like an empty global lock); a corrupt lock is the scope's error.
    pub async fn check(
        &self,
        scope: &ScopeId,
        root: &Path,
        skill_ids: Option<Vec<String>>,
    ) -> Result<UpdateCheckReport, AppError> {
        let lock = ProjectLockFile::read(&root.join("skills-lock.json"))?;
        let full_check = skill_ids.is_none();
        let now = self.clock.now_epoch_ms();
        let selected = skill_ids.map(|ids| {
            ids.into_iter()
                .map(|id| {
                    scope
                        .parse_skill_id(&id)
                        .map(str::to_string)
                        .unwrap_or(id)
                })
                .collect::<BTreeSet<_>>()
        });
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

        let (mut skill_results, groups) = collect_targets(&lock, selected.as_ref());
        // Freshness is commit-keyed and the commit is only known after a
        // network lookup, so the cache is consulted inside each group.
        let max_sources = self.config.max_sources;
        let mut to_check = Vec::new();
        let mut skipped = Vec::new();
        for (index, group) in groups.into_values().enumerate() {
            if index < max_sources {
                to_check.push(group);
            } else {
                skipped.push(group);
            }
        }

        let mut source_results = Vec::new();
        let mut warnings = Vec::new();
        for group in skipped {
            let note = format!(
                "本次检查达到来源上限（{max_sources} 个），该来源尚未检查。"
            );
            let skills: Vec<UpdateSkillResult> = group
                .targets
                .iter()
                .map(|target| pending_result(target, UpdateState::Unknown, note.clone()))
                .collect();
            source_results.push(UpdateSourceResult {
                source: group.source,
                reference: group.reference,
                status: UpdateSourceStatus::Skipped,
                skill_count: group.targets.len(),
                cached: false,
                truncated: false,
                last_success_at_epoch_ms: None,
                error: None,
                note: Some(note),
            });
            skill_results.extend(skills);
        }

        let outcomes = self.run_groups(to_check).await;
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

        let mut report = UpdateCheckReport {
            skills: skill_results,
            sources: source_results,
            checked_at_epoch_ms: now,
            last_success_at_epoch_ms,
            skipped_source_count,
            cached_source_count,
            warnings,
        };
        if full_check {
            let state = UpdateCheckState::from_report(&report);
            if let Err(error) = self.state.store(scope, &state) {
                report
                    .warnings
                    .push(format!("无法写入更新检查状态：{error}"));
            }
        }
        Ok(report)
    }

    async fn run_groups(&self, groups: Vec<Group>) -> Vec<GroupOutcome> {
        if groups.is_empty() {
            return Vec::new();
        }
        // One archive per commit, shared by every group that resolves to it.
        let archives: Arc<Mutex<BTreeMap<String, Arc<ArchiveEntry>>>> =
            Arc::new(Mutex::new(BTreeMap::new()));
        let pending: BTreeMap<GroupKey, Group> = groups
            .iter()
            .cloned()
            .map(|group| (group.key(), group))
            .collect();
        let mut completed = BTreeSet::new();
        let mut outcomes = Vec::new();
        let deadline = tokio::time::Instant::now()
            + std::time::Duration::from_millis(self.config.deadline_ms.max(1));
        let concurrency = self.config.concurrency.max(1);
        let mut stream = futures::stream::iter(groups.into_iter().map(|group| {
            let key = group.key();
            let archives = Arc::clone(&archives);
            async move { (key, self.group_outcome(group, archives).await) }
        }))
        .buffer_unordered(concurrency);

        while let Ok(Some((key, outcome))) = tokio::time::timeout_at(deadline, stream.next()).await
        {
            completed.insert(key);
            outcomes.push(outcome);
        }
        for (key, group) in pending {
            if !completed.contains(&key) {
                outcomes.push(self.timeout_outcome(group));
            }
        }
        outcomes
    }

    async fn group_outcome(
        &self,
        group: Group,
        archives: Arc<Mutex<BTreeMap<String, Arc<ArchiveEntry>>>>,
    ) -> GroupOutcome {
        let commit = match self
            .client
            .latest_commit(&group.source, group.reference.as_deref())
            .await
        {
            Ok(commit) => commit,
            Err(error) => {
                return self.failed_outcome(group, error.to_app_error(), None, false);
            }
        };
        let (entry, reused) = {
            let mut entries = archives.lock().await;
            if let Some(entry) = entries.get(&commit) {
                (Arc::clone(entry), true)
            } else {
                let entry = Arc::new(self.load_or_download(&group.source, &commit).await);
                entries.insert(commit.clone(), Arc::clone(&entry));
                (entry, false)
            }
        };
        let cached = reused || entry.cached;
        let archive = match &entry.archive {
            Ok(archive) => archive,
            Err(failure) => {
                return self.failed_outcome(
                    group,
                    failure.error.clone(),
                    failure.previous_fetched_at,
                    false,
                );
            }
        };

        let mut skills = Vec::new();
        for target in &group.targets {
            let evaluation = evaluate_target(target, archive);
            skills.push(UpdateSkillResult {
                name: target.name.clone(),
                source: Some(group.source.clone()),
                reference: group.reference.clone(),
                lock_hash: Some(target.lock_hash.clone()),
                remote_hash: evaluation.remote_hash,
                state: evaluation.state,
                cached,
                last_success_at_epoch_ms: Some(entry.fetched_at_epoch_ms),
                previous_state: None,
                error: None,
                note: evaluation.note,
            });
        }
        GroupOutcome {
            source: UpdateSourceResult {
                source: group.source,
                reference: group.reference,
                status: if cached {
                    UpdateSourceStatus::Cached
                } else {
                    UpdateSourceStatus::Ok
                },
                skill_count: group.targets.len(),
                cached,
                truncated: false,
                last_success_at_epoch_ms: Some(entry.fetched_at_epoch_ms),
                error: None,
                note: None,
            },
            skills,
            warnings: Vec::new(),
        }
    }

    /// Fresh cache hit first; otherwise one download. A stale entry is kept
    /// for "last successful check" reporting when the download fails.
    async fn load_or_download(&self, source: &str, commit: &str) -> ArchiveEntry {
        let previous = self.cache.load(commit, self.config.limits);
        if let Some(cached) = previous.as_ref()
            && is_fresh(
                cached.fetched_at_epoch_ms,
                self.clock.now_epoch_ms(),
                self.config.cache_ttl_ms,
            )
        {
            return ArchiveEntry {
                archive: Ok(Arc::new(cached.archive.clone())),
                fetched_at_epoch_ms: cached.fetched_at_epoch_ms,
                cached: true,
            };
        }
        let previous_fetched_at = previous.as_ref().map(|cached| cached.fetched_at_epoch_ms);
        let now = self.clock.now_epoch_ms();
        let result = async {
            let raw = self
                .client
                .download_archive(source, commit)
                .await
                .map_err(ArchiveError::from)?;
            if raw.len() as u64 > self.config.limits.max_compressed_bytes {
                return Err(ArchiveError::TooLarge {
                    detail: format!("压缩包为 {} 字节", raw.len()),
                });
            }
            let archive = SourceArchive::from_tar_gz(&raw, self.config.limits)?;
            Ok::<_, ArchiveError>((raw, archive))
        }
        .await;
        match result {
            Ok((raw, archive)) => {
                if let Err(error) = self.cache.store(commit, &raw, now) {
                    // A cache write failure must not fail the check; the
                    // archive is already parsed.
                    let _ = error;
                }
                ArchiveEntry {
                    archive: Ok(Arc::new(archive)),
                    fetched_at_epoch_ms: now,
                    cached: false,
                }
            }
            Err(error) => ArchiveEntry {
                archive: Err(ArchiveFailure {
                    error: error.to_app_error(),
                    previous_fetched_at,
                }),
                fetched_at_epoch_ms: previous_fetched_at.unwrap_or(now),
                cached: false,
            },
        }
    }

    fn failed_outcome(
        &self,
        group: Group,
        error: AppError,
        previous_fetched_at: Option<u64>,
        cached: bool,
    ) -> GroupOutcome {
        let note = previous_fetched_at.map(|_| "已保留上次成功检查结果。".to_string());
        let skills = group
            .targets
            .iter()
            .map(|target| UpdateSkillResult {
                name: target.name.clone(),
                source: Some(group.source.clone()),
                reference: group.reference.clone(),
                lock_hash: Some(target.lock_hash.clone()),
                remote_hash: None,
                state: UpdateState::Error,
                cached,
                last_success_at_epoch_ms: previous_fetched_at,
                previous_state: None,
                error: Some(error.clone()),
                note: note.clone(),
            })
            .collect();
        GroupOutcome {
            source: UpdateSourceResult {
                source: group.source,
                reference: group.reference,
                status: UpdateSourceStatus::Error,
                skill_count: group.targets.len(),
                cached,
                truncated: false,
                last_success_at_epoch_ms: previous_fetched_at,
                error: Some(error),
                note: None,
            },
            skills,
            warnings: Vec::new(),
        }
    }

    fn timeout_outcome(&self, group: Group) -> GroupOutcome {
        self.failed_outcome(
            group,
            AppError::network_error("检查更新超时，已停止本次网络请求。"),
            None,
            false,
        )
    }
}

struct ArchiveEntry {
    archive: Result<Arc<SourceArchive>, ArchiveFailure>,
    fetched_at_epoch_ms: u64,
    cached: bool,
}

struct ArchiveFailure {
    error: AppError,
    previous_fetched_at: Option<u64>,
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
    targets: Vec<ProjectTarget>,
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
struct ProjectTarget {
    name: String,
    skill_path: String,
    lock_hash: String,
}

struct GroupOutcome {
    source: UpdateSourceResult,
    skills: Vec<UpdateSkillResult>,
    warnings: Vec<String>,
}

struct Evaluation {
    state: UpdateState,
    remote_hash: Option<String>,
    note: Option<String>,
}

fn collect_targets(
    lock: &ProjectLockFile,
    selected: Option<&BTreeSet<String>>,
) -> (Vec<UpdateSkillResult>, BTreeMap<GroupKey, Group>) {
    let mut base_results = Vec::new();
    let mut groups: BTreeMap<GroupKey, Group> = BTreeMap::new();
    for entry in lock.iter() {
        if selected.is_some_and(|names| !names.contains(&entry.name)) {
            continue;
        }
        match checkable_target(entry) {
            Ok((target, source, reference)) => {
                let key = GroupKey {
                    source: source.to_ascii_lowercase(),
                    reference: reference.clone(),
                };
                groups
                    .entry(key)
                    .or_insert_with(|| Group {
                        source,
                        reference,
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

/// Validation order mirrors the global check: source type, fingerprint domain,
/// skillPath, then the shared source/ref resolver (ADR 0009).
fn checkable_target(
    entry: &LockEntry,
) -> Result<(ProjectTarget, String, Option<String>), String> {
    if !entry.source_type.eq_ignore_ascii_case("github") {
        return Err("仅支持公开 GitHub 来源，无法检查更新。".to_string());
    }
    let hash = entry.skill_folder_hash.trim();
    if hash.is_empty() {
        return Err("未记录版本指纹，无法检查更新。".to_string());
    }
    if !content_hash::is_content_hash(hash) {
        if crate::git_tree::is_git_tree_hash(hash) {
            return Err(
                "记录的是 40 位 Git tree SHA，无法与内容指纹比较。".to_string(),
            );
        }
        return Err("版本指纹不是 64 位 SHA-256 内容指纹，无法比较。".to_string());
    }
    let Some(skill_path) = entry
        .skill_path
        .as_deref()
        .and_then(super::github::normalize_tree_path)
    else {
        return Err("lock 未记录有效的 skillPath。".to_string());
    };
    let target: UpdateTarget = crate::update_target::resolve(entry).map_err(|error| {
        match error {
            crate::update_target::UpdateTargetError::Source { .. } => {
                "来源不是公开 GitHub 仓库，无法检查更新。".to_string()
            }
            crate::update_target::UpdateTargetError::Reference { .. } => {
                "lock ref 不是安全的分支/标签名，无法检查更新。".to_string()
            }
        }
    })?;
    Ok((
        ProjectTarget {
            name: target.name,
            skill_path,
            lock_hash: hash.to_string(),
        },
        target.source,
        target.reference,
    ))
}

fn evaluate_target(target: &ProjectTarget, archive: &SourceArchive) -> Evaluation {
    let folder = if archive.contains_file(&target.skill_path) {
        parent_path(&target.skill_path)
    } else if archive.contains_folder(&target.skill_path) {
        target.skill_path.clone()
    } else {
        return Evaluation {
            state: UpdateState::PathMissing,
            remote_hash: None,
            note: Some("来源中已找不到记录的 skillPath，可能已被删除或移动。".to_string()),
        };
    };
    let entries = archive.files_under(&folder);
    match content_hash::hash_entries(entries) {
        Ok(remote_hash) => {
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
        Err(FolderHashError::UnsupportedPath(detail)) => Evaluation {
            state: UpdateState::Unknown,
            remote_hash: None,
            note: Some(format!(
                "skill 文件夹 {detail}，无法用 CLI 的排序规则计算内容指纹。"
            )),
        },
        Err(FolderHashError::Io(detail)) => Evaluation {
            state: UpdateState::Unknown,
            remote_hash: None,
            note: Some(format!("无法读取来源文件：{detail}")),
        },
    }
}

fn parent_path(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
        .unwrap_or_default()
}

fn pending_result(target: &ProjectTarget, state: UpdateState, note: String) -> UpdateSkillResult {
    UpdateSkillResult {
        name: target.name.clone(),
        source: None,
        reference: None,
        lock_hash: Some(target.lock_hash.clone()),
        remote_hash: None,
        state,
        cached: false,
        last_success_at_epoch_ms: None,
        previous_state: None,
        error: None,
        note: Some(note),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::github::GitHubFetchError;
    use std::collections::HashMap;
    use std::fs;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    const COMMIT_A: &str = "1aee802d9139d4df84d017de44fb1b37a7b614e9";
    const COMMIT_B: &str = "2bff913e024de5ef95e128ee55fc2c48b8c725fa";
    const ALPHA_HASH: &str = "b9d08da8ac776d566beb814c50480e5edfc5422384e327cdc58629d102b491f1";
    const BETA_HASH: &str = "894071b670b3c183fb53284dba3e725204618c4ef085eb7b6208d2961969534e";
    const ALPHA_OLD_HASH: &str =
        "9a669b2c5f46470c263ccd0b72686556915cd9085ad0de4a04d5ca18c9ca0a17";

    struct TestClock(AtomicU64);

    impl TestClock {
        fn new(start: u64) -> Self {
            Self(AtomicU64::new(start))
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

    #[derive(Default)]
    struct FakeArchiveClient {
        commits: HashMap<(String, Option<String>), Result<String, GitHubFetchError>>,
        archives: HashMap<(String, String), Result<Vec<u8>, GitHubFetchError>>,
        commit_calls: Arc<AtomicUsize>,
        download_calls: Arc<AtomicUsize>,
        delay: Option<std::time::Duration>,
    }

    impl FakeArchiveClient {
        fn with_group(
            mut self,
            source: &str,
            reference: Option<&str>,
            commit: &str,
            archive: Vec<u8>,
        ) -> Self {
            self.commits.insert(
                (source.to_string(), reference.map(str::to_string)),
                Ok(commit.to_string()),
            );
            self.archives
                .insert((source.to_string(), commit.to_string()), Ok(archive));
            self
        }

        fn with_commit_error(
            mut self,
            source: &str,
            reference: Option<&str>,
            error: GitHubFetchError,
        ) -> Self {
            self.commits
                .insert((source.to_string(), reference.map(str::to_string)), Err(error));
            self
        }

        fn with_delay(mut self, delay: std::time::Duration) -> Self {
            self.delay = Some(delay);
            self
        }
    }

    #[async_trait::async_trait]
    impl ArchiveClient for FakeArchiveClient {
        async fn latest_commit(
            &self,
            source: &str,
            reference: Option<&str>,
        ) -> Result<String, GitHubFetchError> {
            self.commit_calls.fetch_add(1, Ordering::SeqCst);
            if let Some(delay) = self.delay {
                tokio::time::sleep(delay).await;
            }
            self.commits
                .get(&(source.to_string(), reference.map(str::to_string)))
                .cloned()
                .unwrap_or_else(|| {
                    Err(GitHubFetchError::http(404, "no scripted commit"))
                })
        }

        async fn download_archive(
            &self,
            source: &str,
            commit: &str,
        ) -> Result<Vec<u8>, GitHubFetchError> {
            self.download_calls.fetch_add(1, Ordering::SeqCst);
            self.archives
                .get(&(source.to_string(), commit.to_string()))
                .cloned()
                .unwrap_or_else(|| {
                    Err(GitHubFetchError::http(404, "no scripted archive"))
                })
        }
    }

    fn tar_gz(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use std::io::Write;
        let mut builder = tar::Builder::new(Vec::new());
        for (path, content) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, path, *content).unwrap();
        }
        let tar = builder.into_inner().unwrap();
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&tar).unwrap();
        encoder.finish().unwrap()
    }

    fn project_root() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn write_lock(root: &Path, entries: serde_json::Value) {
        fs::write(
            root.join("skills-lock.json"),
            serde_json::json!({ "version": 1, "skills": entries }).to_string(),
        )
        .unwrap();
    }

    fn entry(source: &str, source_type: &str, skill_path: &str, hash: &str) -> serde_json::Value {
        serde_json::json!({
            "source": source,
            "sourceUrl": format!("https://github.com/{source}.git"),
            "ref": "main",
            "sourceType": source_type,
            "skillPath": skill_path,
            "computedHash": hash
        })
    }

    fn service(
        client: FakeArchiveClient,
        clock: Arc<TestClock>,
        config: ProjectCheckConfig,
    ) -> (ProjectUpdateService, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(tmp.path().join("home"), tmp.path().join("app-data"));
        let service = ProjectUpdateService::with_dependencies(
            paths,
            Arc::new(client),
            clock,
            config,
        );
        (service, tmp)
    }

    fn scoped_archive() -> Vec<u8> {
        tar_gz(&[
            ("acme-skills-abc/alpha/SKILL.md", b"alpha\n"),
            ("acme-skills-abc/alpha/references/guide.md", b"guide\n"),
            ("acme-skills-abc/beta/SKILL.md", b"beta v2\n"),
        ])
    }

    fn scope() -> ScopeId {
        ScopeId::project(uuid::Uuid::parse_str("5a7b6c1d-2e3f-4a5b-8c9d-0e1f2a3b4c5d").unwrap())
    }

    #[tokio::test]
    async fn groups_by_source_and_ref_and_downloads_each_commit_once() {
        let root = project_root();
        write_lock(
            root.path(),
            serde_json::json!({
                "alpha": entry("acme/skills", "github", "alpha/SKILL.md", ALPHA_HASH),
                "alpha-copy": entry("acme/skills", "github", "alpha/SKILL.md", ALPHA_HASH),
                "beta": entry("acme/skills", "github", "beta/SKILL.md", ALPHA_OLD_HASH),
                "local": entry("/tmp/local", "local", "SKILL.md", ALPHA_HASH),
            }),
        );
        let client = FakeArchiveClient::default()
            .with_group("acme/skills", Some("main"), COMMIT_A, scoped_archive());
        let commit_calls = Arc::clone(&client.commit_calls);
        let download_calls = Arc::clone(&client.download_calls);
        let clock = Arc::new(TestClock::new(1_000));
        let (service, _tmp) = service(client, Arc::clone(&clock), ProjectCheckConfig::default());

        let report = service.check(&scope(), root.path(), None).await.unwrap();

        assert_eq!(commit_calls.load(Ordering::SeqCst), 1);
        assert_eq!(download_calls.load(Ordering::SeqCst), 1);
        assert_eq!(report.skills.len(), 4);
        let alpha = report.skills.iter().find(|s| s.name == "alpha").unwrap();
        assert_eq!(alpha.state, UpdateState::Current);
        assert_eq!(alpha.remote_hash.as_deref(), Some(ALPHA_HASH));
        let beta = report.skills.iter().find(|s| s.name == "beta").unwrap();
        assert_eq!(beta.state, UpdateState::Update);
        assert_eq!(beta.remote_hash.as_deref(), Some(BETA_HASH));
        let local = report.skills.iter().find(|s| s.name == "local").unwrap();
        assert_eq!(local.state, UpdateState::Unknown);
        assert!(local.note.as_deref().unwrap().contains("GitHub"));

        // alpha, alpha-copy and beta share (source, ref): one commit lookup,
        // one archive download.
        assert_eq!(report.candidate_count(), 1);
        assert_eq!(report.sources.len(), 1);
        assert_eq!(report.sources[0].skill_count, 3);
        assert_eq!(report.sources[0].status, UpdateSourceStatus::Ok);

        let state = service.read_state(&scope()).unwrap();
        assert_eq!(state.candidate_count, 1);
        assert_eq!(state.last_attempt_at, 1_000);
    }

    #[tokio::test]
    async fn reuses_the_archive_for_groups_that_resolve_to_the_same_commit() {
        let root = project_root();
        let mut dev = entry("acme/skills", "github", "alpha/SKILL.md", ALPHA_HASH);
        dev["ref"] = serde_json::json!("dev");
        write_lock(
            root.path(),
            serde_json::json!({
                "alpha": entry("acme/skills", "github", "alpha/SKILL.md", ALPHA_HASH),
                "alpha-dev": dev,
            }),
        );
        let client = FakeArchiveClient::default()
            .with_group("acme/skills", Some("main"), COMMIT_A, scoped_archive())
            .with_group("acme/skills", Some("dev"), COMMIT_A, scoped_archive());
        let download_calls = Arc::clone(&client.download_calls);
        let clock = Arc::new(TestClock::new(1_000));
        let (service, _tmp) = service(client, clock, ProjectCheckConfig::default());

        let report = service.check(&scope(), root.path(), None).await.unwrap();
        assert_eq!(download_calls.load(Ordering::SeqCst), 1);
        assert_eq!(report.sources.len(), 2);
        // The second group reports the shared commit as cached, but there is
        // exactly one network download for it.
        assert_eq!(report.skills.len(), 2);
        assert!(report.skills.iter().all(|s| s.state == UpdateState::Current));
        let statuses: Vec<_> = report.sources.iter().map(|s| s.status).collect();
        assert!(statuses.contains(&UpdateSourceStatus::Ok));
        assert!(statuses.contains(&UpdateSourceStatus::Cached));
    }

    #[tokio::test]
    async fn a_cached_archive_skips_the_download_and_is_marked_cached() {
        let root = project_root();
        write_lock(
            root.path(),
            serde_json::json!({
                "alpha": entry("acme/skills", "github", "alpha/SKILL.md", ALPHA_HASH),
            }),
        );
        let client = FakeArchiveClient::default()
            .with_group("acme/skills", Some("main"), COMMIT_A, scoped_archive());
        let clock = Arc::new(TestClock::new(1_000));
        let (service, tmp) = service(client, clock, ProjectCheckConfig::default());
        let paths = AppPaths::new(tmp.path().join("home"), tmp.path().join("app-data"));
        CommitArchiveCache::new(&paths)
            .store(COMMIT_A, &scoped_archive(), 900)
            .unwrap();

        let report = service.check(&scope(), root.path(), None).await.unwrap();
        assert_eq!(report.sources[0].status, UpdateSourceStatus::Cached);
        assert_eq!(report.cached_source_count, 1);
        assert!(report.skills[0].cached);
        assert_eq!(report.skills[0].state, UpdateState::Current);
        // Timestamps come from the cache entry, not the check time.
        assert_eq!(report.skills[0].last_success_at_epoch_ms, Some(900));
    }

    #[tokio::test]
    async fn source_failures_are_isolated_and_do_not_persist_over_other_scopes() {
        let root = project_root();
        write_lock(
            root.path(),
            serde_json::json!({
                "alpha": entry("acme/skills", "github", "alpha/SKILL.md", ALPHA_HASH),
                "beta": entry("broken/skills", "github", "SKILL.md", BETA_HASH),
            }),
        );
        let client = FakeArchiveClient::default()
            .with_group("acme/skills", Some("main"), COMMIT_A, scoped_archive())
            .with_commit_error(
                "broken/skills",
                Some("main"),
                GitHubFetchError::offline("connection refused"),
            );
        let clock = Arc::new(TestClock::new(1_000));
        let (service, _tmp) = service(client, clock, ProjectCheckConfig::default());

        let report = service.check(&scope(), root.path(), None).await.unwrap();
        let alpha = report.skills.iter().find(|s| s.name == "alpha").unwrap();
        assert_eq!(alpha.state, UpdateState::Current);
        let beta = report.skills.iter().find(|s| s.name == "beta").unwrap();
        assert_eq!(beta.state, UpdateState::Error);
        assert_eq!(beta.error.as_ref().unwrap().code, crate::error::ErrorCode::NetworkOffline);
        assert_eq!(report.sources.len(), 2);
        let state = service.read_state(&scope()).unwrap();
        use crate::update::state::UpdateCheckOutcome;
        assert_eq!(state.last_outcome, UpdateCheckOutcome::Partial);
        assert_eq!(state.failed_source_count, 1);
        // Another scope is untouched.
        assert!(service.read_state(&ScopeId::Global).is_none());
    }

    #[tokio::test]
    async fn oversized_archives_fail_the_source_with_a_network_error() {
        let root = project_root();
        write_lock(
            root.path(),
            serde_json::json!({
                "alpha": entry("acme/skills", "github", "alpha/SKILL.md", ALPHA_HASH),
            }),
        );
        let client = FakeArchiveClient::default()
            .with_group("acme/skills", Some("main"), COMMIT_A, scoped_archive());
        let clock = Arc::new(TestClock::new(1_000));
        let config = ProjectCheckConfig {
            limits: ArchiveLimits {
                max_file_bytes: 4,
                ..ArchiveLimits::default()
            },
            ..ProjectCheckConfig::default()
        };
        let (service, _tmp) = service(client, clock, config);

        let report = service.check(&scope(), root.path(), None).await.unwrap();
        assert_eq!(report.sources[0].status, UpdateSourceStatus::Error);
        assert_eq!(report.skills[0].state, UpdateState::Error);
        assert!(report.skills[0].note.is_none());
        assert_eq!(
            report.skills[0].error.as_ref().unwrap().code,
            crate::error::ErrorCode::NetworkError
        );
    }

    #[tokio::test]
    async fn scoped_checks_do_not_advance_the_persisted_state() {
        let root = project_root();
        write_lock(
            root.path(),
            serde_json::json!({
                "alpha": entry("acme/skills", "github", "alpha/SKILL.md", ALPHA_HASH),
                "beta": entry("acme/skills", "github", "beta/SKILL.md", ALPHA_OLD_HASH),
            }),
        );
        let client = FakeArchiveClient::default()
            .with_group("acme/skills", Some("main"), COMMIT_A, scoped_archive());
        let clock = Arc::new(TestClock::new(1_000));
        let (service, _tmp) = service(client, clock, ProjectCheckConfig::default());

        let full = service.check(&scope(), root.path(), None).await.unwrap();
        let state = service.read_state(&scope()).unwrap();
        assert_eq!(state.candidate_count, full.candidate_count());

        let scoped = service
            .check(&scope(), root.path(), Some(vec![format!("{}/alpha", scope().as_string())]))
            .await
            .unwrap();
        assert_eq!(scoped.skills.len(), 1);
        assert_eq!(service.read_state(&scope()), Some(state));
    }

    #[tokio::test]
    async fn check_deadline_turns_a_hanging_source_into_a_source_error() {
        let root = project_root();
        write_lock(
            root.path(),
            serde_json::json!({
                "alpha": entry("acme/skills", "github", "alpha/SKILL.md", ALPHA_HASH),
            }),
        );
        let client = FakeArchiveClient::default()
            .with_group("acme/skills", Some("main"), COMMIT_A, scoped_archive())
            .with_delay(std::time::Duration::from_secs(5));
        let clock = Arc::new(TestClock::new(1_000));
        let config = ProjectCheckConfig {
            deadline_ms: 20,
            ..ProjectCheckConfig::default()
        };
        let (service, _tmp) = service(client, clock, config);

        let report = service.check(&scope(), root.path(), None).await.unwrap();
        assert_eq!(report.sources[0].status, UpdateSourceStatus::Error);
        assert_eq!(report.skills[0].state, UpdateState::Error);
        assert_eq!(
            report.skills[0].error.as_ref().unwrap().code,
            crate::error::ErrorCode::NetworkError
        );
        assert!(
            report.skills[0]
                .error
                .as_ref()
                .unwrap()
                .detail
                .as_deref()
                .unwrap_or_default()
                .contains("超时")
        );
    }

    #[tokio::test]
    async fn corrupt_locks_are_the_scope_error_not_an_empty_report() {
        let root = project_root();
        fs::write(root.path().join("skills-lock.json"), "{not json").unwrap();
        let client = FakeArchiveClient::default();
        let clock = Arc::new(TestClock::new(1_000));
        let (service, _tmp) = service(client, clock, ProjectCheckConfig::default());

        let error = service.check(&scope(), root.path(), None).await.unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::LockCorrupt);
        assert!(service.read_state(&scope()).is_none());
    }

}
