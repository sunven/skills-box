//! Vendored agent catalog from `skills@1.7.4` plus native detection.
//!
//! The catalog is embedded at compile time so the read-only side of the app
//! does not depend on Node. Detection mirrors the CLI's `detectInstalled`
//! rules: directory existence is checked against the same roots the CLI sees
//! (`HOME`, `XDG_CONFIG_HOME`, agent-specific home overrides) and the CLI child
//! working directory.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

static CATALOG_JSON: &str = include_str!("catalog.json");

pub mod extras;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalog {
    pub source: String,
    pub source_repository: String,
    pub source_commit: String,
    pub agents: Vec<AgentDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDefinition {
    pub id: String,
    pub display_name: String,
    pub global_skills_dir: Option<String>,
    pub universal: bool,
    pub skills_dir: String,
    pub install_dir: Option<String>,
    pub global_skills_dir_env: Option<String>,
    #[serde(default)]
    pub dynamic_global_dir: bool,
    #[serde(default)]
    pub shared_global_dir: bool,
    #[serde(default)]
    pub show_in_universal_list: bool,
    #[serde(default)]
    pub show_in_universal_prompt: bool,
    #[serde(default)]
    pub detect_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedAgent {
    pub id: String,
    pub display_name: String,
    pub global_skills_dir: Option<String>,
    pub universal: bool,
}

#[derive(Debug, Clone)]
pub struct DetectionContext {
    pub home: PathBuf,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
}

impl DetectionContext {
    pub fn new(
        home: impl Into<PathBuf>,
        cwd: impl Into<PathBuf>,
        env: BTreeMap<String, String>,
    ) -> Self {
        Self {
            home: home.into(),
            cwd: cwd.into(),
            env,
        }
    }
}

pub fn catalog() -> &'static AgentCatalog {
    static CATALOG: OnceLock<AgentCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(CATALOG_JSON).expect("vendored agent catalog must be valid JSON")
    })
}

/// Catalog membership decides who the pinned CLI attaches. Everything else is
/// an extra agent and is linked by skills-box itself.
pub fn is_catalog_agent(id: &str) -> bool {
    catalog()
        .agents
        .iter()
        .any(|definition| definition.id == id)
}

pub fn detect_agents(context: &DetectionContext) -> Vec<DetectedAgent> {
    detect_definitions(context, &catalog().agents)
}

/// Detect an arbitrary definition list in the given order. Extra agents use
/// this so they follow the same detection and path-resolution rules as the
/// vendored catalog.
pub fn detect_definitions(
    context: &DetectionContext,
    definitions: &[AgentDefinition],
) -> Vec<DetectedAgent> {
    definitions
        .iter()
        .filter(|definition| is_detected(definition, context))
        .map(|definition| DetectedAgent {
            id: definition.id.clone(),
            display_name: definition.display_name.clone(),
            global_skills_dir: resolve_global_skills_dir(definition, context)
                .map(|path| display_path(&path, &context.home)),
            universal: definition.universal,
        })
        .collect()
}

pub fn is_detected(definition: &AgentDefinition, context: &DetectionContext) -> bool {
    if definition.id == "universal" {
        return false;
    }
    if definition.id == "eve" {
        return detect_eve(context);
    }

    let xdg_config = non_empty(context.env.get("XDG_CONFIG_HOME"));
    for raw in &definition.detect_paths {
        // Package metadata paths are not plain existence checks; `eve` is
        // handled above.
        if raw.starts_with("<cwd>/package.json") {
            continue;
        }

        // `configHome = $XDG_CONFIG_HOME || ~/.config`: when the env var is
        // set, the CLI never consults `~/.config`, so skip that sibling.
        if raw.starts_with("~/.config/") && xdg_config.is_some() {
            let relative = &raw["~/.config/".len()..];
            let has_env_sibling = definition
                .detect_paths
                .iter()
                .any(|path| path.strip_prefix("$XDG_CONFIG_HOME/") == Some(relative));
            if has_env_sibling {
                continue;
            }
        }

        if let Some(base) = expand_detect_path(raw, context)
            && base.exists()
        {
            return true;
        }
    }
    false
}

fn expand_detect_path(raw: &str, context: &DetectionContext) -> Option<PathBuf> {
    if let Some(rest) = raw.strip_prefix("$") {
        let (variable, suffix) = rest.split_once('/').unwrap_or((rest, ""));
        let base = non_empty(context.env.get(variable))?;
        return Some(Path::new(base).join(suffix));
    }
    if let Some(rest) = raw.strip_prefix("<cwd>") {
        let suffix = rest.trim_start_matches('/');
        return Some(context.cwd.join(suffix));
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return Some(context.home.join(rest));
    }
    if raw == "~" {
        return Some(context.home.clone());
    }
    if Path::new(raw).is_absolute() {
        return Some(PathBuf::from(raw));
    }
    None
}

fn detect_eve(context: &DetectionContext) -> bool {
    context.cwd.join("agent").exists()
        && package_json_has_dependency(&context.cwd.join("package.json"), "eve")
}

fn package_json_has_dependency(path: &Path, name: &str) -> bool {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return false;
    };
    ["dependencies", "devDependencies"]
        .iter()
        .any(|section| value.get(section).and_then(|deps| deps.get(name)).is_some())
}

pub fn resolve_global_skills_dir(
    definition: &AgentDefinition,
    context: &DetectionContext,
) -> Option<PathBuf> {
    if definition.dynamic_global_dir && definition.id == "openclaw" {
        for root in [".openclaw", ".clawdbot", ".moltbot"] {
            let candidate = context.home.join(root);
            if candidate.exists() {
                return Some(candidate.join("skills"));
            }
        }
        return Some(context.home.join(".openclaw/skills"));
    }

    let template = definition.global_skills_dir.as_deref()?;

    if let Some(env_spec) = definition.global_skills_dir_env.as_deref() {
        let variables: Vec<&str> = env_spec
            .split(['/', ' '])
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect();

        for variable in variables {
            let Some(root) = non_empty(context.env.get(variable)) else {
                continue;
            };
            // `$XDG_CONFIG_HOME` replaces only the `~/.config` prefix.
            if variable == "XDG_CONFIG_HOME" {
                if let Some(rest) = template.strip_prefix("~/.config/") {
                    return Some(Path::new(root).join(rest));
                }
                continue;
            }
            // Agent-specific homes (CLAUDE_CONFIG_DIR, AUTOHAND_HOME, ...)
            // replace the agent's own home-relative root. Universal agents
            // whose native dir is the canonical store keep that path.
            if !definition.universal
                && let Some(rest) = template.strip_prefix("~/")
            {
                if let Some((_first, tail)) = rest.split_once('/') {
                    return Some(Path::new(root).join(tail));
                }
                return Some(PathBuf::from(root));
            }
        }
    }

    if let Some(rest) = template.strip_prefix("~/") {
        Some(context.home.join(rest))
    } else if template == "~" {
        Some(context.home.clone())
    } else {
        Some(PathBuf::from(template))
    }
}

fn non_empty(value: Option<&String>) -> Option<&str> {
    value.map(String::as_str).filter(|value| !value.is_empty())
}

fn display_path(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;

    fn context(home: &Path, cwd: &Path) -> DetectionContext {
        DetectionContext::new(home, cwd, BTreeMap::new())
    }

    fn detected_ids(context: &DetectionContext) -> Vec<String> {
        detect_agents(context)
            .into_iter()
            .map(|agent| agent.id)
            .collect()
    }

    #[test]
    fn catalog_matches_pinned_cli_shape() {
        let catalog = catalog();
        assert_eq!(catalog.source, "skills@1.7.4");
        assert_eq!(catalog.agents.len(), 79);

        let mut ids: Vec<&str> = catalog.agents.iter().map(|a| a.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 79, "agent ids must be unique");

        let universal = catalog.agents.iter().filter(|a| a.universal).count();
        assert_eq!(universal, 24);
        let without_global_dir = catalog
            .agents
            .iter()
            .filter(|a| a.global_skills_dir.is_none())
            .map(|a| a.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(without_global_dir, vec!["eve", "promptscript"]);

        let claude = catalog
            .agents
            .iter()
            .find(|a| a.id == "claude-code")
            .unwrap();
        assert_eq!(claude.display_name, "Claude Code");
        assert_eq!(
            claude.global_skills_dir.as_deref(),
            Some("~/.claude/skills")
        );
        assert!(!claude.universal);

        let gemini = catalog
            .agents
            .iter()
            .find(|a| a.id == "gemini-cli")
            .unwrap();
        assert!(gemini.universal);
        assert_eq!(
            gemini.global_skills_dir.as_deref(),
            Some("~/.gemini/skills")
        );
    }

    #[test]
    fn detects_home_roots_in_catalog_order() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let cwd = tmp.path().join("cli-cwd");
        fs::create_dir_all(home.join(".claude")).unwrap();
        fs::create_dir_all(home.join(".gemini")).unwrap();
        fs::create_dir_all(home.join(".qoder")).unwrap();
        fs::create_dir_all(home.join(".config/amp")).unwrap();
        fs::create_dir_all(&cwd).unwrap();

        let agents = detect_agents(&context(&home, &cwd));
        let ids = agents
            .iter()
            .map(|agent| agent.id.as_str())
            .collect::<Vec<_>>();
        assert!(ids.contains(&"claude-code"));
        assert!(ids.contains(&"gemini-cli"));
        assert!(ids.contains(&"qoder"));
        assert!(ids.contains(&"amp"));
        assert!(!ids.contains(&"codex"));
        assert!(!ids.contains(&"universal"));
        // Catalog order is preserved.
        assert_eq!(ids, vec!["amp", "claude-code", "gemini-cli", "qoder"]);
    }

    #[test]
    fn config_home_env_replaces_dot_config() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let cwd = tmp.path().join("cli-cwd");
        let config = tmp.path().join("xdg-config");
        fs::create_dir_all(home.join(".config/opencode")).unwrap();
        fs::create_dir_all(config.join("amp")).unwrap();

        let mut env = BTreeMap::new();
        env.insert("XDG_CONFIG_HOME".to_string(), config.display().to_string());
        let ctx = DetectionContext::new(&home, &cwd, env);

        let ids = detected_ids(&ctx);
        // XDG is set: the CLI only checks $XDG_CONFIG_HOME/amp and never
        // falls back to ~/.config, so stale ~/.config/opencode must not count.
        assert!(ids.contains(&"amp".to_string()));
        assert!(!ids.contains(&"opencode".to_string()));

        // Without XDG_CONFIG_HOME the same ~/.config/opencode is detected.
        let ids = detected_ids(&context(&home, &cwd));
        assert!(ids.contains(&"opencode".to_string()));
    }

    #[test]
    fn agent_specific_home_overrides_are_honored() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let cwd = tmp.path().join("cli-cwd");
        let claude_home = tmp.path().join("claude-config");
        fs::create_dir_all(&claude_home).unwrap();

        let mut env = BTreeMap::new();
        env.insert(
            "CLAUDE_CONFIG_DIR".to_string(),
            claude_home.display().to_string(),
        );
        let ctx = DetectionContext::new(&home, &cwd, env);
        let agents = detect_agents(&ctx);
        let claude = agents
            .iter()
            .find(|agent| agent.id == "claude-code")
            .unwrap();
        assert_eq!(
            claude.global_skills_dir.as_deref(),
            Some(format!("{}/skills", claude_home.display()).as_str())
        );
    }

    #[test]
    fn detects_cwd_scoped_agents_only_within_cli_cwd() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let cwd = tmp.path().join("cli-cwd");
        fs::create_dir_all(cwd.join(".replit")).unwrap();
        fs::create_dir_all(cwd.join(".promptscript")).unwrap();

        let ids = detected_ids(&context(&home, &cwd));
        assert!(ids.contains(&"replit".to_string()));
        assert!(ids.contains(&"promptscript".to_string()));
        assert!(!ids.contains(&"continue".to_string()));
    }

    #[test]
    fn eve_requires_agent_dir_and_package_dependency() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let cwd = tmp.path().join("cli-cwd");
        fs::create_dir_all(cwd.join("agent")).unwrap();
        fs::write(cwd.join("package.json"), r#"{"dependencies":{}}"#).unwrap();
        assert!(!detected_ids(&context(&home, &cwd)).contains(&"eve".to_string()));

        fs::write(
            cwd.join("package.json"),
            r#"{"devDependencies":{"eve":"^1.0.0"}}"#,
        )
        .unwrap();
        assert!(detected_ids(&context(&home, &cwd)).contains(&"eve".to_string()));
    }

    #[test]
    fn shared_directories_detect_both_alias_agents() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let cwd = tmp.path().join("cli-cwd");
        fs::create_dir_all(home.join(".zencoder")).unwrap();
        let ids = detected_ids(&context(&home, &cwd));
        assert!(ids.contains(&"zencoder".to_string()));
        assert!(ids.contains(&"zenflow".to_string()));
    }

    #[test]
    fn resolves_display_paths_and_openclaw_dynamic_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let cwd = tmp.path().join("cli-cwd");
        fs::create_dir_all(home.join(".claude")).unwrap();
        fs::create_dir_all(home.join(".clawdbot")).unwrap();
        let agents = detect_agents(&context(&home, &cwd));

        let claude = agents
            .iter()
            .find(|agent| agent.id == "claude-code")
            .unwrap();
        assert_eq!(
            claude.global_skills_dir.as_deref(),
            Some("~/.claude/skills")
        );

        let openclaw = agents.iter().find(|agent| agent.id == "openclaw").unwrap();
        assert_eq!(
            openclaw.global_skills_dir.as_deref(),
            Some("~/.clawdbot/skills")
        );
    }

    #[test]
    fn universal_pseudo_agent_is_never_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let cwd = tmp.path().join("cli-cwd");
        fs::create_dir_all(home.join(".agents/skills")).unwrap();
        assert!(!detected_ids(&context(&home, &cwd)).contains(&"universal".to_string()));
    }
}
