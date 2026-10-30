//! App-registered project roots and the active scope.
//!
//! The registry is app data (`projects.json`, version 1). It stores only the
//! canonical root path and display metadata; adding a project never touches the
//! project directory. A corrupt or unsupported file is never overwritten: the
//! registry falls back to an empty global state and reports a warning.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;
use crate::paths::AppPaths;
use crate::scope::ScopeId;
use crate::update::state::UpdateCheckStateStore;

pub const PROJECTS_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: Uuid,
    pub root: PathBuf,
    pub display_name: String,
    pub added_at: u64,
    pub last_opened_at: u64,
}

impl Project {
    pub fn scope(&self) -> ScopeId {
        ScopeId::project(self.id)
    }
}

/// A registry entry plus its current disk state. An unavailable root is a
/// value the UI can render, never a failed list command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEntry {
    #[serde(flatten)]
    pub project: Project,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectList {
    pub projects: Vec<ProjectEntry>,
    pub last_active_scope: ScopeId,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectsFile {
    version: u32,
    #[serde(default)]
    last_active_scope: ScopeId,
    #[serde(default)]
    projects: Vec<Project>,
}

impl Default for ProjectsFile {
    fn default() -> Self {
        Self {
            version: PROJECTS_VERSION,
            last_active_scope: ScopeId::global(),
            projects: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
enum Loaded {
    /// The file parsed and uses the supported version.
    Ready(ProjectsFile),
    /// No file yet: a valid empty registry.
    Missing,
    /// The file exists but cannot be used. It must be preserved untouched.
    Unusable(String),
}

pub struct ProjectRegistry {
    paths: AppPaths,
    clock: Arc<dyn Fn() -> u64 + Send + Sync>,
    lock: Mutex<()>,
}

impl ProjectRegistry {
    pub fn new(paths: AppPaths) -> Self {
        Self::with_clock(paths, Arc::new(epoch_ms))
    }

    pub fn with_clock(paths: AppPaths, clock: Arc<dyn Fn() -> u64 + Send + Sync>) -> Self {
        Self {
            paths,
            clock,
            lock: Mutex::new(()),
        }
    }

    pub fn list(&self) -> ProjectList {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        self.list_unlocked()
    }

    pub fn add(&self, root: &str) -> Result<ProjectEntry, AppError> {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let mut file = self.load_mutable()?;
        let canonical = self.validate_root(root, &file.projects, None)?;
        let now = (self.clock)();
        let project = Project {
            id: Uuid::new_v4(),
            root: canonical.clone(),
            display_name: display_name(&canonical),
            added_at: now,
            last_opened_at: now,
        };
        file.projects.push(project.clone());
        file.last_active_scope = project.scope();
        self.save(&file)?;
        Ok(self.entry(project))
    }

    pub fn relocate(&self, id: &str, root: &str) -> Result<ProjectEntry, AppError> {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let id = parse_id(id)?;
        let mut file = self.load_mutable()?;
        let index = project_index(&file.projects, id)?;
        let canonical = self.validate_root(root, &file.projects, Some(id))?;
        let now = (self.clock)();
        {
            let project = &mut file.projects[index];
            project.root = canonical;
            project.display_name = display_name(&project.root);
            project.last_opened_at = now;
        }
        let project = file.projects[index].clone();
        self.save(&file)?;
        self.forget_check_state(&project.scope());
        Ok(self.entry(project))
    }

    pub fn forget(&self, id: &str) -> Result<(), AppError> {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let id = parse_id(id)?;
        let mut file = self.load_mutable()?;
        let index = project_index(&file.projects, id)?;
        let scope = file.projects[index].scope();
        file.projects.remove(index);
        if file.last_active_scope == scope {
            file.last_active_scope = ScopeId::global();
        }
        self.save(&file)?;
        self.forget_check_state(&scope);
        Ok(())
    }

    pub fn set_active_scope(&self, scope: &ScopeId) -> Result<(), AppError> {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let mut file = self.load_mutable()?;
        match scope {
            ScopeId::Global => file.last_active_scope = ScopeId::global(),
            ScopeId::Project(id) => {
                let index = project_index(&file.projects, *id)?;
                let root = file.projects[index].root.clone();
                if let Err(reason) = availability(&root) {
                    return Err(AppError::project_root_unavailable(
                        &file.projects[index].display_name,
                        reason,
                    ));
                }
                let now = (self.clock)();
                file.projects[index].last_opened_at = now;
                file.last_active_scope = scope.clone();
            }
        }
        self.save(&file)
    }

    /// The app-level warning for a registry that exists but cannot be used.
    pub fn registry_warning(&self) -> Option<String> {
        match self.load() {
            Loaded::Unusable(detail) => Some(warning_message(&self.paths.projects_path(), &detail)),
            Loaded::Ready(_) | Loaded::Missing => None,
        }
    }

    fn list_unlocked(&self) -> ProjectList {
        match self.load() {
            Loaded::Ready(file) => {
                let warnings = Vec::new();
                let mut entries: Vec<ProjectEntry> = file
                    .projects
                    .into_iter()
                    .map(|project| self.entry(project))
                    .collect();
                entries.sort_by(|left, right| {
                    right
                        .project
                        .last_opened_at
                        .cmp(&left.project.last_opened_at)
                        .then_with(|| right.project.added_at.cmp(&left.project.added_at))
                        .then_with(|| left.project.id.cmp(&right.project.id))
                });
                let last_active_scope = resolve_active_scope(&entries, file.last_active_scope);
                ProjectList {
                    projects: entries,
                    last_active_scope,
                    warnings,
                }
            }
            Loaded::Missing => ProjectList {
                projects: Vec::new(),
                last_active_scope: ScopeId::global(),
                warnings: Vec::new(),
            },
            Loaded::Unusable(detail) => ProjectList {
                projects: Vec::new(),
                last_active_scope: ScopeId::global(),
                warnings: vec![warning_message(&self.paths.projects_path(), &detail)],
            },
        }
    }

    fn entry(&self, project: Project) -> ProjectEntry {
        match availability(&project.root) {
            Ok(()) => ProjectEntry {
                project,
                available: true,
                unavailable_reason: None,
            },
            Err(reason) => ProjectEntry {
                project,
                available: false,
                unavailable_reason: Some(reason),
            },
        }
    }

    fn load(&self) -> Loaded {
        let path = self.paths.projects_path();
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Loaded::Missing,
            Err(error) => {
                return Loaded::Unusable(format!("无法读取 {}：{error}", path.display()));
            }
        };
        let file: ProjectsFile = match serde_json::from_str(&raw) {
            Ok(file) => file,
            Err(error) => {
                return Loaded::Unusable(format!("无法解析 {}：{error}", path.display()));
            }
        };
        if file.version != PROJECTS_VERSION {
            return Loaded::Unusable(format!(
                "{} 的版本 v{} 不受支持（需要 v{PROJECTS_VERSION}）。",
                path.display(),
                file.version
            ));
        }
        Loaded::Ready(file)
    }

    /// Load for a mutation. An unusable file is an explicit error so a broken
    /// registry is never silently replaced.
    fn load_mutable(&self) -> Result<ProjectsFile, AppError> {
        match self.load() {
            Loaded::Ready(file) => Ok(file),
            Loaded::Missing => Ok(ProjectsFile::default()),
            Loaded::Unusable(detail) => Err(AppError::project_registry_corrupt(detail)),
        }
    }

    fn save(&self, file: &ProjectsFile) -> Result<(), AppError> {
        let path = self.paths.projects_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                AppError::path_app_data(format!("{}：{error}", parent.display()))
            })?;
        }
        let serialized = serde_json::to_string_pretty(file)
            .map_err(|error| AppError::project_registry_corrupt(error.to_string()))?;
        write_atomic(&path, serialized.as_bytes())
            .map_err(|error| AppError::path_app_data(format!("{}：{error}", path.display())))
    }

    fn validate_root(
        &self,
        raw: &str,
        existing: &[Project],
        skip: Option<Uuid>,
    ) -> Result<PathBuf, AppError> {
        let candidate = Path::new(raw);
        if raw.is_empty() || !candidate.is_absolute() {
            return Err(AppError::project_invalid_root(format!(
                "必须提供绝对路径：{raw:?}"
            )));
        }
        let metadata = fs::metadata(candidate).map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => {
                AppError::project_invalid_root(format!("目录不存在：{}", candidate.display()))
            }
            _ => AppError::project_invalid_root(format!(
                "无法读取目录 {}：{error}",
                candidate.display()
            )),
        })?;
        if !metadata.is_dir() {
            return Err(AppError::project_invalid_root(format!(
                "不是目录：{}",
                candidate.display()
            )));
        }
        fs::read_dir(candidate).map_err(|error| {
            AppError::project_invalid_root(format!("目录不可读 {}：{error}", candidate.display()))
        })?;
        let canonical = fs::canonicalize(candidate).map_err(|error| {
            AppError::project_invalid_root(format!("无法解析目录 {}：{error}", candidate.display()))
        })?;

        if canonical == Path::new("/") {
            return Err(AppError::project_invalid_root("不能添加文件系统根目录 /。"));
        }
        if canonical == canonical_or_raw(self.paths.home()) {
            return Err(AppError::project_invalid_root("不能添加用户 home 目录。"));
        }
        let canonical_skills = canonical_or_raw(&self.paths.canonical_skills_dir());
        if canonical == canonical_skills || canonical.starts_with(&canonical_skills) {
            return Err(AppError::project_invalid_root(format!(
                "不能添加全局 canonical store 或其内部路径：{}",
                canonical.display()
            )));
        }
        if let Some(existing) = existing
            .iter()
            .find(|project| project.root == canonical && Some(project.id) != skip)
        {
            return Err(AppError::project_duplicate(
                &canonical,
                &existing.display_name,
            ));
        }
        Ok(canonical)
    }

    fn forget_check_state(&self, scope: &ScopeId) {
        let store = UpdateCheckStateStore::new(&self.paths);
        let _ = store.forget_scope(scope);
    }
}

fn resolve_active_scope(entries: &[ProjectEntry], stored: ScopeId) -> ScopeId {
    match stored {
        ScopeId::Global => ScopeId::Global,
        ScopeId::Project(id) => entries
            .iter()
            .find(|entry| entry.project.id == id && entry.available)
            .map(|entry| entry.project.scope())
            .unwrap_or(ScopeId::Global),
    }
}

fn project_index(projects: &[Project], id: Uuid) -> Result<usize, AppError> {
    projects
        .iter()
        .position(|project| project.id == id)
        .ok_or_else(|| AppError::project_not_found(&id.to_string()))
}

fn parse_id(raw: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(raw)
        .ok()
        .filter(|id| id.to_string() == raw)
        .ok_or_else(|| AppError::project_not_found(raw))
}

fn display_name(root: &Path) -> String {
    root.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| root.display().to_string())
}

fn availability(root: &Path) -> Result<(), String> {
    let metadata = fs::metadata(root).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => "目录不存在".to_string(),
        _ => format!("无法读取目录：{error}"),
    })?;
    if !metadata.is_dir() {
        return Err("不是目录".to_string());
    }
    fs::read_dir(root).map_err(|error| format!("目录不可读：{error}"))?;
    Ok(())
}

fn canonical_or_raw(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn warning_message(path: &Path, detail: &str) -> String {
    format!(
        "项目注册表不可用，已按空列表处理，原文件保持不变（{}）。{detail}",
        path.display()
    )
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".tmp-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    fs::write(&temp, bytes)?;
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    Ok(())
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

/// Read-only helper for the environment report.
pub fn registry_warning(paths: &AppPaths) -> Option<String> {
    ProjectRegistry::new(paths.clone()).registry_warning()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::fs::symlink;

    struct TestClock {
        now: Mutex<u64>,
    }

    impl TestClock {
        fn new(start: u64) -> Arc<Self> {
            Arc::new(Self {
                now: Mutex::new(start),
            })
        }

        fn tick(&self) -> u64 {
            let mut now = self.now.lock().unwrap();
            *now += 1_000;
            *now
        }
    }

    struct Fixture {
        tmp: tempfile::TempDir,
        home: PathBuf,
        paths: AppPaths,
        registry: ProjectRegistry,
    }

    impl Fixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path().join("home");
            fs::create_dir_all(&home).unwrap();
            let paths = AppPaths::new(&home, tmp.path().join("app-data"));
            let clock = TestClock::new(1_000);
            let tick = Arc::clone(&clock);
            let registry =
                ProjectRegistry::with_clock(paths.clone(), Arc::new(move || tick.tick()));
            Self {
                tmp,
                home,
                paths,
                registry,
            }
        }

        fn root(&self, name: &str) -> PathBuf {
            let root = self.tmp.path().join("projects").join(name);
            fs::create_dir_all(&root).unwrap();
            root
        }

        fn add(&self, root: &Path) -> ProjectEntry {
            self.registry.add(root.to_str().unwrap()).unwrap()
        }

        fn scope(&self) -> ScopeId {
            self.registry.list().last_active_scope
        }

        fn raw_file(&self) -> serde_json::Value {
            serde_json::from_str(&fs::read_to_string(self.paths.projects_path()).unwrap()).unwrap()
        }
    }

    #[test]
    fn missing_registry_lists_nothing_and_stays_global_without_creating_a_file() {
        let fixture = Fixture::new();
        let list = fixture.registry.list();
        assert!(list.projects.is_empty());
        assert_eq!(list.last_active_scope, ScopeId::global());
        assert!(list.warnings.is_empty());
        assert!(!fixture.paths.projects_path().exists());
    }

    #[test]
    fn add_records_only_the_canonical_path_and_activates_the_project() {
        let fixture = Fixture::new();
        let root = fixture.root("my-app");
        fs::write(root.join("README.md"), "hello").unwrap();
        let before: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();

        let entry = fixture.add(&root);

        assert_eq!(entry.project.root, fs::canonicalize(&root).unwrap());
        assert_eq!(entry.project.display_name, "my-app");
        assert_eq!(entry.project.added_at, 2_000);
        assert_eq!(entry.project.last_opened_at, 2_000);
        assert!(entry.available);
        assert_eq!(entry.unavailable_reason, None);
        assert_eq!(entry.project.id.get_version_num(), 4);
        assert_eq!(fixture.scope(), entry.project.scope());

        let after: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(before, after, "adding a project must not create files");
        assert!(!root.join(".agents").exists());
        assert!(!root.join("skills-lock.json").exists());

        let raw = fixture.raw_file();
        assert_eq!(raw["version"], 1);
        assert_eq!(raw["lastActiveScope"], entry.project.scope().as_string());
        assert_eq!(raw["projects"][0]["displayName"], "my-app");
        assert_eq!(
            raw["projects"][0]["root"],
            entry.project.root.to_str().unwrap()
        );
    }

    #[test]
    fn adds_are_persisted_atomically_and_reload_cleanly() {
        let fixture = Fixture::new();
        let root = fixture.root("alpha");
        let entry = fixture.add(&root);

        let leftovers: Vec<_> = fs::read_dir(fixture.paths.app_data())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "temp files remain: {leftovers:?}");

        let reloaded = ProjectRegistry::new(fixture.paths.clone()).list();
        assert_eq!(reloaded.projects.len(), 1);
        assert_eq!(reloaded.projects[0].project.id, entry.project.id);
        assert_eq!(reloaded.last_active_scope, entry.project.scope());
    }

    #[test]
    fn rejects_missing_non_directory_relative_and_reserved_roots() {
        let fixture = Fixture::new();
        let file = fixture.tmp.path().join("plain.txt");
        fs::write(&file, "x").unwrap();
        let canonical = fixture.paths.canonical_skills_dir();
        fs::create_dir_all(canonical.join("inside")).unwrap();

        let cases: Vec<(String, &str)> = vec![
            (
                fixture
                    .tmp
                    .path()
                    .join("missing")
                    .to_string_lossy()
                    .into_owned(),
                "missing",
            ),
            (file.to_string_lossy().into_owned(), "file"),
            ("relative/path".to_string(), "relative"),
            ("/".to_string(), "filesystem root"),
            (fixture.home.to_string_lossy().into_owned(), "home"),
            (canonical.to_string_lossy().into_owned(), "canonical store"),
            (
                canonical.join("inside").to_string_lossy().into_owned(),
                "inside canonical store",
            ),
        ];
        for (root, label) in cases {
            let error = fixture.registry.add(&root).unwrap_err();
            assert_eq!(
                error.code,
                ErrorCode::ProjectInvalidRoot,
                "{label}: {error:?}"
            );
        }
        assert!(fixture.registry.list().projects.is_empty());
        assert!(!fixture.paths.projects_path().exists());
    }

    #[test]
    fn rejects_unreadable_directories() {
        let fixture = Fixture::new();
        let root = fixture.root("locked");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o000)).unwrap();
        let readable = fs::read_dir(&root).is_ok();
        let result = fixture.registry.add(root.to_str().unwrap());
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        if readable {
            // Running with elevated permissions: the unreadable case cannot be
            // exercised, so only assert the cleanup path is sane.
            return;
        }
        let error = result.unwrap_err();
        assert_eq!(error.code, ErrorCode::ProjectInvalidRoot);
        assert!(fixture.registry.list().projects.is_empty());
    }

    #[test]
    fn dedupes_canonical_paths_and_allows_nested_and_overlapping_roots() {
        let fixture = Fixture::new();
        let parent = fixture.root("parent");
        let child = parent.join("child");
        fs::create_dir_all(&child).unwrap();
        let alias = fixture.tmp.path().join("alias");
        symlink(&parent, &alias).unwrap();

        let first = fixture.add(&parent);
        let duplicate = fixture.registry.add(parent.to_str().unwrap()).unwrap_err();
        assert_eq!(duplicate.code, ErrorCode::ProjectDuplicate);
        let aliased = fixture.registry.add(alias.to_str().unwrap()).unwrap_err();
        assert_eq!(aliased.code, ErrorCode::ProjectDuplicate);

        // Nested and overlapping distinct roots are allowed: the same directory
        // reached through a symlink is not a distinct root.
        let nested = fixture.add(&child);
        assert!(nested.project.root.starts_with(&first.project.root));
        assert_eq!(fixture.registry.list().projects.len(), 2);
    }

    #[test]
    fn list_orders_by_last_opened_at_descending() {
        let fixture = Fixture::new();
        let alpha = fixture.add(&fixture.root("alpha"));
        let beta = fixture.add(&fixture.root("beta"));
        let gamma = fixture.add(&fixture.root("gamma"));

        let order = |registry: &ProjectRegistry| -> Vec<String> {
            registry
                .list()
                .projects
                .into_iter()
                .map(|entry| entry.project.display_name)
                .collect()
        };
        assert_eq!(order(&fixture.registry), vec!["gamma", "beta", "alpha"]);

        fixture
            .registry
            .set_active_scope(&alpha.project.scope())
            .unwrap();
        assert_eq!(order(&fixture.registry), vec!["alpha", "gamma", "beta"]);
        assert_eq!(fixture.scope(), alpha.project.scope());

        let relocated = fixture
            .registry
            .relocate(
                &beta.project.id.to_string(),
                fixture.root("beta-2").to_str().unwrap(),
            )
            .unwrap();
        assert_eq!(relocated.project.id, beta.project.id);
        assert_eq!(relocated.project.display_name, "beta-2");
        assert_eq!(order(&fixture.registry), vec!["beta-2", "alpha", "gamma"]);
        let _ = gamma;
    }

    #[test]
    fn corrupt_registry_is_reported_and_never_overwritten() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.paths.app_data()).unwrap();
        let corrupt = "{not json";
        fs::write(fixture.paths.projects_path(), corrupt).unwrap();

        let list = fixture.registry.list();
        assert!(list.projects.is_empty());
        assert_eq!(list.last_active_scope, ScopeId::global());
        assert_eq!(list.warnings.len(), 1);
        assert!(list.warnings[0].contains("项目注册表不可用"));
        assert!(list.warnings[0].contains("projects.json"));
        assert_eq!(
            fs::read_to_string(fixture.paths.projects_path()).unwrap(),
            corrupt
        );
        assert_eq!(
            registry_warning(&fixture.paths),
            Some(list.warnings[0].clone())
        );

        let root = fixture.root("alpha");
        for error in [
            fixture.registry.add(root.to_str().unwrap()).unwrap_err(),
            fixture
                .registry
                .set_active_scope(&ScopeId::global())
                .unwrap_err(),
            fixture
                .registry
                .forget(&Uuid::new_v4().to_string())
                .unwrap_err(),
        ] {
            assert_eq!(error.code, ErrorCode::ProjectRegistryCorrupt, "{error:?}");
        }
        assert_eq!(
            fs::read_to_string(fixture.paths.projects_path()).unwrap(),
            corrupt
        );
    }

    #[test]
    fn unsupported_registry_versions_are_preserved_like_corrupt_files() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.paths.app_data()).unwrap();
        let future = serde_json::json!({
            "version": 99,
            "lastActiveScope": "global",
            "projects": []
        })
        .to_string();
        fs::write(fixture.paths.projects_path(), &future).unwrap();

        let list = fixture.registry.list();
        assert!(list.projects.is_empty());
        assert!(list.warnings[0].contains("v99"));
        let error = fixture
            .registry
            .add(fixture.root("alpha").to_str().unwrap())
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::ProjectRegistryCorrupt);
        assert_eq!(
            fs::read_to_string(fixture.paths.projects_path()).unwrap(),
            future
        );
    }

    #[test]
    fn unavailable_roots_are_listed_and_the_active_scope_falls_back_to_global() {
        let fixture = Fixture::new();
        let root = fixture.root("alpha");
        let entry = fixture.add(&root);
        assert_eq!(fixture.scope(), entry.project.scope());

        fs::remove_dir_all(&root).unwrap();
        let list = fixture.registry.list();
        assert_eq!(list.projects.len(), 1);
        assert_eq!(list.projects[0].project.id, entry.project.id);
        assert!(!list.projects[0].available);
        assert_eq!(
            list.projects[0].unavailable_reason.as_deref(),
            Some("目录不存在")
        );
        assert_eq!(list.last_active_scope, ScopeId::global());
        assert_eq!(fixture.raw_file()["projects"].as_array().unwrap().len(), 1);

        let error = fixture
            .registry
            .set_active_scope(&entry.project.scope())
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::ProjectRootUnavailable);

        fs::write(&root, "now a file").unwrap();
        let list = fixture.registry.list();
        assert!(!list.projects[0].available);
        assert_eq!(
            list.projects[0].unavailable_reason.as_deref(),
            Some("不是目录")
        );
    }

    #[test]
    fn forgetting_removes_only_the_entry_and_its_check_state() {
        let fixture = Fixture::new();
        let alpha_root = fixture.root("alpha");
        fs::write(alpha_root.join("keep.md"), "keep").unwrap();
        let alpha = fixture.add(&alpha_root);
        let beta_root = fixture.root("beta");
        let beta = fixture.add(&beta_root);
        assert_eq!(fixture.scope(), beta.project.scope());

        let check_path = fixture.paths.update_check_state_path();
        fs::create_dir_all(check_path.parent().unwrap()).unwrap();
        let check_state = serde_json::json!({
            "version": 2,
            "scopes": {
                "global": { "version": 1, "lastAttemptAt": 1 },
                alpha.project.scope().as_string(): { "version": 1, "lastAttemptAt": 2 },
                beta.project.scope().as_string(): { "version": 1, "lastAttemptAt": 3 }
            }
        });
        fs::write(&check_path, check_state.to_string()).unwrap();

        fixture
            .registry
            .forget(&alpha.project.id.to_string())
            .unwrap();

        let list = fixture.registry.list();
        assert_eq!(list.projects.len(), 1);
        assert_eq!(list.projects[0].project.id, beta.project.id);
        assert!(alpha_root.is_dir());
        assert_eq!(
            fs::read_to_string(alpha_root.join("keep.md")).unwrap(),
            "keep"
        );
        let saved: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&check_path).unwrap()).unwrap();
        assert!(saved["scopes"].get("global").is_some());
        assert!(
            saved["scopes"]
                .get(beta.project.scope().as_string())
                .is_some()
        );
        assert!(
            saved["scopes"]
                .get(alpha.project.scope().as_string())
                .is_none()
        );

        fixture
            .registry
            .forget(&beta.project.id.to_string())
            .unwrap();
        assert!(fixture.registry.list().projects.is_empty());
        assert_eq!(fixture.scope(), ScopeId::global());
        assert!(beta_root.is_dir());

        let missing = fixture
            .registry
            .forget(&Uuid::new_v4().to_string())
            .unwrap_err();
        assert_eq!(missing.code, ErrorCode::ProjectNotFound);
    }

    #[test]
    fn relocate_preserves_identity_and_clears_scope_state() {
        let fixture = Fixture::new();
        let old_root = fixture.root("old");
        fs::write(old_root.join("keep.md"), "keep").unwrap();
        let entry = fixture.add(&old_root);
        let added_at = entry.project.added_at;
        let new_root = fixture.root("new-name");

        let check_path = fixture.paths.update_check_state_path();
        fs::create_dir_all(check_path.parent().unwrap()).unwrap();
        fs::write(
            &check_path,
            serde_json::json!({
                "version": 2,
                "scopes": {
                    "global": { "version": 1, "lastAttemptAt": 1 },
                    entry.project.scope().as_string(): { "version": 1, "lastAttemptAt": 2 }
                }
            })
            .to_string(),
        )
        .unwrap();

        let relocated = fixture
            .registry
            .relocate(&entry.project.id.to_string(), new_root.to_str().unwrap())
            .unwrap();

        assert_eq!(relocated.project.id, entry.project.id);
        assert_eq!(relocated.project.added_at, added_at);
        assert_eq!(relocated.project.root, fs::canonicalize(&new_root).unwrap());
        assert_eq!(relocated.project.display_name, "new-name");
        assert!(relocated.project.last_opened_at > entry.project.last_opened_at);
        assert_eq!(fixture.scope(), relocated.project.scope());
        assert!(old_root.is_dir());
        assert_eq!(
            fs::read_to_string(old_root.join("keep.md")).unwrap(),
            "keep"
        );
        let saved: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&check_path).unwrap()).unwrap();
        assert!(saved["scopes"].get("global").is_some());
        assert!(
            saved["scopes"]
                .get(entry.project.scope().as_string())
                .is_none()
        );

        let other = fixture.add(&fixture.root("other"));
        let duplicate = fixture
            .registry
            .relocate(
                &relocated.project.id.to_string(),
                other.project.root.to_str().unwrap(),
            )
            .unwrap_err();
        assert_eq!(duplicate.code, ErrorCode::ProjectDuplicate);
        let current = fixture
            .registry
            .list()
            .projects
            .into_iter()
            .find(|entry| entry.project.id == relocated.project.id)
            .unwrap();
        assert_eq!(current.project.root, fs::canonicalize(&new_root).unwrap());

        // Relocating to the project's current root is a no-op update, not a
        // duplicate of itself.
        let same = fixture
            .registry
            .relocate(
                &relocated.project.id.to_string(),
                new_root.to_str().unwrap(),
            )
            .unwrap();
        assert_eq!(same.project.id, relocated.project.id);
        assert!(same.project.last_opened_at > relocated.project.last_opened_at);

        let missing = fixture
            .registry
            .relocate(&Uuid::new_v4().to_string(), new_root.to_str().unwrap())
            .unwrap_err();
        assert_eq!(missing.code, ErrorCode::ProjectNotFound);
    }

    #[test]
    fn set_active_scope_persists_and_bumps_last_opened_at() {
        let fixture = Fixture::new();
        let alpha = fixture.add(&fixture.root("alpha"));
        let beta = fixture.add(&fixture.root("beta"));
        assert_eq!(fixture.scope(), beta.project.scope());

        fixture
            .registry
            .set_active_scope(&alpha.project.scope())
            .unwrap();
        assert_eq!(fixture.scope(), alpha.project.scope());
        let reopened = fixture
            .registry
            .list()
            .projects
            .into_iter()
            .find(|entry| entry.project.id == alpha.project.id)
            .unwrap();
        assert_eq!(reopened.project.last_opened_at, 4_000);
        assert_eq!(
            fixture.raw_file()["lastActiveScope"],
            alpha.project.scope().as_string()
        );

        fixture
            .registry
            .set_active_scope(&ScopeId::global())
            .unwrap();
        assert_eq!(fixture.scope(), ScopeId::global());
        assert_eq!(fixture.raw_file()["lastActiveScope"], "global");

        let missing = fixture
            .registry
            .set_active_scope(&ScopeId::project(Uuid::new_v4()))
            .unwrap_err();
        assert_eq!(missing.code, ErrorCode::ProjectNotFound);
    }

    #[test]
    fn list_serializes_the_frontend_contract() {
        let fixture = Fixture::new();
        let entry = fixture.add(&fixture.root("my-app"));
        let list = fixture.registry.list();
        let json = serde_json::to_value(&list).unwrap();
        assert_eq!(json["lastActiveScope"], entry.project.scope().as_string());
        assert!(json["warnings"].as_array().unwrap().is_empty());
        let project = &json["projects"][0];
        assert_eq!(project["id"], entry.project.id.to_string());
        assert_eq!(project["root"], entry.project.root.to_str().unwrap());
        assert_eq!(project["displayName"], "my-app");
        assert_eq!(project["addedAt"], 2_000);
        assert_eq!(project["lastOpenedAt"], 2_000);
        assert_eq!(project["available"], true);
        assert!(project.get("unavailableReason").is_none());
    }

    #[test]
    fn restores_last_active_scope_only_when_the_project_is_available() {
        let fixture = Fixture::new();
        let root = fixture.root("alpha");
        let entry = fixture.add(&root);
        let file = fixture.raw_file();

        let mut future = file.clone();
        future["lastActiveScope"] = serde_json::json!(ScopeId::project(Uuid::new_v4()).as_string());
        fs::write(
            fixture.paths.projects_path(),
            serde_json::to_string(&future).unwrap(),
        )
        .unwrap();
        assert_eq!(fixture.scope(), ScopeId::global());

        fs::write(
            fixture.paths.projects_path(),
            serde_json::to_string(&file).unwrap(),
        )
        .unwrap();
        assert_eq!(fixture.scope(), entry.project.scope());

        fs::remove_dir_all(&root).unwrap();
        assert_eq!(fixture.scope(), ScopeId::global());
        assert_eq!(fixture.registry.list().projects.len(), 1);
    }
}
