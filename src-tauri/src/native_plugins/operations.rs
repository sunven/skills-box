use super::{
    inventory::{identity, valid_name, Observation},
    model::*,
    transport::{Output, Runtime},
};
use serde_json::{json, Value};
use std::path::Path;

pub fn arguments(request: &Request, obs: &Observation) -> Result<Vec<String>, String> {
    let snapshot = &obs.snapshot;
    if snapshot.host != request.host {
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
        return Err("此操作尚未通过原生契约验证。".into());
    }
    let host = request.host;
    let action = request.action;
    let mut args = vec!["plugin".into()];
    if action == Action::AddMarket {
        validate_source(request)?;
        if !snapshot.source_kinds.contains(&request.source_kind) {
            return Err("此来源格式尚未通过当前 CLI 的隔离验证。".into());
        }
        args.extend(["marketplace".into(), "add".into()]);
        let mut source = request.source.trim().to_string();
        if host == Host::Claude && !request.reference.is_empty() {
            source.push('#');
            source.push_str(&request.reference);
        }
        args.push(source);
        if host == Host::Codex && !request.reference.is_empty() {
            args.extend(["--ref".into(), request.reference.clone()]);
        }
        if host == Host::Claude {
            args.extend(["--scope".into(), "user".into()]);
        }
        args.push("--json".into());
        for path in &request.sparse_paths {
            if host == Host::Codex
                || args
                    .last()
                    .is_none_or(|_| !args.contains(&"--sparse".to_string()))
            {
                args.push("--sparse".into());
            }
            args.push(path.clone());
        }
        return Ok(args);
    }
    if matches!(action, Action::RemoveMarket | Action::SyncMarket) {
        if !valid_name(&request.target) {
            return Err("无效市场身份。".into());
        }
        let market = snapshot
            .markets
            .iter()
            .find(|m| m.name == request.target)
            .ok_or("市场已不存在，请重新读取。")?;
        if let Some(reason) = &market.read_only_reason {
            return Err(reason.clone());
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
            for p in snapshot
                .plugins
                .iter()
                .filter(|p| p.marketplace == market.name && p.installation_state != "absent")
            {
                if p.install_blocked_reason.is_some()
                    || p.read_only_reason.is_some()
                    || p.installations.iter().any(|i| i.scope != "user")
                {
                    return Err("同步可能影响非用户插件或需要批准的安装，请在宿主处理。".into());
                }
            }
        }
        args.extend([
            "marketplace".into(),
            if action == Action::RemoveMarket {
                "remove"
            } else if host == Host::Codex {
                "upgrade"
            } else {
                "update"
            }
            .into(),
            request.target.clone(),
        ]);
        if host == Host::Claude && action == Action::RemoveMarket {
            args.extend(["--scope".into(), "user".into()]);
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
        if !matches!(action, Action::Enable | Action::Disable)
            && plugin.installations.iter().any(|i| i.scope != "user")
        {
            return Err("原生操作可能影响共享的项目安装，请在宿主中处理。".into());
        }
        if action == Action::Install && plugin.installation_state != "absent" {
            return Err("目标已有安装或待核对记录，请重新读取。".into());
        }
        if action != Action::Install && plugin.installation_state == "absent" {
            return Err("此插件没有用户级安装。".into());
        }
        if host == Host::Codex && matches!(action, Action::Enable | Action::Disable) {
            if obs.user_version.is_none() {
                return Err("缺少用户配置版本，无法安全写入。".into());
            }
            return Ok(vec![
                "config/value/write".into(),
                format!(
                    "plugins.{}.enabled",
                    serde_json::to_string(&request.target).unwrap()
                ),
                (action == Action::Enable).to_string(),
            ]);
        }
        let command = match (host, action) {
            (Host::Codex, Action::Install | Action::Reinstall) => "add",
            (Host::Codex, Action::Uninstall) => "remove",
            (Host::Claude, Action::Install) => "install",
            (Host::Claude, Action::Update) => "update",
            (Host::Claude, Action::Uninstall) => "uninstall",
            (Host::Claude, Action::Enable) => "enable",
            (Host::Claude, Action::Disable) => "disable",
            _ => return Err("宿主不支持此动作。".into()),
        };
        args.extend([command.into(), request.target.clone()]);
        if host == Host::Claude {
            args.extend(["--scope".into(), "user".into()]);
        }
    }
    args.push("--json".into());
    Ok(args)
}
fn validate_source(r: &Request) -> Result<(), String> {
    let source = r.source.trim();
    if source.is_empty() || source.starts_with('-') || source.chars().any(char::is_control) {
        return Err("请输入有效市场来源。".into());
    }
    if !matches!(r.source_kind.as_str(), "local" | "git" | "url") {
        return Err("来源类型无效。".into());
    }
    if r.source_kind == "local" {
        let path = Path::new(source);
        if !path.is_absolute() || !path.exists() || (r.host == Host::Codex && !path.is_dir()) {
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
    if r.source_kind == "url" && r.host != Host::Claude {
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
pub fn confirmation_fingerprint(request: &Request, obs: &Observation) -> Result<String, String> {
    if request.action != Action::AddMarket || request.source_kind != "local" {
        return Ok(obs.snapshot.fingerprint.clone());
    }
    let root = std::fs::canonicalize(request.source.trim()).map_err(|e| e.to_string())?;
    let path = if root.is_file() {
        root.clone()
    } else if request.host == Host::Codex && root.join(".agents/plugins/marketplace.json").exists()
    {
        root.join(".agents/plugins/marketplace.json")
    } else {
        root.join(".claude-plugin/marketplace.json")
    };
    let doc = super::inventory::optional_json(&path)?;
    Ok(super::inventory::hash(&json!([
        obs.snapshot.fingerprint,
        root,
        doc
    ])))
}
pub fn consequences(r: &Request, obs: &Observation) -> (Vec<String>, Vec<String>) {
    let mut lines=vec!["操作仅指定用户级设置；实际生效请在宿主新会话中确认。开始写入后，正常退出会等待写入和核验结束。".into()];
    let market_name = identity(&r.target)
        .map(|(_, market)| market)
        .unwrap_or(&r.target);
    if let Some(market) = obs.snapshot.markets.iter().find(|m| m.name == market_name) {
        lines.push(format!("市场来源：{} · {}", market.name, market.source));
    }
    let affected = obs
        .snapshot
        .markets
        .iter()
        .find(|m| m.name == r.target)
        .map(|m| m.affected_plugins.clone())
        .unwrap_or_default();
    match r.action {
        Action::SyncMarket=>lines.push("同步是写操作，可能刷新或安装该市场已配置插件的文件，包括禁用插件；不会改变宿主自动更新设置。".into()),
        Action::Reinstall=>lines.push("Codex 会重新启用此插件，包括原先禁用的插件；重新安装不保证取得最新版。".into()),
        Action::RemoveMarket if r.host==Host::Claude=>lines.push("Claude Code 会连带卸载所列插件，并尽可能删除保存的选项、密钥和插件数据。没有自动回滚。".into()),
        Action::RemoveMarket=>lines.push("Codex 会移除市场配置和 Git 市场快照；插件配置与缓存可能保留，后续安装状态需核对。".into()),
        Action::Uninstall=>lines.push("交由宿主卸载插件及其默认清理范围，可能删除插件数据、选项和密钥；Skills Box 不追加删除来源或凭据，不提供回滚。".into()),
        Action::AddMarket|Action::Install=>lines.push("使用宿主的来源解析与本机凭据；此确认不保证远程内容不可变。需要命令批准、认证或信任时请在宿主处理。".into()),
        _=>(),
    }
    (lines, affected)
}
pub fn execute(rt: &Runtime, r: &Request, obs: &Observation) -> Result<Output, String> {
    let args = arguments(r, obs)?;
    if r.host == Host::Codex && matches!(r.action, Action::Enable | Action::Disable) {
        let v=rt.rpc("config/value/write",json!({"keyPath":args[1],"value":r.action==Action::Enable,"mergeStrategy":"replace","filePath":rt.root.join("config.toml"),"expectedVersion":obs.user_version}))?;
        Ok(Output {
            success: v["status"] == "ok" || v["status"] == "okOverridden",
            diagnostic: super::transport::diagnostic(&v.to_string()),
            value: Some(v),
        })
    } else {
        rt.run(&args, true)
    }
}
pub fn response_matches(r: &Request, out: &Output) -> bool {
    let Some(v) = &out.value else { return false };
    if !out.success {
        return false;
    }
    if r.host == Host::Claude {
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
                        && v["scope"] == "user"
                        && (r.action != Action::Update
                            || matches!(
                                v["updateOutcome"].as_str(),
                                Some("updated" | "up_to_date")
                            ))
                }
            }
    } else {
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
pub fn postcondition(r: &Request, before: &Observation, after: &Observation, out: &Output) -> bool {
    let next = &after.snapshot;
    let plugin = next.plugins.iter().find(|p| p.id == r.target);
    let value = out.value.as_ref().unwrap_or(&Value::Null);
    match r.action {
        Action::AddMarket => {
            let name = value[if r.host == Host::Codex {
                "marketplaceName"
            } else {
                "marketplace"
            }]
            .as_str();
            next.markets
                .iter()
                .any(|m| Some(m.name.as_str()) == name && m.read_only_reason.is_none())
        }
        Action::RemoveMarket => {
            !next.markets.iter().any(|m| m.name == r.target)
                && (r.host == Host::Codex
                    || !next.plugins.iter().any(|p| {
                        p.marketplace == r.target
                            && p.installations.iter().any(|i| i.scope == "user")
                    }))
        }
        Action::SyncMarket => {
            next.markets
                .iter()
                .any(|m| m.name == r.target && m.catalog_error.is_none())
                && before
                    .snapshot
                    .plugins
                    .iter()
                    .filter(|p| p.marketplace == r.target)
                    .all(|p| {
                        next.plugins
                            .iter()
                            .find(|n| n.id == p.id)
                            .is_some_and(|n| n.user_enabled == p.user_enabled)
                    })
        }
        Action::Install | Action::Reinstall => plugin.is_some_and(|p| {
            p.installation_state == "installed"
                && p.user_enabled == Some(true)
                && p.local_root.as_ref().is_some_and(|p| Path::new(p).is_dir())
        }),
        Action::Enable => plugin.is_some_and(|p| p.user_enabled == Some(true)),
        Action::Disable => plugin.is_some_and(|p| p.user_enabled == Some(false)),
        Action::Uninstall => {
            plugin.is_none_or(|p| p.installation_state == "absent" && p.user_enabled.is_none())
        }
        Action::Update => plugin.is_some_and(|p| {
            p.installation_state == "installed"
                && p.version.as_deref() == value["newVersion"].as_str()
                && before
                    .snapshot
                    .plugins
                    .iter()
                    .find(|b| b.id == p.id)
                    .is_some_and(|b| b.user_enabled == p.user_enabled)
        }),
    }
}
pub fn outcome(
    preview: Preview,
    before: &Observation,
    after: Result<Observation, String>,
    output: Result<Output, String>,
) -> Outcome {
    let executed = true;
    let command_succeeded = output.as_ref().ok().map(|o| o.success);
    let error = output.as_ref().err().cloned().or_else(|| {
        output
            .as_ref()
            .ok()
            .filter(|o| !response_matches(&preview.request, o))
            .map(|o| o.diagnostic.clone())
    });
    match after {
        Err(e) => Outcome {
            status: "unconfirmed".into(),
            message: format!("原生命令已结束，但状态核验失败：{e}。请重新读取，不要直接重复写入。"),
            executed,
            command_succeeded,
            verification: "unknown".into(),
            snapshot: None,
            preview,
        },
        Ok(after) => {
            if let Some(reason) = after
                .snapshot
                .capabilities
                .iter()
                .find(|c| c.action == preview.request.action)
                .and_then(|c| c.blocked_reason.as_ref())
            {
                return Outcome {
                    status: "unconfirmed".into(),
                    message: format!("原生命令已结束，结果待核对：{reason} 没有自动重试。"),
                    executed,
                    command_succeeded,
                    verification: "unknown".into(),
                    snapshot: Some(after.snapshot),
                    preview,
                };
            }
            let verified = after
                .snapshot
                .capabilities
                .iter()
                .any(|c| c.action == preview.request.action && c.blocked_reason.is_none())
                && output.as_ref().is_ok_and(|o| {
                    response_matches(&preview.request, o)
                        && postcondition(&preview.request, before, &after, o)
                });
            let changed = before.snapshot.fingerprint != after.snapshot.fingerprint;
            let needs_host = error.as_ref().is_some_and(|e| {
                let e = e.to_lowercase();
                [
                    "approval",
                    "authentication",
                    "credential",
                    "trust",
                    "permission",
                    "command_source_refused",
                    "headers_helper_refused",
                    "not an interactive terminal",
                ]
                .iter()
                .any(|w| e.contains(w))
            });
            let status = if verified {
                "succeeded"
            } else if changed {
                "partial"
            } else if needs_host {
                "needs_host"
            } else if command_succeeded == Some(false) {
                "failed"
            } else {
                "unconfirmed"
            };
            let message = if verified {
                "原生命令完成，用户设置和原生记录已核对。请在宿主新会话中确认实际效果。".into()
            } else {
                format!(
                    "{}{} 当前可读取的原生状态已保留；没有自动回滚或重试。",
                    if needs_host {
                        "需要在宿主中完成批准、认证或信任。 "
                    } else {
                        "操作未完整通过核验。 "
                    },
                    if needs_host {
                        String::new()
                    } else {
                        error.unwrap_or_default()
                    }
                )
            };
            Outcome {
                status: status.into(),
                message,
                executed,
                command_succeeded,
                verification: if verified { "verified" } else { "observed" }.into(),
                snapshot: Some(after.snapshot),
                preview,
            }
        }
    }
}
