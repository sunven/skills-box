//! Host adapters provide default capabilities independently of CLI versions.
//! Observed failures and isolated probes can disable individual operations.
use super::{inventory, model::*, operations, transport::Runtime};
use serde_json::json;
use std::{collections::BTreeMap, fs};
#[derive(Clone)]
pub struct Contract {
    pub capabilities: Vec<Capability>,
    pub sources: Vec<String>,
}
impl Contract {
    pub fn apply_probe(&mut self, tested: &Contract) {
        // The basic probe does not cover sync, remote sources or cascading
        // removal. Preserve their current state, including observed failures.
        for capability in &mut self.capabilities {
            if matches!(capability.action, Action::SyncMarket | Action::RemoveMarket) {
                continue;
            }
            if let Some(result) = tested
                .capabilities
                .iter()
                .find(|result| result.action == capability.action)
            {
                *capability = result.clone();
            }
        }
    }
}
pub fn actions(host: Host) -> Vec<Action> {
    let mut a = vec![
        Action::AddMarket,
        Action::SyncMarket,
        Action::RemoveMarket,
        Action::Install,
        Action::Enable,
        Action::Disable,
        Action::Uninstall,
    ];
    a.push(if host == Host::Codex {
        Action::Reinstall
    } else {
        Action::Update
    });
    a
}
pub fn defaults(host: Host) -> Contract {
    Contract {
        capabilities: actions(host)
            .into_iter()
            .map(|action| Capability {
                action,
                blocked_reason: None,
            })
            .collect(),
        sources: match host {
            Host::Codex => vec!["local".into(), "git".into()],
            Host::Claude => vec!["local".into(), "git".into(), "url".into()],
        },
    }
}
pub fn key(rt: &Runtime, version: &str) -> String {
    let executable = fs::canonicalize(&rt.executable).unwrap_or(rt.executable.clone());
    let meta = fs::metadata(&executable).ok();
    inventory::hash(&json!([
        rt.host,
        version,
        executable,
        meta.as_ref().map(|m| m.len()),
        meta.and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos().to_string())
    ]))
}
pub fn isolated_runtime(rt: &Runtime, root: &std::path::Path) -> Result<Runtime, String> {
    let home = root.join("home");
    let config = home.join(if rt.host == Host::Codex {
        ".codex"
    } else {
        ".claude"
    });
    fs::create_dir_all(&config).map_err(|e| e.to_string())?;
    let mut env = BTreeMap::new();
    env.insert(
        "PATH".into(),
        rt.env
            .get("PATH")
            .cloned()
            .unwrap_or_else(|| "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin".into()),
    );
    for (key, value) in [
        ("GIT_CONFIG_GLOBAL", "/dev/null"),
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("DISABLE_TELEMETRY", "1"),
        ("DISABLE_ERROR_REPORTING", "1"),
    ] {
        env.insert(key.into(), value.into());
    }
    env.insert(
        "XDG_CONFIG_HOME".into(),
        home.join(".config").to_string_lossy().into(),
    );
    Ok(Runtime {
        host: rt.host,
        home,
        root: config,
        executable: rt.executable.clone(),
        cwd: root.to_path_buf(),
        env,
        isolated: true,
    })
}
pub fn fixture(root: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let market = root.join("market");
    for dir in [".claude-plugin", "demo/.claude-plugin", "demo/skills/hello"] {
        fs::create_dir_all(market.join(dir)).map_err(|e| e.to_string())?;
    }
    for (path,data) in [(".claude-plugin/marketplace.json",json!({"name":"skills-box-verification","owner":{"name":"Skills Box fixture"},"plugins":[{"name":"demo","source":"./demo"}]}).to_string()),("demo/.claude-plugin/plugin.json",json!({"name":"demo","version":"1.0.0"}).to_string()),("demo/skills/hello/SKILL.md","---\nname: hello\ndescription: Static compatibility fixture\n---\nNo executable content.\n".into())]{fs::write(market.join(path),data).map_err(|e|e.to_string())?;}
    Ok(market)
}
pub fn verify(rt: &Runtime) -> Result<Contract, String> {
    let temp = tempfile::Builder::new()
        .prefix("skills-box-plugin-check-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let isolated = isolated_runtime(rt, temp.path())?;
    let market = fixture(temp.path())?;
    let mut contract = Contract {
        capabilities: actions(rt.host)
            .into_iter()
            .map(|action| Capability {
                action,
                blocked_reason: Some("此操作未通过隔离验证。".into()),
            })
            .collect(),
        sources: vec![],
    };
    let permissive: Vec<_> = actions(rt.host)
        .into_iter()
        .map(|action| Capability {
            action,
            blocked_reason: None,
        })
        .collect();
    let mut sequence = vec![
        Action::AddMarket,
        Action::Install,
        Action::Disable,
        if rt.host == Host::Codex {
            Action::Reinstall
        } else {
            Action::Update
        },
        Action::Enable,
        Action::Uninstall,
        Action::RemoveMarket,
    ];
    // Codex reinstall enables again; Claude update preserves the disabled setting.
    if rt.host == Host::Codex {
        sequence.insert(4, Action::Disable);
    }
    for action in sequence {
        let request = Request {
            context: Context::default(),
            host: rt.host,
            action,
            target: if action == Action::RemoveMarket {
                "skills-box-verification"
            } else {
                "demo@skills-box-verification"
            }
            .into(),
            source: market.to_string_lossy().into(),
            source_kind: "local".into(),
            reference: String::new(),
            sparse_paths: vec![],
        };
        let result = (|| {
            let mut before = inventory::load(&isolated, permissive.clone(), &[])?;
            before.snapshot.source_kinds = vec!["local".into()];
            let out = operations::execute(&isolated, &request, &before)?;
            let after = inventory::load(&isolated, permissive.clone(), &[])?;
            if !operations::response_matches(&request, &out)
                || !operations::postcondition(&request, &before, &after, &out)
            {
                return Err(out.diagnostic);
            }
            Ok::<(), String>(())
        })();
        if let Some(c) = contract
            .capabilities
            .iter_mut()
            .find(|c| c.action == action)
        {
            c.blocked_reason = result.err().map(|e| format!("隔离验证未通过：{e}"));
        }
    }
    // Sync, remote source effects and cascading removal require the broader
    // release verification suite. Local success does not certify those effects.
    if contract
        .capabilities
        .iter()
        .any(|c| c.action == Action::AddMarket && c.blocked_reason.is_none())
    {
        contract.sources.push("local".into());
    }
    // Empty-market cleanup does not test cascading removal. This result is
    // excluded when merging the probe into the host's current capabilities.
    if let Some(c) = contract
        .capabilities
        .iter_mut()
        .find(|c| c.action == Action::RemoveMarket)
    {
        c.blocked_reason = Some("仅检测了空市场移除；基本兼容性检查不覆盖连带移除影响。".into());
    }
    Ok(contract)
}

pub fn context_key(rt: &Runtime, version: &str, layer: Layer) -> String {
    let key = key(rt, version);
    if layer == Layer::User {
        key
    } else {
        format!("{key}:{}", layer.native())
    }
}
