use super::{
    model::*,
    transport::{Runtime, same_path},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub struct Observation {
    pub snapshot: Snapshot,
    pub user_version: Option<String>,
    pub layers: Vec<super::context::ConfigLayer>,
}
pub fn optional_json(path: &Path) -> Result<Value, String> {
    match fs::read(path) {
        Ok(bytes) if bytes.len() <= 8 * 1024 * 1024 => serde_json::from_slice(&bytes)
            .map_err(|_| format!("无法解析 {}，相关操作暂不可用。", path.display())),
        Ok(_) => Err(format!("{} 过大，无法可靠核对。", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(format!("无法读取 {}：{e}", path.display())),
    }
}
pub fn hash(value: &impl serde::Serialize) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).unwrap_or_default())
    )
}
pub fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() < 200
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && s != "."
        && s != ".."
        && !s.starts_with('-')
}
pub fn identity(s: &str) -> Result<(&str, &str), String> {
    let (name, market) = s.split_once('@').ok_or("未知插件身份")?;
    if !valid_name(name) || !valid_name(market) {
        return Err("未知插件身份".into());
    }
    Ok((name, market))
}
fn text(v: &Value, k: &str) -> Option<String> {
    v[k].as_str().map(str::to_string)
}
fn array(v: &Value) -> Result<&Vec<Value>, String> {
    v.as_array().ok_or("原生清单格式不兼容。".into())
}
fn obj(v: &Value) -> Result<&serde_json::Map<String, Value>, String> {
    v.as_object().ok_or("原生配置格式不兼容。".into())
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
pub fn load(
    rt: &Runtime,
    capabilities: Vec<Capability>,
    projects: &[PathBuf],
) -> Result<Observation, String> {
    load_context(rt, capabilities, projects, ResolvedContext::default())
}
pub fn load_context(
    rt: &Runtime,
    mut capabilities: Vec<Capability>,
    projects: &[PathBuf],
    context: ResolvedContext,
) -> Result<Observation, String> {
    let version = rt.version()?;
    let raw_markets = rt.json(&["plugin", "marketplace", "list", "--json"])?;
    let listed = rt.json(&["plugin", "list", "--json"]);
    let inventory_error = listed.as_ref().err().cloned();
    let raw_plugins = listed.unwrap_or_else(|_| {
        if rt.host == Host::Codex {
            json!({"installed":[]})
        } else {
            json!([])
        }
    });
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
    let (config, user, user_version, records, known) = if rt.host == Host::Codex {
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
        (config, user, user_version, records, json!({}))
    } else {
        let user = optional_json(&rt.root.join("settings.json"))?;
        for key in ["extraKnownMarketplaces", "enabledPlugins"] {
            if !user[key].is_null() {
                obj(&user[key])?;
            }
        }
        let known = optional_json(&rt.root.join("plugins/known_marketplaces.json"))?;
        obj(&known)?;
        let installed = optional_json(&rt.root.join("plugins/installed_plugins.json"))?;
        let mut records = vec![];
        if installed != json!({}) {
            if installed["version"] != 2 {
                return Err("Claude 安装记录版本未知，无法确定跨作用域影响。".into());
            }
            for (id, rows) in obj(&installed["plugins"])? {
                identity(id)?;
                for row in array(rows)? {
                    let mut row = row.clone();
                    if !row.is_object()
                        || !matches!(
                            row["scope"].as_str(),
                            Some("user" | "project" | "local" | "managed")
                        )
                    {
                        return Err("Claude 安装作用域未知。".into());
                    }
                    row["id"] = json!(id);
                    records.push(row);
                }
            }
        }
        let listed = array(&raw_plugins)?;
        // Session plugins are transient CLI loads, not persisted installations.
        // Every other scope must have an authoritative installation record.
        for row in listed.iter().filter(|row| row["scope"] != "session") {
            if !records
                .iter()
                .any(|r| r["id"] == row["id"] && r["scope"] == row["scope"])
            {
                return Err("CLI 与原生安装记录不一致，请重新读取。".into());
            }
        }
        evidence.extend([user.clone(), known.clone(), installed]);
        (json!({}), user, None, records, known)
    };
    let market_rows = if rt.host == Host::Codex {
        array(&raw_markets["marketplaces"])?
    } else {
        array(&raw_markets)?
    };
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
        let (source_kind, source, root, owned, reference, sparse) = if rt.host == Host::Codex {
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
        } else {
            let s = &known[&name]["source"];
            let kind = text(row, "source").unwrap_or_default();
            let field = match kind.as_str() {
                "github" => "repo",
                "git" | "url" => "url",
                "directory" | "file" => "path",
                _ => "unknown",
            };
            let owned = user["extraKnownMarketplaces"][&name]["source"].is_object()
                && user["extraKnownMarketplaces"][&name]["source"] == *s
                && s["source"] == kind
                && row[field].is_string()
                && row[field] == s[field]
                && row["ref"] == s["ref"];
            (
                kind,
                text(row, field).unwrap_or_default(),
                text(row, "installLocation"),
                owned,
                text(row, "ref"),
                s["sparsePaths"].clone(),
            )
        };
        let mut affected = vec![];
        let mut non_user = false;
        for r in &records {
            let id = r[if rt.host == Host::Codex {
                "pluginId"
            } else {
                "id"
            }]
            .as_str()
            .ok_or("插件身份缺失。")?;
            if identity(id)?.1 == name {
                affected.push(id.to_string());
                non_user |= rt.host == Host::Claude && r["scope"] != "user";
            }
        }
        let mut project_uncertain = false;
        if rt.host == Host::Codex {
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
            source: super::transport::redact_urls(&source),
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
            remove_blocked_reason: if non_user {
                Some("存在项目或其他作用域安装；原生移除会连带卸载，已阻止。".into())
            } else if project_uncertain {
                Some("可观测项目含 Codex 配置，无法确定移除市场的全部影响，请在宿主中处理。".into())
            } else {
                None
            },
            sync_blocked_reason: if rt.host == Host::Codex && source_kind != "git" {
                Some("Codex 仅支持同步 Git 市场；本地市场请重新读取。".into())
            } else if rt.host == Host::Claude && !known[&name]["source"]["headersHelper"].is_null()
            {
                Some("市场需要认证命令，请在宿主中批准并同步。".into())
            } else {
                None
            },
            catalog_error: None,
            affected_plugins: affected,
            declarations: vec![],
            declared_in_target: owned,
        };
        let catalog = (|| {
            let root = PathBuf::from(root.ok_or("原生工具未提供本地目录，目录内容不可用。")?);
            if !root.is_absolute() {
                return Err("市场目录不是绝对路径。".into());
            }
            let paths = if root.is_file() {
                vec![root.clone()]
            } else if rt.host == Host::Codex {
                // Curated API and ChatGPT markets can share one checkout.
                // Select the document with the identity reported by the host.
                vec![
                    root.join(".agents/plugins/marketplace.json"),
                    root.join(".agents/plugins/api_marketplace.json"),
                    root.join(".claude-plugin/marketplace.json"),
                ]
            } else {
                vec![root.join(".claude-plugin/marketplace.json")]
            };
            let mut catalog = None;
            let mut read_error = None;
            for path in paths {
                match optional_json(&path) {
                    Ok(doc) => {
                        evidence.push(json!({"path":path,"content":doc}));
                        if doc["name"] == name {
                            catalog = Some(doc);
                            break;
                        }
                    }
                    Err(error) => {
                        read_error.get_or_insert(error);
                    }
                }
            }
            let doc = catalog.ok_or_else(|| {
                read_error
                    .unwrap_or_else(|| "本地目录缺失或市场名称不匹配；搜索结果不完整。".into())
            })?;
            let content_root = if root.is_file() {
                root.parent()
                    .and_then(Path::parent)
                    .ok_or("市场 JSON 文件缺少来源根目录。")?
            } else {
                &root
            };
            for entry in array(&doc["plugins"])? {
                let plugin_name = entry["name"]
                    .as_str()
                    .filter(|s| valid_name(s))
                    .ok_or("插件目录身份不兼容。")?;
                let id = format!("{plugin_name}@{name}");
                if catalogs.contains_key(&id) {
                    return Err("目录包含重复插件身份。".into());
                }
                let local = entry["source"]
                    .as_str()
                    .or_else(|| {
                        (entry["source"]["source"] == "local")
                            .then(|| entry["source"]["path"].as_str())
                            .flatten()
                    })
                    .and_then(|source| local_path(content_root, source).ok())
                    .filter(|p| p.is_dir());
                catalogs.insert(id, (entry.clone(), local));
            }
            Ok::<(), String>(())
        })();
        market.catalog_error = catalog.err();
        markets.push(market);
    }
    let mut ids: BTreeMap<String, ()> = catalogs.keys().map(|id| (id.clone(), ())).collect();
    for r in &records {
        let id = r[if rt.host == Host::Codex {
            "pluginId"
        } else {
            "id"
        }]
        .as_str()
        .ok_or("原生插件身份缺失。")?;
        identity(id)?;
        ids.insert(id.into(), ());
    }
    if rt.host == Host::Codex {
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
        let relevant: Vec<_> = records
            .iter()
            .filter(|r| {
                r[if rt.host == Host::Codex {
                    "pluginId"
                } else {
                    "id"
                }] == *id
            })
            .collect();
        let installed = relevant
            .iter()
            .find(|r| rt.host == Host::Codex || r["scope"] == "user");
        let unconfirmed = installed.is_none()
            && (inventory_error.is_some()
                || rt.host == Host::Codex && !config["config"]["plugins"][id].is_null());
        let (entry, local) = catalogs.get(id).cloned().unwrap_or_default();
        let source_missing = local.is_none();
        let version = installed.and_then(|r| text(r, "version"));
        let user_enabled = if rt.host == Host::Codex {
            user["plugins"][id]["enabled"].as_bool()
        } else {
            user["enabledPlugins"][id].as_bool()
        };
        let effective_enabled = if rt.host == Host::Codex {
            installed.and_then(|r| r["enabled"].as_bool())
        } else {
            raw_plugins
                .as_array()
                .and_then(|rs| rs.iter().find(|r| r["id"] == *id && r["scope"] == "user"))
                .and_then(|r| r["enabled"].as_bool())
        };
        let local_root = installed
            .and_then(|r| {
                text(r, "readFromFolder")
                    .or_else(|| text(r, "installedPath"))
                    .or_else(|| text(r, "installPath"))
            })
            .map(PathBuf::from)
            .or_else(|| {
                if rt.host == Host::Codex {
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
                } else {
                    None
                }
            })
            .or_else(|| local.clone())
            .map(|p| p.to_string_lossy().into_owned());
        let read_only = if rt.host == Host::Codex
            && !config["config"]["plugins"][id].is_null()
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
        } else if source_missing && install_restriction(rt.host, &entry).is_none() {
            Some("市场内插件路径不存在或超出来源目录，无法确认安装内容。".into())
        } else {
            install_restriction(rt.host, &entry)
        };
        let installations = relevant
            .iter()
            .map(|r| Installation {
                scope: if rt.host == Host::Codex {
                    "user".into()
                } else {
                    text(r, "scope").unwrap_or_default()
                },
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
    let executable = fs::canonicalize(&rt.executable).unwrap_or(rt.executable.clone());
    let executable_digest = fs::metadata(&executable)
        .map(|m| {
            json!([
                m.len(),
                m.modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos().to_string())
            ])
        })
        .unwrap_or(Value::Null);
    let root = fs::canonicalize(&rt.root)
        .unwrap_or(rt.root.clone())
        .to_string_lossy()
        .into_owned();
    let fingerprint = hash(&json!([
        root,
        executable,
        executable_digest,
        version,
        evidence
    ]));
    let mut observation = Observation {
        snapshot: Snapshot {
            host: rt.host,
            context,
            config_root: root,
            executable: executable.to_string_lossy().into_owned(),
            version,
            source_kinds: vec![],
            capabilities,
            markets,
            plugins,
            warnings,
            fingerprint,
        },
        user_version,
        layers: vec![],
    };
    super::context::project(rt, &mut observation, projects, &config, &raw_plugins)?;
    Ok(observation)
}
fn source_summary(source: &Value) -> Value {
    if let Some(source) = source.as_str() {
        return json!(super::transport::redact_urls(source));
    }
    let mut summary = serde_json::Map::new();
    for key in ["source", "path", "url", "repo", "ref"] {
        if let Some(v) = source[key].as_str() {
            summary.insert(key.into(), json!(super::transport::redact_urls(v)));
        }
    }
    Value::Object(summary)
}
pub(super) fn install_restriction(host: Host, entry: &Value) -> Option<String> {
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
    if host == Host::Codex
        && !entry["policy"].is_null()
        && !matches!(
            entry["policy"]["installation"].as_str(),
            Some("AVAILABLE" | "INSTALLED_BY_DEFAULT")
        )
    {
        return Some("市场策略不允许安装，或安装策略尚未验证，请在 Codex 中处理。".into());
    }
    let path = entry["source"].as_str().or_else(|| {
        (host == Host::Codex && entry["source"]["source"] == "local")
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
pub fn local_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let p = Path::new(relative);
    if p.is_absolute()
        || p.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir | std::path::Component::Prefix(_)
            )
        })
    {
        return Err("本地内容路径超出插件目录。".into());
    }
    let root = fs::canonicalize(root).map_err(|_| "本地内容尚未下载。")?;
    let path = fs::canonicalize(root.join(p)).map_err(|_| "本地内容不存在或不可读。")?;
    if !path.starts_with(&root) {
        return Err("本地内容符号链接指向目录外，已跳过。".into());
    }
    Ok(path)
}
pub fn details(plugin: &Plugin) -> Details {
    let mut d = Details::default();
    let Some(root) = plugin.local_root.as_ref().map(PathBuf::from) else {
        d.messages
            .push("没有可读取的本地内容；不会为预览下载插件。".into());
        return d;
    };
    for file in [".codex-plugin/plugin.json", ".claude-plugin/plugin.json"] {
        if let Ok(path) = local_path(&root, file) {
            match optional_json(&path) {
                Ok(v) => {
                    d.manifest = v;
                    break;
                }
                Err(e) => d.messages.push(e),
            }
        }
    }
    // Only metadata is returned, never hook commands, MCP credentials or arbitrary paths.
    for (kind, dir) in [
        ("Skills", "skills"),
        ("Agents", "agents"),
        ("Commands", "commands"),
    ] {
        let mut names = vec![];
        if let Ok(dir) = local_path(&root, dir) {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.take(501).flatten() {
                    if names.len() >= 500 {
                        d.messages.push("组件过多，只展示前 500 项。".into());
                        break;
                    }
                    if entry.file_type().is_ok_and(|t| !t.is_symlink()) {
                        names.push(entry.file_name().to_string_lossy().into_owned());
                    }
                }
            }
        }
        if let Some(value) = d.manifest.get(dir) {
            if let Some(fields) = value.as_object() {
                names.extend(fields.keys().cloned());
            } else if !value.is_null() {
                names.push("清单声明了额外组件路径（未执行）".into());
            }
        }
        names.sort();
        names.dedup();
        d.components.insert(kind.into(), names);
    }
    for (kind, file, key) in [
        ("Hooks", "hooks/hooks.json", "hooks"),
        ("MCP", ".mcp.json", "mcpServers"),
    ] {
        let mut names = vec![];
        if let Ok(path) = local_path(&root, file) {
            if let Ok(v) = optional_json(&path) {
                if let Some(o) = v[key].as_object().or_else(|| {
                    if key == "mcpServers" {
                        v.as_object()
                    } else {
                        None
                    }
                }) {
                    names.extend(o.keys().cloned());
                }
            }
        }
        let field = if kind == "Hooks" {
            "hooks"
        } else {
            "mcpServers"
        };
        if let Some(o) = d.manifest[field].as_object() {
            names.extend(o.keys().cloned());
        }
        if d.manifest[field].is_string() || d.manifest[field].is_array() {
            names.push("清单声明了额外组件路径（未执行）".into());
        }
        names.sort();
        names.dedup();
        d.components.insert(kind.into(), names);
    }
    d.manifest = json!({"name":d.manifest["name"],"version":d.manifest["version"],"description":d.manifest["description"],"author":d.manifest["author"]["name"]});
    for file in ["README.md", "readme.md", "README.MD"] {
        if let Ok(path) = local_path(&root, file) {
            if fs::metadata(&path).is_ok_and(|m| m.len() <= 128 * 1024) {
                d.readme = fs::read_to_string(path).ok();
            } else {
                d.messages.push("说明文件超过 128 KB，未加载。".into());
            }
            break;
        }
    }
    if d.readme.is_none() {
        d.messages.push("本地没有可读取的 README。".into());
    }
    d
}
