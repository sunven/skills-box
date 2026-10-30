//! Per-scope filesystem layout.
//!
//! Global scope keeps the existing paths and detected global agent dirs.
//! Project scope follows the pinned CLI: `<root>/.agents/skills` is canonical,
//! `<root>/skills-lock.json` is the lock, every catalog agent's *project*
//! skills directory is scanned regardless of global detection, and extra
//! agents join only when they configure a `projectSkillsDir` whose directory
//! exists.
//!
//! Directories that lexically or physically resolve outside the project root
//! are never scanned. They are reported as escaping paths so the read model can
//! mark an anomaly and content commands can refuse them.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::agents::catalog;
use crate::agents::extras::{self, ExtraAgents};
use crate::error::AppError;
use crate::inventory::{self, AgentDir, LocatedSkill};
use crate::paths::AppPaths;
use crate::project_lock::ProjectLockFile;
use crate::projects::{Project, ProjectRegistry};
use crate::scope::ScopeId;
use crate::security::{AllowedRoot, normalize_absolute};

#[derive(Debug, Clone)]
pub struct ScopeLayout {
    pub scope: ScopeId,
    pub project: Option<Project>,
    /// Canonicalized project root, or the home directory for global scope.
    pub root: PathBuf,
    pub canonical_dir: PathBuf,
    pub lock_path: PathBuf,
    pub agents: Vec<AgentDir>,
    pub extras: ExtraAgents,
    /// Layout warnings (unreadable or non-directory project dirs, bad extra
    /// `projectSkillsDir` values). They are scope warnings, never errors.
    pub warnings: Vec<String>,
    /// Human-readable labels of directories/skills that resolve outside the
    /// project root. Surface them as the `escape` anomaly group.
    pub escapes: Vec<String>,
}

impl ScopeLayout {
    pub fn global(paths: &AppPaths, agents: Vec<AgentDir>) -> Self {
        Self {
            scope: ScopeId::Global,
            project: None,
            root: paths.home().to_path_buf(),
            canonical_dir: paths.canonical_skills_dir(),
            lock_path: paths.lock_path(),
            agents,
            extras: extras::load(paths),
            warnings: Vec::new(),
            escapes: Vec::new(),
        }
    }

    pub fn project(paths: &AppPaths, project: Project) -> Self {
        let root = fs::canonicalize(&project.root).unwrap_or_else(|_| project.root.clone());
        let canonical_dir = root.join(".agents").join("skills");
        let lock_path = root.join("skills-lock.json");
        let extras = extras::load(paths);
        let mut warnings = Vec::new();
        let mut escapes = Vec::new();
        let agents =
            project_agent_dirs(&root, &canonical_dir, &extras, &mut warnings, &mut escapes);
        Self {
            scope: project.scope(),
            project: Some(project),
            root,
            canonical_dir,
            lock_path,
            agents,
            extras,
            warnings,
            escapes,
        }
    }

    /// Resolve one scope through the app registry. A project that is missing
    /// from the registry is `project.not_found`; an unavailable root is a
    /// scope state that blocks every mutation before work starts.
    pub fn resolve(
        paths: &AppPaths,
        projects: &ProjectRegistry,
        scope: &ScopeId,
    ) -> Result<Self, AppError> {
        match scope {
            ScopeId::Global => Ok(Self::global(
                paths,
                inventory::detected_agent_dirs(paths),
            )),
            ScopeId::Project(id) => {
                let entry = projects.lookup(*id)?;
                if !entry.available {
                    return Err(AppError::project_root_unavailable(
                        &entry.project.display_name,
                        entry
                            .unavailable_reason
                            .unwrap_or_else(|| "目录不可用".to_string()),
                    ));
                }
                Ok(Self::project(paths, entry.project))
            }
        }
    }

    pub fn is_global(&self) -> bool {
        self.project.is_none()
    }

    pub fn project_root(&self) -> Option<&Path> {
        self.project.as_ref().map(|_| self.root.as_path())
    }

    /// Project mutations use the project root as cwd; global mutations keep
    /// the isolated CLI working directory so no project scope can appear.
    pub fn cli_cwd(&self, paths: &AppPaths) -> PathBuf {
        match self.project {
            Some(_) => self.root.clone(),
            None => paths.cli_cwd(),
        }
    }

    /// A project lock that cannot be parsed or uses an unsupported version is
    /// a scope error: every mutation and preview for that scope is rejected
    /// before a task is registered. Global behavior is unchanged.
    pub fn ensure_mutation_ready(&self) -> Result<(), AppError> {
        if self.project.is_some() {
            ProjectLockFile::read(&self.lock_path)?;
        }
        Ok(())
    }

    /// Strict scope catalog. Project scope uses the project lock and canonical
    /// store; escapes stay reported, never followed.
    pub(crate) fn catalog(&self, paths: &AppPaths) -> Result<inventory::Catalog, AppError> {
        match self.project {
            None => inventory::build_catalog(paths, &self.agents),
            Some(_) => inventory::build_project_catalog(
                &self.root,
                &self.canonical_dir,
                &self.lock_path,
                &self.agents,
            ),
        }
    }

    pub(crate) fn locate(
        &self,
        paths: &AppPaths,
        name: &str,
    ) -> Result<Option<LocatedSkill>, AppError> {
        Ok(inventory::locate_in_catalog(&self.catalog(paths)?, name))
    }

    /// Agent targets for the install wizard and attach dialog (spec §3.5).
    ///
    /// Global scope keeps the detected agents exactly as before. Project scope
    /// is globally detected catalog agents mapped to their project directory
    /// plus every configured extra with a valid `projectSkillsDir`; neither
    /// directory has to exist yet.
    pub fn install_targets(&self, detected: &[AgentDir]) -> Vec<AgentDir> {
        if self.project.is_none() {
            return self.agents.clone();
        }
        let Some(root_normalized) = normalize_absolute(&self.root) else {
            return Vec::new();
        };
        let canonical_normalized = normalize_absolute(&self.canonical_dir);
        let mut seen_dirs = BTreeSet::new();
        let mut targets = Vec::new();

        for definition in &catalog().agents {
            if definition.universal || definition.id == "universal" {
                // Universal agents read canonical directly; there is no
                // separate project target for them.
                continue;
            }
            if !detected.iter().any(|agent| agent.id == definition.id) {
                continue;
            }
            let Some(dir) = project_target_dir(
                &definition.skills_dir,
                &self.root,
                &root_normalized,
                canonical_normalized.as_deref(),
            ) else {
                continue;
            };
            if !seen_dirs.insert(dir.clone()) {
                continue;
            }
            targets.push(AgentDir {
                id: definition.id.clone(),
                display_name: definition.display_name.clone(),
                universal: false,
                skills_dir: dir,
            });
        }

        for extra in &self.extras.agents {
            let Some(raw) = extra.project_skills_dir.as_deref() else {
                continue;
            };
            let Some(dir) = project_target_dir(
                raw,
                &self.root,
                &root_normalized,
                canonical_normalized.as_deref(),
            ) else {
                continue;
            };
            if !seen_dirs.insert(dir.clone()) {
                continue;
            }
            targets.push(AgentDir {
                id: extra.id.clone(),
                display_name: extra.display_name.clone(),
                universal: false,
                skills_dir: dir,
            });
        }
        targets
    }

    /// Read roots for this scope. Global keeps canonical plus every detected
    /// agent directory; project scope allows only the registered project root.
    pub fn allowed_roots(&self) -> Vec<AllowedRoot> {
        if self.project.is_some() {
            return vec![AllowedRoot::project(self.root.clone())];
        }
        let mut roots = vec![AllowedRoot::canonical(self.canonical_dir.clone())];
        for agent in &self.agents {
            if crate::inventory::same_dir(&agent.skills_dir, &self.canonical_dir) {
                continue;
            }
            if roots
                .iter()
                .any(|root| crate::inventory::same_dir(&root.path, &agent.skills_dir))
            {
                continue;
            }
            roots.push(AllowedRoot::agent(agent.skills_dir.clone()));
        }
        roots
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ProjectDirState {    Missing,
    Directory,
    Escape(PathBuf),
    NotDirectory,
    Unreadable(String),
}

/// Resolve a project-relative agent skills directory. Invalid values (empty,
/// absolute, escaping the root, or pointing at canonical) are not targets.
fn project_target_dir(
    raw: &str,
    root: &Path,
    root_normalized: &Path,
    canonical_normalized: Option<&Path>,
) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || Path::new(trimmed).is_absolute() {
        return None;
    }
    let dir = root.join(trimmed);
    let normalized = normalize_absolute(&dir)?;
    if !normalized.starts_with(root_normalized) {
        return None;
    }
    if canonical_normalized == Some(normalized.as_path()) {
        return None;
    }
    Some(dir)
}

fn classify_project_dir(dir: &Path, root: &Path) -> ProjectDirState {
    match fs::symlink_metadata(dir) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ProjectDirState::Missing,
        Err(error) => ProjectDirState::Unreadable(error.to_string()),
        Ok(metadata) if metadata.file_type().is_symlink() => match fs::canonicalize(dir) {
            Ok(target) if !target.starts_with(root) => ProjectDirState::Escape(target),
            Ok(target) if target.is_dir() => ProjectDirState::Directory,
            Ok(_) => ProjectDirState::NotDirectory,
            Err(error) => ProjectDirState::Unreadable(error.to_string()),
        },
        Ok(metadata) if metadata.is_dir() => ProjectDirState::Directory,
        Ok(_) => ProjectDirState::NotDirectory,
    }
}

fn project_agent_dirs(
    root: &Path,
    canonical_dir: &Path,
    extras: &ExtraAgents,
    warnings: &mut Vec<String>,
    escapes: &mut Vec<String>,
) -> Vec<AgentDir> {
    let Some(root_normalized) = normalize_absolute(root) else {
        warnings.push(format!("项目根路径无法规范化：{}", root.display()));
        return Vec::new();
    };
    let canonical_normalized = normalize_absolute(canonical_dir);
    let mut seen = BTreeSet::new();
    let mut agents = Vec::new();

    for definition in &catalog().agents {
        // `universal` is the CLI's pseudo target; universal catalog agents
        // already point at `.agents/skills` and read canonical directly.
        if definition.id == "universal" {
            continue;
        }
        let raw = definition.skills_dir.trim();
        if raw.is_empty() {
            continue;
        }
        let relative = Path::new(raw);
        if relative.is_absolute() {
            warnings.push(format!(
                "忽略 agent「{}」的项目目录：必须是相对项目根的路径（{raw}）。",
                definition.display_name
            ));
            continue;
        }
        let dir = root.join(relative);
        let Some(normalized) = normalize_absolute(&dir) else {
            warnings.push(format!(
                "忽略 agent「{}」的项目目录：无法规范化（{raw}）。",
                definition.display_name
            ));
            continue;
        };
        if !normalized.starts_with(&root_normalized) {
            warnings.push(format!(
                "忽略 agent「{}」的项目目录：解析后位于项目根之外（{raw}）。",
                definition.display_name
            ));
            escapes.push(format!("{} · {raw}", definition.display_name));
            continue;
        }
        if canonical_normalized.as_ref() == Some(&normalized) {
            continue;
        }
        if !seen.insert(normalized) {
            // Several catalog ids can share one project directory (aliases).
            // Scan it once so attachments are not duplicated.
            continue;
        }
        match classify_project_dir(&dir, root) {
            ProjectDirState::Directory => agents.push(AgentDir {
                id: definition.id.clone(),
                display_name: definition.display_name.clone(),
                universal: definition.universal,
                skills_dir: dir,
            }),
            ProjectDirState::Missing => {}
            ProjectDirState::Escape(_) => {
                warnings.push(format!(
                    "忽略 agent「{}」的项目目录：符号链接逃逸项目根（{}）。",
                    definition.display_name,
                    dir.display()
                ));
                escapes.push(format!("{} · {raw}", definition.display_name));
            }
            ProjectDirState::NotDirectory => warnings.push(format!(
                "忽略 agent「{}」的项目目录：不是目录（{}）。",
                definition.display_name,
                dir.display()
            )),
            ProjectDirState::Unreadable(error) => warnings.push(format!(
                "无法检查 agent「{}」的项目目录 {}：{error}",
                definition.display_name,
                dir.display()
            )),
        }
    }

    for extra in &extras.agents {
        let Some(raw) = extra.project_skills_dir.as_deref() else {
            continue;
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() || Path::new(trimmed).is_absolute() {
            warnings.push(format!(
                "忽略额外 agent「{}」的项目目录：必须是非空的相对路径（{raw:?}）。",
                extra.display_name
            ));
            continue;
        }
        let dir = root.join(trimmed);
        let Some(normalized) = normalize_absolute(&dir) else {
            warnings.push(format!(
                "忽略额外 agent「{}」的项目目录：无法规范化（{trimmed}）。",
                extra.display_name
            ));
            continue;
        };
        if !normalized.starts_with(&root_normalized) {
            warnings.push(format!(
                "忽略额外 agent「{}」的项目目录：解析后位于项目根之外（{trimmed}）。",
                extra.display_name
            ));
            escapes.push(format!("{} · {trimmed}", extra.display_name));
            continue;
        }
        if canonical_normalized.as_ref() == Some(&normalized) {
            warnings.push(format!(
                "忽略额外 agent「{}」的项目目录：不能使用 canonical store（{trimmed}）。",
                extra.display_name
            ));
            continue;
        }
        if !seen.insert(normalized) {
            continue;
        }
        match classify_project_dir(&dir, root) {
            ProjectDirState::Directory => agents.push(AgentDir {
                id: extra.id.clone(),
                display_name: extra.display_name.clone(),
                universal: false,
                skills_dir: dir,
            }),
            // Spec: an extra joins project scanning only when its directory
            // already exists. Install/attach targets are a separate concern.
            ProjectDirState::Missing => {}
            ProjectDirState::Escape(_) => {
                warnings.push(format!(
                    "忽略额外 agent「{}」的项目目录：符号链接逃逸项目根（{}）。",
                    extra.display_name,
                    dir.display()
                ));
                escapes.push(format!("{} · {trimmed}", extra.display_name));
            }
            ProjectDirState::NotDirectory => warnings.push(format!(
                "忽略额外 agent「{}」的项目目录：不是目录（{}）。",
                extra.display_name,
                dir.display()
            )),
            ProjectDirState::Unreadable(error) => warnings.push(format!(
                "无法检查额外 agent「{}」的项目目录 {}：{error}",
                extra.display_name,
                dir.display()
            )),
        }
    }

    agents
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use uuid::Uuid;

    struct Fixture {
        tmp: tempfile::TempDir,
        paths: AppPaths,
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path().join("home");
            fs::create_dir_all(&home).unwrap();
            let paths = AppPaths::new(&home, tmp.path().join("app-data"));
            let root = tmp.path().join("projects/app");
            fs::create_dir_all(root.join(".agents/skills")).unwrap();
            Self {
                tmp,
                paths,
                root: fs::canonicalize(root).unwrap(),
            }
        }

        fn project(&self) -> Project {
            Project {
                id: Uuid::new_v4(),
                root: self.root.clone(),
                display_name: "app".to_string(),
                added_at: 1,
                last_opened_at: 1,
            }
        }

        fn write_extra(&self, raw: &str) {
            fs::create_dir_all(self.paths.app_data()).unwrap();
            fs::write(self.paths.agents_config_path(), raw).unwrap();
        }

        fn layout(&self) -> ScopeLayout {
            ScopeLayout::project(&self.paths, self.project())
        }
    }

    fn agent_ids(layout: &ScopeLayout) -> Vec<String> {
        layout.agents.iter().map(|agent| agent.id.clone()).collect()
    }

    #[test]
    fn project_layout_uses_cli_paths_and_scans_every_existing_catalog_dir() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.root.join(".claude/skills")).unwrap();
        fs::create_dir_all(fixture.root.join(".qoder/skills")).unwrap();
        // A catalog dir that does not exist is simply absent from the scan.
        let layout = fixture.layout();

        assert_eq!(
            layout.scope,
            ScopeId::project(layout.project.as_ref().unwrap().id)
        );
        assert_eq!(layout.canonical_dir, fixture.root.join(".agents/skills"));
        assert_eq!(layout.lock_path, fixture.root.join("skills-lock.json"));
        let ids = agent_ids(&layout);
        assert!(ids.contains(&"claude-code".to_string()), "{ids:?}");
        assert!(ids.contains(&"qoder".to_string()), "{ids:?}");
        assert!(!ids.contains(&"universal".to_string()));
        assert!(
            layout
                .agents
                .iter()
                .all(|agent| agent.skills_dir.starts_with(&fixture.root)),
            "{:?}",
            layout.agents
        );
        // Universal agents share canonical and are never separate mounts.
        assert!(
            agent_ids(&layout)
                .iter()
                .all(|id| id != "codex" && id != "amp")
        );
    }

    #[test]
    fn aliases_sharing_one_project_dir_are_scanned_once() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.root.join(".qoder/skills")).unwrap();
        let layout = fixture.layout();
        let qoder_dirs = layout
            .agents
            .iter()
            .filter(|agent| agent.skills_dir == fixture.root.join(".qoder/skills"))
            .count();
        assert_eq!(qoder_dirs, 1);
    }

    #[test]
    fn extras_join_only_with_an_existing_project_skills_dir() {
        let fixture = Fixture::new();
        fixture.write_extra(
            r#"{
                "version": 1,
                "agents": [
                    {"id": "acme", "displayName": "Acme", "skillsDir": "~/.acme/skills", "projectSkillsDir": ".acme/skills"},
                    {"id": "ghost", "displayName": "Ghost", "skillsDir": "~/.ghost/skills", "projectSkillsDir": ".ghost/skills"},
                    {"id": "plain", "displayName": "Plain", "skillsDir": "~/.plain/skills"}
                ]
            }"#,
        );
        fs::create_dir_all(fixture.root.join(".acme/skills")).unwrap();

        let layout = fixture.layout();
        let ids = agent_ids(&layout);
        assert!(ids.contains(&"acme".to_string()), "{ids:?}");
        assert!(!ids.contains(&"ghost".to_string()), "missing dir: {ids:?}");
        assert!(!ids.contains(&"plain".to_string()), "unconfigured: {ids:?}");
    }

    #[test]
    fn install_targets_include_detected_catalog_and_configured_extras_even_when_missing() {
        let fixture = Fixture::new();
        fixture.write_extra(
            r#"{
                "version": 1,
                "agents": [
                    {"id": "acme", "displayName": "Acme", "skillsDir": "~/.acme/skills", "projectSkillsDir": ".acme/skills"},
                    {"id": "plain", "displayName": "Plain", "skillsDir": "~/.plain/skills"}
                ]
            }"#,
        );
        let detected = vec![
            AgentDir {
                id: "claude-code".to_string(),
                display_name: "Claude Code".to_string(),
                universal: false,
                skills_dir: fixture.paths.home().join(".claude/skills"),
            },
            AgentDir {
                id: "codex".to_string(),
                display_name: "Codex".to_string(),
                universal: true,
                skills_dir: fixture.paths.canonical_skills_dir(),
            },
        ];
        let layout = fixture.layout();
        let targets = layout.install_targets(&detected);
        let ids: Vec<&str> = targets.iter().map(|agent| agent.id.as_str()).collect();

        assert!(ids.contains(&"claude-code"), "{ids:?}");
        assert!(ids.contains(&"acme"), "{ids:?}");
        assert!(!ids.contains(&"codex"), "universal reads canonical: {ids:?}");
        assert!(!ids.contains(&"plain"), "unconfigured extra: {ids:?}");
        // Both target directories are absent and are still offered.
        let claude = targets.iter().find(|agent| agent.id == "claude-code").unwrap();
        let acme = targets.iter().find(|agent| agent.id == "acme").unwrap();
        assert_eq!(claude.skills_dir, fixture.root.join(".claude/skills"));
        assert_eq!(acme.skills_dir, fixture.root.join(".acme/skills"));
        assert!(!claude.skills_dir.exists());
        assert!(!acme.skills_dir.exists());
    }

    #[test]
    fn escaping_directories_are_skipped_and_reported() {
        let fixture = Fixture::new();
        let outside = fixture.tmp.path().join("outside-skills");
        fs::create_dir_all(&outside).unwrap();
        fs::create_dir_all(fixture.root.join(".claude")).unwrap();
        symlink(&outside, fixture.root.join(".claude/skills")).unwrap();
        fixture.write_extra(
            r#"{
                "version": 1,
                "agents": [
                    {"id": "acme", "displayName": "Acme", "skillsDir": "~/.acme/skills", "projectSkillsDir": "../outside-skills"}
                ]
            }"#,
        );

        let layout = fixture.layout();
        assert!(!agent_ids(&layout).contains(&"claude-code".to_string()));
        assert!(!agent_ids(&layout).contains(&"acme".to_string()));
        assert!(
            layout
                .warnings
                .iter()
                .any(|warning| warning.contains("逃逸") || warning.contains("之外")),
            "{:?}",
            layout.warnings
        );
        assert!(layout.escapes.len() >= 2, "{:?}", layout.escapes);
    }

    #[test]
    fn project_allowed_roots_are_the_registered_root_only() {
        let fixture = Fixture::new();
        let layout = fixture.layout();
        let roots = layout.allowed_roots();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].path, fixture.root);
        assert_eq!(roots[0].kind, crate::security::RootKind::Project);
    }
}
