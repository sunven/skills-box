use super::model::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};
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
