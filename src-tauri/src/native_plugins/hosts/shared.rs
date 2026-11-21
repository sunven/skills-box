use super::super::{
    inventory::{hash, identity, local_path, optional_json, valid_name},
    model::*,
    transport::Runtime,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
pub(super) fn text(v: &Value, k: &str) -> Option<String> {
    v[k].as_str().map(str::to_string)
}
pub(super) fn array(v: &Value) -> Result<&Vec<Value>, String> {
    v.as_array().ok_or("原生清单格式不兼容。".into())
}
pub(super) fn obj(v: &Value) -> Result<&serde_json::Map<String, Value>, String> {
    v.as_object().ok_or("原生配置格式不兼容。".into())
}
pub(super) fn source_summary(source: &Value) -> Value {
    if let Some(source) = source.as_str() {
        return json!(super::super::transport::redact_urls(source));
    }
    let mut summary = serde_json::Map::new();
    for key in ["source", "path", "url", "repo", "ref"] {
        if let Some(v) = source[key].as_str() {
            summary.insert(key.into(), json!(super::super::transport::redact_urls(v)));
        }
    }
    Value::Object(summary)
}
pub(super) fn installation_labels(p: &Plugin) -> Vec<String> {
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
pub(super) type Catalogs = BTreeMap<String, (Value, Option<PathBuf>)>;
pub(super) fn read_catalog(
    root: Option<String>,
    name: &str,
    manifests: &[&str],
    catalogs: &mut Catalogs,
    evidence: &mut Vec<Value>,
) -> Result<(), String> {
    let root = PathBuf::from(root.ok_or("原生工具未提供本地目录，目录内容不可用。")?);
    if !root.is_absolute() {
        return Err("市场目录不是绝对路径。".into());
    }
    let paths = if root.is_file() {
        vec![root.clone()]
    } else {
        manifests.iter().map(|p| root.join(p)).collect()
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
        read_error.unwrap_or_else(|| "本地目录缺失或市场名称不匹配；搜索结果不完整。".into())
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
}
pub(super) fn finish(
    rt: &Runtime,
    context: ResolvedContext,
    capabilities: Vec<Capability>,
    markets: Vec<Market>,
    plugins: Vec<Plugin>,
    warnings: Vec<String>,
    version: String,
    evidence: Vec<Value>,
) -> Snapshot {
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
    Snapshot {
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
    }
}

pub(super) fn check_context(request: &Request, snapshot: &Snapshot) -> Result<(), String> {
    if snapshot.host != request.host || snapshot.context.target != request.context {
        return Err("宿主身份不匹配。".into());
    }
    if let Some(c) = snapshot
        .capabilities
        .iter()
        .find(|c| c.action == request.action)
    {
        if let Some(reason) = &c.blocked_reason {
            return Err(reason.clone());
        }
    } else {
        return Err("当前宿主不支持此操作。".into());
    }
    Ok(())
}
pub(super) fn check_target(
    request: &Request,
    snapshot: &Snapshot,
    sync_requires_owner: bool,
) -> Result<(), String> {
    let action = request.action;
    if matches!(action, Action::RemoveMarket | Action::SyncMarket) {
        if !valid_name(&request.target) {
            return Err("无效市场身份。".into());
        }
        let market = snapshot
            .markets
            .iter()
            .find(|m| m.name == request.target)
            .ok_or("市场已不存在，请重新读取。")?;
        if action == Action::RemoveMarket || sync_requires_owner {
            if let Some(reason) = &market.read_only_reason {
                return Err(reason.clone());
            }
        }
        if action == Action::RemoveMarket {
            if let Some(reason) = &market.remove_blocked_reason {
                return Err(reason.clone());
            }
        } else {
            if let Some(reason) = &market.sync_blocked_reason {
                return Err(reason.clone());
            }
            if market.catalog_error.is_some() {
                return Err("市场目录读取不完整，无法确认同步影响，请在宿主处理。".into());
            }
            for p in snapshot.plugins.iter().filter(|p| {
                p.marketplace == market.name
                    && (p.installation_state != "absent" || !p.installations.is_empty())
            }) {
                if p.install_blocked_reason.is_some() || p.read_only_reason.is_some() {
                    return Err("同步涉及无法确认来源或需要批准的安装，请在宿主处理。".into());
                }
            }
        }
    } else {
        identity(&request.target)?;
        let plugin = snapshot
            .plugins
            .iter()
            .find(|p| p.id == request.target)
            .ok_or("插件已不存在，请重新读取。")?;
        if let Some(reason) = &plugin.read_only_reason {
            return Err(reason.clone());
        }
        if matches!(action, Action::Install | Action::Reinstall | Action::Update) {
            if let Some(reason) = &plugin.install_blocked_reason {
                return Err(reason.clone());
            }
        }
        if action == Action::Uninstall
            && plugin
                .installations
                .iter()
                .any(|i| !snapshot.context.matches_installation(i))
        {
            return Err("原生操作可能影响共享的项目安装，请在宿主中处理。".into());
        }
        if action == Action::Install
            && (plugin.installed_in_target || plugin.installation_state == "unconfirmed")
        {
            return Err("目标已有安装或待核对记录，请重新读取。".into());
        }
        if action != Action::Install && plugin.installation_state == "absent" {
            return Err("当前项目没有可用的原生安装。".into());
        }
        if matches!(
            action,
            Action::Uninstall | Action::Update | Action::Reinstall
        ) && !plugin.installed_in_target
        {
            return Err("此安装来自其他配置层，请切换到安装所属层操作。".into());
        }
    }
    Ok(())
}
pub(super) fn validate_source(
    r: &Request,
    directories_only: bool,
    supports_url: bool,
) -> Result<(), String> {
    let source = r.source.trim();
    if source.is_empty() || source.starts_with('-') || source.chars().any(char::is_control) {
        return Err("请输入有效市场来源。".into());
    }
    if !matches!(r.source_kind.as_str(), "local" | "git" | "url") {
        return Err("来源类型无效。".into());
    }
    if r.source_kind == "local" {
        let path = Path::new(source);
        if !path.is_absolute() || !path.exists() || (directories_only && !path.is_dir()) {
            return Err("请输入存在的本地目录绝对路径；Claude 也支持市场 JSON 文件。".into());
        }
    } else if source.starts_with("https://") || source.starts_with("http://") {
        let url = reqwest::Url::parse(source).map_err(|_| "来源 URL 无效。")?;
        if !url.username().is_empty() || url.password().is_some() || url.query().is_some() {
            return Err("来源不能包含凭据或查询参数；请使用本机已有 Git 凭据。".into());
        }
    } else if r.source_kind != "git"
        || (!source.starts_with("git@")
            && !source.starts_with("ssh://")
            && !(source.split('/').count() == 2 && source.split('/').all(valid_name)))
    {
        return Err("请输入 Git URL 或 owner/repo 简写。".into());
    }
    if source.starts_with("ssh://") {
        let url = reqwest::Url::parse(source).map_err(|_| "SSH 来源 URL 无效。")?;
        if url.password().is_some() || url.query().is_some() {
            return Err("SSH 来源不能包含密码或查询参数。".into());
        }
    }
    if r.source_kind == "url" && !supports_url {
        return Err("此宿主不支持市场 JSON URL。".into());
    }
    if r.source_kind != "git" && (!r.reference.is_empty() || !r.sparse_paths.is_empty()) {
        return Err("仅 Git 来源支持分支和 sparse 参数。".into());
    }
    if !r.reference.is_empty()
        && (r.reference.starts_with('-')
            || r.reference
                .chars()
                .any(|c| c.is_whitespace() || c.is_control())
            || source.contains('#'))
    {
        return Err("分支格式无效或与来源重复。".into());
    }
    for p in &r.sparse_paths {
        if p.is_empty()
            || p.starts_with('-')
            || Path::new(p).is_absolute()
            || p.chars().any(char::is_control)
            || Path::new(p)
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err("sparse 必须是仓库内的相对路径。".into());
        }
    }
    Ok(())
}
pub(super) fn consequences(r: &Request, snapshot: &Snapshot) -> (Vec<String>, Vec<String>) {
    let mut lines = vec![format!(
        "操作目标：{}。实际生效请在宿主新会话中确认。开始写入后，正常退出会等待写入和核验结束。",
        snapshot.context.label()
    )];
    let market_name = identity(&r.target)
        .map(|(_, market)| market)
        .unwrap_or(&r.target);
    if let Some(market) = snapshot.markets.iter().find(|m| m.name == market_name) {
        lines.push(format!("市场来源：{} · {}", market.name, market.source));
    }
    let affected = snapshot
        .markets
        .iter()
        .find(|m| m.name == r.target)
        .map(|m| m.affected_plugins.clone())
        .unwrap_or_else(|| {
            snapshot
                .plugins
                .iter()
                .find(|p| p.id == r.target)
                .map(installation_labels)
                .unwrap_or_default()
        });
    if matches!(
        r.action,
        Action::SyncMarket | Action::Update | Action::Reinstall
    ) {
        lines.push("市场目录和插件内容可能由全局与多个项目共享；本次内容变化会影响所列安装，不改变其他配置层的启用设置。".into());
    }
    if r.action == Action::SyncMarket {
        lines.push("同步是写操作，可能刷新或安装该市场已配置插件的文件，包括禁用插件；不会改变宿主自动更新设置。".into());
    }
    (lines, affected)
}
pub(super) fn preserves_installations(r: &Request, before: &Snapshot, after: &Snapshot) -> bool {
    let foreign = |snapshot: &Snapshot| {
        let mut rows = Vec::new();
        for p in &snapshot.plugins {
            for i in &p.installations {
                if !snapshot.context.matches_installation(i) {
                    let content_change = (matches!(r.action, Action::Update | Action::Reinstall)
                        && p.id == r.target)
                        || r.action == Action::SyncMarket && p.marketplace == r.target;
                    rows.push((
                        p.id.clone(),
                        i.scope.clone(),
                        i.project_path.clone(),
                        if content_change {
                            None
                        } else {
                            i.version.clone()
                        },
                        if content_change { None } else { i.path.clone() },
                    ));
                }
            }
        }
        rows.sort();
        rows
    };
    foreign(before) == foreign(after)
}
