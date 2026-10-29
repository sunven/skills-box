//! One owner for the lock/filesystem read model.
//!
//! The catalog cache holds the lock entries and directory topology. Per-skill
//! views (description, baseline assessment, lock metadata, attachments and
//! detail fields) are rebuilt on demand, so local modifications never go stale.
//! `get_inventory` always rebuilds the catalog; detail and content reuse the
//! cached catalog until its fingerprint changes.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::attachment_target::{self, DetachEligibility};
use crate::baseline::{BaselineStore, LocalModification};
use crate::detail::{SkillDetail, SkillMdStatus, directory_mtime_epoch_ms, read_skill_md};
use crate::error::AppError;
use crate::inventory::{
    self, AgentAttachment, AgentDir, AnomalyGroup, AnomalySummary, AttachmentMode, Catalog,
    InventorySkill, InventorySnapshot, ManagementState, frontmatter,
};
use crate::paths::AppPaths;
use crate::security::{self, AllowedRoot};

pub struct ReadModel {
    paths: AppPaths,
    cache: Mutex<Option<CachedCatalog>>,
    #[cfg(test)]
    agents_override: Mutex<Option<Vec<AgentDir>>>,
    #[cfg(test)]
    catalog_builds: std::sync::atomic::AtomicU64,
}

struct CachedCatalog {
    catalog: Arc<Catalog>,
    agents: Arc<Vec<AgentDir>>,
    fingerprint: Fingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Fingerprint {
    lock: FileStamp,
    extras: FileStamp,
    canonical: DirStamp,
    agents: Vec<DirStamp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileStamp {
    len: Option<u64>,
    modified_ns: Option<u128>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DirStamp {
    path: PathBuf,
    modified_ns: Option<u128>,
}

impl FileStamp {
    fn capture(path: &Path) -> Self {
        match fs::metadata(path) {
            Ok(metadata) => Self {
                len: Some(metadata.len()),
                modified_ns: modified_ns(&metadata),
            },
            Err(_) => Self {
                len: None,
                modified_ns: None,
            },
        }
    }
}

impl DirStamp {
    fn capture(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            modified_ns: fs::metadata(path)
                .ok()
                .and_then(|metadata| modified_ns(&metadata)),
        }
    }
}

impl Fingerprint {
    fn capture(paths: &AppPaths, agents: &[AgentDir]) -> Self {
        Self {
            lock: FileStamp::capture(&paths.lock_path()),
            extras: FileStamp::capture(&paths.agents_config_path()),
            canonical: DirStamp::capture(&paths.canonical_skills_dir()),
            agents: agents
                .iter()
                .map(|agent| DirStamp::capture(&agent.skills_dir))
                .collect(),
        }
    }
}

fn modified_ns(metadata: &fs::Metadata) -> Option<u128> {
    metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_nanos())
}

impl ReadModel {
    pub fn new(paths: AppPaths) -> Self {
        Self {
            paths,
            cache: Mutex::new(None),
            #[cfg(test)]
            agents_override: Mutex::new(None),
            #[cfg(test)]
            catalog_builds: std::sync::atomic::AtomicU64::new(0),
        }
    }

    #[cfg(test)]
    fn with_agents(paths: AppPaths, agents: Vec<AgentDir>) -> Self {
        let model = Self::new(paths);
        *model
            .agents_override
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(agents);
        model
    }

    #[cfg(test)]
    fn build_count(&self) -> u64 {
        self.catalog_builds
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    fn detected_agents(&self) -> Vec<AgentDir> {
        #[cfg(test)]
        if let Some(agents) = self
            .agents_override
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
        {
            return agents.clone();
        }
        inventory::detected_agent_dirs(&self.paths)
    }

    /// Rebuild the catalog and every per-skill view. This is the refresh point
    /// used by `get_inventory`; the new catalog replaces the cache.
    pub fn snapshot(&self) -> Result<InventorySnapshot, AppError> {
        let agents = self.detected_agents();
        let catalog = match inventory::build_catalog(&self.paths, &agents) {
            Ok(catalog) => catalog,
            Err(error) => {
                self.clear_cache();
                return Err(error);
            }
        };
        #[cfg(test)]
        self.catalog_builds
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let fingerprint = Fingerprint::capture(&self.paths, &agents);
        let snapshot = build_snapshot(&self.paths, &catalog, &agents);
        *self.lock_cache() = Some(CachedCatalog {
            catalog: Arc::new(catalog),
            agents: Arc::new(agents),
            fingerprint,
        });
        Ok(snapshot)
    }

    /// Project one skill from the cached catalog; SKILL.md and the baseline
    /// are read fresh.
    pub fn detail(&self, skill_id: &str) -> Result<SkillDetail, AppError> {
        security::validate_skill_id(skill_id)?;
        let (catalog, agents) = self.catalog()?;
        let blocked = blocked_agents(&agents, &catalog.canonical_dir);
        let view = build_view(&self.paths, &catalog, &blocked, skill_id)
            .ok_or_else(|| AppError::skill_not_found(skill_id))?;
        Ok(view.to_detail(&self.paths))
    }

    /// Resolve the physical folder used by content browsing and open actions.
    pub fn resolve_root(&self, skill_id: &str) -> Result<PathBuf, AppError> {
        let (catalog, agents) = self.catalog()?;
        resolve_root_in_catalog(&self.paths, &catalog, &agents, skill_id)
    }

    /// Explicitly drop the catalog after an environment recheck.
    pub fn invalidate(&self) {
        self.clear_cache();
    }

    fn catalog(&self) -> Result<(Arc<Catalog>, Arc<Vec<AgentDir>>), AppError> {
        {
            let cache = self.lock_cache();
            if let Some(cached) = cache.as_ref()
                && cached.fingerprint == Fingerprint::capture(&self.paths, &cached.agents)
            {
                return Ok((Arc::clone(&cached.catalog), Arc::clone(&cached.agents)));
            }
        }

        let agents = self.detected_agents();
        let catalog = match inventory::build_catalog(&self.paths, &agents) {
            Ok(catalog) => catalog,
            Err(error) => {
                self.clear_cache();
                return Err(error);
            }
        };
        #[cfg(test)]
        self.catalog_builds
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let fingerprint = Fingerprint::capture(&self.paths, &agents);
        let catalog = Arc::new(catalog);
        let agents = Arc::new(agents);
        *self.lock_cache() = Some(CachedCatalog {
            catalog: Arc::clone(&catalog),
            agents: Arc::clone(&agents),
            fingerprint,
        });
        Ok((catalog, agents))
    }

    fn clear_cache(&self) {
        *self.lock_cache() = None;
    }

    fn lock_cache(&self) -> std::sync::MutexGuard<'_, Option<CachedCatalog>> {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// One-shot read for fixtures and integration tests.
pub(crate) fn snapshot_with_agents(
    paths: &AppPaths,
    agents: &[AgentDir],
) -> Result<InventorySnapshot, AppError> {
    let catalog = inventory::build_catalog(paths, agents)?;
    Ok(build_snapshot(paths, &catalog, agents))
}

/// One-shot detail for fixtures.
pub(crate) fn detail_with_agents(
    paths: &AppPaths,
    agents: &[AgentDir],
    skill_id: &str,
) -> Result<SkillDetail, AppError> {
    security::validate_skill_id(skill_id)?;
    let catalog = inventory::build_catalog(paths, agents)?;
    let blocked = blocked_agents(agents, &catalog.canonical_dir);
    let view = build_view(paths, &catalog, &blocked, skill_id)
        .ok_or_else(|| AppError::skill_not_found(skill_id))?;
    Ok(view.to_detail(paths))
}

/// One-shot root resolution for fixtures.
pub(crate) fn resolve_root_with_agents(
    paths: &AppPaths,
    agents: &[AgentDir],
    skill_id: &str,
) -> Result<PathBuf, AppError> {
    let catalog = inventory::build_catalog(paths, agents)?;
    resolve_root_in_catalog(paths, &catalog, agents, skill_id)
}

fn build_snapshot(paths: &AppPaths, catalog: &Catalog, agents: &[AgentDir]) -> InventorySnapshot {
    let blocked = blocked_agents(agents, &catalog.canonical_dir);
    let mut skills = Vec::new();
    let mut stale = Vec::new();
    for name in &catalog.names {
        match build_view(paths, catalog, &blocked, name) {
            Some(view) => skills.push(view.to_inventory(paths)),
            None => stale.push(name.clone()),
        }
    }
    let anomalies = anomalies(stale, &skills);
    let (agents, extra_warnings) = inventory::agent_rows(paths, agents, &catalog.canonical_dir);
    let mut warnings = catalog.warnings.clone();
    warnings.extend(extra_warnings);
    InventorySnapshot {
        skills,
        agents,
        anomalies,
        warnings,
        generated_at_epoch_ms: now_ms(),
    }
}

fn build_view(
    paths: &AppPaths,
    catalog: &Catalog,
    blocked: &BTreeMap<&str, bool>,
    name: &str,
) -> Option<SkillView> {
    let located = inventory::locate_in_catalog(catalog, name)?;
    let lock_entry = located.lock_entry.clone();
    let canonical_path = located.canonical_path.clone();
    let primary_path = located.primary_path.clone();
    let primary = primary_path.as_deref();
    let (description, warnings) = read_description(primary);
    let assessment = BaselineStore::new(paths).assess(
        name,
        primary,
        lock_entry
            .as_ref()
            .map(|entry| entry.skill_folder_hash.as_str()),
    );
    let (local_modification, local_modification_detail) =
        if located.management == ManagementState::Unmanaged {
            (
                LocalModification::Unknown,
                Some("未受管：没有 lock 条目，无法判断本地改动。".to_string()),
            )
        } else {
            (assessment.state, assessment.detail)
        };

    let attachments: Vec<AgentAttachment> = located
        .attachments
        .iter()
        .map(|attachment| AgentAttachment {
            agent_id: attachment.agent_id.clone(),
            agent_name: attachment.agent_name.clone(),
            path: paths.display(&attachment.path),
            mode: attachment.mode,
            detachable: attachment_target::detach_eligibility(
                located.management,
                canonical_path.is_some(),
                blocked
                    .get(attachment.agent_id.as_str())
                    .copied()
                    .unwrap_or(false),
                Some(attachment.mode),
            ) == DetachEligibility::Detachable,
        })
        .collect();
    let broken_warnings: Vec<String> = located
        .attachments
        .iter()
        .filter(|attachment| attachment.mode == AttachmentMode::Broken)
        .map(|attachment| format!("断链：{}", attachment.agent_name))
        .collect();
    let directory_mtime = primary.and_then(directory_mtime_epoch_ms);

    Some(SkillView {
        name: name.to_string(),
        description,
        source: lock_entry.as_ref().map(|entry| entry.source.clone()),
        source_type: lock_entry.as_ref().map(|entry| entry.source_type.clone()),
        reference: lock_entry
            .as_ref()
            .and_then(|entry| entry.reference.clone()),
        skill_path: lock_entry
            .as_ref()
            .and_then(|entry| entry.skill_path.clone()),
        installed_at: lock_entry.as_ref().map(|entry| entry.installed_at.clone()),
        updated_at: lock_entry.as_ref().map(|entry| entry.updated_at.clone()),
        hash: lock_entry
            .as_ref()
            .map(|entry| entry.skill_folder_hash.clone())
            .filter(|hash| !hash.is_empty()),
        management: located.management,
        canonical_path,
        primary_path,
        attachments,
        local_modification,
        local_modification_detail,
        directory_mtime_epoch_ms: directory_mtime,
        warnings,
        broken_warnings,
    })
}

fn read_description(primary: Option<&Path>) -> (Option<String>, Vec<String>) {
    let Some(primary) = primary else {
        return (None, vec!["未找到 SKILL.md".to_string()]);
    };
    let skill_md = read_skill_md(primary);
    match skill_md.status {
        SkillMdStatus::Ok => match frontmatter::parse(skill_md.content.as_deref().unwrap_or("")) {
            Ok(Some(frontmatter)) => (frontmatter.description, Vec::new()),
            Ok(None) => (None, Vec::new()),
            Err(error) => (
                None,
                vec![format!("SKILL.md frontmatter 解析失败：{error}")],
            ),
        },
        SkillMdStatus::Missing => (None, vec!["未找到 SKILL.md".to_string()]),
        SkillMdStatus::TooLarge => (None, vec!["SKILL.md 超过 1 MiB，未加载内容。".to_string()]),
        SkillMdStatus::Unreadable => (
            None,
            vec![format!(
                "无法读取 SKILL.md：{}",
                skill_md.error.unwrap_or_else(|| "未知错误".to_string())
            )],
        ),
    }
}

struct SkillView {
    name: String,
    description: Option<String>,
    source: Option<String>,
    source_type: Option<String>,
    reference: Option<String>,
    skill_path: Option<String>,
    installed_at: Option<String>,
    updated_at: Option<String>,
    hash: Option<String>,
    management: ManagementState,
    canonical_path: Option<PathBuf>,
    primary_path: Option<PathBuf>,
    attachments: Vec<AgentAttachment>,
    local_modification: LocalModification,
    local_modification_detail: Option<String>,
    directory_mtime_epoch_ms: Option<u64>,
    warnings: Vec<String>,
    broken_warnings: Vec<String>,
}

impl SkillView {
    fn to_inventory(&self, paths: &AppPaths) -> InventorySkill {
        InventorySkill {
            name: self.name.clone(),
            description: self.description.clone(),
            source: self.source.clone(),
            management: self.management,
            updated_at: self.updated_at.clone(),
            canonical_path: self.canonical_path.as_ref().map(|path| paths.display(path)),
            attachments: self.attachments.clone(),
            local_modification: self.local_modification,
            warnings: self.warnings.clone(),
        }
    }

    fn to_detail(&self, paths: &AppPaths) -> SkillDetail {
        let mut warnings = self.warnings.clone();
        for warning in &self.broken_warnings {
            push_unique(&mut warnings, warning.clone());
        }
        SkillDetail {
            id: self.name.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            source: self.source.clone(),
            source_type: self.source_type.clone(),
            reference: self.reference.clone(),
            skill_path: self.skill_path.clone(),
            installed_at: self.installed_at.clone(),
            updated_at: self.updated_at.clone(),
            canonical_path: self.canonical_path.as_ref().map(|path| paths.display(path)),
            directory_path: self.primary_path.as_ref().map(|path| paths.display(path)),
            directory_mtime_epoch_ms: self.directory_mtime_epoch_ms,
            hash: self.hash.clone(),
            management: self.management,
            local_modification: self.local_modification,
            local_modification_detail: self.local_modification_detail.clone(),
            attachments: self.attachments.clone(),
            warnings,
        }
    }
}

fn blocked_agents<'a>(agents: &'a [AgentDir], canonical_dir: &Path) -> BTreeMap<&'a str, bool> {
    agents
        .iter()
        .map(|agent| {
            (
                agent.id.as_str(),
                agent.universal || inventory::same_dir(&agent.skills_dir, canonical_dir),
            )
        })
        .collect()
}

fn anomalies(stale: Vec<String>, skills: &[InventorySkill]) -> AnomalySummary {
    let unmanaged: Vec<String> = skills
        .iter()
        .filter(|skill| skill.management == ManagementState::Unmanaged)
        .map(|skill| skill.name.clone())
        .collect();
    let broken: Vec<String> = skills
        .iter()
        .flat_map(|skill| {
            skill
                .attachments
                .iter()
                .filter(|attachment| attachment.mode == AttachmentMode::Broken)
                .map(|attachment| format!("{} · {}", skill.name, attachment.agent_name))
        })
        .collect();
    let modified: Vec<String> = skills
        .iter()
        .filter(|skill| skill.management != ManagementState::Unmanaged)
        .filter_map(|skill| match skill.local_modification {
            LocalModification::Clean => None,
            LocalModification::Modified => Some(format!("{} · 本地改动", skill.name)),
            LocalModification::Unknown => Some(format!("{} · 本地改动?", skill.name)),
        })
        .collect();

    let stale_group = group(stale.iter().cloned(), stale.len());
    let unmanaged_group = group(unmanaged.iter().cloned(), unmanaged.len());
    let broken_count = broken.len();
    let broken_group = group(broken.into_iter(), broken_count);
    let modified_count = modified.len();
    let modified_group = group(modified.into_iter(), modified_count);
    AnomalySummary {
        total: stale_group.count
            + unmanaged_group.count
            + broken_group.count
            + modified_group.count,
        stale: stale_group,
        unmanaged: unmanaged_group,
        broken: broken_group,
        modified: modified_group,
    }
}

fn group(items: impl Iterator<Item = String>, count: usize) -> AnomalyGroup {
    AnomalyGroup {
        count,
        examples: items.take(3).collect(),
    }
}

fn resolve_root_in_catalog(
    paths: &AppPaths,
    catalog: &Catalog,
    agents: &[AgentDir],
    skill_id: &str,
) -> Result<PathBuf, AppError> {
    security::validate_skill_id(skill_id)?;
    let located = inventory::locate_in_catalog(catalog, skill_id)
        .ok_or_else(|| AppError::skill_not_found(skill_id))?;
    if located.management == ManagementState::CopyOnly {
        let copies: Vec<_> = located
            .attachments
            .iter()
            .filter(|attachment| attachment.mode == AttachmentMode::Copy)
            .collect();
        if copies.len() > 1 {
            let detail = copies
                .iter()
                .map(|attachment| {
                    format!("{}（{}）", attachment.path.display(), attachment.agent_name)
                })
                .collect::<Vec<_>>()
                .join("、");
            return Err(AppError::skill_ambiguous(skill_id, detail));
        }
    }
    let folder = located
        .canonical_path
        .or(located.primary_path)
        .ok_or_else(|| AppError::skill_not_found(skill_id))?;
    let roots = allowed_roots(paths, agents);
    security::ensure_allowed(&folder, &roots)?;

    let metadata = fs::metadata(&folder).map_err(|error| {
        AppError::new(crate::error::codes::SKILL_NOT_FOUND, "skill 目录不存在。")
            .detail(format!("{}（{error}）", folder.display()))
    })?;
    if !metadata.is_dir() {
        return Err(AppError::path_invalid(format!(
            "目标不是目录：{}",
            folder.display()
        )));
    }
    Ok(folder)
}

fn allowed_roots(paths: &AppPaths, agents: &[AgentDir]) -> Vec<AllowedRoot> {
    let canonical = paths.canonical_skills_dir();
    let mut roots = vec![AllowedRoot::canonical(canonical.clone())];
    for agent in agents {
        if inventory::same_dir(&agent.skills_dir, &canonical) {
            continue;
        }
        if roots
            .iter()
            .any(|root| inventory::same_dir(&root.path, &agent.skills_dir))
        {
            continue;
        }
        roots.push(AllowedRoot::agent(agent.skills_dir.clone()));
    }
    roots
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    struct Fixture {
        _tmp: tempfile::TempDir,
        paths: AppPaths,
        canonical: PathBuf,
        agents: Vec<AgentDir>,
    }

    impl Fixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path().join("home");
            let paths = AppPaths::new(&home, tmp.path().join("app-data"));
            paths.ensure_dirs().unwrap();
            let canonical_root = paths.canonical_skills_dir();
            fs::create_dir_all(&canonical_root).unwrap();
            let canonical = canonical_root.join("alpha");
            fs::create_dir_all(&canonical).unwrap();
            fs::write(
                canonical.join("SKILL.md"),
                "---\nname: alpha\ndescription: alpha desc\n---\n# Alpha\n",
            )
            .unwrap();
            let agent_dir = home.join(".claude/skills");
            fs::create_dir_all(&agent_dir).unwrap();
            symlink(&canonical, agent_dir.join("alpha")).unwrap();
            let agents = vec![AgentDir {
                id: "claude-code".to_string(),
                display_name: "Claude Code".to_string(),
                universal: false,
                skills_dir: agent_dir,
            }];
            Self {
                _tmp: tmp,
                paths,
                canonical,
                agents,
            }
        }

        fn model(&self) -> ReadModel {
            ReadModel::with_agents(self.paths.clone(), self.agents.clone())
        }

        fn write_lock(&self) {
            fs::create_dir_all(self.paths.agents_root()).unwrap();
            fs::write(self.paths.lock_path(), r#"{"version":3,"skills":{}}"#).unwrap();
        }
    }

    #[test]
    fn snapshot_primes_detail_and_root_without_rebuilding() {
        let fixture = Fixture::new();
        let model = fixture.model();
        let snapshot = model.snapshot().unwrap();
        assert_eq!(snapshot.skills.len(), 1);
        assert_eq!(model.build_count(), 1);
        let detail = model.detail("alpha").unwrap();
        assert_eq!(detail.description.as_deref(), Some("alpha desc"));
        assert_eq!(model.build_count(), 1);
        assert_eq!(model.resolve_root("alpha").unwrap(), fixture.canonical);
        assert_eq!(model.build_count(), 1);
    }

    #[test]
    fn fingerprint_change_rebuilds_the_catalog() {
        let fixture = Fixture::new();
        let model = fixture.model();
        model.snapshot().unwrap();
        model.detail("alpha").unwrap();
        assert_eq!(model.build_count(), 1);
        fixture.write_lock();
        model.detail("alpha").unwrap();
        assert_eq!(model.build_count(), 2);
    }

    #[test]
    fn invalidate_forces_a_rebuild() {
        let fixture = Fixture::new();
        let model = fixture.model();
        model.snapshot().unwrap();
        model.detail("alpha").unwrap();
        assert_eq!(model.build_count(), 1);
        model.invalidate();
        model.detail("alpha").unwrap();
        assert_eq!(model.build_count(), 2);
    }

    #[test]
    fn snapshot_always_rebuilds_the_catalog() {
        let fixture = Fixture::new();
        let model = fixture.model();
        model.snapshot().unwrap();
        model.snapshot().unwrap();
        assert_eq!(model.build_count(), 2);
    }

    #[test]
    fn detail_reads_skill_md_fresh_without_rebuilding_the_catalog() {
        let fixture = Fixture::new();
        let model = fixture.model();
        model.snapshot().unwrap();
        assert_eq!(
            model.detail("alpha").unwrap().description.as_deref(),
            Some("alpha desc")
        );
        fs::write(
            fixture.canonical.join("SKILL.md"),
            "---\nname: alpha\ndescription: edited desc\n---\n# Alpha\n",
        )
        .unwrap();
        let detail = model.detail("alpha").unwrap();
        assert_eq!(detail.description.as_deref(), Some("edited desc"));
        assert_eq!(model.build_count(), 1);
    }
}
