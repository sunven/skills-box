use super::super::{
    contracts::Contract,
    inventory::{hash, identity, optional_json, valid_name},
    model::*,
    transport::{self, Output, Runtime, same_path},
};
use super::{Evidence, Observation, shared::*};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
pub(super) struct NativeEvidence {
    pub(super) layers: Vec<ConfigLayer>,
}
#[derive(Clone, Serialize)]
pub(super) struct ConfigLayer {
    pub(super) scope: String,
    pub(super) root: Option<PathBuf>,
    pub(super) path: PathBuf,
    pub(super) settings: Value,
}
impl ConfigLayer {
    fn selected(&self, context: &ResolvedContext) -> bool {
        self.scope == context.target.layer.native()
            && match (&self.root, &context.project_root) {
                (None, None) => true,
                (Some(a), Some(b)) => same_path(a, Path::new(b)),
                _ => false,
            }
    }
    fn label(&self) -> String {
        let scope = match self.scope.as_str() {
            "user" => "全局",
            "project" => "项目共享",
            "local" => "项目个人",
            _ => "组织策略",
        };
        self.root
            .as_ref()
            .map(|r| format!("{scope} · {}", r.display()))
            .unwrap_or_else(|| scope.into())
    }
    fn applies(&self, context: &ResolvedContext) -> bool {
        self.root.as_ref().is_none_or(|r| {
            context
                .project_root
                .as_ref()
                .is_some_and(|p| Path::new(p).starts_with(r))
        })
    }
}
fn read_layer(scope: &str, root: Option<PathBuf>, path: PathBuf) -> Result<ConfigLayer, String> {
    let settings = optional_json(&path)?;
    if !settings.is_object() {
        return Err(format!("原生配置格式未知：{}", path.display()));
    }
    for field in ["enabledPlugins", "extraKnownMarketplaces"] {
        if !settings[field].is_null() && !settings[field].is_object() {
            return Err(format!("原生配置 {field} 格式未知：{}", path.display()));
        }
    }
    Ok(ConfigLayer {
        scope: scope.into(),
        root,
        path,
        settings,
    })
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
    let raw_plugins = listed.unwrap_or_else(|_| json!([]));
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
    let (user, records, known) = {
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
        (user, records, known)
    };
    let market_rows = array(&raw_markets)?;
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
            let id = r["id"].as_str().ok_or("插件身份缺失。")?;
            if identity(id)?.1 == name {
                affected.push(id.to_string());
                non_user |= r["scope"] != "user";
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
            remove_blocked_reason: if non_user {
                Some("存在项目或其他作用域安装；原生移除会连带卸载，已阻止。".into())
            } else {
                None
            },
            sync_blocked_reason: (!known[&name]["source"]["headersHelper"].is_null())
                .then(|| "市场需要认证命令，请在宿主中批准并同步。".into()),
            catalog_error: None,
            affected_plugins: affected,
            declarations: vec![],
            declared_in_target: owned,
        };
        market.catalog_error = read_catalog(
            root,
            &name,
            &[".claude-plugin/marketplace.json"],
            &mut catalogs,
            &mut evidence,
        )
        .err();
        markets.push(market);
    }
    let mut ids: BTreeMap<String, ()> = catalogs.keys().map(|id| (id.clone(), ())).collect();
    for r in &records {
        let id = r["id"].as_str().ok_or("原生插件身份缺失。")?;
        identity(id)?;
        ids.insert(id.into(), ());
    }

    let mut plugins = vec![];
    for id in ids.keys() {
        let (name, market) = identity(id)?;
        let market_row = markets.iter().find(|m| m.name == market);
        let relevant: Vec<_> = records.iter().filter(|r| r["id"] == *id).collect();
        let installed = relevant.iter().find(|r| r["scope"] == "user");
        let unconfirmed = installed.is_none() && inventory_error.is_some();
        let (entry, local) = catalogs.get(id).cloned().unwrap_or_default();
        let source_missing = local.is_none();
        let version = installed.and_then(|r| text(r, "version"));
        let user_enabled = user["enabledPlugins"][id].as_bool();
        let effective_enabled = {
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
            .or_else(|| local.clone())
            .map(|p| p.to_string_lossy().into_owned());
        let read_only = market_row.and_then(|m| m.read_only_reason.clone());
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
                scope: { text(r, "scope").unwrap_or_default() },
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
    let layers = project(rt, &mut snapshot, projects, &raw_plugins)?;
    snapshot.fingerprint = hash(&(snapshot.fingerprint.clone(), &snapshot.context, &layers));
    Ok(Observation {
        snapshot,
        evidence: Evidence::Claude(NativeEvidence { layers }),
    })
}
fn project(
    rt: &Runtime,
    snapshot: &mut Snapshot,
    projects: &[PathBuf],
    raw: &Value,
) -> Result<Vec<ConfigLayer>, String> {
    let context = snapshot.context.clone();
    let known = optional_json(&rt.root.join("plugins/known_marketplaces.json"))?;
    let mut roots: BTreeSet<PathBuf> = projects.iter().cloned().collect();
    if let Some(root) = &context.project_root {
        roots.insert(root.into());
    }
    for p in &snapshot.plugins {
        for i in &p.installations {
            if matches!(i.scope.as_str(), "project" | "local") {
                let root = i
                    .project_path
                    .as_ref()
                    .filter(|p| Path::new(p).is_absolute())
                    .ok_or("原生项目安装缺少绝对路径，无法核对作用域。")?;
                roots.insert(root.into());
            }
        }
    }
    // Only observe known roots and the active root's ancestors, never scan disk.
    if let Some(root) = &context.project_root {
        for parent in Path::new(root).ancestors().skip(1) {
            if parent == Path::new("/") || parent == rt.home {
                break;
            }
            if parent.join(".claude/settings.json").exists()
                || parent.join(".claude/settings.local.json").exists()
            {
                roots.insert(parent.into());
            }
        }
    }
    let mut roots: Vec<_> = roots.into_iter().collect();
    roots.sort_by_key(|p| p.components().count());
    let mut layers = vec![read_layer("user", None, rt.root.join("settings.json"))?];
    for root in roots {
        for (scope, file) in [
            ("project", "settings.json"),
            ("local", "settings.local.json"),
        ] {
            layers.push(read_layer(
                scope,
                Some(root.clone()),
                root.join(".claude").join(file),
            )?);
        }
    }
    layers.push(read_layer(
        "managed",
        None,
        rt.root.join("managed-settings.json"),
    )?);
    if !rt.isolated {
        layers.push(read_layer(
            "managed",
            None,
            PathBuf::from("/Library/Application Support/ClaudeCode/managed-settings.json"),
        )?);
    }
    // Preserve declarations without a cache and enabled settings without an
    // installation. Missing native content must not look like empty state.
    for layer in &layers {
        if let Some(entries) = layer.settings["enabledPlugins"].as_object() {
            for id in entries.keys() {
                let (name, market) = identity(id)?;
                if !snapshot.plugins.iter().any(|p| p.id == *id) {
                    snapshot.plugins.push(Plugin {
                        id: id.clone(),
                        name: name.into(),
                        marketplace: market.into(),
                        description: None,
                        version: None,
                        installation_state: "unconfirmed".into(),
                        installations: vec![],
                        user_enabled: layers[0].settings["enabledPlugins"][id].as_bool(),
                        selected_enabled: None,
                        installed_in_target: false,
                        enabled_source: None,
                        enabled_overridden: false,
                        action_blocks: Default::default(),
                        effective_enabled: None,
                        source: Value::Null,
                        local_root: None,
                        read_only_reason: None,
                        install_blocked_reason: Some("市场或安装内容缺失，请在宿主核对。".into()),
                    });
                }
            }
        }
        if let Some(entries) = layer.settings["extraKnownMarketplaces"].as_object() {
            for (name, declaration) in entries {
                if !valid_name(name) {
                    return Err("市场声明身份未知。".into());
                }
                if !snapshot.markets.iter().any(|m| m.name == *name) {
                    let source = &declaration["source"];
                    snapshot.markets.push(Market {
                        name: name.clone(),
                        source_kind: source["source"].as_str().unwrap_or("unknown").into(),
                        source: transport::redact_urls(
                            source["repo"]
                                .as_str()
                                .or(source["url"].as_str())
                                .or(source["path"].as_str())
                                .unwrap_or("来源未知"),
                        ),
                        root: None,
                        reference: None,
                        sparse_paths: vec![],
                        read_only_reason: None,
                        remove_blocked_reason: Some(
                            "市场仅有配置声明，原生缓存与删除影响无法确认，请在宿主处理。".into(),
                        ),
                        sync_blocked_reason: None,
                        catalog_error: Some("市场尚无可读取的原生目录。".into()),
                        affected_plugins: vec![],
                        declarations: vec![],
                        declared_in_target: false,
                    });
                }
            }
        }
    }
    for m in &mut snapshot.markets {
        m.declarations = layers
            .iter()
            .filter(|l| !l.settings["extraKnownMarketplaces"][&m.name].is_null())
            .map(ConfigLayer::label)
            .collect();
        m.declared_in_target = layers.iter().any(|l| {
            l.selected(&context)
                && l.settings["extraKnownMarketplaces"][&m.name]["source"].is_object()
                && l.settings["extraKnownMarketplaces"][&m.name]["source"]
                    == known[&m.name]["source"]
        });
        m.read_only_reason = (!m.declared_in_target).then(|| {
            "此市场继承自其他配置层，或仅存在于共享缓存；当前层没有匹配的市场声明。".into()
        });
        let foreign_install = snapshot
            .plugins
            .iter()
            .filter(|p| p.marketplace == m.name)
            .any(|p| {
                p.installations
                    .iter()
                    .any(|i| !context.matches_installation(i))
            });
        // Native last-declaration detection is not independently enumerable
        // across all projects. Never approve a cascade into another layer.
        if foreign_install {
            m.remove_blocked_reason =
                Some("移除市场可能删除其他项目或配置层的安装，已阻止；请在宿主处理。".into());
        } else if !known[&m.name].is_null() {
            m.remove_blocked_reason = None;
        }
        m.affected_plugins = snapshot
            .plugins
            .iter()
            .filter(|p| p.marketplace == m.name)
            .flat_map(|p| installation_labels(p))
            .collect();
        for layer in &layers {
            if let Some(entries) = layer.settings["enabledPlugins"].as_object() {
                for id in entries
                    .keys()
                    .filter(|id| identity(id).is_ok_and(|(_, market)| market == m.name))
                {
                    m.affected_plugins
                        .push(format!("{id} · 配置 · {}", layer.label()));
                }
            }
        }
        m.affected_plugins.sort();
        m.affected_plugins.dedup();
    }
    for p in &mut snapshot.plugins {
        let selected = p
            .installations
            .iter()
            .find(|i| context.matches_installation(i));
        let visible = selected.or_else(|| {
            p.installations.iter().rev().find(|i| {
                i.scope == "user"
                    || i.scope == "managed"
                    || context
                        .project_root
                        .as_ref()
                        .zip(i.project_path.as_ref())
                        .is_some_and(|(a, b)| same_path(Path::new(a), Path::new(b)))
            })
        });
        p.installed_in_target = selected.is_some();
        if let Some(i) = visible {
            p.version = i.version.clone();
            if i.path.is_some() {
                p.local_root = i.path.clone();
            }
        }
        if p.installation_state != "unconfirmed" {
            p.installation_state = if visible.is_some() {
                "installed"
            } else {
                "absent"
            }
            .into();
        }
        p.selected_enabled = layers
            .iter()
            .find(|l| l.selected(&context))
            .and_then(|l| l.settings["enabledPlugins"][&p.id].as_bool());
        let effective = layers
            .iter()
            .rev()
            .find(|l| l.applies(&context) && l.settings["enabledPlugins"][&p.id].is_boolean());
        if visible.is_none() && effective.is_some() {
            p.installation_state = "unconfirmed".into();
        }
        p.enabled_source = effective.map(ConfigLayer::label);
        p.enabled_overridden =
            p.selected_enabled.is_some() && effective.is_some_and(|l| !l.selected(&context));
        // CLI list can include unrelated projects. Only a matching native
        // row can attest the host state; configuration alone is not loading.
        p.effective_enabled = raw
            .as_array()
            .and_then(|rs| {
                rs.iter().find(|r| {
                    r["id"] == p.id
                        && visible.is_some_and(|i| r["scope"] == i.scope)
                        && (if matches!(r["scope"].as_str(), Some("project" | "local")) {
                            context
                                .project_root
                                .as_ref()
                                .zip(r["projectPath"].as_str())
                                .is_some_and(|(root, path)| {
                                    same_path(Path::new(root), Path::new(path))
                                })
                        } else {
                            r["projectPath"].is_null()
                        })
                })
            })
            .and_then(|r| r["enabled"].as_bool());
        if let Some(e) = effective {
            if p.effective_enabled != e.settings["enabledPlugins"][&p.id].as_bool() {
                p.effective_enabled = None;
            }
        }
        p.read_only_reason = layers
            .iter()
            .any(|l| l.scope == "managed" && !l.settings["enabledPlugins"][&p.id].is_null())
            .then(|| "插件启用设置由组织策略控制，请在宿主中处理。".into());
    }

    if !context.target.scope.is_global() {
        snapshot.warnings.push("项目共享配置可随仓库提交，协作者仍需本机安装。继承、显式启用和显式禁用不同；清除覆盖需在宿主配置中处理后重新读取。".into());
        if snapshot
            .plugins
            .iter()
            .flat_map(|p| &p.installations)
            .any(|i| {
                i.project_path
                    .as_ref()
                    .is_some_and(|p| !Path::new(p).is_dir())
            })
        {
            snapshot.warnings.push("原生安装记录中存在已不可用的项目路径。重新定位不会迁移这些记录，请查看插件详情并在宿主修复或重新安装。".into());
        }
    }
    Ok(layers)
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
    let path = entry["source"].as_str();
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
    _evidence: &NativeEvidence,
) -> Result<(), String> {
    check_context(r, snapshot)?;
    if r.action == Action::AddMarket {
        validate_source(r, false, true)?;
        if !snapshot.source_kinds.contains(&r.source_kind) {
            return Err("当前宿主不支持此市场来源格式。".into());
        }
        return Ok(());
    }
    check_target(r, snapshot, false)?;
    if !ACTIONS.contains(&r.action) {
        return Err("宿主不支持此动作。".into());
    }
    Ok(())
}
pub(super) fn arguments(r: &Request) -> Vec<String> {
    let mut args = vec!["plugin".into()];
    if r.action == Action::AddMarket {
        args.extend(["marketplace".into(), "add".into()]);
        let mut source = r.source.trim().to_string();
        if !r.reference.is_empty() {
            source.push('#');
            source.push_str(&r.reference);
        }
        args.push(source);
        args.extend(["--scope".into(), r.context.layer.native().into()]);
        args.push("--json".into());
        if !r.sparse_paths.is_empty() {
            args.push("--sparse".into());
            args.extend(r.sparse_paths.clone());
        }
        return args;
    }
    if matches!(r.action, Action::RemoveMarket | Action::SyncMarket) {
        args.extend([
            "marketplace".into(),
            if r.action == Action::RemoveMarket {
                "remove"
            } else {
                "update"
            }
            .into(),
            r.target.clone(),
        ]);
        if r.action == Action::RemoveMarket {
            args.extend(["--scope".into(), r.context.layer.native().into()]);
        }
    } else {
        let command = match r.action {
            Action::Install => "install",
            Action::Update => "update",
            Action::Uninstall => "uninstall",
            Action::Enable => "enable",
            Action::Disable => "disable",
            _ => unreachable!("validated host action"),
        };
        args.extend([command.into(), r.target.clone()]);
        args.extend(["--scope".into(), r.context.layer.native().into()]);
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
    } else {
        root.join(".claude-plugin/marketplace.json")
    };
    let doc = optional_json(&path)?;
    Ok(hash(&json!([snapshot.fingerprint, root, doc])))
}
pub(super) fn consequences(r: &Request, snapshot: &Snapshot) -> (Vec<String>, Vec<String>) {
    let (mut lines, affected) = super::shared::consequences(r, snapshot);
    match r.action {
        Action::RemoveMarket=>lines.push("Claude Code 会连带卸载所列插件，并尽可能删除保存的选项、密钥和插件数据。没有自动回滚。".into()),
        Action::Uninstall=>lines.push("交由宿主卸载插件及其默认清理范围，可能删除插件数据、选项和密钥；Skills Box 不追加删除来源或凭据，不提供回滚。".into()),
        Action::AddMarket|Action::Install=>lines.push("使用宿主的来源解析与本机凭据；此确认不保证远程内容不可变。需要命令批准、认证或信任时请在宿主处理。".into()),
        _=>(),
    }
    (lines, affected)
}
pub(super) fn execute(
    rt: &Runtime,
    r: &Request,
    _evidence: &NativeEvidence,
) -> Result<Output, String> {
    rt.run(&arguments(r), true)
}
pub(super) fn response_matches(r: &Request, out: &Output) -> bool {
    let Some(v) = &out.value else { return false };
    if !out.success {
        return false;
    }
    {
        let command = match r.action {
            Action::AddMarket => "marketplace-add",
            Action::SyncMarket => "marketplace-update",
            Action::RemoveMarket => "marketplace-remove",
            Action::Install => "install",
            Action::Enable => "enable",
            Action::Disable => "disable",
            Action::Update => "update",
            Action::Uninstall => "uninstall",
            _ => return false,
        };
        v["outcome"] == "ok"
            && v["command"] == command
            && match r.action {
                Action::AddMarket => v["marketplace"].as_str().is_some_and(valid_name),
                Action::SyncMarket | Action::RemoveMarket => v["marketplace"] == r.target,
                _ => {
                    v["pluginId"] == r.target
                        && v["scope"] == r.context.layer.native()
                        && (r.action != Action::Update
                            || matches!(
                                v["updateOutcome"].as_str(),
                                Some("updated" | "up_to_date")
                            ))
                }
            }
    }
}
pub(super) fn postcondition(
    r: &Request,
    before: &Snapshot,
    after: &Snapshot,
    out: &Output,
    before_evidence: &NativeEvidence,
    after_evidence: &NativeEvidence,
) -> bool {
    if before.context != after.context
        || !preserves_other_layers(r, before, after, before_evidence, after_evidence)
    {
        return false;
    }
    let next = after;
    let plugin = next.plugins.iter().find(|p| p.id == r.target);
    let value = out.value.as_ref().unwrap_or(&Value::Null);
    match r.action {
        Action::AddMarket => {
            let name = value["marketplace"].as_str();
            next.markets
                .iter()
                .any(|m| Some(m.name.as_str()) == name && m.declared_in_target)
        }
        Action::RemoveMarket => {
            !next
                .markets
                .iter()
                .any(|m| m.name == r.target && m.declared_in_target)
                && (!next
                    .plugins
                    .iter()
                    .any(|p| p.marketplace == r.target && p.installed_in_target))
        }
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
fn preserves_other_layers(
    r: &Request,
    before: &Snapshot,
    after: &Snapshot,
    before_evidence: &NativeEvidence,
    after_evidence: &NativeEvidence,
) -> bool {
    for layer in &before_evidence.layers {
        if !layer.selected(&before.context)
            && after_evidence
                .layers
                .iter()
                .find(|l| l.path == layer.path)
                .is_none_or(|l| l.settings != layer.settings)
        {
            return false;
        }
    }
    for layer in &after_evidence.layers {
        if !layer.selected(&after.context)
            && !before_evidence.layers.iter().any(|l| l.path == layer.path)
            && layer.settings.as_object().is_none_or(|s| !s.is_empty())
        {
            return false;
        }
    }
    preserves_installations(r, before, after)
}
const ACTIONS: &[Action] = &[
    Action::AddMarket,
    Action::SyncMarket,
    Action::RemoveMarket,
    Action::Install,
    Action::Enable,
    Action::Disable,
    Action::Uninstall,
    Action::Update,
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
        sources: vec!["local".into(), "git".into(), "url".into()],
    }
}
pub(super) fn probe_sequence() -> Vec<Action> {
    let sequence = vec![
        Action::AddMarket,
        Action::Install,
        Action::Disable,
        Action::Update,
        Action::Enable,
        Action::Uninstall,
        Action::RemoveMarket,
    ];
    sequence
}
