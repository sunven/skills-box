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
