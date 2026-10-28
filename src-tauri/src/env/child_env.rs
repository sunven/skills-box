//! Minimal, explicit environment handed to every child process.
//!
//! The executor calls `env_clear()` first; this module decides what survives.
//! The list deliberately excludes tokens, proxies, `NODE_OPTIONS`,
//! `XDG_STATE_HOME` (which would move the CLI lock), and runtime agent
//! identity variables such as `CURSOR_AGENT`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Identity and path-affecting variables preserved from the parent process.
/// Agent-specific homes must survive so the CLI reads and writes the same
/// directories the GUI inspects.
const FORWARDED_VARS: &[&str] = &[
    "USER",
    "LOGNAME",
    "TMPDIR",
    "XDG_CONFIG_HOME",
    "CLAUDE_CONFIG_DIR",
    "CODEX_HOME",
    "GROK_HOME",
    "AUTOHAND_HOME",
    "HERMES_HOME",
    "VIBE_HOME",
    "SARVAM_HOME",
];

const BASE_PATH_DIRS: &[&str] = &[
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "/usr/bin",
    "/bin",
    "/usr/sbin",
    "/sbin",
];

pub struct ChildEnvParams<'a> {
    pub home: &'a Path,
    pub node_dir: Option<&'a Path>,
    pub git_dir: Option<&'a Path>,
    pub parent: &'a BTreeMap<String, String>,
}

pub fn build_child_env(params: ChildEnvParams<'_>) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();

    env.insert(
        "HOME".to_string(),
        params.home.to_string_lossy().into_owned(),
    );

    for key in FORWARDED_VARS {
        if let Some(value) = params
            .parent
            .get(*key)
            .filter(|value| !value.trim().is_empty())
        {
            env.insert((*key).to_string(), value.clone());
        }
    }

    if !env.contains_key("LANG") && !env.contains_key("LC_ALL") {
        env.insert("LANG".to_string(), "en_US.UTF-8".to_string());
    }

    env.insert(
        "PATH".to_string(),
        build_path(params.node_dir, params.git_dir),
    );
    env.insert("NO_COLOR".to_string(), "1".to_string());
    // CI=true (exact string) suppresses clack spinner frames in skills@1.7.4.
    env.insert("CI".to_string(), "true".to_string());
    env.insert("DISABLE_TELEMETRY".to_string(), "1".to_string());
    env.insert("DO_NOT_TRACK".to_string(), "1".to_string());
    env.insert("GIT_TERMINAL_PROMPT".to_string(), "0".to_string());
    env.insert("npm_config_yes".to_string(), "true".to_string());
    env.insert(
        "npm_config_update_notifier".to_string(),
        "false".to_string(),
    );
    env.insert("npm_config_fund".to_string(), "false".to_string());
    env.insert("npm_config_audit".to_string(), "false".to_string());

    env
}

pub fn build_path(node_dir: Option<&Path>, git_dir: Option<&Path>) -> String {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(node_dir) = node_dir {
        dirs.push(node_dir.to_path_buf());
    }
    if let Some(git_dir) = git_dir {
        dirs.push(git_dir.to_path_buf());
    }
    for dir in BASE_PATH_DIRS {
        dirs.push(PathBuf::from(dir));
    }

    let mut seen = std::collections::HashSet::new();
    let mut rendered = Vec::new();
    for dir in dirs {
        let key = dir.to_string_lossy().into_owned();
        if seen.insert(key.clone()) {
            rendered.push(key);
        }
    }
    rendered.join(":")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn parent_env() -> BTreeMap<String, String> {
        [
            ("HOME", "/Users/real"),
            ("USER", "tester"),
            ("LOGNAME", "tester"),
            ("TMPDIR", "/var/folders/tmp"),
            ("SHELL", "/bin/zsh"),
            ("LANG", "zh_CN.UTF-8"),
            ("XDG_CONFIG_HOME", "/Users/real/.config"),
            ("CLAUDE_CONFIG_DIR", "/Users/real/.claude-alt"),
            ("GITHUB_TOKEN", "secret"),
            ("GH_TOKEN", "secret"),
            ("GH_HOST", "enterprise.example"),
            ("NODE_OPTIONS", "--inspect"),
            ("NODE_PATH", "/tmp/node_modules"),
            ("XDG_STATE_HOME", "/tmp/state"),
            ("http_proxy", "http://127.0.0.1:7890"),
            ("CURSOR_AGENT", "1"),
            ("npm_config_prefix", "/tmp/npm"),
        ]
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
    }

    #[test]
    fn forwards_only_the_allowlist_and_sets_telemetry_and_stability_flags() {
        let parent = parent_env();
        let env = build_child_env(ChildEnvParams {
            home: Path::new("/Users/tester"),
            node_dir: Some(Path::new("/Users/tester/.nvm/versions/node/v24.15.0/bin")),
            git_dir: Some(Path::new("/usr/bin")),
            parent: &parent,
        });

        for forwarded in [
            "USER",
            "LOGNAME",
            "TMPDIR",
            "LANG",
            "XDG_CONFIG_HOME",
            "CLAUDE_CONFIG_DIR",
        ] {
            assert!(
                env.contains_key(forwarded),
                "{forwarded} should be forwarded"
            );
        }
        assert!(
            !env.contains_key("SHELL"),
            "SHELL must not be forwarded to the child"
        );
        for excluded in [
            "GITHUB_TOKEN",
            "GH_TOKEN",
            "GH_HOST",
            "NODE_OPTIONS",
            "NODE_PATH",
            "XDG_STATE_HOME",
            "http_proxy",
            "CURSOR_AGENT",
            "npm_config_prefix",
        ] {
            assert!(!env.contains_key(excluded), "{excluded} must be dropped");
        }

        assert_eq!(env["HOME"], "/Users/tester", "HOME must not be inherited");
        assert_eq!(env["CI"], "true");
        assert_eq!(env["DISABLE_TELEMETRY"], "1");
        assert_eq!(env["DO_NOT_TRACK"], "1");
        assert_eq!(env["GIT_TERMINAL_PROMPT"], "0");
        assert_eq!(env["NO_COLOR"], "1");
        assert_eq!(env["npm_config_yes"], "true");
        assert_eq!(env["npm_config_update_notifier"], "false");
    }

    #[test]
    fn node_dir_leads_path_for_npx_shebang_and_npm_exec() {
        let parent = parent_env();
        let env = build_child_env(ChildEnvParams {
            home: Path::new("/Users/tester"),
            node_dir: Some(Path::new("/Users/tester/.nvm/versions/node/v24.15.0/bin")),
            git_dir: Some(Path::new("/usr/bin")),
            parent: &parent,
        });
        let path = &env["PATH"];
        assert!(path.starts_with("/Users/tester/.nvm/versions/node/v24.15.0/bin:"));
        assert!(path.contains("/usr/bin"));
        assert!(path.contains("/opt/homebrew/bin"));
        let parts = path.split(':').collect::<Vec<_>>();
        assert_eq!(
            parts.iter().filter(|part| **part == "/usr/bin").count(),
            1,
            "PATH entries must be de-duplicated"
        );
    }

    #[test]
    fn missing_lang_falls_back_to_utf8_locale() {
        let mut parent = parent_env();
        parent.remove("LANG");
        let env = build_child_env(ChildEnvParams {
            home: Path::new("/Users/tester"),
            node_dir: None,
            git_dir: None,
            parent: &parent,
        });
        assert_eq!(env["LANG"], "en_US.UTF-8");
        assert!(!env.contains_key("LC_ALL"));
    }

    #[test]
    fn empty_forwarded_values_are_skipped() {
        let mut parent = parent_env();
        parent.insert("CLAUDE_CONFIG_DIR".to_string(), String::new());
        let env = build_child_env(ChildEnvParams {
            home: Path::new("/Users/tester"),
            node_dir: None,
            git_dir: None,
            parent: &parent,
        });
        assert!(!env.contains_key("CLAUDE_CONFIG_DIR"));
    }
}
