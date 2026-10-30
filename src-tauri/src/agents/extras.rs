//! Extra agents: agents outside the vendored CLI catalog.
//!
//! Builtin extras ship with the app; user-defined extras live in the app-data
//! `agents.json` and are also editable by hand. Both resolve to the same
//! runtime shape: a global skills directory template (`~/...` or absolute) and
//! a detection rule ("the parent directory exists"). skills-box attaches their
//! skills natively instead of through the pinned CLI.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::paths::AppPaths;
use crate::security::normalize_absolute;

use super::AgentDefinition;

pub const CONFIG_VERSION: u64 = 1;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserExtraAgent {
    pub id: String,
    pub display_name: String,
    pub skills_dir: String,
    /// Project-relative skills directory. Only read by the project inventory in
    /// this ticket; validation, UI and guards arrive with ticket 07.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_skills_dir: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtraAgent {
    pub id: String,
    pub display_name: String,
    pub skills_dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_skills_dir: Option<String>,
    pub builtin: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveExtraAgentResult {
    pub id: String,
    pub display_name: String,
    /// Skill ids detached before the definition was removed.
    pub detached: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentOrigin {
    Catalog,
    Builtin,
    User,
}

#[derive(Debug, Clone, Default)]
pub struct ExtraAgents {
    pub agents: Vec<ExtraAgent>,
    /// Load problems. They never block the app: invalid entries are skipped
    /// and surfaced in the inventory warnings.
    pub warnings: Vec<String>,
}

impl ExtraAgent {
    /// The catalog-shaped definition used by the shared detection and display
    /// helpers. `skills_dir` (project scope) is unused because this app is
    /// global-only.
    pub fn definition(&self) -> AgentDefinition {
        AgentDefinition {
            id: self.id.clone(),
            display_name: self.display_name.clone(),
            global_skills_dir: Some(self.skills_dir.clone()),
            universal: false,
            skills_dir: String::new(),
            install_dir: None,
            global_skills_dir_env: None,
            dynamic_global_dir: false,
            shared_global_dir: false,
            show_in_universal_list: false,
            show_in_universal_prompt: false,
            detect_paths: vec![detect_path(&self.skills_dir)],
        }
    }
}

pub fn builtin_extras() -> Vec<ExtraAgent> {
    vec![ExtraAgent {
        id: "workbuddy".to_string(),
        display_name: "WorkBuddy".to_string(),
        skills_dir: "~/.workbuddy/skills".to_string(),
        project_skills_dir: None,
        builtin: true,
    }]
}

/// Builtin extras plus the valid user entries from `agents.json`.
pub fn load(paths: &AppPaths) -> ExtraAgents {
    let mut result = ExtraAgents {
        agents: builtin_extras(),
        warnings: Vec::new(),
    };
    let (entries, mut warnings) = read_user_entries(paths);
    result.warnings.append(&mut warnings);

    let mut seen: BTreeSet<String> = result.agents.iter().map(|agent| agent.id.clone()).collect();
    for entry in entries {
        match validate(paths, &entry) {
            Ok(agent) => {
                if super::is_catalog_agent(&agent.id) {
                    result.warnings.push(format!(
                        "忽略额外 agent「{}」：id 与内置 agent 冲突。",
                        entry.id
                    ));
                    continue;
                }
                if !seen.insert(agent.id.clone()) {
                    result
                        .warnings
                        .push(format!("忽略额外 agent「{}」：id 重复。", entry.id));
                    continue;
                }
                result.agents.push(agent);
            }
            Err(message) => result
                .warnings
                .push(format!("忽略额外 agent 配置：{message}")),
        }
    }
    result
}

pub fn find(paths: &AppPaths, id: &str) -> Option<ExtraAgent> {
    load(paths).agents.into_iter().find(|agent| agent.id == id)
}

pub fn origin_of(paths: &AppPaths, id: &str) -> AgentOrigin {
    if let Some(agent) = find(paths, id) {
        if agent.builtin {
            AgentOrigin::Builtin
        } else {
            AgentOrigin::User
        }
    } else {
        AgentOrigin::Catalog
    }
}

pub fn is_extra(paths: &AppPaths, id: &str) -> bool {
    find(paths, id).is_some()
}

/// Append one validated user entry. Existing entries are preserved verbatim,
/// including ones that failed validation, so a hand-edited file is never
/// silently rewritten into a lossy form.
pub fn add_user_agent(paths: &AppPaths, entry: &UserExtraAgent) -> Result<ExtraAgent, AppError> {
    let agent = validate(paths, entry).map_err(AppError::extra_agent_invalid)?;
    let (mut entries, _) = read_user_entries(paths);
    if entries.iter().any(|existing| existing.id == agent.id)
        || builtin_extras()
            .iter()
            .any(|builtin| builtin.id == agent.id)
        || super::catalog()
            .agents
            .iter()
            .any(|catalog| catalog.id == agent.id)
    {
        return Err(AppError::extra_agent_duplicate(&agent.id));
    }
    entries.push(UserExtraAgent {
        id: agent.id.clone(),
        display_name: agent.display_name.clone(),
        skills_dir: agent.skills_dir.clone(),
        project_skills_dir: agent.project_skills_dir.clone(),
    });
    save_user_entries(paths, &entries)?;
    Ok(agent)
}

/// Remove one user entry. Builtin extras cannot be removed. Callers detach
/// attachments first; this only edits the configuration file.
pub fn remove_user_agent(paths: &AppPaths, id: &str) -> Result<ExtraAgent, AppError> {
    let agent = find(paths, id).ok_or_else(|| AppError::extra_agent_not_found(id))?;
    if agent.builtin {
        return Err(AppError::extra_agent_builtin(&agent.display_name));
    }
    let (mut entries, _) = read_user_entries(paths);
    let before = entries.len();
    entries.retain(|entry| entry.id != id);
    if entries.len() == before {
        return Err(AppError::extra_agent_not_found(id));
    }
    save_user_entries(paths, &entries)?;
    Ok(agent)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigFile {
    #[serde(default)]
    version: Option<u64>,
    #[serde(default)]
    agents: Vec<UserExtraAgent>,
}

/// Raw, unvalidated entries. `serde` ignores unknown fields so a future
/// version's file still loads the known shape.
pub fn read_user_entries(paths: &AppPaths) -> (Vec<UserExtraAgent>, Vec<String>) {
    let path = paths.agents_config_path();
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (Vec::new(), Vec::new());
        }
        Err(error) => {
            return (
                Vec::new(),
                vec![format!("无法读取 {}：{error}", path.display())],
            );
        }
    };
    match serde_json::from_str::<ConfigFile>(&raw) {
        Ok(config) => {
            if let Some(version) = config.version
                && version != CONFIG_VERSION
            {
                return (
                    Vec::new(),
                    vec![format!(
                        "{} 的版本是 {version}，当前只支持 {CONFIG_VERSION}；已忽略用户自定义 agent。",
                        path.display()
                    )],
                );
            }
            (config.agents, Vec::new())
        }
        Err(error) => (
            Vec::new(),
            vec![format!("{} 不是有效的 JSON：{error}", path.display())],
        ),
    }
}

pub fn save_user_entries(paths: &AppPaths, entries: &[UserExtraAgent]) -> Result<(), AppError> {
    let path = paths.agents_config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AppError::extra_agent_config(format!("{}：{error}", parent.display()))
        })?;
    }
    let payload = serde_json::json!({
        "version": CONFIG_VERSION,
        "agents": entries,
    });
    let raw = serde_json::to_string_pretty(&payload)
        .map_err(|error| AppError::extra_agent_config(error.to_string()))?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, format!("{raw}\n"))
        .map_err(|error| AppError::extra_agent_config(format!("{}：{error}", temp.display())))?;
    fs::rename(&temp, &path)
        .map_err(|error| AppError::extra_agent_config(format!("{}：{error}", path.display())))
}

fn validate(paths: &AppPaths, entry: &UserExtraAgent) -> Result<ExtraAgent, String> {
    let id = entry.id.trim();
    let id_invalid = id.is_empty()
        || id.len() > 64
        || id == "universal"
        || id == "*"
        || id.starts_with('-')
        || !id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        });
    if id_invalid {
        return Err(format!("id「{}」不合法。", entry.id));
    }

    let display_name = entry.display_name.trim();
    if display_name.is_empty()
        || display_name.chars().count() > 100
        || display_name.chars().any(char::is_control)
    {
        return Err(format!("agent「{id}」的显示名不合法。"));
    }

    let skills_dir = normalize_skills_dir(paths, &entry.skills_dir)?;
    Ok(ExtraAgent {
        id: id.to_string(),
        display_name: display_name.to_string(),
        skills_dir,
        project_skills_dir: entry.project_skills_dir.clone(),
        builtin: false,
    })
}

/// Normalize a skills directory template to `~/...` (when under home) or an
/// absolute path. Rejects relative paths, traversal-free fictions, the shared
/// home directory itself, and the canonical store.
pub fn normalize_skills_dir(paths: &AppPaths, raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
        return Err("skills 目录不能为空或包含控制字符。".to_string());
    }
    let expanded: PathBuf = if let Some(rest) = trimmed.strip_prefix("~/") {
        if rest.is_empty() {
            return Err("skills 目录不能是主目录本身。".to_string());
        }
        paths.home().join(rest)
    } else if trimmed.starts_with('/') {
        PathBuf::from(trimmed)
    } else {
        return Err(format!(
            "skills 目录「{trimmed}」必须以 `~/` 开头或是绝对路径。"
        ));
    };
    let normalized = normalize_absolute(&expanded)
        .ok_or_else(|| format!("skills 目录「{trimmed}」无法规范化。"))?;
    if normalized == paths.home() {
        return Err("skills 目录不能是主目录本身。".to_string());
    }
    if normalized == paths.canonical_skills_dir() {
        return Err("skills 目录不能是 canonical store（~/.agents/skills）。".to_string());
    }
    if normalized.parent().is_none() {
        return Err("skills 目录必须位于某个父目录之下。".to_string());
    }
    Ok(template(paths, &normalized))
}

fn template(paths: &AppPaths, path: &Path) -> String {
    match path.strip_prefix(paths.home()) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

fn detect_path(skills_dir: &str) -> String {
    Path::new(skills_dir)
        .parent()
        .map(|parent| parent.display().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> (tempfile::TempDir, AppPaths) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        (tmp, paths)
    }

    fn entry(id: &str, skills_dir: &str) -> UserExtraAgent {
        UserExtraAgent {
            id: id.to_string(),
            display_name: id.to_uppercase(),
            skills_dir: skills_dir.to_string(),
            project_skills_dir: None,
        }
    }

    #[test]
    fn project_skills_dir_is_read_from_hand_written_entries_and_preserved_on_add() {
        let (_tmp, paths) = paths();
        fs::create_dir_all(paths.app_data()).unwrap();
        fs::write(
            paths.agents_config_path(),
            r#"{
                "version": 1,
                "agents": [
                    {"id": "acme", "displayName": "Acme", "skillsDir": "~/.acme/skills", "projectSkillsDir": ".acme/skills"}
                ]
            }"#,
        )
        .unwrap();
        let loaded = load(&paths);
        let acme = loaded
            .agents
            .iter()
            .find(|agent| agent.id == "acme")
            .unwrap();
        assert_eq!(acme.project_skills_dir.as_deref(), Some(".acme/skills"));

        let added = add_user_agent(
            &paths,
            &UserExtraAgent {
                id: "other".to_string(),
                display_name: "Other".to_string(),
                skills_dir: "~/.other/skills".to_string(),
                project_skills_dir: Some(".other/skills".to_string()),
            },
        )
        .unwrap();
        assert_eq!(added.project_skills_dir.as_deref(), Some(".other/skills"));
        let reloaded = load(&paths);
        assert_eq!(
            reloaded
                .agents
                .iter()
                .find(|agent| agent.id == "other")
                .unwrap()
                .project_skills_dir
                .as_deref(),
            Some(".other/skills")
        );
        // The hand-written entry round-trips untouched, including its field.
        assert_eq!(
            reloaded
                .agents
                .iter()
                .find(|agent| agent.id == "acme")
                .unwrap()
                .project_skills_dir
                .as_deref(),
            Some(".acme/skills")
        );
    }

    #[test]
    fn builtin_workbuddy_is_loaded_and_detected_by_parent() {
        let (_tmp, paths) = paths();
        let loaded = load(&paths);
        let workbuddy = loaded
            .agents
            .iter()
            .find(|agent| agent.id == "workbuddy")
            .unwrap();
        assert!(workbuddy.builtin);
        let definition = workbuddy.definition();
        assert_eq!(definition.detect_paths, vec!["~/.workbuddy"]);
        assert_eq!(
            definition.global_skills_dir.as_deref(),
            Some("~/.workbuddy/skills")
        );
    }

    #[test]
    fn user_entries_round_trip_and_normalize_home_paths() {
        let (_tmp, paths) = paths();
        let home = paths.home().to_path_buf();
        let added = add_user_agent(
            &paths,
            &entry("acme", &format!("{}/acme/skills/", home.display())),
        )
        .unwrap();
        assert_eq!(added.skills_dir, "~/acme/skills");
        let loaded = load(&paths);
        let acme = loaded
            .agents
            .iter()
            .find(|agent| agent.id == "acme")
            .unwrap();
        assert!(!acme.builtin);
        let raw = fs::read_to_string(paths.agents_config_path()).unwrap();
        assert!(raw.contains("\"version\": 1"));
        assert!(raw.contains("\"skillsDir\": \"~/acme/skills\""));

        remove_user_agent(&paths, "acme").unwrap();
        assert!(!load(&paths).agents.iter().any(|agent| agent.id == "acme"));
    }

    #[test]
    fn invalid_entries_are_skipped_with_warnings() {
        let (_tmp, paths) = paths();
        save_user_entries(
            &paths,
            &[
                entry("good", "~/.good/skills"),
                entry("claude-code", "~/.dup/skills"),
                entry("bad dir", "~/.bad/skills"),
                entry("relative", "relative/skills"),
                entry("store", "~/.agents/skills"),
            ],
        )
        .unwrap();
        let loaded = load(&paths);
        let ids: Vec<&str> = loaded
            .agents
            .iter()
            .map(|agent| agent.id.as_str())
            .collect();
        assert!(ids.contains(&"good"));
        assert!(!ids.contains(&"claude-code"));
        assert!(!ids.contains(&"bad dir"));
        assert!(!ids.contains(&"relative"));
        assert!(!ids.contains(&"store"));
        assert!(loaded.warnings.len() >= 4, "{:?}", loaded.warnings);
    }

    #[test]
    fn duplicate_and_builtin_ids_are_rejected_on_add() {
        let (_tmp, paths) = paths();
        let error = add_user_agent(&paths, &entry("workbuddy", "~/.wb/skills")).unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::ExtraDuplicate);
        let error = add_user_agent(&paths, &entry("qoder", "~/.q/skills")).unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::ExtraDuplicate);
        add_user_agent(&paths, &entry("acme", "~/.acme/skills")).unwrap();
        let error = add_user_agent(&paths, &entry("acme", "~/.acme2/skills")).unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::ExtraDuplicate);
    }

    #[test]
    fn builtin_extras_cannot_be_removed() {
        let (_tmp, paths) = paths();
        let error = remove_user_agent(&paths, "workbuddy").unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::ExtraBuiltin);
    }

    #[test]
    fn corrupt_config_warns_but_keeps_builtins() {
        let (_tmp, paths) = paths();
        fs::create_dir_all(paths.app_data()).unwrap();
        fs::write(paths.agents_config_path(), "{not json").unwrap();
        let loaded = load(&paths);
        assert_eq!(loaded.agents.len(), 1);
        assert!(
            loaded
                .warnings
                .iter()
                .any(|warning| warning.contains("JSON"))
        );
    }
}
