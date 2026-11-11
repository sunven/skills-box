//! Application-level source discovery. Each source commits independently;
//! registry associations and user decisions are checked again at publication.
mod history;
mod store;
#[cfg(test)]
mod tests;

use crate::update::{
    api::GitHubApiClient,
    archive::{ArchiveClient, SourceArchiveClient},
    github::GitHubFetchError,
};
use crate::{
    error::AppError,
    paths::AppPaths,
    process::CancelToken,
    projects::ProjectRegistry,
    read_model::{ReadModel, SkillIdentity},
    scope::ScopeId,
    scope_layout::ScopeLayout,
    tasks::{TaskManager, parser::ParsedSkill},
};
use futures::{StreamExt, stream};
pub use history::DiscoveryId;
use history::{Cooldown, History, ScopeSnapshot, SourceRecord, name_identity};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

pub const INTERVAL_MS: u64 = 6 * 60 * 60 * 1000;
const BATCH_SIZE: usize = 20;
const CONCURRENCY: usize = 4;

#[async_trait::async_trait]
pub trait DiscoveryTransport: Send + Sync {
    async fn version(&self, source: &str, cancel: CancelToken) -> Result<String, GitHubFetchError>;
    async fn preview(
        &self,
        source: &str,
        cancel: CancelToken,
    ) -> Result<Vec<ParsedSkill>, AppError>;
}
struct NativeTransport {
    versions: SourceArchiveClient,
    paths: AppPaths,
    runtime: Arc<dyn crate::cli::CliRuntimeProvider>,
    executor: Arc<dyn crate::process::ProcessExecutor>,
}
#[async_trait::async_trait]
impl DiscoveryTransport for NativeTransport {
    async fn version(&self, source: &str, cancel: CancelToken) -> Result<String, GitHubFetchError> {
        tokio::select! {
            result = self.versions.latest_commit(source, None) => result,
            _ = cancel.cancelled() => Err(GitHubFetchError::git("发现检查已取消")),
        }
    }
    async fn preview(
        &self,
        source: &str,
        cancel: CancelToken,
    ) -> Result<Vec<ParsedSkill>, AppError> {
        let layout = ScopeLayout::global(&self.paths, vec![]);
        let (output, parsed) = crate::tasks::preview::source_preview(
            self.runtime.as_ref(),
            self.executor.as_ref(),
            &self.paths,
            &layout,
            source,
            crate::timeouts::PREVIEW_TIMEOUT,
            cancel,
            Some(crate::cli::commands::CliCommandContext::background(
                "discovery",
                Some(source.into()),
            )),
        )
        .await?;
        if !output.success() {
            return Err(
                crate::error::cli::classify_add_failure(&output.stdout, &output.stderr)
                    .to_app_error(crate::error::cli::CliAction::Preview),
            );
        }
        match parsed {
            crate::tasks::PreviewParseOutcome::Skills(skills) => Ok(skills),
            crate::tasks::PreviewParseOutcome::NoSkills => Ok(vec![]),
            crate::tasks::PreviewParseOutcome::Failed { .. } => {
                Err(AppError::source_preview_failed(
                    "CLI 输出无法确认为有效目录，请手动预览来源后重试。",
                ))
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryProgress {
    pub run_id: u64,
    pub running: bool,
    pub total: usize,
    pub completed: usize,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverySource {
    pub source: String,
    pub last_attempt_at: Option<u64>,
    pub last_success_at: Option<u64>,
    pub error: Option<String>,
    pub scanning: bool,
    pub needs_revalidation: bool,
    pub paused_until: Option<u64>,
}
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum InstallationStatus {
    Installed,
    NotInstalled,
    Conflict,
    Stale,
    Unknown,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeInstallation {
    pub scope: ScopeId,
    pub label: String,
    pub status: InstallationStatus,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredSkill {
    pub id: DiscoveryId,
    pub name: String,
    pub description: String,
    pub group: Option<String>,
    pub is_new: bool,
    pub unread: bool,
    pub ignored: bool,
    pub first_seen_at: u64,
    pub install_issue: Option<String>,
    pub installations: Vec<ScopeInstallation>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryState {
    pub sources: Vec<DiscoverySource>,
    pub skills: Vec<DiscoveredSkill>,
    pub unread_count: usize,
    pub progress: DiscoveryProgress,
    pub warnings: Vec<String>,
    pub error: Option<AppError>,
}

struct Projection {
    scope: ScopeId,
    label: String,
    entries: Option<Vec<crate::lock::LockEntry>>,
}
struct RuntimeState {
    history: Result<History, AppError>,
    // Active associations are never loaded directly from the persisted file.
    // Every scope must still exist at the same registry revision.
    associations: BTreeMap<String, BTreeSet<String>>,
    generations: BTreeMap<String, u64>,
    urgent: BTreeSet<String>,
    scanning: BTreeSet<String>,
    warnings: Vec<String>,
    projections: Vec<Projection>,
    write_error: Option<AppError>,
    progress: DiscoveryProgress,
    cancel: CancelToken,
    closed: bool,
}
struct Inner {
    paths: AppPaths,
    projects: Arc<ProjectRegistry>,
    read_model: Arc<ReadModel>,
    transport: Arc<dyn DiscoveryTransport>,
    api: GitHubApiClient,
    tasks: TaskManager,
    state: Mutex<RuntimeState>,
    clock: Arc<dyn Fn() -> u64 + Send + Sync>,
}
#[derive(Clone)]
pub struct DiscoveryService {
    inner: Arc<Inner>,
}
impl DiscoveryService {
    pub fn new(
        paths: AppPaths,
        projects: Arc<ProjectRegistry>,
        read_model: Arc<ReadModel>,
        runtime: Arc<dyn crate::cli::CliRuntimeProvider>,
        tasks: TaskManager,
        api: GitHubApiClient,
        executor: Arc<dyn crate::process::ProcessExecutor>,
    ) -> Self {
        let transport = Arc::new(NativeTransport {
            versions: SourceArchiveClient::with_api(paths.clone(), api.clone()),
            paths: paths.clone(),
            runtime,
            executor,
        });
        Self::with_transport(
            paths,
            projects,
            read_model,
            tasks,
            api,
            transport,
            Arc::new(crate::time::now_ms),
        )
    }
    pub fn with_transport(
        paths: AppPaths,
        projects: Arc<ProjectRegistry>,
        read_model: Arc<ReadModel>,
        tasks: TaskManager,
        api: GitHubApiClient,
        transport: Arc<dyn DiscoveryTransport>,
        clock: Arc<dyn Fn() -> u64 + Send + Sync>,
    ) -> Self {
        let history = store::load(&paths.app_data().join("discovery.json"));
        Self {
            inner: Arc::new(Inner {
                paths,
                projects,
                read_model,
                tasks,
                api,
                transport,
                clock,
                state: Mutex::new(RuntimeState {
                    history,
                    associations: BTreeMap::new(),
                    generations: BTreeMap::new(),
                    urgent: BTreeSet::new(),
                    scanning: BTreeSet::new(),
                    warnings: vec![],
                    projections: vec![],
                    write_error: None,
                    progress: DiscoveryProgress {
                        run_id: 0,
                        running: false,
                        total: 0,
                        completed: 0,
                    },
                    cancel: CancelToken::new(),
                    closed: false,
                }),
            }),
        }
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, RuntimeState> {
        self.inner.state.lock().unwrap_or_else(|p| p.into_inner())
    }
    fn save(&self, state: &mut RuntimeState, next: History) -> Result<(), AppError> {
        if let Err(error) =
            store::persist(&self.inner.paths.app_data().join("discovery.json"), &next)
        {
            state.write_error = Some(error.clone());
            return Err(error);
        }
        state.history = Ok(next);
        state.write_error = None;
        Ok(())
    }
    /// Refresh source associations from installation records, without scanning
    /// skill content. Failed scope reads retain only the matching revision.
    pub fn refresh(&self) {
        let mut state = self.lock();
        if state.closed || state.history.is_err() {
            return;
        }
        let projects = self.inner.projects.list();
        let mut scopes = vec![(ScopeId::Global, "Global".to_string(), "global".to_string())];
        scopes.extend(projects.projects.iter().map(|e| {
            (
                e.project.scope(),
                e.project.display_name.clone(),
                e.project.check_revision.clone(),
            )
        }));
        let mut next = state.history.as_ref().unwrap().clone();
        let known: BTreeSet<String> = scopes.iter().map(|(s, _, _)| s.as_string()).collect();
        next.scopes.retain(|id, _| known.contains(id));
        let mut warnings = projects.warnings;
        let mut projections = vec![];
        for (scope, label, revision) in scopes {
            let id = scope.as_string();
            if next.scopes.get(&id).is_some_and(|s| s.revision != revision) {
                next.scopes.remove(&id);
            }
            let read =
                ScopeLayout::resolve_detected(&self.inner.paths, &self.inner.projects, &scope)
                    .and_then(|layout| layout.lock())
                    .map(|lock| lock.entries().values().cloned().collect::<Vec<_>>());
            let entries = match read {
                Ok(entries) => {
                    let sources: BTreeSet<_> = entries
                        .iter()
                        .filter_map(|e| crate::update_target::resolve_source(e).ok())
                        .map(|s| crate::tasks::source::source_identity(&s))
                        .collect();
                    next.scopes.insert(
                        id.clone(),
                        ScopeSnapshot {
                            revision,
                            sources: sources.into_iter().collect(),
                        },
                    );
                    Some(entries)
                }
                Err(error) => {
                    warnings.push(format!(
                        "{label}：{}；沿用同一项目版本的上次来源记录。",
                        diagnostic(&error)
                    ));
                    None
                }
            };
            projections.push(Projection {
                scope,
                label,
                entries,
            });
        }
        let mut associations: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (scope, snapshot) in &next.scopes {
            for source in &snapshot.sources {
                associations
                    .entry(source.clone())
                    .or_default()
                    .insert(format!("{scope}/{}", snapshot.revision));
                next.sources.entry(source.clone()).or_default();
            }
        }
        let identities: BTreeSet<_> = state
            .associations
            .keys()
            .chain(associations.keys())
            .cloned()
            .collect();
        for source in identities {
            if state.associations.get(&source) != associations.get(&source) {
                *state.generations.entry(source.clone()).or_default() += 1;
                if associations.contains_key(&source) {
                    // Startup isn't reactivation: only a genuinely new
                    // association, or one revoked during this process, is urgent.
                    let previous = state
                        .history
                        .as_ref()
                        .unwrap()
                        .scopes
                        .values()
                        .any(|s| s.sources.contains(&source));
                    if !previous || state.generations[&source] > 1 {
                        state.urgent.insert(source.clone());
                    }
                }
            }
            if !associations.contains_key(&source) {
                state.urgent.remove(&source);
            }
        }
        state.associations = associations;
        state.warnings = warnings;
        state.projections = projections;
        if state.history.as_ref().unwrap().scopes != next.scopes {
            // Revocation applies in memory even when the auxiliary file is
            // unwritable. Old roots never derive authority from this file.
            if self.save(&mut state, next.clone()).is_err() {
                state.history = Ok(next);
            }
        } else {
            state.history = Ok(next);
        }
    }
    pub fn get_state(&self) -> DiscoveryState {
        self.refresh();
        let state = self.lock();
        let mut warnings = state.warnings.clone();
        // Scheduling and source publication only need lock records. Build the
        // lightweight installation projection once when the UI requests state.
        let projections = state
            .projections
            .iter()
            .map(|projection| {
                let identities = if projection.entries.is_some() {
                    match self.inner.read_model.skill_identities(&projection.scope) {
                        Ok(skills) => Some(skills),
                        Err(error) => {
                            warnings.push(format!(
                                "{} 安装状态未知：{}",
                                projection.label,
                                diagnostic(&error)
                            ));
                            None
                        }
                    }
                } else {
                    None
                };
                (projection, identities)
            })
            .collect::<Vec<_>>();
        let now = (self.inner.clock)();
        let credential = self.inner.api.credential_context().ok();
        let mut sources = vec![];
        let mut skills = vec![];
        if let Ok(history) = &state.history {
            for source in state.associations.keys() {
                let Some(record) = history.sources.get(source) else {
                    continue;
                };
                let paused_until = history
                    .cooldown
                    .as_ref()
                    .filter(|c| {
                        credential.as_ref().is_some_and(|(context, revision)| {
                            c.context == *context && c.revision == *revision
                        }) && c.until > now
                            && is_github(source)
                    })
                    .map(|c| c.until);
                sources.push(DiscoverySource {
                    source: source.clone(),
                    last_attempt_at: record.last_attempt_at,
                    last_success_at: record.last_success_at,
                    error: record.error.clone(),
                    scanning: state.scanning.contains(source),
                    needs_revalidation: !record.valid,
                    paused_until,
                });
                for (name, catalog) in &record.catalog {
                    let h = &record.history[name];
                    let installations = projections
                        .iter()
                        .map(|(p, identities)| ScopeInstallation {
                            scope: p.scope.clone(),
                            label: p.label.clone(),
                            status: installation(p, identities.as_deref(), source, name),
                        })
                        .collect();
                    skills.push(DiscoveredSkill {
                        id: DiscoveryId {
                            source: source.clone(),
                            name: name.clone(),
                        },
                        name: catalog.skill.name.clone(),
                        description: catalog.skill.description.clone(),
                        group: catalog.skill.group.clone(),
                        is_new: h.is_new,
                        unread: record.unread(name),
                        ignored: h.ignored,
                        first_seen_at: h.first_seen_at,
                        install_issue: catalog.install_issue.clone(),
                        installations,
                    });
                }
            }
        }
        let unread_count = skills.iter().filter(|s| s.unread).count();
        DiscoveryState {
            sources,
            skills,
            unread_count,
            progress: state.progress.clone(),
            warnings,
            error: state
                .history
                .as_ref()
                .err()
                .cloned()
                .or_else(|| state.write_error.clone()),
        }
    }
    /// Starts an asynchronous run; returns immediately and is single-flight.
    /// Progress/state polling never waits for all sources to complete.
    pub fn start(&self, manual: bool) -> Result<DiscoveryProgress, AppError> {
        let activity = self.inner.paths.data_coordination.activity()?;
        self.refresh();
        let mut state = self.lock();
        if state.closed {
            return Err(AppError::task_shutting_down());
        }
        state.history.as_ref().map_err(Clone::clone)?;
        if state.progress.running || (!manual && self.inner.tasks.active_mutation().is_some()) {
            return Ok(state.progress.clone());
        }
        let now = (self.inner.clock)();
        let history = state.history.as_ref().unwrap();
        let mut queue: Vec<_> = state
            .associations
            .keys()
            .filter(|s| {
                manual
                    || state.urgent.contains(*s)
                    || history
                        .sources
                        .get(*s)
                        .and_then(|r| r.last_attempt_at)
                        .is_none_or(|last| now.saturating_sub(last) >= INTERVAL_MS)
            })
            .map(|s| {
                (
                    s.clone(),
                    state.generations[s],
                    history
                        .sources
                        .get(s)
                        .and_then(|r| r.last_attempt_at)
                        .unwrap_or(0),
                )
            })
            .collect();
        queue.sort_by_key(|(_, _, last)| *last);
        state.progress = DiscoveryProgress {
            run_id: state.progress.run_id + 1,
            running: !queue.is_empty(),
            total: queue.len(),
            completed: 0,
        };
        state.cancel = CancelToken::new();
        let progress = state.progress.clone();
        let cancel = state.cancel.clone();
        if !queue.is_empty() {
            let service = self.clone();
            let run = progress.run_id;
            tauri::async_runtime::spawn(async move {
                let _activity = activity;
                service.run(queue, run, cancel).await;
            });
        }
        Ok(progress)
    }
    async fn run(&self, queue: Vec<(String, u64, u64)>, run: u64, cancel: CancelToken) {
        for batch in queue.chunks(BATCH_SIZE) {
            if cancel.is_cancelled() {
                break;
            }
            let mut work = stream::iter(batch.iter().cloned())
                .map(|(source, generation, _)| {
                    let service = self.clone();
                    let token = cancel.clone();
                    async move { service.scan_one(source, generation, run, token).await }
                })
                .buffer_unordered(CONCURRENCY);
            while work.next().await.is_some() {}
        }
        let mut state = self.lock();
        if state.progress.run_id == run {
            state.progress.running = false;
            state.scanning.clear();
        }
    }
    fn current(state: &RuntimeState, source: &str, generation: u64, run: u64) -> bool {
        !state.closed
            && state.progress.run_id == run
            && state.associations.contains_key(source)
            && state.generations.get(source) == Some(&generation)
    }
    fn cooldown(&self, state: &RuntimeState, source: &str) -> bool {
        let Ok((context, revision)) = self.inner.api.credential_context() else {
            return false;
        };
        is_github(source)
            && state
                .history
                .as_ref()
                .ok()
                .and_then(|h| h.cooldown.as_ref())
                .is_some_and(|c| {
                    c.context == context && c.revision == revision && c.until > (self.inner.clock)()
                })
    }
    async fn scan_one(&self, source: String, generation: u64, run: u64, cancel: CancelToken) {
        let (context, revision) = self.inner.api.credential_context().unwrap_or_default();
        let cached = {
            let mut state = self.lock();
            if cancel.is_cancelled() || !Self::current(&state, &source, generation, run) {
                return;
            }
            if self.cooldown(&state, &source) {
                state.progress.completed += 1;
                return;
            }
            let mut next = state.history.as_ref().unwrap().clone();
            let record = next.sources.entry(source.clone()).or_default();
            let cached = record.clone();
            record.last_attempt_at = Some((self.inner.clock)());
            if self.save(&mut state, next).is_err() {
                state.progress.completed += 1;
                return;
            }
            state.scanning.insert(source.clone());
            state.urgent.remove(&source);
            cached
        };
        let result = self.fetch(&source, &cached, cancel.clone()).await;
        // Recollect before publication so external lock changes, Forget and
        // Relocate invalidate a request even if the discovery page is closed.
        self.refresh();
        let mut state = self.lock();
        if state.progress.run_id == run {
            state.scanning.remove(&source);
            state.progress.completed += 1;
        }
        if cancel.is_cancelled() || !Self::current(&state, &source, generation, run) {
            return;
        }
        let now = (self.inner.clock)();
        let mut next = state.history.as_ref().unwrap().clone();
        let record = next.sources.entry(source).or_default();
        match result {
            Ok((commit, Some(skills))) => record.publish(skills, commit, now),
            Ok((_, None)) => {
                record.last_success_at = Some(now);
                record.error = None;
            }
            Err(ScanError::App(error)) => record.error = Some(diagnostic(&error)),
            Err(ScanError::Fetch(error)) => {
                record.error = Some(diagnostic(&error.to_app_error()));
                if let GitHubFetchError::RateLimited {
                    reset_at,
                    context: error_context,
                    revision: error_revision,
                    ..
                } = error
                {
                    let (current, current_revision) =
                        self.inner.api.credential_context().unwrap_or_default();
                    let request_context = error_context.unwrap_or(context);
                    if request_context == current
                        && error_revision.unwrap_or(revision) == current_revision
                    {
                        let until = reset_at.unwrap_or(now.saturating_add(INTERVAL_MS));
                        let until = next
                            .cooldown
                            .as_ref()
                            .filter(|c| c.context == current && c.revision == current_revision)
                            .map(|c| c.until.max(until))
                            .unwrap_or(until);
                        next.cooldown = Some(Cooldown {
                            context: current,
                            until,
                            revision: current_revision,
                        });
                    }
                }
            }
        }
        let _ = self.save(&mut state, next);
    }
    async fn fetch(
        &self,
        source: &str,
        cached: &SourceRecord,
        cancel: CancelToken,
    ) -> Result<(String, Option<Vec<ParsedSkill>>), ScanError> {
        let before = self
            .inner
            .transport
            .version(source, cancel.clone())
            .await
            .map_err(ScanError::Fetch)?;
        if cached.reusable(&before) {
            return Ok((before, None));
        }
        {
            let state = self.lock();
            if self.cooldown(&state, source) {
                return Err(ScanError::App(AppError::network_rate_limited(
                    "GitHub 获取已暂停，旧目录已保留。",
                )));
            }
        }
        let skills = self
            .inner
            .transport
            .preview(source, cancel.clone())
            .await
            .map_err(ScanError::App)?;
        if skills.len() > 20_000
            || skills.iter().any(|s| {
                s.name.is_empty()
                    || s.name.len() > 4096
                    || s.description.len() > 65_536
                    || s.group.as_ref().is_some_and(|g| g.len() > 4096)
            })
        {
            return Err(ScanError::App(AppError::source_preview_failed(
                "来源目录元数据超过上限，旧目录已保留。",
            )));
        }
        {
            let state = self.lock();
            if self.cooldown(&state, source) {
                return Err(ScanError::App(AppError::network_rate_limited(
                    "GitHub 获取已暂停，旧目录已保留。",
                )));
            }
        }
        let after = self
            .inner
            .transport
            .version(source, cancel)
            .await
            .map_err(ScanError::Fetch)?;
        if before != after {
            return Err(ScanError::App(AppError::source_preview_failed(
                "预览期间默认分支发生变化，旧目录已保留，请稍后重新扫描。",
            )));
        }
        Ok((after, Some(skills)))
    }
    fn edit(
        &self,
        f: impl FnOnce(&mut History, &BTreeMap<String, BTreeSet<String>>),
    ) -> Result<(), AppError> {
        self.refresh();
        let mut state = self.lock();
        let mut next = state.history.as_ref().map_err(Clone::clone)?.clone();
        f(&mut next, &state.associations);
        self.save(&mut state, next)
    }
    pub fn acknowledge(&self, ids: Option<Vec<DiscoveryId>>) -> Result<(), AppError> {
        self.edit(|h, active| {
            if let Some(ids) = ids {
                for id in ids {
                    if !active.contains_key(&id.source) {
                        continue;
                    }
                    if let Some(r) = h.sources.get_mut(&id.source)
                        && r.catalog.contains_key(&id.name)
                        && let Some(o) = r.history.get_mut(&id.name)
                    {
                        o.processed = true;
                    }
                }
            } else {
                for source in active.keys() {
                    if let Some(r) = h.sources.get_mut(source) {
                        for name in r.catalog.keys() {
                            if let Some(o) = r.history.get_mut(name) {
                                o.processed = true;
                            }
                        }
                    }
                }
            }
        })
    }
    pub fn acknowledge_installed(
        &self,
        source: &str,
        results: &[crate::tasks::TaskSkillResult],
    ) -> Result<(), AppError> {
        let ids = results
            .iter()
            .filter(|s| s.status == crate::tasks::TaskSkillStatus::Installed)
            .map(|s| DiscoveryId {
                source: source.into(),
                name: name_identity(&s.name),
            })
            .collect::<Vec<_>>();
        if ids.is_empty() {
            return Ok(());
        }
        self.acknowledge(Some(ids))
    }
    pub fn set_ignored(&self, id: DiscoveryId, ignored: bool) -> Result<(), AppError> {
        self.edit(|h, active| {
            if active.contains_key(&id.source)
                && let Some(o) = h
                    .sources
                    .get_mut(&id.source)
                    .and_then(|r| r.history.get_mut(&id.name))
            {
                o.ignored = ignored;
                o.processed = true;
            }
        })
    }
    pub fn invalidate(&self, source: String) -> Result<(), AppError> {
        let source = crate::tasks::validate_source(&source)?.identity();
        self.refresh();
        let mut state = self.lock();
        let mut next = state.history.as_ref().map_err(Clone::clone)?.clone();
        if let Some(record) = next.sources.get_mut(&source) {
            record.valid = false;
        }
        let result = self.save(&mut state, next.clone());
        if result.is_err() {
            state.history = Ok(next);
        }
        result
    }
    pub fn auth_succeeded(&self, context: &str, revision: u64, remaining: u64) {
        if remaining == 0 {
            return;
        }
        let mut state = self.lock();
        if self
            .inner
            .api
            .credential_context()
            .ok()
            .as_ref()
            .is_none_or(|(current, current_revision)| {
                current != context || *current_revision != revision
            })
        {
            return;
        }
        if let Ok(mut next) = state.history.clone() {
            next.cooldown = None;
            if self.save(&mut state, next.clone()).is_err() {
                state.history = Ok(next);
            }
        }
    }
    pub async fn shutdown_and_wait(&self) {
        self.shutdown();
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if !self.lock().progress.running {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await;
    }
    pub fn shutdown(&self) {
        let mut state = self.lock();
        state.closed = true;
        state.cancel.cancel();
    }
}
enum ScanError {
    App(AppError),
    Fetch(GitHubFetchError),
}
fn is_github(source: &str) -> bool {
    !source.starts_with("https://")
}
fn diagnostic(error: &AppError) -> String {
    match &error.detail {
        Some(d) => format!("{}：{d}", error.message),
        None => error.message.clone(),
    }
}
/// The pinned CLI uses a sanitized directory/lock key while discovery sees
/// metadata names. Occupancy and actual metadata both participate in projection.
fn install_directory_name(name: &str) -> String {
    let mut value = String::new();
    let mut replacing = false;
    for c in name.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_') {
            value.push(c);
            replacing = false;
        } else if !replacing {
            value.push('-');
            replacing = true;
        }
    }
    let name = value.trim_matches(['.', '-']);
    if name.is_empty() {
        "unnamed-skill".into()
    } else {
        name.chars().take(255).collect()
    }
}
fn installation(
    p: &Projection,
    skills: Option<&[SkillIdentity]>,
    source: &str,
    name: &str,
) -> InstallationStatus {
    let (Some(entries), Some(skills)) = (&p.entries, skills) else {
        return InstallationStatus::Unknown;
    };
    let disk_name = install_directory_name(name);
    let matches: Vec<_> = skills
        .iter()
        .filter(|s| {
            name_identity(&s.name) == name
                || s.metadata_name
                    .as_deref()
                    .is_some_and(|n| name_identity(n) == name)
                || s.name == disk_name
        })
        .collect();
    if matches.len() > 1 {
        return InstallationStatus::Conflict;
    }
    if let Some(skill) = matches.first() {
        let entry = entries.iter().find(|e| e.name == skill.name);
        let same_name = name_identity(&skill.name) == name
            || skill
                .metadata_name
                .as_deref()
                .is_some_and(|n| name_identity(n) == name);
        return if same_name
            && skill.management != crate::inventory::ManagementState::Unmanaged
            && entry.is_some_and(|e| {
                crate::update_target::resolve_source(e)
                    .ok()
                    .is_some_and(|s| crate::tasks::source::source_identity(&s) == source)
            }) {
            InstallationStatus::Installed
        } else {
            InstallationStatus::Conflict
        };
    }
    if entries
        .iter()
        .any(|e| name_identity(&e.name) == name || e.name == disk_name)
    {
        InstallationStatus::Stale
    } else {
        InstallationStatus::NotInstalled
    }
}
