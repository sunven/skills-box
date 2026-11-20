//! Projection of native configuration layers. Scope selects an app Project;
//! native installation records and shared content remain owned by the host.
use super::{
    inventory::{Observation, hash, identity, optional_json},
    model::*,
    transport::{Runtime, same_path},
};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize)]
pub struct ConfigLayer {
    pub scope: String,
    pub root: Option<PathBuf>,
    pub path: PathBuf,
    pub settings: Value,
}
impl ConfigLayer {
    pub fn selected(&self, context: &ResolvedContext) -> bool {
        self.scope == context.target.layer.native()
            && match (&self.root, &context.project_root) {
                (None, None) => true,
                (Some(a), Some(b)) => same_path(a, Path::new(b)),
                _ => false,
            }
    }
    pub fn label(&self) -> String {
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

pub fn project(
    rt: &Runtime,
    obs: &mut Observation,
    projects: &[PathBuf],
    config: &Value,
    raw: &Value,
) -> Result<(), String> {
    let context = obs.snapshot.context.clone();
    if rt.host == Host::Codex {
        let layers = config["layers"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        for market in &mut obs.snapshot.markets {
            market.declarations = layers
                .iter()
                .filter(|l| !l["config"]["marketplaces"][&market.name].is_null())
                .map(codex_layer_label)
                .collect();
            if !context.target.scope.is_global() {
                market.declared_in_target = layers.iter().any(|l| {
                    codex_selected(l, &context)
                        && !l["config"]["marketplaces"][&market.name].is_null()
                });
            }
        }
        if !context.target.scope.is_global() {
            for c in &mut obs.snapshot.capabilities {
                c.blocked_reason = Some(if matches!(c.action,Action::Enable|Action::Disable|Action::AddMarket|Action::RemoveMarket) {
                    "Codex 官方接口目前仅允许写用户配置，项目配置只读。请在 Codex 中处理，或切换全局管理。"
                } else { "Codex 此操作的项目目标与共享影响尚未验证，暂不可用；可切换全局管理。" }.into());
            }
            let project_layer = layers.iter().find(|l| codex_selected(l, &context));
            for l in layers.iter().filter(|l| l["disabledReason"].is_string()) {
                obs.snapshot.warnings.push(format!(
                    "{} 未生效：{}",
                    codex_layer_label(l),
                    l["disabledReason"].as_str().unwrap()
                ));
            }
            for p in &mut obs.snapshot.plugins {
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
            obs.snapshot.warnings.push(
                "项目配置按 Codex 的信任与覆盖规则读取；现有会话实际加载状态需在 Codex 中核对。"
                    .into(),
            );
        }
    } else {
        let known = optional_json(&rt.root.join("plugins/known_marketplaces.json"))?;
        let mut roots: BTreeSet<PathBuf> = projects.iter().cloned().collect();
        if let Some(root) = &context.project_root {
            roots.insert(root.into());
        }
        for p in &obs.snapshot.plugins {
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
                    if !obs.snapshot.plugins.iter().any(|p| p.id == *id) {
                        obs.snapshot.plugins.push(Plugin {
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
                            install_blocked_reason: Some(
                                "市场或安装内容缺失，请在宿主核对。".into(),
                            ),
                        });
                    }
                }
            }
            if let Some(entries) = layer.settings["extraKnownMarketplaces"].as_object() {
                for (name, declaration) in entries {
                    if !super::inventory::valid_name(name) {
                        return Err("市场声明身份未知。".into());
                    }
                    if !obs.snapshot.markets.iter().any(|m| m.name == *name) {
                        let source = &declaration["source"];
                        obs.snapshot.markets.push(Market {
                            name: name.clone(),
                            source_kind: source["source"].as_str().unwrap_or("unknown").into(),
                            source: super::transport::redact_urls(
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
                                "市场仅有配置声明，原生缓存与删除影响无法确认，请在宿主处理。"
                                    .into(),
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
        for m in &mut obs.snapshot.markets {
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
            let foreign_install = obs
                .snapshot
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
            m.affected_plugins = obs
                .snapshot
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
        for p in &mut obs.snapshot.plugins {
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
        obs.layers = layers;
        if !context.target.scope.is_global() {
            obs.snapshot.warnings.push("项目共享配置可随仓库提交，协作者仍需本机安装。继承、显式启用和显式禁用不同；清除覆盖需在宿主配置中处理后重新读取。".into());
            if obs
                .snapshot
                .plugins
                .iter()
                .flat_map(|p| &p.installations)
                .any(|i| {
                    i.project_path
                        .as_ref()
                        .is_some_and(|p| !Path::new(p).is_dir())
                })
            {
                obs.snapshot.warnings.push("原生安装记录中存在已不可用的项目路径。重新定位不会迁移这些记录，请查看插件详情并在宿主修复或重新安装。".into());
            }
        }
    }
    obs.snapshot.fingerprint = hash(&(obs.snapshot.fingerprint.clone(), &context, &obs.layers));
    // One source of action eligibility for the UI and server execution gate.
    let blocks: Vec<_> = obs
        .snapshot
        .plugins
        .iter()
        .map(|p| {
            let mut blocks = std::collections::BTreeMap::new();
            for action in [
                Action::Install,
                Action::Enable,
                Action::Disable,
                Action::Update,
                Action::Reinstall,
                Action::Uninstall,
            ] {
                let r = Request {
                    host: rt.host,
                    action,
                    context: context.target.clone(),
                    target: p.id.clone(),
                    source: String::new(),
                    source_kind: String::new(),
                    reference: String::new(),
                    sparse_paths: vec![],
                };
                if let Err(reason) = super::operations::arguments(&r, obs) {
                    blocks.insert(
                        serde_json::to_value(action)
                            .unwrap()
                            .as_str()
                            .unwrap()
                            .into(),
                        reason,
                    );
                }
            }
            blocks
        })
        .collect();
    for (p, blocks) in obs.snapshot.plugins.iter_mut().zip(blocks) {
        p.action_blocks = blocks;
    }
    let market_blocks: Vec<_> = obs
        .snapshot
        .markets
        .iter()
        .map(|market| {
            let blocked = |action| {
                super::operations::arguments(
                    &Request {
                        host: rt.host,
                        action,
                        context: context.target.clone(),
                        target: market.name.clone(),
                        source: String::new(),
                        source_kind: String::new(),
                        reference: String::new(),
                        sparse_paths: vec![],
                    },
                    obs,
                )
                .err()
            };
            (blocked(Action::SyncMarket), blocked(Action::RemoveMarket))
        })
        .collect();
    for (market, (sync, remove)) in obs.snapshot.markets.iter_mut().zip(market_blocks) {
        market.sync_blocked_reason = sync;
        market.remove_blocked_reason = remove;
    }
    Ok(())
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
pub fn installation_labels(p: &Plugin) -> Vec<String> {
    p.installations
        .iter()
        .map(|i| {
            format!(
                "{} · {}{}",
                p.id,
                i.scope,
                i.project_path
                    .as_ref()
                    .map(|r| format!(" · {r}"))
                    .unwrap_or_default()
            )
        })
        .collect()
}
