use super::Member;
use crate::error::AppError;

/// Preserve the author's text; add missing identity fields and an explicit name map.
pub fn adapt(
    slug: &str,
    title: &str,
    summary: &str,
    raw: &str,
    members: &[Member],
) -> Result<(String, String), AppError> {
    super::validate_slug(slug)?;
    if raw.trim().is_empty() || raw.len() > 2 * 1024 * 1024 || raw.contains('\0') {
        return Err(AppError::marketplace_unavailable(
            "工作流为空或格式不受支持",
        ));
    }
    let front =
        crate::inventory::frontmatter::parse(raw).map_err(AppError::marketplace_unavailable)?;
    let mut fields = std::collections::BTreeSet::new();
    if front.is_some() {
        let header = raw
            .lines()
            .skip(1)
            .take_while(|line| line.trim_end() != "---")
            .collect::<Vec<_>>()
            .join("\n");
        let yaml: serde_yaml_ng::Mapping = serde_yaml_ng::from_str(&header).map_err(|e| {
            AppError::marketplace_unavailable(format!("无法解析工作流 frontmatter：{e}"))
        })?;
        if yaml.contains_key(serde_yaml_ng::Value::String("<<".into())) {
            return Err(AppError::marketplace_unavailable(
                "工作流身份不支持 YAML 合并字段",
            ));
        }
        for key in ["name", "description"] {
            if let Some(value) = yaml.get(serde_yaml_ng::Value::String(key.into())) {
                let value = value
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| {
                        AppError::marketplace_unavailable("工作流身份字段必须为非空文本")
                    })?;
                let parsed = front.as_ref().and_then(|f| {
                    if key == "name" {
                        f.name.as_deref()
                    } else {
                        f.description.as_deref()
                    }
                });
                if parsed != Some(value.trim()) {
                    return Err(AppError::marketplace_unavailable(
                        "工作流身份字段格式无法被技能读取器识别",
                    ));
                }
                fields.insert(key);
            }
        }
    }
    let name = front
        .as_ref()
        .and_then(|f| f.name.clone())
        .unwrap_or_else(|| slug.into());
    let mut added = String::new();
    if !fields.contains("name") {
        added.push_str(&format!(
            "name: {}\n",
            serde_json::to_string(&name).unwrap()
        ));
    }
    if !fields.contains("description") {
        let description = if summary.is_empty() {
            title.to_string()
        } else {
            format!("{title} — {summary}")
        };
        added.push_str(&format!(
            "description: {}\n",
            serde_json::to_string(&description).unwrap()
        ));
    }
    let mut installed = if front.is_some() {
        let first_line = raw
            .find('\n')
            .ok_or_else(|| AppError::marketplace_unavailable("无法解析工作流"))?
            + 1;
        format!("{}{}{}", &raw[..first_line], added, &raw[first_line..])
    } else {
        format!("---\n{added}---\n{raw}")
    };
    if crate::marketplace::original_name(&installed)? != name {
        return Err(AppError::marketplace_unavailable("工作流 name 无效"));
    }
    installed.push_str("\n\n## 本地技能名称对照\n\n以下对照对应此次专家包安装；执行上方流程时使用本地技能名。\n\n| 官方成员引用 | 完整来源 | 本地技能名 |\n| --- | --- | --- |\n");
    for member in members {
        // References are validated public slugs/coordinates, never arbitrary Markdown.
        installed.push_str(&format!(
            "| `{}` | `{}` | `{}` |\n",
            member.reference,
            member.coordinate.canonical(),
            member.name.replace('|', "\\|")
        ));
    }
    Ok((name, installed))
}
