//! One owner for the per-scope lock/filesystem read model.
//!
//! Each scope (global or one registered project) has its own canonical store,
//! lock and agent dirs. The catalog cache is keyed by scope; a fingerprint over
//! the lock, canonical dir, agent dirs and `agents.json` decides when `detail`
//! and content reads must rebuild. `get_inventory` is always the refresh point
//! and rebuilds only the requested scope, so a broken project never disturbs
//! the global cache or another project.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

use crate::attachment_target::{self, DetachEligibility};
use crate::baseline::{BaselineStore, LocalModification};
use crate::detail::{SkillDetail, SkillMdStatus, directory_mtime_epoch_ms, read_skill_md};
use crate::error::AppError;
use crate::inventory::{
    self, AgentAttachment, AgentDir, AnomalyGroup, AnomalySummary, AttachmentMode, Catalog,
    InventorySkill, InventorySnapshot, ManagementState, frontmatter,
};
use crate::paths::AppPaths;
use crate::projects::ProjectRegistry;
use crate::scope::ScopeId;
use crate::scope_layout::ScopeLayout;
use crate::security::{self, AllowedRoot};
use crate::time::now_ms;

pub struct ReadModel {
    paths: AppPaths,
    projects: Arc<ProjectRegistry>,
    cache: Mutex<HashMap<ScopeId, CachedCatalog>>,
    /// Injectable detected-agent list for fixtures. Production leaves it empty
    /// and runs detection.
    agents_override: Mutex<Option<Vec<AgentDir>>>,
    #[cfg(test)]
    catalog_builds: std::sync::atomic::AtomicU64,
}

struct CachedCatalog {
    catalog: Arc<Catalog>,
    layout: Arc<ScopeLayout>,
    fingerprint: Fingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Fingerprint {
    /// Distinguishes a relocated project even when both roots have identical
    /// lock/canonical stamps; `None` for global.
    root: Option<PathBuf>,
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
    fn capture(paths: &AppPaths, layout: &ScopeLayout) -> Self {
        Self {
            root: layout.project.as_ref().map(|_| layout.root.clone()),
            lock: FileStamp::capture(&layout.lock_path),
            extras: FileStamp::capture(&paths.agents_config_path()),
            canonical: DirStamp::capture(&layout.canonical_dir),
            agents: layout
                .agents
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
    /// Shared app-data registry is created on demand. Production uses
    /// `with_projects` so reads and the registry commands see one object.
    pub fn new(paths: AppPaths) -> Self {
        let projects = Arc::new(ProjectRegistry::new(paths.clone()));
        Self::with_projects(paths, projects)
    }

    pub fn with_projects(paths: AppPaths, projects: Arc<ProjectRegistry>) -> Self {
        Self {
            paths,
            projects,
            cache: Mutex::new(HashMap::new()),
            agents_override: Mutex::new(None),
            #[cfg(test)]
            catalog_builds: std::sync::atomic::AtomicU64::new(0),
        }
    }

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

    /// Resolve the paths for one scope. An unavailable project is a distinct
    /// scope state (`project.root_unavailable`), never an inventory error.
    fn layout(&self, scope: &ScopeId) -> Result<ScopeLayout, AppError> {
        match scope {
            ScopeId::Global => Ok(ScopeLayout::global(&self.paths, self.detected_agents())),
            ScopeId::Project(id) => {
                let entry = self.projects.lookup_available(*id)?;
                Ok(ScopeLayout::project(&self.paths, entry.project))
            }
        }
    }

    /// Rebuild the requested scope's catalog and every per-skill view. This is
    /// the `get_inventory` refresh point; other scopes' caches stay intact.
    pub fn snapshot(&self, scope: &ScopeId) -> Result<InventorySnapshot, AppError> {
        let layout = match self.layout(scope) {
            Ok(layout) => layout,
            Err(error) => {
                self.remove_cached(scope);
                return Err(error);
            }
        };
        let catalog = match layout.catalog(&self.paths) {
            Ok(catalog) => catalog,
            Err(error) => {
                self.remove_cached(scope);
                return Err(error);
            }
        };
        #[cfg(test)]
        self.catalog_builds
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let fingerprint = Fingerprint::capture(&self.paths, &layout);
        let snapshot = build_snapshot(&self.paths, &layout, &catalog);
        let layout = Arc::new(layout);
        self.lock_cache().insert(
            scope.clone(),
            CachedCatalog {
                catalog: Arc::new(catalog),
                layout,
                fingerprint,
            },
        );
        Ok(snapshot)
    }

    /// Project one skill from the scope's cached catalog; SKILL.md and the
    /// global baseline are read fresh.
    pub fn detail(&self, scope: &ScopeId, skill_id: &str) -> Result<SkillDetail, AppError> {
        let name = scope.parse_skill_id(skill_id)?.to_string();
        let (catalog, layout) = self.catalog(scope)?;
        let blocked = blocked_agents(&layout.agents, &catalog.canonical_dir);
        let view = build_view(
            &self.paths,
            &layout,
            &catalog,
            &blocked,
            &scope.skill_id(&name),
            &name,
        )
        .ok_or_else(|| AppError::skill_not_found(&name))?;
        Ok(view.to_detail(&self.paths))
    }

    /// Resolve the physical folder used by content browsing and open actions.
    /// The result is guaranteed to be inside one of the scope's allowed roots.
    pub fn resolve_root(&self, scope: &ScopeId, skill_id: &str) -> Result<PathBuf, AppError> {
        let name = scope.parse_skill_id(skill_id)?;
        let (catalog, layout) = self.catalog(scope)?;
        resolve_root_in_catalog(&catalog, &layout.allowed_roots(), name)
    }

    /// Explicitly drop every scope cache after an environment recheck.
    pub fn invalidate(&self) {
        self.lock_cache().clear();
    }

    fn catalog(&self, scope: &ScopeId) -> Result<(Arc<Catalog>, Arc<ScopeLayout>), AppError> {
        let cached_hit = {
            let cache = self.lock_cache();
            match cache.get(scope) {
                Some(cached) => {
                    // Project layouts are cheap to recompute and must notice a
                    // new agent dir; global detection is expensive, so reuse
                    // the cached detected agents exactly like before.
                    let layout = if scope.is_global() {
                        Some(Arc::clone(&cached.layout))
                    } else {
                        None
                    };
                    Some((layout, cached.fingerprint.clone()))
                }
                None => None,
            }
        };
        if let Some((Some(cached_layout), fingerprint)) = &cached_hit
            && Fingerprint::capture(&self.paths, cached_layout) == *fingerprint
        {
            let cache = self.lock_cache();
            if let Some(cached) = cache.get(scope) {
                return Ok((Arc::clone(&cached.catalog), Arc::clone(&cached.layout)));
            }
        }

        let layout = match self.layout(scope) {
            Ok(layout) => Arc::new(layout),
            Err(error) => {
                self.remove_cached(scope);
                return Err(error);
            }
        };
        let fingerprint = Fingerprint::capture(&self.paths, &layout);
        if let Some((None, cached_fingerprint)) = &cached_hit
            && fingerprint == *cached_fingerprint
        {
            let cache = self.lock_cache();
            if let Some(cached) = cache.get(scope) {
                return Ok((Arc::clone(&cached.catalog), Arc::clone(&layout)));
            }
        }

        let catalog = match layout.catalog(&self.paths) {
            Ok(catalog) => Arc::new(catalog),
            Err(error) => {
                self.remove_cached(scope);
                return Err(error);
            }
        };
        #[cfg(test)]
        self.catalog_builds
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.lock_cache().insert(
            scope.clone(),
            CachedCatalog {
                catalog: Arc::clone(&catalog),
                layout: Arc::clone(&layout),
                fingerprint,
            },
        );
        Ok((catalog, layout))
    }

    fn remove_cached(&self, scope: &ScopeId) {
        self.lock_cache().remove(scope);
    }

    fn lock_cache(&self) -> std::sync::MutexGuard<'_, HashMap<ScopeId, CachedCatalog>> {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// One-shot read for fixtures and integration tests (global scope).
pub(crate) fn snapshot_with_agents(
    paths: &AppPaths,
    agents: &[AgentDir],
) -> Result<InventorySnapshot, AppError> {
    ReadModel::with_agents(paths.clone(), agents.to_vec()).snapshot(&ScopeId::global())
}

/// One-shot detail for fixtures (global scope).
pub(crate) fn detail_with_agents(
    paths: &AppPaths,
    agents: &[AgentDir],
    skill_id: &str,
) -> Result<SkillDetail, AppError> {
    ReadModel::with_agents(paths.clone(), agents.to_vec()).detail(&ScopeId::global(), skill_id)
}

/// One-shot root resolution for fixtures (global scope).
#[cfg(test)]
pub(crate) fn resolve_root_with_agents(
    paths: &AppPaths,
    agents: &[AgentDir],
    skill_id: &str,
) -> Result<PathBuf, AppError> {
    ReadModel::with_agents(paths.clone(), agents.to_vec())
        .resolve_root(&ScopeId::global(), skill_id)
}

fn build_snapshot(paths: &AppPaths, layout: &ScopeLayout, catalog: &Catalog) -> InventorySnapshot {
    let blocked = blocked_agents(&layout.agents, &catalog.canonical_dir);
    let mut skills = Vec::new();
    let mut stale = Vec::new();
    let scope = layout.scope.clone();
    for name in &catalog.names {
        match build_view(
            paths,
            layout,
            catalog,
            &blocked,
            &scope.skill_id(name),
            name,
        ) {
            Some(view) => skills.push(view.to_inventory(paths)),
            None => stale.push(name.clone()),
        }
    }
    let escapes: Vec<String> = catalog
        .escapes
        .iter()
        .chain(layout.escapes.iter())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let anomalies = anomalies(stale, &skills, layout.project.is_none(), &escapes);
    let agent_rows = inventory::agent_rows(
        paths,
        &layout.agents,
        &catalog.canonical_dir,
        &layout.extras,
        layout.project.is_none(),
    );
    let mut warnings = catalog.warnings.clone();
    warnings.extend(layout.extras.warnings.iter().cloned());
    warnings.extend(layout.warnings.iter().cloned());
    InventorySnapshot {
        skills,
        agents: agent_rows,
        install_targets: inventory::install_target_rows(paths, layout),
        anomalies,
        warnings,
        generated_at_epoch_ms: now_ms(),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_view(
    paths: &AppPaths,
    layout: &ScopeLayout,
    catalog: &Catalog,
    blocked: &BTreeMap<&str, bool>,
    id: &str,
    name: &str,
) -> Option<SkillView> {
    let located = inventory::locate_in_catalog(catalog, name)?;
    let lock_entry = located.lock_entry.clone();
    let canonical_path = located.canonical_path.clone();
    let primary_path = located.primary_path.clone();
    let primary = primary_path.as_deref();
    let project_root = layout.project.as_ref().map(|_| layout.root.as_path());
    let (description, warnings) = read_description(primary, project_root);
    let (local_modification, local_modification_detail) =
        if located.management == ManagementState::Unmanaged {
            (
                LocalModification::Unknown,
                Some("未受管：没有 lock 条目，无法判断本地改动。".to_string()),
            )
        } else {
            // Baselines are per scope: a project skill is never compared with
            // a global baseline of the same name. Project locks carry a
            // SHA-256 content hash, so only the recorded baseline can decide
            // clean vs modified; an externally installed skill stays unknown.
            let assessment = BaselineStore::for_scope(paths, &layout.scope).assess(
                name,
                primary,
                lock_entry
                    .as_ref()
                    .map(|entry| entry.skill_folder_hash.as_str()),
            );
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
        id: id.to_string(),
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
        installed_at: lock_entry
            .as_ref()
            .and_then(|entry| entry.installed_at.clone()),
        updated_at: lock_entry
            .as_ref()
            .and_then(|entry| entry.updated_at.clone()),
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

fn read_description(
    primary: Option<&Path>,
    project_root: Option<&Path>,
) -> (Option<String>, Vec<String>) {
    let Some(primary) = primary else {
        return (None, vec!["未找到 SKILL.md".to_string()]);
    };
    if let Some(root) = project_root {
        match fs::canonicalize(primary) {
            Ok(resolved) if !resolved.starts_with(root) => {
                return (
                    None,
                    vec!["skill 路径解析到项目根之外，已拒绝读取。".to_string()],
                );
            }
            Ok(_) => {}
            Err(error) => {
                return (None, vec![format!("无法解析 skill 目录：{error}")]);
            }
        }
    }
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
    id: String,
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
            id: self.id.clone(),
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
            id: self.id.clone(),
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

fn anomalies(
    stale: Vec<String>,
    skills: &[InventorySkill],
    count_unknown_modified: bool,
    escapes: &[String],
) -> AnomalySummary {
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
            LocalModification::Unknown if count_unknown_modified => {
                Some(format!("{} · 本地改动?", skill.name))
            }
            LocalModification::Unknown => None,
        })
        .collect();

    let stale_group = group(stale.iter().cloned(), stale.len());
    let unmanaged_group = group(unmanaged.iter().cloned(), unmanaged.len());
    let broken_count = broken.len();
    let broken_group = group(broken.into_iter(), broken_count);
    let modified_count = modified.len();
    let modified_group = group(modified.into_iter(), modified_count);
    let escape_group = group(escapes.iter().cloned(), escapes.len());
    AnomalySummary {
        total: stale_group.count
            + unmanaged_group.count
            + broken_group.count
            + modified_group.count
            + escape_group.count,
        stale: stale_group,
        unmanaged: unmanaged_group,
        broken: broken_group,
        modified: modified_group,
        escape: escape_group,
    }
}

fn group(items: impl Iterator<Item = String>, count: usize) -> AnomalyGroup {
    AnomalyGroup {
        count,
        examples: items.take(3).collect(),
    }
}

fn resolve_root_in_catalog(
    catalog: &Catalog,
    allowed_roots: &[AllowedRoot],
    name: &str,
) -> Result<PathBuf, AppError> {
    security::validate_skill_id(name)?;
    let located = inventory::locate_in_catalog(catalog, name)
        .ok_or_else(|| AppError::skill_not_found(name))?;
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
            return Err(AppError::skill_ambiguous(name, detail));
        }
    }
    let folder = located
        .canonical_path
        .or(located.primary_path)
        .ok_or_else(|| AppError::skill_not_found(name))?;
    security::ensure_allowed(&folder, allowed_roots)?;

    let metadata =
        fs::metadata(&folder).map_err(|error| AppError::skill_dir_not_found(&folder, error))?;
    if !metadata.is_dir() {
        return Err(AppError::path_invalid(format!(
            "目标不是目录：{}",
            folder.display()
        )));
    }
    Ok(folder)
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
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
    fn project_cache_is_per_scope_and_fingerprint_driven() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        paths.ensure_dirs().unwrap();
        let root = tmp.path().join("project");
        fs::create_dir_all(root.join(".agents/skills/alpha")).unwrap();
        fs::write(
            root.join(".agents/skills/alpha/SKILL.md"),
            "---\nname: alpha\ndescription: project alpha\n---\n",
        )
        .unwrap();
        fs::write(
            root.join("skills-lock.json"),
            r#"{"version":1,"skills":{"alpha":{"source":"acme/alpha","sourceType":"github","sourceUrl":"https://github.com/acme/alpha.git","computedHash":"abc"}}}"#,
        )
        .unwrap();
        let registry = Arc::new(ProjectRegistry::with_clock(
            paths.clone(),
            Arc::new(|| 1_000),
        ));
        let entry = registry.add(root.to_str().unwrap()).unwrap();
        let scope = entry.project.scope();
        let model = ReadModel::with_projects(paths, registry);

        let snapshot = model.snapshot(&scope).unwrap();
        assert_eq!(snapshot.skills.len(), 1);
        assert_eq!(model.build_count(), 1);
        let alpha_id = format!("{}/alpha", scope.as_string());
        model.detail(&scope, &alpha_id).unwrap();
        assert_eq!(model.build_count(), 1, "detail reuses the scope cache");

        fs::create_dir_all(root.join(".agents/skills/beta")).unwrap();
        fs::write(
            root.join(".agents/skills/beta/SKILL.md"),
            "---\nname: beta\n---\n",
        )
        .unwrap();
        fs::write(
            root.join("skills-lock.json"),
            r#"{"version":1,"skills":{"alpha":{"source":"acme/alpha","sourceType":"github","computedHash":"abc"},"beta":{"source":"acme/beta","sourceType":"github","computedHash":"def"}}}"#,
        )
        .unwrap();
        model
            .detail(&scope, &format!("{}/beta", scope.as_string()))
            .unwrap();
        assert_eq!(model.build_count(), 2, "lock change rebuilds the scope");

        // A global refresh does not evict the project cache; the next project
        // detail still reuses it.
        model.snapshot(&ScopeId::global()).unwrap();
        assert_eq!(model.build_count(), 3);
        model.detail(&scope, &alpha_id).unwrap();
        assert_eq!(model.build_count(), 3);
    }

    #[test]
    fn corrupt_project_lock_leaves_the_global_cache_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(home.join(".agents/skills/global")).unwrap();
        fs::write(
            home.join(".agents/skills/global/SKILL.md"),
            "---\nname: global\n---\n",
        )
        .unwrap();
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        paths.ensure_dirs().unwrap();
        let root = tmp.path().join("project");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("skills-lock.json"), "{not json").unwrap();
        let registry = Arc::new(ProjectRegistry::with_clock(
            paths.clone(),
            Arc::new(|| 1_000),
        ));
        let entry = registry.add(root.to_str().unwrap()).unwrap();
        let scope = entry.project.scope();
        let model = ReadModel::with_projects(paths, registry);

        assert_eq!(model.snapshot(&ScopeId::global()).unwrap().skills.len(), 1);
        assert_eq!(model.build_count(), 1);
        let error = model.snapshot(&scope).unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::LockCorrupt);
        assert_eq!(model.build_count(), 1, "failed builds are not cached");

        let global = model.snapshot(&ScopeId::global()).unwrap();
        assert_eq!(global.skills.len(), 1);
        assert_eq!(global.skills[0].id, "global");
        assert_eq!(model.build_count(), 2);
    }

    #[test]
    fn snapshot_primes_detail_and_root_without_rebuilding() {
        let fixture = Fixture::new();
        let model = fixture.model();
        let snapshot = model.snapshot(&ScopeId::global()).unwrap();
        assert_eq!(snapshot.skills.len(), 1);
        assert_eq!(snapshot.skills[0].id, "alpha");
        assert_eq!(model.build_count(), 1);
        let detail = model.detail(&ScopeId::global(), "alpha").unwrap();
        assert_eq!(detail.id, "alpha");
        assert_eq!(detail.description.as_deref(), Some("alpha desc"));
        assert_eq!(model.build_count(), 1);
        assert_eq!(
            model.resolve_root(&ScopeId::global(), "alpha").unwrap(),
            fixture.canonical
        );
        assert_eq!(model.build_count(), 1);
    }

    #[test]
    fn fingerprint_change_rebuilds_the_catalog() {
        let fixture = Fixture::new();
        let model = fixture.model();
        model.snapshot(&ScopeId::global()).unwrap();
        model.detail(&ScopeId::global(), "alpha").unwrap();
        assert_eq!(model.build_count(), 1);
        fixture.write_lock();
        model.detail(&ScopeId::global(), "alpha").unwrap();
        assert_eq!(model.build_count(), 2);
    }

    #[test]
    fn invalidate_forces_a_rebuild() {
        let fixture = Fixture::new();
        let model = fixture.model();
        model.snapshot(&ScopeId::global()).unwrap();
        model.detail(&ScopeId::global(), "alpha").unwrap();
        assert_eq!(model.build_count(), 1);
        model.invalidate();
        model.detail(&ScopeId::global(), "alpha").unwrap();
        assert_eq!(model.build_count(), 2);
    }

    #[test]
    fn snapshot_always_rebuilds_the_catalog() {
        let fixture = Fixture::new();
        let model = fixture.model();
        model.snapshot(&ScopeId::global()).unwrap();
        model.snapshot(&ScopeId::global()).unwrap();
        assert_eq!(model.build_count(), 2);
    }

    #[test]
    fn detail_reads_skill_md_fresh_without_rebuilding_the_catalog() {
        let fixture = Fixture::new();
        let model = fixture.model();
        model.snapshot(&ScopeId::global()).unwrap();
        assert_eq!(
            model
                .detail(&ScopeId::global(), "alpha")
                .unwrap()
                .description
                .as_deref(),
            Some("alpha desc")
        );
        fs::write(
            fixture.canonical.join("SKILL.md"),
            "---\nname: alpha\ndescription: edited desc\n---\n# Alpha\n",
        )
        .unwrap();
        let detail = model.detail(&ScopeId::global(), "alpha").unwrap();
        assert_eq!(detail.description.as_deref(), Some("edited desc"));
        assert_eq!(model.build_count(), 1);
    }
}
