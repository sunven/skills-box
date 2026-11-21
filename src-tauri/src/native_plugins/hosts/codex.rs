use super::super::{
    contracts::Contract,
    inventory::{hash, identity, optional_json, valid_name},
    model::*,
    transport::{self, Output, Runtime, same_path},
};
use super::{Evidence, Observation, shared::*};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
pub(super) struct NativeEvidence {
    pub(super) user_version: Option<String>,
}
fn user_origin(origin: &Value, path: &Path) -> bool {
    origin["name"]["type"] == "user"
        && origin["name"]["profile"].is_null()
        && origin["name"]["file"]
            .as_str()
            .is_some_and(|p| same_path(Path::new(p), path))
}
fn owned_value(
    value: &Value,
    key: &str,
    origins: &serde_json::Map<String, Value>,
    path: &Path,
) -> bool {
    if let Some(origin) = origins.get(key) {
        return user_origin(origin, path);
    }
    match value {
        Value::Array(items) => {
            !items.is_empty()
                && items
                    .iter()
                    .enumerate()
                    .all(|(i, v)| owned_value(v, &format!("{key}.{i}"), origins, path))
        }
        Value::Object(fields) => {
            !fields.is_empty()
                && fields
                    .iter()
                    .all(|(k, v)| owned_value(v, &format!("{key}.{k}"), origins, path))
        }
        _ => false,
    }
}
pub(super) fn owns(config: &Value, user: &Value, section: &str, key: &str, path: &Path) -> bool {
    let prefix = format!("{section}.{key}.");
    let Some(fields) = user[section][key].as_object() else {
        return false;
    };
    let Some(origins) = config["origins"].as_object() else {
        return false;
    };
    !fields.is_empty()
        && fields.iter().all(|(field, value)| {
            config["config"][section][key][field] == *value
                && owned_value(value, &format!("{prefix}{field}"), origins, path)
        })
        && origins
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix))
            .all(|(_, o)| user_origin(o, path))
}
pub(super) fn observe(
    rt: &Runtime,
    mut capabilities: Vec<Capability>,
    projects: &[PathBuf],
    context: ResolvedContext,
) -> Result<Observation, String> {
    let version = rt.version()?;
    let raw_markets = rt.json(&["plugin", "marketplace", "list", "--json"])?;
    let listed = rt.json(&["plugin", "list", "--json"]);
    let inventory_error = listed.as_ref().err().cloned();
    let raw_plugins = listed.unwrap_or_else(|_| json!({"installed":[]}));
    let mut warnings = vec![];
    if let Some(error) = &inventory_error {
        warnings.push(format!(
            "原生安装清单读取失败：{error}。目录仍可浏览，安装状态待核对。"
        ));
        for capability in &mut capabilities {
            if capability.action != Action::AddMarket {
                capability.blocked_reason =
                    Some("原生安装清单读取不完整，无法确认操作影响，请重新读取。".into());
            }
        }
    }
    let mut evidence = vec![raw_markets.clone(), raw_plugins.clone()];
    let (config, user, user_version, records) = {
        let config = rt.rpc("config/read", json!({"includeLayers":true,"cwd":rt.cwd}))?;
        let layers = array(&config["layers"])?;
        let layer = layers
            .iter()
            .find(|l| user_origin(l, &rt.root.join("config.toml")));
        let user = layer.map(|v| v["config"].clone()).unwrap_or(json!({}));
        let user_version = layer.and_then(|v| text(v, "version"));
        // Include native configuration versions, not unrelated computed session defaults.
        evidence.push(json!({"layers":config["layers"],"origins":config["origins"]}));
        let records = array(&raw_plugins["installed"])?.clone();
        (config, user, user_version, records)
    };
    let market_rows = array(&raw_markets["marketplaces"])?;
    let mut markets = vec![];
    let mut catalogs = BTreeMap::new();
    for row in market_rows {
        let name = row["name"]
            .as_str()
            .filter(|s| valid_name(s))
            .ok_or("市场身份无效。")?
            .to_string();
        if markets.iter().any(|m: &Market| m.name == name) {
            return Err("市场身份重复。".into());
        }
        let (source_kind, source, root, owned, reference, sparse) = {
            let s = &row["marketplaceSource"];
            let kind = text(s, "sourceType").unwrap_or_default();
            let location = text(s, "source").unwrap_or_default();
            let cfg = &config["config"]["marketplaces"][&name];
            let matches = cfg["source_type"] == kind
                && cfg["source"].as_str().is_some_and(|p| {
                    if kind == "local" {
                        same_path(Path::new(p), Path::new(&location))
                    } else {
                        p == location
                    }
                });
            (
                kind,
                location,
                text(row, "root"),
                matches
                    && owns(
                        &config,
                        &user,
                        "marketplaces",
                        &name,
                        &rt.root.join("config.toml"),
                    ),
                text(cfg, "ref"),
                cfg["sparse_paths"].clone(),
            )
        };
        let mut affected = vec![];
        for r in &records {
            let id = r["pluginId"].as_str().ok_or("插件身份缺失。")?;
            if identity(id)?.1 == name {
                affected.push(id.to_string());
            }
        }
        let mut project_uncertain = false;
        {
            if let Some(entries) = config["config"]["plugins"].as_object() {
                for id in entries.keys() {
                    if identity(id)?.1 == name && !affected.contains(id) {
                        affected.push(id.clone());
                    }
                }
            }
            let mut roots = projects.to_vec();
            if let Some(p) = config["config"]["projects"].as_object() {
                roots.extend(p.keys().map(PathBuf::from));
            }
            for p in roots {
                let path = p.join(".codex/config.toml");
                match fs::read(&path) {
                    Ok(bytes) => {
                        evidence.push(json!({"path":path,"digest":hash(&bytes)}));
                        project_uncertain = true;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                    Err(_) => project_uncertain = true,
                }
            }
        }
        affected.sort();
        affected.dedup();
        let mut market = Market {
            name: name.clone(),
            source_kind: source_kind.clone(),
            source: transport::redact_urls(&source),
            root: root.clone(),
            reference,
            sparse_paths: sparse
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            read_only_reason: (!owned)
                .then(|| "无法确认来源完全属于当前用户配置，仅供查看。".into()),
            remove_blocked_reason: if project_uncertain {
                Some("可观测项目含 Codex 配置，无法确定移除市场的全部影响，请在宿主中处理。".into())
            } else {
                None
            },
            sync_blocked_reason: (source_kind != "git")
                .then(|| "Codex 仅支持同步 Git 市场；本地市场请重新读取。".into()),
            catalog_error: None,
            affected_plugins: affected,
            declarations: vec![],
            declared_in_target: owned,
        };
        market.catalog_error = read_catalog(
            root,
            &name,
            &[
                ".agents/plugins/marketplace.json",
                ".agents/plugins/api_marketplace.json",
                ".claude-plugin/marketplace.json",
            ],
            &mut catalogs,
            &mut evidence,
        )
        .err();
        markets.push(market);
    }
    let mut ids: BTreeMap<String, ()> = catalogs.keys().map(|id| (id.clone(), ())).collect();
    for r in &records {
        let id = r["pluginId"].as_str().ok_or("原生插件身份缺失。")?;
        identity(id)?;
        ids.insert(id.into(), ());
    }
    {
        if let Some(entries) = config["config"]["plugins"].as_object() {
            for id in entries.keys() {
                identity(id)?;
                ids.insert(id.clone(), ());
            }
        }
    }
    let mut plugins = vec![];
    for id in ids.keys() {
        let (name, market) = identity(id)?;
        let market_row = markets.iter().find(|m| m.name == market);
        let relevant: Vec<_> = records.iter().filter(|r| r["pluginId"] == *id).collect();
        let installed = relevant.first();
        let unconfirmed = installed.is_none()
            && (inventory_error.is_some() || !config["config"]["plugins"][id].is_null());
        let (entry, local) = catalogs.get(id).cloned().unwrap_or_default();
        let source_missing = local.is_none();
        let version = installed.and_then(|r| text(r, "version"));
        let user_enabled = user["plugins"][id]["enabled"].as_bool();
        let effective_enabled = installed.and_then(|r| r["enabled"].as_bool());
        let local_root = installed
            .and_then(|r| {
                text(r, "readFromFolder")
                    .or_else(|| text(r, "installedPath"))
                    .or_else(|| text(r, "installPath"))
            })
            .map(PathBuf::from)
            .or_else(|| {
                {
                    version
                        .as_ref()
                        .map(|v| {
                            rt.root
                                .join("plugins/cache")
                                .join(market)
                                .join(name)
                                .join(v)
                        })
                        .filter(|p| p.is_dir())
                }
            })
            .or_else(|| local.clone())
            .map(|p| p.to_string_lossy().into_owned());
        let read_only = if !config["config"]["plugins"][id].is_null()
            && !owns(&config, &user, "plugins", id, &rt.root.join("config.toml"))
        {
            Some("插件配置来自非用户层或覆盖来源不明，仅供查看。".into())
        } else {
            market_row.and_then(|m| m.read_only_reason.clone())
        };
        let install_blocked = if market_row.is_none() {
            Some("市场不可用，无法确认安装来源。".into())
        } else if market_row.is_some_and(|m| m.catalog_error.is_some()) {
            Some("市场目录读取不完整，请修复后重新读取。".into())
        } else if local
            .as_ref()
            .is_some_and(|root| root.join("package.json").exists())
        {
            Some("插件包含 Node 包清单；依赖安装副作用尚未验证，请在宿主中处理。".into())
        } else if source_missing && install_restriction(&entry).is_none() {
            Some("市场内插件路径不存在或超出来源目录，无法确认安装内容。".into())
        } else {
            install_restriction(&entry)
        };
        let installations = relevant
            .iter()
            .map(|r| Installation {
                scope: "user".into(),
                version: text(r, "version"),
                path: text(r, "installPath").or_else(|| text(r, "installedPath")),
                project_path: text(r, "projectPath"),
            })
            .collect();
        plugins.push(Plugin {
            id: id.clone(),
            name: name.into(),
            marketplace: market.into(),
            description: text(&entry, "description"),
            version,
            installation_state: if installed.is_some() {
                "installed"
            } else if unconfirmed {
                "unconfirmed"
            } else {
                "absent"
            }
            .into(),
            installations,
            user_enabled,
            selected_enabled: user_enabled,
            installed_in_target: installed.is_some(),
            enabled_source: user_enabled.map(|_| "全局".into()),
            enabled_overridden: false,
            action_blocks: BTreeMap::new(),
            effective_enabled,
            // Expose source identity, never command bodies, headers or credentials.
            source: source_summary(&entry["source"]),
            local_root,
            read_only_reason: read_only,
            install_blocked_reason: install_blocked,
        });
    }
    let mut snapshot = finish(
        rt,
        context,
        capabilities,
        markets,
        plugins,
        warnings,
        version,
        evidence,
    );
    project(&mut snapshot, &config);
    snapshot.fingerprint = hash(&(
        snapshot.fingerprint.clone(),
        &snapshot.context,
        Vec::<Value>::new(),
    ));
    Ok(Observation {
        snapshot,
        evidence: Evidence::Codex(NativeEvidence { user_version }),
    })
}
fn project(snapshot: &mut Snapshot, config: &Value) {
    let context = snapshot.context.clone();
    let layers = config["layers"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    for market in &mut snapshot.markets {
        market.declarations = layers
            .iter()
            .filter(|l| !l["config"]["marketplaces"][&market.name].is_null())
            .map(codex_layer_label)
            .collect();
        if !context.target.scope.is_global() {
            market.declared_in_target = layers.iter().any(|l| {
                codex_selected(l, &context) && !l["config"]["marketplaces"][&market.name].is_null()
            });
        }
    }
    if !context.target.scope.is_global() {
        for c in &mut snapshot.capabilities {
            c.blocked_reason = Some(if matches!(c.action,Action::Enable|Action::Disable|Action::AddMarket|Action::RemoveMarket) {
            "Codex 官方接口目前仅允许写用户配置，项目配置只读。请在 Codex 中处理，或切换全局管理。"
        } else { "Codex 此操作的项目目标与共享影响尚未验证，暂不可用；可切换全局管理。" }.into());
        }
        let project_layer = layers.iter().find(|l| codex_selected(l, &context));
        for l in layers.iter().filter(|l| l["disabledReason"].is_string()) {
            snapshot.warnings.push(format!(
                "{} 未生效：{}",
                codex_layer_label(l),
                l["disabledReason"].as_str().unwrap()
            ));
        }
        for p in &mut snapshot.plugins {
            p.installed_in_target = false;
            p.selected_enabled =
                project_layer.and_then(|l| l["config"]["plugins"][&p.id]["enabled"].as_bool());
            p.effective_enabled = config["config"]["plugins"][&p.id]["enabled"].as_bool();
            let origin = &config["origins"][format!("plugins.{}.enabled", p.id)];
            p.enabled_source = origin["name"]
                .is_object()
                .then(|| codex_layer_label(origin));
            p.enabled_overridden = p.selected_enabled.is_some()
                && origin["name"].is_object()
                && !codex_selected(origin, &context);
        }
        snapshot.warnings.push(
            "项目配置按 Codex 的信任与覆盖规则读取；现有会话实际加载状态需在 Codex 中核对。".into(),
        );
    }
}
fn codex_selected(layer: &Value, context: &ResolvedContext) -> bool {
    layer["name"]["type"] == "project"
        && layer["name"]["dotCodexFolder"]
            .as_str()
            .zip(context.project_root.as_deref())
            .is_some_and(|(a, b)| same_path(Path::new(a), &Path::new(b).join(".codex")))
}
fn codex_layer_label(layer: &Value) -> String {
    match layer["name"]["type"].as_str() {
        Some("user") => "全局".into(),
        Some("project") => format!(
            "项目共享 · {}",
            layer["name"]["dotCodexFolder"]
                .as_str()
                .unwrap_or("路径未知")
        ),
        Some(kind) => format!("宿主配置 · {kind}"),
        None => "来源未知".into(),
    }
}
pub(super) fn install_restriction(entry: &Value) -> Option<String> {
    if entry["source"]["source"] == "command"
        || !entry["source"]["headersHelper"].is_null()
        || !entry["headersHelper"].is_null()
    {
        return Some("此来源要求执行安装或认证命令，请在宿主中批准并安装。".into());
    }
    if entry
        .get("dependencies")
        .is_some_and(|d| d.as_array().is_none_or(|a| !a.is_empty()))
    {
        return Some("包含依赖安装，影响范围尚未验证，请在宿主安装。".into());
    }
    if !entry["policy"].is_null()
        && !matches!(
            entry["policy"]["installation"].as_str(),
            Some("AVAILABLE" | "INSTALLED_BY_DEFAULT")
        )
    {
        return Some("市场策略不允许安装，或安装策略尚未验证，请在 Codex 中处理。".into());
    }
    let path = entry["source"].as_str().or_else(|| {
        (entry["source"]["source"] == "local")
            .then(|| entry["source"]["path"].as_str())
            .flatten()
    });
    if path.is_none_or(|p| {
        !p.starts_with("./")
            || Path::new(p)
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
    }) {
        return Some("此插件来源的安装契约尚未验证；请在宿主安装。".into());
    }
    None
}

pub(super) fn check(
    r: &Request,
    snapshot: &Snapshot,
    evidence: &NativeEvidence,
) -> Result<(), String> {
    check_context(r, snapshot)?;
    if r.action == Action::AddMarket {
        validate_source(r, true, false)?;
        if !snapshot.source_kinds.contains(&r.source_kind) {
            return Err("当前宿主不支持此市场来源格式。".into());
        }
        return Ok(());
    }
    check_target(r, snapshot, true)?;
    if matches!(r.action, Action::Enable | Action::Disable) && evidence.user_version.is_none() {
        return Err("缺少用户配置版本，无法安全写入。".into());
    }
    if !ACTIONS.contains(&r.action) {
        return Err("宿主不支持此动作。".into());
    }
    Ok(())
}
pub(super) fn arguments(r: &Request) -> Vec<String> {
    if matches!(r.action, Action::Enable | Action::Disable) {
        return vec![
            "config/value/write".into(),
            format!(
                "plugins.{}.enabled",
                serde_json::to_string(&r.target).unwrap()
            ),
            (r.action == Action::Enable).to_string(),
        ];
    }
    let mut args = vec!["plugin".into()];
    if r.action == Action::AddMarket {
        args.extend(["marketplace".into(), "add".into()]);
        let source = r.source.trim().to_string();
        args.push(source);
        if !r.reference.is_empty() {
            args.extend(["--ref".into(), r.reference.clone()]);
        }
        args.push("--json".into());
        for path in &r.sparse_paths {
            args.extend(["--sparse".into(), path.clone()]);
        }
        return args;
    }
    if matches!(r.action, Action::RemoveMarket | Action::SyncMarket) {
        args.extend([
            "marketplace".into(),
            if r.action == Action::RemoveMarket {
                "remove"
            } else {
                "upgrade"
            }
            .into(),
            r.target.clone(),
        ]);
    } else {
        let command = match r.action {
            Action::Install | Action::Reinstall => "add",
            Action::Uninstall => "remove",
            _ => unreachable!("validated host action"),
        };
        args.extend([command.into(), r.target.clone()]);
    }
    args.push("--json".into());
    args
}
pub(super) fn confirmation_fingerprint(
    request: &Request,
    snapshot: &Snapshot,
) -> Result<String, String> {
    if request.action != Action::AddMarket || request.source_kind != "local" {
        return Ok(snapshot.fingerprint.clone());
    }
    let root = std::fs::canonicalize(request.source.trim()).map_err(|e| e.to_string())?;
    let path = if root.is_file() {
        root.clone()
    } else if root.join(".agents/plugins/marketplace.json").exists() {
        root.join(".agents/plugins/marketplace.json")
    } else {
        root.join(".claude-plugin/marketplace.json")
    };
    let doc = optional_json(&path)?;
    Ok(hash(&json!([snapshot.fingerprint, root, doc])))
}
pub(super) fn consequences(r: &Request, snapshot: &Snapshot) -> (Vec<String>, Vec<String>) {
    let (mut lines, affected) = super::shared::consequences(r, snapshot);
    match r.action {
        Action::Reinstall=>lines.push("Codex 会重新启用此插件，包括原先禁用的插件；重新安装不保证取得最新版。".into()),
        Action::RemoveMarket=>lines.push("Codex 会移除市场配置和 Git 市场快照；插件配置与缓存可能保留，后续安装状态需核对。".into()),
        Action::Uninstall=>lines.push("交由宿主卸载插件及其默认清理范围，可能删除插件数据、选项和密钥；Skills Box 不追加删除来源或凭据，不提供回滚。".into()),
        Action::AddMarket|Action::Install=>lines.push("使用宿主的来源解析与本机凭据；此确认不保证远程内容不可变。需要命令批准、认证或信任时请在宿主处理。".into()),
        _=>(),
    }
    (lines, affected)
}
pub(super) fn execute(
    rt: &Runtime,
    r: &Request,
    evidence: &NativeEvidence,
) -> Result<Output, String> {
    let args = arguments(r);
    if matches!(r.action, Action::Enable | Action::Disable) {
        let v=rt.rpc("config/value/write",json!({"keyPath":args[1],"value":r.action==Action::Enable,"mergeStrategy":"replace","filePath":rt.root.join("config.toml"),"expectedVersion":evidence.user_version}))?;
        Ok(Output {
            success: v["status"] == "ok" || v["status"] == "okOverridden",
            diagnostic: transport::diagnostic(&v.to_string()),
            value: Some(v),
        })
    } else {
        rt.run(&args, true)
    }
}
pub(super) fn response_matches(r: &Request, out: &Output) -> bool {
    let Some(v) = &out.value else { return false };
    if !out.success {
        return false;
    }
    {
        match r.action {
            Action::AddMarket => {
                v["marketplaceName"].as_str().is_some_and(valid_name)
                    && v["installedRoot"].is_string()
                    && v["alreadyAdded"].is_boolean()
            }
            Action::SyncMarket => {
                v["selectedMarketplaces"] == json!([r.target])
                    && v["upgradedRoots"].as_array().is_some_and(|a| a.len() == 1)
                    && v["errors"] == json!([])
            }
            Action::RemoveMarket => {
                v["marketplaceName"] == r.target && v.get("installedRoot").is_some()
            }
            Action::Enable | Action::Disable => {
                matches!(v["status"].as_str(), Some("ok" | "okOverridden"))
                    && v["version"].is_string()
                    && v["filePath"].is_string()
            }
            Action::Install | Action::Reinstall => {
                v["pluginId"] == r.target && v["installedPath"].is_string()
            }
            Action::Uninstall => v["pluginId"] == r.target,
            _ => false,
        }
    }
}
pub(super) fn postcondition(
    r: &Request,
    before: &Snapshot,
    after: &Snapshot,
    out: &Output,
) -> bool {
    if before.context != after.context || !preserves_installations(r, before, after) {
        return false;
    }
    let next = after;
    let plugin = next.plugins.iter().find(|p| p.id == r.target);
    let value = out.value.as_ref().unwrap_or(&Value::Null);
    match r.action {
        Action::AddMarket => {
            let name = value["marketplaceName"].as_str();
            next.markets
                .iter()
                .any(|m| Some(m.name.as_str()) == name && m.declared_in_target)
        }
        Action::RemoveMarket => !next
            .markets
            .iter()
            .any(|m| m.name == r.target && m.declared_in_target),
        Action::SyncMarket => {
            next.markets
                .iter()
                .any(|m| m.name == r.target && m.catalog_error.is_none())
                && before
                    .plugins
                    .iter()
                    .filter(|p| {
                        p.marketplace == r.target
                            && (p.installation_state != "absent"
                                || !p.installations.is_empty()
                                || p.user_enabled.is_some())
                    })
                    .all(|p| {
                        next.plugins.iter().find(|n| n.id == p.id).is_some_and(|n| {
                            n.user_enabled == p.user_enabled
                                && n.selected_enabled == p.selected_enabled
                        })
                    })
        }
        Action::Install | Action::Reinstall => plugin.is_some_and(|p| {
            p.installed_in_target
                && p.selected_enabled == Some(true)
                && p.local_root.as_ref().is_some_and(|p| Path::new(p).is_dir())
        }),
        Action::Enable => plugin.is_some_and(|p| p.selected_enabled == Some(true)),
        Action::Disable => plugin.is_some_and(|p| p.selected_enabled == Some(false)),
        Action::Uninstall => {
            plugin.is_none_or(|p| !p.installed_in_target && p.selected_enabled.is_none())
        }
        Action::Update => plugin.is_some_and(|p| {
            p.installed_in_target
                && p.version.as_deref() == value["newVersion"].as_str()
                && before
                    .plugins
                    .iter()
                    .find(|b| b.id == p.id)
                    .is_some_and(|b| b.selected_enabled == p.selected_enabled)
        }),
    }
}
const ACTIONS: &[Action] = &[
    Action::AddMarket,
    Action::SyncMarket,
    Action::RemoveMarket,
    Action::Install,
    Action::Enable,
    Action::Disable,
    Action::Uninstall,
    Action::Reinstall,
];
pub(super) fn defaults() -> Contract {
    Contract {
        capabilities: ACTIONS
            .iter()
            .copied()
            .map(|action| Capability {
                action,
                blocked_reason: None,
            })
            .collect(),
        sources: vec!["local".into(), "git".into()],
    }
}
pub(super) fn probe_sequence() -> Vec<Action> {
    let mut sequence = vec![
        Action::AddMarket,
        Action::Install,
        Action::Disable,
        Action::Reinstall,
        Action::Enable,
        Action::Uninstall,
        Action::RemoveMarket,
    ];
    // Reinstall enables again, so verify disabling once more before enable.
    sequence.insert(4, Action::Disable);
    sequence
}

pub(super) fn validate_context(context: &ResolvedContext) -> Result<(), String> {
    if context.target.layer == Layer::Local {
        Err("Codex 没有此项目个人配置层。".into())
    } else {
        Ok(())
    }
}
