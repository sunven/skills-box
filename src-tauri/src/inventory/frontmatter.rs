//! Minimal `SKILL.md` frontmatter reader.
//!
//! Only `name` and `description` are needed by the read model. The parser is
//! deliberately small: it understands the scalar shapes installers write in
//! practice (plain, quoted, `>` / `|` blocks) and reports malformed
//! frontmatter as a recoverable per-row warning rather than failing the
//! inventory.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
}

/// Returns `Ok(None)` when the file has no frontmatter block, `Ok(Some(_))`
/// when a block was parsed (fields may still be absent), and `Err(_)` when the
/// block is malformed.
pub fn parse(raw: &str) -> Result<Option<Frontmatter>, String> {
    let mut lines = raw.lines();
    if lines.next().map(|line| line.trim_end()) != Some("---") {
        return Ok(None);
    }
    let mut body: Vec<&str> = Vec::new();
    let mut closed = false;
    for line in lines {
        if line.trim_end() == "---" {
            closed = true;
            break;
        }
        body.push(line);
    }
    if !closed {
        return Err("frontmatter 缺少结束分隔符".to_string());
    }

    let mut frontmatter = Frontmatter::default();
    let mut index = 0;
    while index < body.len() {
        let line = body[index];
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            index += 1;
            continue;
        }
        if line.starts_with([' ', '\t']) {
            // Continuation without a recognised key: ignore (nested YAML).
            index += 1;
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            index += 1;
            continue;
        };
        let key = key.trim();
        let value = value.trim();

        let parsed = match value {
            ">" | "|" | ">-" | "|-" | ">+" | "|+" => {
                let (block, next) = read_block(&body, index + 1, value.starts_with('>'));
                index = next;
                Some(block)
            }
            _ => {
                index += 1;
                Some(parse_scalar(value, key)?)
            }
        };

        match key {
            "name" => frontmatter.name = parsed.filter(|value| !value.is_empty()),
            "description" => frontmatter.description = parsed.filter(|value| !value.is_empty()),
            _ => {}
        }
    }

    Ok(Some(frontmatter))
}

fn read_block(lines: &[&str], start: usize, folded: bool) -> (String, usize) {
    let mut index = start;
    let mut collected: Vec<String> = Vec::new();
    while index < lines.len() {
        let line = lines[index];
        if !line.trim().is_empty() && !line.starts_with([' ', '\t']) {
            break;
        }
        collected.push(line.trim().to_string());
        index += 1;
    }
    while collected
        .last()
        .map(|line| line.is_empty())
        .unwrap_or(false)
    {
        collected.pop();
    }
    let joined = if folded {
        collected.join(" ")
    } else {
        collected.join("\n")
    };
    (joined.trim().to_string(), index)
}

fn parse_scalar(value: &str, key: &str) -> Result<String, String> {
    if value.is_empty() {
        return Ok(String::new());
    }
    if let Some(inner) = value.strip_prefix('"') {
        let Some(inner) = inner.strip_suffix('"') else {
            return Err(format!("{key} 的引号未闭合"));
        };
        return Ok(inner.replace("\\\"", "\"").replace("\\\\", "\\"));
    }
    if let Some(inner) = value.strip_prefix('\'') {
        let Some(inner) = inner.strip_suffix('\'') else {
            return Err(format!("{key} 的引号未闭合"));
        };
        return Ok(inner.replace("''", "'"));
    }
    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_frontmatter_fields() {
        let raw = "---\nname: alpha\ndescription: 一个小工具\n---\n\n# Alpha\n";
        let frontmatter = parse(raw).unwrap().unwrap();
        assert_eq!(frontmatter.name.as_deref(), Some("alpha"));
        assert_eq!(frontmatter.description.as_deref(), Some("一个小工具"));
    }

    #[test]
    fn strips_matching_quotes_and_handles_escapes() {
        let raw = "---\nname: \"alpha\"\ndescription: \"带 \\\"引号\\\" 的描述\"\n---\n";
        let frontmatter = parse(raw).unwrap().unwrap();
        assert_eq!(frontmatter.name.as_deref(), Some("alpha"));
        assert_eq!(
            frontmatter.description.as_deref(),
            Some("带 \"引号\" 的描述")
        );

        let raw = "---\ndescription: 'it''s fine'\n---\n";
        assert_eq!(
            parse(raw).unwrap().unwrap().description.as_deref(),
            Some("it's fine")
        );
    }

    #[test]
    fn parses_folded_and_literal_blocks() {
        let raw = "---\ndescription: >\n  第一行\n  第二行\n---\n";
        assert_eq!(
            parse(raw).unwrap().unwrap().description.as_deref(),
            Some("第一行 第二行")
        );

        let raw = "---\ndescription: |\n  line one\n  line two\n---\n";
        assert_eq!(
            parse(raw).unwrap().unwrap().description.as_deref(),
            Some("line one\nline two")
        );
    }

    #[test]
    fn missing_frontmatter_is_none_not_an_error() {
        assert_eq!(parse("# Alpha\n\ndescription: nope\n").unwrap(), None);
        assert_eq!(parse("").unwrap(), None);
        assert_eq!(parse("--- not a fence\n").unwrap(), None);
    }

    #[test]
    fn valid_block_without_target_fields_yields_empty_frontmatter() {
        let frontmatter = parse("---\nfoo: bar\n---\n").unwrap().unwrap();
        assert_eq!(frontmatter, Frontmatter::default());
    }

    #[test]
    fn unterminated_fence_or_quote_is_reported() {
        assert!(parse("---\nname: alpha\n").is_err());
        assert!(parse("---\ndescription: \"unterminated\n---\n").is_err());
    }

    #[test]
    fn ignores_nested_blocks_and_comments() {
        let raw =
            "---\nname: alpha\n# comment\nmetadata:\n  owner: someone\ndescription: ok\n---\n";
        let frontmatter = parse(raw).unwrap().unwrap();
        assert_eq!(frontmatter.description.as_deref(), Some("ok"));
    }
}
