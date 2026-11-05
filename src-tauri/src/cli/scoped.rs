//! The pinned CLI's commands and execution scope.
//!
//! The same layout used for local backup and verification chooses both cwd
//! and `-g`. Plans contain semantic operation data, never another execution
//! scope. Command construction shares no task/session lifecycle.

use std::path::Path;
use std::time::Duration;

use crate::paths::AppPaths;
use crate::process::CommandSpec;
use crate::scope_layout::ScopeLayout;

use super::{CliRuntime, PINNED_CLI_VERSION};

pub struct ScopedCli<'a> {
    runtime: &'a CliRuntime,
    layout: &'a ScopeLayout,
}

impl<'a> ScopedCli<'a> {
    pub fn new(runtime: &'a CliRuntime, layout: &'a ScopeLayout) -> Self {
        Self { runtime, layout }
    }

    /// Install and Update both use `add <source> [-g] -s… -a… -y --json`.
    /// Names and agent ids are already validated; callers append universal
    /// and omit Extra agents when planning the Install targets.
    pub fn install(
        &self,
        source: &str,
        skills: &[String],
        agent_ids: &[String],
        timeout: Duration,
    ) -> CommandSpec {
        let args = install_args(
            &self.runtime.pinned_version,
            self.layout.is_global(),
            source,
            skills,
            agent_ids,
        );
        self.command(args, &self.runtime.cwd, timeout)
    }

    /// Read-only display of the same pinned arguments used by Install/Update.
    /// A project command includes its cwd so the displayed line is copyable.
    pub fn install_preview(
        layout: &ScopeLayout,
        source: &str,
        skills: &[String],
        agent_ids: &[String],
    ) -> String {
        let args = install_args(
            PINNED_CLI_VERSION,
            layout.is_global(),
            source,
            skills,
            agent_ids,
        );
        let command = std::iter::once("npx".to_string())
            .chain(args.iter().map(|arg| shell_quote(arg)))
            .collect::<Vec<_>>()
            .join(" ");
        match layout.project_root() {
            Some(root) => format!("cd {} && {command}", shell_quote(&root.to_string_lossy())),
            None => command,
        }
    }

    /// Remove the named Skill. Per-agent remove can delete the Canonical store
    /// and Lock entry, so `-a` and `--all` are never used. No JSON is available.
    pub fn uninstall(&self, name: &str, timeout: Duration) -> CommandSpec {
        let mut args = self.args("remove");
        self.push_global(&mut args);
        args.push("-s".to_string());
        args.push(name.to_string());
        args.push("-y".to_string());
        self.command(args, &self.runtime.cwd, timeout)
    }

    /// Read-only `add <source> -l [-g]`. List mode exits before prompting and
    /// is incompatible with `--json`. Global Preview retains the app's cwd;
    /// Global mutations retain the runtime's cwd. Both are isolated in the
    /// production runtime, while adapters may supply different directories.
    pub fn preview(&self, paths: &AppPaths, source: &str, timeout: Duration) -> CommandSpec {
        let mut args = self.args("add");
        args.push(source.to_string());
        args.push("-l".to_string());
        self.push_global(&mut args);
        self.command(args, &paths.cli_cwd(), timeout)
    }

    fn args(&self, operation: &str) -> Vec<String> {
        vec![
            "-y".to_string(),
            format!("skills@{}", self.runtime.pinned_version),
            operation.to_string(),
        ]
    }

    fn push_global(&self, args: &mut Vec<String>) {
        if self.layout.is_global() {
            args.push("-g".to_string());
        }
    }

    fn command(&self, args: Vec<String>, global_cwd: &Path, timeout: Duration) -> CommandSpec {
        CommandSpec::new(
            &self.runtime.npx_path,
            args,
            self.layout.project_root().unwrap_or(global_cwd),
            self.runtime.env.clone(),
            timeout,
        )
    }
}

fn install_args(
    version: &str,
    global: bool,
    source: &str,
    skills: &[String],
    agent_ids: &[String],
) -> Vec<String> {
    let mut args = vec![
        "-y".into(),
        format!("skills@{version}"),
        "add".into(),
        source.into(),
    ];
    if global {
        args.push("-g".into());
    }
    for name in skills {
        args.extend(["-s".into(), name.clone()]);
    }
    for id in agent_ids {
        args.extend(["-a".into(), id.clone()]);
    }
    args.extend(["-y".into(), "--json".into()]);
    args
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_./:@%+=,-".contains(c))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::PINNED_CLI_VERSION;
    use crate::projects::Project;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;

    fn runtime(paths: &AppPaths) -> CliRuntime {
        CliRuntime {
            npx_path: PathBuf::from("/node/bin/npx"),
            node_dir: PathBuf::from("/node/bin"),
            git_dir: Some(PathBuf::from("/usr/bin")),
            // Deliberately distinguish runtime cwd from the app's Preview cwd.
            cwd: paths.app_data().join("runtime-cwd"),
            home: paths.home().to_path_buf(),
            env: BTreeMap::from([
                ("CI".to_string(), "true".to_string()),
                ("DISABLE_TELEMETRY".to_string(), "1".to_string()),
                ("PATH".to_string(), "/node/bin:/usr/bin:/bin".to_string()),
            ]),
            pinned_version: PINNED_CLI_VERSION.to_string(),
        }
    }

    fn fixture(project: bool) -> (tempfile::TempDir, AppPaths, ScopeLayout) {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(tmp.path().join("home"), tmp.path().join("data"));
        paths.ensure_dirs().unwrap();
        let layout = if project {
            let root = tmp.path().join("project");
            fs::create_dir_all(&root).unwrap();
            ScopeLayout::project(
                &paths,
                Project {
                    id: uuid::Uuid::new_v4(),
                    root,
                    display_name: "project".to_string(),
                    added_at: 1,
                    last_opened_at: 1,
                    check_revision: "initial".to_string(),
                },
            )
        } else {
            ScopeLayout::global(&paths, Vec::new())
        };
        (tmp, paths, layout)
    }

    #[test]
    fn preview_command_is_pinned_and_read_only() {
        let (_tmp, paths, layout) = fixture(false);
        let runtime = runtime(&paths);
        let spec = ScopedCli::new(&runtime, &layout).preview(
            &paths,
            "owner/repo",
            Duration::from_secs(60),
        );
        assert_eq!(spec.program, PathBuf::from("/node/bin/npx"));
        assert_eq!(
            spec.args,
            vec!["-y", "skills@1.7.4", "add", "owner/repo", "-l", "-g"]
        );
        assert_eq!(spec.cwd, paths.cli_cwd());
        assert_eq!(spec.env, runtime.env);
        assert_eq!(spec.timeout, Duration::from_secs(60));
        for forbidden in [
            "--all", "--copy", "--json", "--yes", "-a", "update", "check", "list", "upgrade",
        ] {
            assert!(
                !spec.args.iter().any(|arg| arg == forbidden),
                "{forbidden} must never be part of preview"
            );
        }
    }

    #[test]
    fn install_preview_preserves_the_execution_arguments_and_project_cwd() {
        for project in [false, true] {
            let (_tmp, paths, layout) = fixture(project);
            let runtime = runtime(&paths);
            let skills = vec!["alpha".into()];
            let agents = vec!["claude-code".into(), "universal".into()];
            let source = "owner/repo#release/v2";
            let spec = ScopedCli::new(&runtime, &layout).install(
                source,
                &skills,
                &agents,
                Duration::from_secs(900),
            );
            let display = ScopedCli::install_preview(&layout, source, &skills, &agents);
            let expected = format!(
                "{}npx -y skills@1.7.4 add 'owner/repo#release/v2'{} -s alpha -a claude-code -a universal -y --json",
                if project {
                    format!("cd {} && ", spec.cwd.display())
                } else {
                    String::new()
                },
                if project { "" } else { " -g" },
            );
            assert_eq!(display, expected);
            assert_eq!(spec.args[3], source);
            assert_eq!(spec.args.iter().any(|arg| arg == "-g"), !project);
        }
    }

    #[test]
    fn install_preview_quotes_project_paths_with_spaces_and_apostrophes() {
        let (tmp, _paths, mut layout) = fixture(true);
        layout.root = tmp.path().join("O'Brien skills");
        let display = ScopedCli::install_preview(
            &layout,
            "owner/repo",
            &["alpha".into()],
            &["universal".into()],
        );
        assert!(display.starts_with(&format!(
            "cd '{}/O'\\''Brien skills' && npx ",
            tmp.path().display()
        )));
        assert!(!display.contains(" -g "));
    }

    #[test]
    fn project_preview_runs_in_the_project_root_without_global() {
        let (_tmp, paths, layout) = fixture(true);
        let runtime = runtime(&paths);
        let spec = ScopedCli::new(&runtime, &layout).preview(
            &paths,
            "owner/repo",
            Duration::from_secs(60),
        );
        assert_eq!(
            spec.args,
            vec!["-y", "skills@1.7.4", "add", "owner/repo", "-l"]
        );
        assert_eq!(spec.cwd, layout.root);
        assert!(!spec.args.iter().any(|arg| arg == "-g"));
    }

    #[test]
    fn scope_selects_cwd_and_global_flag_for_every_command() {
        for project in [false, true] {
            let (_tmp, paths, layout) = fixture(project);
            let runtime = runtime(&paths);
            assert_ne!(runtime.cwd, paths.cli_cwd());
            let cli = ScopedCli::new(&runtime, &layout);
            let skills = vec!["alpha".to_string()];
            let agents = vec!["qoder".to_string(), "universal".to_string()];
            let timeout = Duration::from_secs(900);
            let commands = [
                cli.install("owner/repo", &skills, &agents, timeout),
                cli.uninstall("alpha", timeout),
                cli.preview(&paths, "owner/repo", timeout),
            ];
            for (index, spec) in commands.iter().enumerate() {
                let expected_cwd = if project {
                    layout.root.clone()
                } else if index == 2 {
                    paths.cli_cwd()
                } else {
                    runtime.cwd.clone()
                };
                assert_eq!(spec.cwd, expected_cwd);
                assert_eq!(spec.args.iter().any(|arg| arg == "-g"), !project);
                assert_eq!(spec.program, runtime.npx_path);
                assert_eq!(spec.env, runtime.env);
                assert_eq!(spec.timeout, timeout);
            }
        }
    }
}
