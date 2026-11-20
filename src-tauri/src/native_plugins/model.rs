use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Host {
    Codex,
    Claude,
}
impl Host {
    pub fn command(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    AddMarket,
    SyncMarket,
    RemoveMarket,
    Install,
    Enable,
    Disable,
    Update,
    Reinstall,
    Uninstall,
}
impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Self::AddMarket => "添加市场",
            Self::SyncMarket => "同步市场",
            Self::RemoveMarket => "移除市场",
            Self::Install => "安装",
            Self::Enable => "启用",
            Self::Disable => "禁用",
            Self::Update => "更新",
            Self::Reinstall => "重新安装",
            Self::Uninstall => "卸载",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub host: Host,
    pub action: Action,
    #[serde(default)]
    pub context: Context,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub source_kind: String,
    #[serde(default)]
    pub reference: String,
    #[serde(default)]
    pub sparse_paths: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capability {
    pub action: Action,
    pub blocked_reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Market {
    pub name: String,
    pub source_kind: String,
    pub source: String,
    pub root: Option<String>,
    pub reference: Option<String>,
    pub sparse_paths: Vec<String>,
    pub read_only_reason: Option<String>,
    pub remove_blocked_reason: Option<String>,
    pub sync_blocked_reason: Option<String>,
    pub catalog_error: Option<String>,
    pub affected_plugins: Vec<String>,
    #[serde(default)]
    pub declarations: Vec<String>,
    #[serde(default)]
    pub declared_in_target: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Installation {
    pub scope: String,
    pub version: Option<String>,
    pub path: Option<String>,
    pub project_path: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plugin {
    pub id: String,
    pub name: String,
    pub marketplace: String,
    pub description: Option<String>,
    pub version: Option<String>,
    pub installation_state: String,
    pub installations: Vec<Installation>,
    pub user_enabled: Option<bool>,
    #[serde(default)]
    pub selected_enabled: Option<bool>,
    #[serde(default)]
    pub installed_in_target: bool,
    #[serde(default)]
    pub enabled_source: Option<String>,
    #[serde(default)]
    pub enabled_overridden: bool,
    #[serde(default)]
    pub action_blocks: BTreeMap<String, String>,
    pub effective_enabled: Option<bool>,
    pub source: Value,
    pub local_root: Option<String>,
    pub read_only_reason: Option<String>,
    pub install_blocked_reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub host: Host,
    #[serde(default)]
    pub context: ResolvedContext,
    pub config_root: String,
    pub executable: String,
    pub version: String,
    pub capabilities: Vec<Capability>,
    pub markets: Vec<Market>,
    pub plugins: Vec<Plugin>,
    pub warnings: Vec<String>,
    pub fingerprint: String,
    pub source_kinds: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub token: String,
    pub request: Request,
    pub config_root: String,
    #[serde(default)]
    pub context: ResolvedContext,
    pub version: String,
    pub command: String,
    pub consequences: Vec<String>,
    pub affected_plugins: Vec<String>,
    pub fingerprint: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub status: String,
    pub message: String,
    pub executed: bool,
    pub command_succeeded: Option<bool>,
    pub verification: String,
    pub snapshot: Option<Snapshot>,
    pub preview: Preview,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Details {
    pub readme: Option<String>,
    pub manifest: Value,
    pub components: BTreeMap<String, Vec<String>>,
    pub messages: Vec<String>,
}

/// An app Scope and a host configuration layer, never a caller-supplied cwd.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Context {
    #[serde(default)]
    pub scope: crate::scope::ScopeId,
    #[serde(default)]
    pub layer: Layer,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    #[default]
    User,
    Project,
    Local,
}
impl Layer {
    pub fn native(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Project => "project",
            Self::Local => "local",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::User => "全局",
            Self::Project => "项目共享",
            Self::Local => "仅自己在此项目",
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedContext {
    #[serde(flatten)]
    pub target: Context,
    pub project_root: Option<String>,
    pub project_name: Option<String>,
    pub revision: Option<String>,
}
impl ResolvedContext {
    pub fn matches_installation(&self, i: &Installation) -> bool {
        i.scope == self.target.layer.native()
            && (self.target.scope.is_global()
                || self
                    .project_root
                    .as_ref()
                    .zip(i.project_path.as_ref())
                    .is_some_and(|(a, b)| {
                        super::transport::same_path(
                            std::path::Path::new(a),
                            std::path::Path::new(b),
                        )
                    }))
    }
    pub fn label(&self) -> String {
        self.project_name
            .as_ref()
            .map(|name| format!("{} · {}", name, self.target.layer.label()))
            .unwrap_or_else(|| self.target.layer.label().into())
    }
}
