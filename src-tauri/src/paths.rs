//! Injectable application paths.
//!
//! Production resolves `~/Library/Application Support/com.sunven.skills-box`.
//! In debug builds `SKILLS_BOX_ROOT` may redirect only the app-data root (never
//! `HOME`) so tests and manual sandboxes cannot write to the real app data.
//! Release builds ignore `SKILLS_BOX_ROOT` entirely. Tests construct
//! `AppPaths::new(home, app_data)` with temp directories.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub const APP_IDENTIFIER: &str = "com.sunven.skills-box";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    home: PathBuf,
    app_data: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        let root = std::env::var_os("SKILLS_BOX_ROOT");
        Self::from_env_values(home, root, cfg!(debug_assertions))
    }

    /// Deterministic constructor used by tests and by `discover`.
    ///
    /// `debug` mirrors `cfg!(debug_assertions)`; when false the override root
    /// is ignored, which is the release-build contract.
    pub fn from_env_values(home: PathBuf, root: Option<OsString>, debug: bool) -> Self {
        let app_data = match (debug, root) {
            (true, Some(root)) if !root.is_empty() => PathBuf::from(root),
            _ => default_app_data_root(&home),
        };
        Self { home, app_data }
    }

    /// Fully injectable constructor for tests and future settings plumbing.
    pub fn new(home: impl Into<PathBuf>, app_data: impl Into<PathBuf>) -> Self {
        Self {
            home: home.into(),
            app_data: app_data.into(),
        }
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn app_data(&self) -> &Path {
        &self.app_data
    }

    pub fn agents_root(&self) -> PathBuf {
        self.home.join(".agents")
    }

    pub fn canonical_skills_dir(&self) -> PathBuf {
        self.agents_root().join("skills")
    }

    pub fn lock_path(&self) -> PathBuf {
        self.agents_root().join(".skill-lock.json")
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.app_data.join("backups")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.app_data.join("logs")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.app_data.join("cache")
    }

    /// Persisted summary of the last full update check. Kept outside `cache/`
    /// because it is user-visible state, not disposable data.
    pub fn update_check_state_path(&self) -> PathBuf {
        self.app_data.join("update-check.json")
    }

    /// User-defined extra agents. Editable by hand; the UI writes the same file.
    pub fn agents_config_path(&self) -> PathBuf {
        self.app_data.join("agents.json")
    }

    pub fn settings_path(&self) -> PathBuf {
        self.app_data.join("settings.json")
    }

    /// Registered project roots and the last active scope.
    pub fn projects_path(&self) -> PathBuf {
        self.app_data.join("projects.json")
    }

    pub fn baselines_dir(&self) -> PathBuf {
        self.app_data.join("baselines")
    }

    /// Runtime coordination files (the interrupted-mutation marker).
    pub fn runtime_dir(&self) -> PathBuf {
        self.app_data.join("runtime")
    }

    pub fn interrupted_marker_path(&self) -> PathBuf {
        self.runtime_dir().join("interrupted-mutation.json")
    }

    /// Empty working directory handed to CLI child processes. Keeping cwd out
    /// of any project avoids project-scoped agent/skill discovery.
    pub fn cli_cwd(&self) -> PathBuf {
        self.app_data.join("cli-cwd")
    }

    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        for dir in [
            self.app_data.clone(),
            self.backups_dir(),
            self.logs_dir(),
            self.cache_dir(),
            self.baselines_dir(),
            self.runtime_dir(),
            self.cli_cwd(),
        ] {
            std::fs::create_dir_all(&dir)?;
        }
        Ok(())
    }

    /// Abbreviate the home prefix with `~` for display in the UI.
    pub fn display(&self, path: &Path) -> String {
        match path.strip_prefix(&self.home) {
            Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => path.display().to_string(),
        }
    }
}

pub fn default_app_data_root(home: &Path) -> PathBuf {
    home.join("Library")
        .join("Application Support")
        .join(APP_IDENTIFIER)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/Users/tester")
    }

    #[test]
    fn default_app_data_lives_under_application_support() {
        let paths = AppPaths::from_env_values(home(), None, true);
        assert_eq!(
            paths.app_data(),
            Path::new("/Users/tester/Library/Application Support/com.sunven.skills-box")
        );
        assert_eq!(paths.home(), home());
    }

    #[test]
    fn debug_build_honors_skills_box_root_for_app_data_only() {
        let paths = AppPaths::from_env_values(
            home(),
            Some(OsString::from("/tmp/skills-box-sandbox")),
            true,
        );
        assert_eq!(paths.app_data(), Path::new("/tmp/skills-box-sandbox"));
        assert_eq!(paths.home(), home());
        assert_eq!(
            paths.lock_path(),
            Path::new("/Users/tester/.agents/.skill-lock.json")
        );
    }

    #[test]
    fn release_build_ignores_skills_box_root() {
        let paths = AppPaths::from_env_values(
            home(),
            Some(OsString::from("/tmp/skills-box-sandbox")),
            false,
        );
        assert_eq!(paths.app_data(), default_app_data_root(&home()));
    }

    #[test]
    fn exposes_shared_paths_and_ensures_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(tmp.path().join("home"), tmp.path().join("app-data"));
        paths.ensure_dirs().unwrap();
        for dir in [
            paths.app_data(),
            &paths.backups_dir(),
            &paths.logs_dir(),
            &paths.cache_dir(),
            &paths.baselines_dir(),
            &paths.runtime_dir(),
            &paths.cli_cwd(),
        ] {
            assert!(dir.is_dir(), "{} should be a directory", dir.display());
        }
        assert_eq!(
            paths.canonical_skills_dir(),
            tmp.path().join("home/.agents/skills")
        );
        assert_eq!(
            paths.lock_path(),
            tmp.path().join("home/.agents/.skill-lock.json")
        );
        assert_eq!(
            paths.update_check_state_path(),
            tmp.path().join("app-data/update-check.json")
        );
        assert_eq!(
            paths.projects_path(),
            tmp.path().join("app-data/projects.json")
        );
    }

    #[test]
    fn display_abbreviates_home_prefix() {
        let paths = AppPaths::new("/Users/tester", "/tmp/app-data");
        assert_eq!(
            paths.display(Path::new("/Users/tester/.claude/skills")),
            "~/.claude/skills"
        );
        assert_eq!(paths.display(Path::new("/tmp/app-data")), "/tmp/app-data");
        assert_eq!(paths.display(Path::new("/Users/tester")), "~");
    }
}
