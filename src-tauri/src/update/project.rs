//! Project-scope update checking by content fingerprint.
//!
//! Project locks store `computedHash` (SHA-256, see [`content_hash`]) instead
//! of a Git tree SHA, so the global tree path cannot be reused. Entries are
//! grouped by `(source, ref)`, the latest commit per group is resolved, and
//! the codeload archive for each commit is downloaded once, cached by commit,
//! and hashed per skill folder with the CLI-identical algorithm.
//!
//! Unsupported sources, unsafe refs and non-64-hex fingerprints are reported as
//! unsupported rows; source failures stay inside their source. Publication and
//! persisted summaries belong to the Project Update check lifecycle.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use super::archive::{ArchiveClient, ArchiveLimits, CommitArchiveCache, SourceArchives};
use super::attempt::{
    self, GroupExecutor, GroupOutcome, PreparedCheck, PreparedSource, SchedulingLimits,
};
use super::content_check::{ContentTarget as ProjectTarget, evaluate_target};
use super::content_hash;
use super::{
    Clock, SystemClock, UPDATE_CHECK_CACHE_TTL_MS, UPDATE_CHECK_CONCURRENCY,
    UPDATE_CHECK_DEADLINE_MS, UPDATE_CHECK_MAX_SOURCES, UpdateCheckReport, UpdateSkillResult,
    UpdateSourceResult, UpdateSourceStatus, uncheckable_result,
};
use crate::error::AppError;
use crate::inventory::UpdateState;
use crate::lock::LockEntry;
use crate::paths::AppPaths;
use crate::project_lock::ProjectLockFile;
#[cfg(test)]
use crate::scope::ScopeId;
use crate::scope_layout::ScopeLayout;
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
    client: Arc<dyn ArchiveClient>,
    cache: CommitArchiveCache,
    clock: Arc<dyn Clock>,
    config: ProjectCheckConfig,
}

impl ProjectUpdateService {
    pub fn new(paths: AppPaths) -> Self {
        Self::with_api(paths, super::api::GitHubApiClient::anonymous())
    }

    pub fn with_api(paths: AppPaths, api: super::api::GitHubApiClient) -> Self {
        Self::with_dependencies(
            paths.clone(),
            Arc::new(super::archive::SourceArchiveClient::with_api(paths, api)),
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
            cache: CommitArchiveCache::new(&paths),
            client,
            clock,
            config,
        }
    }

    /// Read-only check of one Project scope. A missing lock is an empty report
    /// (like an empty global lock); a corrupt lock is the scope's error.
    pub async fn check(
        &self,
        layout: &ScopeLayout,
        skill_ids: Option<Vec<String>>,
    ) -> Result<UpdateCheckReport, AppError> {
        layout.lock()?;
        let market_records = crate::marketplace::records::Records::read(&layout.lock_path)?;
        let lock = ProjectLockFile::read(&layout.lock_path)?;
        let now = self.clock.now_epoch_ms();
        let selected = skill_ids
            .map(|ids| {
                ids.into_iter()
                    .map(|id| layout.scope.parse_skill_id(&id).map(str::to_string))
                    .collect::<Result<BTreeSet<_>, AppError>>()
            })
            .transpose()?;
        let (skills, groups) = collect_targets(&lock, selected.as_ref());
        // Freshness is commit-keyed and the commit is only known after a
        // network lookup, so every group still needs a source-budget slot.
        let slots = Arc::new(tokio::sync::Semaphore::new(self.config.concurrency.max(1)));
        let market_check = crate::marketplace::updates::check(
            market_records,
            selected.as_ref(),
            self.config.max_sources.saturating_sub(groups.len()),
            self.config.concurrency,
            self.config.deadline_ms,
            &slots,
        );
        let git_check = attempt::run(
            PreparedCheck {
                checked_at_epoch_ms: now,
                skills,
                sources: groups.into_values().map(PreparedSource::Pending).collect(),
            },
            SchedulingLimits {
                max_sources: self.config.max_sources,
                concurrency: self.config.concurrency,
                deadline_ms: self.config.deadline_ms,
            },
            attempt::SharedSlots {
                slots: Arc::clone(&slots),
                executor: ProjectGroupExecutor {
                    service: self,
                    archives: SourceArchives::new(
                        Arc::clone(&self.client),
                        self.cache.clone(),
                        Arc::clone(&self.clock),
                        self.config.cache_ttl_ms,
                        self.config.limits,
                    ),
                },
            },
        );
        let (mut report, market_results) = tokio::join!(git_check, market_check);
        crate::marketplace::updates::merge(&mut report, market_results);
        Ok(report)
    }

    async fn group_outcome(&self, group: Group, archives: &SourceArchives) -> GroupOutcome {
        let acquired = match archives
            .acquire(&group.source, group.reference.as_deref())
            .await
        {
            Ok(acquired) => acquired,
            Err(failure) => {
                return self.failed_outcome(
                    group,
                    failure.error,
                    failure.previous_fetched_at,
                    false,
                );
            }
        };
        let cached = acquired.cached;

        let mut skills = Vec::new();
        for target in &group.targets {
            let evaluation = evaluate_target(target, &acquired.archive);
            skills.push(UpdateSkillResult {
                name: target.name.clone(),
                source: Some(group.source.clone()),
                reference: group.reference.clone(),
                lock_hash: Some(target.lock_hash.clone()),
                remote_hash: evaluation.remote_hash,
                state: evaluation.state,
                unknown_reason: (evaluation.state == UpdateState::Unknown)
                    .then_some(super::UpdateUnknownReason::Uncheckable),
                cached,
                last_success_at_epoch_ms: Some(acquired.fetched_at_epoch_ms),
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
                last_success_at_epoch_ms: Some(acquired.fetched_at_epoch_ms),
                error: None,
                note: None,
            },
            skills,
            warnings: Vec::new(),
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
                unknown_reason: None,
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

struct ProjectGroupExecutor<'a> {
    service: &'a ProjectUpdateService,
    archives: SourceArchives,
}

#[async_trait::async_trait]
impl GroupExecutor for ProjectGroupExecutor<'_> {
    type Group = Group;

    async fn check(&self, group: &Group) -> GroupOutcome {
        self.service
            .group_outcome(group.clone(), &self.archives)
            .await
    }

    fn skipped(&self, group: &Group, max_sources: usize) -> GroupOutcome {
        let note = format!("本次检查达到来源上限（{max_sources} 个），该来源尚未检查。");
        GroupOutcome {
            source: UpdateSourceResult {
                source: group.source.clone(),
                reference: group.reference.clone(),
                status: UpdateSourceStatus::Skipped,
                skill_count: group.targets.len(),
                cached: false,
                truncated: false,
                last_success_at_epoch_ms: None,
                error: None,
                note: Some(note.clone()),
            },
            skills: group
                .targets
                .iter()
                .map(|target| pending_result(target, UpdateState::Unknown, note.clone()))
                .collect(),
            warnings: Vec::new(),
        }
    }

    fn timed_out(&self, group: &Group) -> GroupOutcome {
        self.service.timeout_outcome(group.clone())
    }
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
                    source: crate::tasks::source::source_identity(&source),
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
fn checkable_target(entry: &LockEntry) -> Result<(ProjectTarget, String, Option<String>), String> {
    if !crate::update_target::supported_type(&entry.source_type) {
        return Err("仅支持公开 HTTPS Git 来源，无法检查更新。".to_string());
    }
    let hash = entry.skill_folder_hash.trim();
    if hash.is_empty() {
        return Err("未记录版本指纹，无法检查更新。".to_string());
    }
    if !content_hash::is_content_hash(hash) {
        if crate::git_tree::is_git_tree_hash(hash) {
            return Err("记录的是 40 位 Git tree SHA，无法与内容指纹比较。".to_string());
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
    let target: UpdateTarget =
        crate::update_target::resolve(entry).map_err(|error| match error {
            crate::update_target::UpdateTargetError::Source { .. } => {
                "来源不是公开 HTTPS Git 仓库，无法检查更新。".to_string()
            }
            crate::update_target::UpdateTargetError::Reference { .. } => {
                "lock ref 不是安全的分支/标签名，无法检查更新。".to_string()
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

fn pending_result(target: &ProjectTarget, state: UpdateState, note: String) -> UpdateSkillResult {
    UpdateSkillResult {
        name: target.name.clone(),
        source: None,
        reference: None,
        lock_hash: Some(target.lock_hash.clone()),
        remote_hash: None,
        state,
        unknown_reason: (state == UpdateState::Unknown)
            .then_some(super::UpdateUnknownReason::NotChecked),
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
    use crate::projects::Project;
    use crate::update::github::GitHubFetchError;
    use std::collections::HashMap;
    use std::fs;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    const COMMIT_A: &str = "1aee802d9139d4df84d017de44fb1b37a7b614e9";
    const COMMIT_B: &str = "0123456789abcdef0123456789abcdef01234567";
    const ALPHA_HASH: &str = "b9d08da8ac776d566beb814c50480e5edfc5422384e327cdc58629d102b491f1";
    const BETA_HASH: &str = "894071b670b3c183fb53284dba3e725204618c4ef085eb7b6208d2961969534e";
    const ALPHA_OLD_HASH: &str = "9a669b2c5f46470c263ccd0b72686556915cd9085ad0de4a04d5ca18c9ca0a17";

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
        download_pause:
            Option<tokio::sync::mpsc::UnboundedSender<(String, tokio::sync::oneshot::Sender<()>)>>,
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
            self.commits.insert(
                (source.to_string(), reference.map(str::to_string)),
                Err(error),
            );
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
                .unwrap_or_else(|| Err(GitHubFetchError::http(404, "no scripted commit")))
        }

        async fn download_archive(
            &self,
            source: &str,
            commit: &str,
        ) -> Result<Vec<u8>, GitHubFetchError> {
            self.download_calls.fetch_add(1, Ordering::SeqCst);
            if let Some(paused) = &self.download_pause {
                let (resume, waiting) = tokio::sync::oneshot::channel();
                paused.send((commit.to_string(), resume)).unwrap();
                waiting.await.unwrap();
            }
            self.archives
                .get(&(source.to_string(), commit.to_string()))
                .cloned()
                .unwrap_or_else(|| Err(GitHubFetchError::http(404, "no scripted archive")))
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
        let service =
            ProjectUpdateService::with_dependencies(paths, Arc::new(client), clock, config);
        (service, tmp)
    }

    fn scoped_archive() -> Vec<u8> {
        tar_gz(&[
            ("acme-skills-abc/alpha/SKILL.md", b"alpha\n"),
            ("acme-skills-abc/alpha/references/guide.md", b"guide\n"),
            ("acme-skills-abc/beta/SKILL.md", b"beta v2\n"),
        ])
    }

    fn scope_uuid() -> uuid::Uuid {
        uuid::Uuid::parse_str("5a7b6c1d-2e3f-4a5b-8c9d-0e1f2a3b4c5d").unwrap()
    }

    fn scope() -> ScopeId {
        ScopeId::project(scope_uuid())
    }

    fn layout(root: &Path) -> ScopeLayout {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(tmp.path().join("home"), tmp.path().join("app-data"));
        ScopeLayout::project(
            &paths,
            Project {
                id: scope_uuid(),
                root: root.to_path_buf(),
                display_name: "project".to_string(),
                added_at: 0,
                last_opened_at: 0,
                check_revision: "initial".into(),
            },
        )
    }

    #[tokio::test]
    async fn git_and_gitlab_project_entries_use_content_fingerprints() {
        let root = project_root();
        let source = "https://cnb.cool/cnb/skills/cnb-skill.git";
        let git_entry = |name: &str, hash: &str, path: &str| {
            serde_json::json!({
                "source": source, "sourceUrl": source, "sourceType": name,
                "ref": "main", "skillPath": path, "computedHash": hash,
            })
        };
        write_lock(
            root.path(),
            serde_json::json!({
                "alpha": git_entry("git", ALPHA_HASH, "alpha/SKILL.md"),
                "beta": git_entry("gitlab", ALPHA_OLD_HASH, "beta/SKILL.md"),
                "missing": git_entry("git", ALPHA_HASH, "missing/SKILL.md"),
            }),
        );
        let client = FakeArchiveClient::default().with_group(
            source,
            Some("main"),
            COMMIT_A,
            scoped_archive(),
        );
        let (service, _tmp) = service(
            client,
            Arc::new(TestClock::new(1_000)),
            ProjectCheckConfig::default(),
        );
        let report = service.check(&layout(root.path()), None).await.unwrap();
        assert_eq!(
            report
                .skills
                .iter()
                .find(|s| s.name == "alpha")
                .unwrap()
                .state,
            UpdateState::Current
        );
        assert_eq!(
            report
                .skills
                .iter()
                .find(|s| s.name == "beta")
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
        assert_eq!(report.sources.len(), 1);
        let cached = service.check(&layout(root.path()), None).await.unwrap();
        assert!(cached.sources[0].cached);
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
        let client = FakeArchiveClient::default().with_group(
            "acme/skills",
            Some("main"),
            COMMIT_A,
            scoped_archive(),
        );
        let commit_calls = Arc::clone(&client.commit_calls);
        let download_calls = Arc::clone(&client.download_calls);
        let clock = Arc::new(TestClock::new(1_000));
        let (service, _tmp) = service(client, Arc::clone(&clock), ProjectCheckConfig::default());

        let report = service.check(&layout(root.path()), None).await.unwrap();

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
        assert!(local.note.as_deref().unwrap().contains("HTTPS Git"));

        // alpha, alpha-copy and beta share (source, ref): one commit lookup,
        // one archive download.
        assert_eq!(report.candidate_count(), 1);
        assert_eq!(report.sources.len(), 1);
        assert_eq!(report.sources[0].skill_count, 3);
        assert_eq!(report.sources[0].status, UpdateSourceStatus::Ok);
    }

    #[tokio::test]
    async fn different_commits_download_before_either_finishes() {
        let root = project_root();
        write_lock(
            root.path(),
            serde_json::json!({
                "alpha": entry("acme/skills", "github", "alpha/SKILL.md", ALPHA_HASH),
                "beta": entry("other/skills", "github", "beta/SKILL.md", ALPHA_OLD_HASH),
            }),
        );
        let (paused, mut requests) = tokio::sync::mpsc::unbounded_channel();
        let mut client = FakeArchiveClient::default()
            .with_group("acme/skills", Some("main"), COMMIT_A, scoped_archive())
            .with_group("other/skills", Some("main"), COMMIT_B, scoped_archive());
        client.download_pause = Some(paused);
        let (service, _tmp) = service(
            client,
            Arc::new(TestClock::new(1_000)),
            ProjectCheckConfig::default(),
        );
        let layout = layout(root.path());
        let (report, ()) = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            tokio::join!(service.check(&layout, None), async {
                let (first, resume_first) = requests.recv().await.unwrap();
                let (second, resume_second) = requests.recv().await.unwrap();
                assert_ne!(first, second);
                resume_first.send(()).unwrap();
                resume_second.send(()).unwrap();
            })
        })
        .await
        .expect("different commits must enter download before either is released");

        let report = report.unwrap();
        assert_eq!(report.sources.len(), 2);
        assert!(
            report
                .sources
                .iter()
                .all(|source| source.status == UpdateSourceStatus::Ok)
        );
        assert_eq!(report.candidate_count(), 1);
    }

    #[tokio::test]
    async fn slow_download_does_not_block_a_completed_source_at_deadline() {
        let root = project_root();
        write_lock(
            root.path(),
            serde_json::json!({
                "alpha": entry("acme/skills", "github", "alpha/SKILL.md", ALPHA_HASH),
                "beta": entry("other/skills", "github", "beta/SKILL.md", ALPHA_OLD_HASH),
            }),
        );
        let (paused, mut requests) = tokio::sync::mpsc::unbounded_channel();
        let mut client = FakeArchiveClient::default()
            .with_group("acme/skills", Some("main"), COMMIT_A, scoped_archive())
            .with_group("other/skills", Some("main"), COMMIT_B, scoped_archive());
        client.download_pause = Some(paused);
        let (service, _tmp) = service(
            client,
            Arc::new(TestClock::new(1_000)),
            ProjectCheckConfig {
                deadline_ms: 20,
                ..ProjectCheckConfig::default()
            },
        );
        let layout = layout(root.path());
        let (report, slow) = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            tokio::join!(service.check(&layout, None), async {
                let first = requests.recv().await.unwrap();
                let second = requests.recv().await.unwrap();
                let (fast, slow) = if first.0 == COMMIT_A {
                    (first, second)
                } else {
                    (second, first)
                };
                fast.1.send(()).unwrap();
                slow.1
            })
        })
        .await
        .expect("both downloads must begin before the check finishes");
        let report = report.unwrap();
        let alpha = report
            .skills
            .iter()
            .find(|skill| skill.name == "alpha")
            .unwrap();
        let beta = report
            .skills
            .iter()
            .find(|skill| skill.name == "beta")
            .unwrap();
        assert_eq!(alpha.state, UpdateState::Current);
        assert_eq!(beta.state, UpdateState::Error);
        assert!(
            slow.is_closed(),
            "the deadline must release the slow download"
        );
        assert!(
            service
                .cache
                .load(COMMIT_B, ArchiveLimits::default())
                .is_none()
        );
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

        let report = service.check(&layout(root.path()), None).await.unwrap();
        assert_eq!(download_calls.load(Ordering::SeqCst), 1);
        assert_eq!(report.sources.len(), 2);
        // The second group reports the shared commit as cached, but there is
        // exactly one network download for it.
        assert_eq!(report.skills.len(), 2);
        assert!(
            report
                .skills
                .iter()
                .all(|s| s.state == UpdateState::Current)
        );
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
        let client = FakeArchiveClient::default().with_group(
            "acme/skills",
            Some("main"),
            COMMIT_A,
            scoped_archive(),
        );
        let clock = Arc::new(TestClock::new(1_000));
        let (service, tmp) = service(client, clock, ProjectCheckConfig::default());
        let paths = AppPaths::new(tmp.path().join("home"), tmp.path().join("app-data"));
        CommitArchiveCache::new(&paths)
            .store(COMMIT_A, &scoped_archive(), 900)
            .unwrap();

        let report = service.check(&layout(root.path()), None).await.unwrap();
        assert_eq!(report.sources[0].status, UpdateSourceStatus::Cached);
        assert_eq!(report.cached_source_count, 1);
        assert!(report.skills[0].cached);
        assert_eq!(report.skills[0].state, UpdateState::Current);
        // Timestamps come from the cache entry, not the check time.
        assert_eq!(report.skills[0].last_success_at_epoch_ms, Some(900));
    }

    #[tokio::test]
    async fn cached_archives_still_consume_the_source_budget() {
        let root = project_root();
        write_lock(
            root.path(),
            serde_json::json!({
                "alpha": entry("acme/skills", "github", "alpha/SKILL.md", ALPHA_HASH),
            }),
        );
        let client = FakeArchiveClient::default().with_group(
            "acme/skills",
            Some("main"),
            COMMIT_A,
            scoped_archive(),
        );
        let commits = Arc::clone(&client.commit_calls);
        let downloads = Arc::clone(&client.download_calls);
        let (mut service, _tmp) = service(
            client,
            Arc::new(TestClock::new(1_000)),
            ProjectCheckConfig {
                max_sources: 0,
                ..ProjectCheckConfig::default()
            },
        );
        service
            .cache
            .store(COMMIT_A, &scoped_archive(), 900)
            .unwrap();

        let report = service.check(&layout(root.path()), None).await.unwrap();
        assert_eq!(commits.load(Ordering::SeqCst), 0);
        assert_eq!(downloads.load(Ordering::SeqCst), 0);
        assert_eq!(report.skipped_source_count, 1);
        assert_eq!(report.cached_source_count, 0);
        assert_eq!(report.skills[0].state, UpdateState::Unknown);
        assert_eq!(report.last_success_at_epoch_ms, None);

        service.config.max_sources = 1;
        let report = service.check(&layout(root.path()), None).await.unwrap();
        assert_eq!(commits.load(Ordering::SeqCst), 1);
        assert_eq!(downloads.load(Ordering::SeqCst), 0);
        assert_eq!(report.skipped_source_count, 0);
        assert_eq!(report.cached_source_count, 1);
        assert_eq!(report.skills[0].state, UpdateState::Current);
        assert_eq!(report.last_success_at_epoch_ms, Some(900));
    }

    #[tokio::test]
    async fn a_stale_cached_archive_is_redownloaded_and_a_failed_redownload_keeps_it() {
        let root = project_root();
        write_lock(
            root.path(),
            serde_json::json!({
                "alpha": entry("acme/skills", "github", "alpha/SKILL.md", ALPHA_HASH),
            }),
        );
        let paths_for_cache = |tmp: &tempfile::TempDir| {
            AppPaths::new(tmp.path().join("home"), tmp.path().join("app-data"))
        };

        // Stale but successful download: the fresh copy is stored anew.
        let client = FakeArchiveClient::default().with_group(
            "acme/skills",
            Some("main"),
            COMMIT_A,
            scoped_archive(),
        );
        let downloads = Arc::clone(&client.download_calls);
        let clock = Arc::new(TestClock::new(1_000));
        let config = ProjectCheckConfig {
            cache_ttl_ms: 100,
            ..ProjectCheckConfig::default()
        };
        let (fresh_service, tmp) = service(client, Arc::clone(&clock), config.clone());
        CommitArchiveCache::new(&paths_for_cache(&tmp))
            .store(COMMIT_A, &scoped_archive(), 0)
            .unwrap();
        clock.set(200);
        let report = fresh_service
            .check(&layout(root.path()), None)
            .await
            .unwrap();
        assert_eq!(downloads.load(Ordering::SeqCst), 1);
        assert_eq!(report.sources[0].status, UpdateSourceStatus::Ok);
        assert_eq!(report.skills[0].last_success_at_epoch_ms, Some(200));

        // Stale cache and a failing download: the source fails but reports the
        // previous successful check time.
        // The commit resolves, but the archive download fails (unscripted).
        let mut client = FakeArchiveClient::default();
        client.commits.insert(
            ("acme/skills".to_string(), Some("main".to_string())),
            Ok(COMMIT_A.to_string()),
        );
        let clock = Arc::new(TestClock::new(1_000));
        let (failing_service, tmp) = service(client, clock, config);
        CommitArchiveCache::new(&paths_for_cache(&tmp))
            .store(COMMIT_A, &scoped_archive(), 0)
            .unwrap();
        let report = failing_service
            .check(&layout(root.path()), None)
            .await
            .unwrap();
        assert_eq!(report.sources[0].status, UpdateSourceStatus::Error);
        assert_eq!(report.skills[0].last_success_at_epoch_ms, Some(0));
        assert_eq!(
            report.skills[0].note.as_deref(),
            Some("已保留上次成功检查结果。")
        );
    }

    #[tokio::test]
    async fn source_failures_are_isolated() {
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

        let report = service.check(&layout(root.path()), None).await.unwrap();
        let alpha = report.skills.iter().find(|s| s.name == "alpha").unwrap();
        assert_eq!(alpha.state, UpdateState::Current);
        let beta = report.skills.iter().find(|s| s.name == "beta").unwrap();
        assert_eq!(beta.state, UpdateState::Error);
        assert_eq!(
            beta.error.as_ref().unwrap().code,
            crate::error::ErrorCode::NetworkOffline
        );
        assert_eq!(report.sources.len(), 2);
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
        let client = FakeArchiveClient::default().with_group(
            "acme/skills",
            Some("main"),
            COMMIT_A,
            scoped_archive(),
        );
        let clock = Arc::new(TestClock::new(1_000));
        let config = ProjectCheckConfig {
            limits: ArchiveLimits {
                max_file_bytes: 4,
                ..ArchiveLimits::default()
            },
            ..ProjectCheckConfig::default()
        };
        let (service, _tmp) = service(client, clock, config);

        let report = service.check(&layout(root.path()), None).await.unwrap();
        assert_eq!(report.sources[0].status, UpdateSourceStatus::Error);
        assert_eq!(report.skills[0].state, UpdateState::Error);
        assert!(report.skills[0].note.is_none());
        assert_eq!(
            report.skills[0].error.as_ref().unwrap().code,
            crate::error::ErrorCode::NetworkError
        );
    }

    #[tokio::test]
    async fn scoped_checks_return_only_the_selected_skill() {
        let root = project_root();
        write_lock(
            root.path(),
            serde_json::json!({
                "alpha": entry("acme/skills", "github", "alpha/SKILL.md", ALPHA_HASH),
                "beta": entry("acme/skills", "github", "beta/SKILL.md", ALPHA_OLD_HASH),
            }),
        );
        let client = FakeArchiveClient::default().with_group(
            "acme/skills",
            Some("main"),
            COMMIT_A,
            scoped_archive(),
        );
        let clock = Arc::new(TestClock::new(1_000));
        let (service, _tmp) = service(client, clock, ProjectCheckConfig::default());
        let scoped = service
            .check(
                &layout(root.path()),
                Some(vec![format!("{}/alpha", scope().as_string())]),
            )
            .await
            .unwrap();
        assert_eq!(scoped.skills.len(), 1);
    }

    #[tokio::test]
    async fn skill_ids_from_another_scope_are_rejected_instead_of_ignored() {
        let root = project_root();
        write_lock(
            root.path(),
            serde_json::json!({
                "alpha": entry("acme/skills", "github", "alpha/SKILL.md", ALPHA_HASH),
            }),
        );
        let (service, _tmp) = service(
            FakeArchiveClient::default(),
            Arc::new(TestClock::new(1_000)),
            ProjectCheckConfig::default(),
        );
        let foreign = format!(
            "project:{}/alpha",
            uuid::Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap()
        );

        let error = service
            .check(&layout(root.path()), Some(vec![foreign]))
            .await
            .unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::PathInvalid);
    }

    #[tokio::test]
    async fn source_limit_marks_pending_skills_not_checked() {
        let root = project_root();
        write_lock(
            root.path(),
            serde_json::json!({
                "alpha": entry("acme/skills", "github", "alpha/SKILL.md", ALPHA_HASH),
            }),
        );
        let (service, _tmp) = service(
            FakeArchiveClient::default(),
            Arc::new(TestClock::new(1_000)),
            ProjectCheckConfig {
                max_sources: 0,
                ..ProjectCheckConfig::default()
            },
        );

        let report = service.check(&layout(root.path()), None).await.unwrap();
        let alpha = report
            .skills
            .iter()
            .find(|skill| skill.name == "alpha")
            .unwrap();
        assert_eq!(alpha.state, UpdateState::Unknown);
        assert_eq!(
            alpha.unknown_reason,
            Some(crate::update::UpdateUnknownReason::NotChecked)
        );
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

        let report = service.check(&layout(root.path()), None).await.unwrap();
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

        let error = service.check(&layout(root.path()), None).await.unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::LockCorrupt);
    }
}
