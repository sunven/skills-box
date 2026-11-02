//! Parser for `skills add <source> -l -g` human output (skills@1.7.4).
//!
//! The CLI has no machine-readable preview format and `NO_COLOR` is ignored in
//! 1.7.4, so we strip ANSI, anchor on the `◇  Available Skills` marker, and
//! distinguish names/descriptions by the indentation after the `│` guide.
//! There is deliberately no dependency on paths or hashes — the preview does
//! not print them.

use serde::{Deserialize, Serialize};

/// Names and descriptions can be long; keep raw fallback payloads bounded.
const RAW_OUTPUT_LIMIT: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedSkill {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewParseOutcome {
    Skills(Vec<ParsedSkill>),
    NoSkills,
    Failed { raw: String },
}

pub fn parse_preview_output(raw: &str) -> PreviewParseOutcome {
    let cleaned = strip_ansi(raw);
    let lines: Vec<&str> = cleaned.split('\n').collect();

    let anchor = lines
        .iter()
        .position(|line| line.trim_end() == "◇  Available Skills");
    let Some(anchor) = anchor else {
        if lines.iter().any(|line| {
            line.trim_end() == "◇  No skills found"
                || line.contains("No valid skills found. Skills require a SKILL.md")
        }) {
            return PreviewParseOutcome::NoSkills;
        }
        return PreviewParseOutcome::Failed {
            raw: truncate_chars(&cleaned, RAW_OUTPUT_LIMIT),
        };
    };

    let mut skills: Vec<ParsedSkill> = Vec::new();
    let mut group: Option<String> = None;
    let mut current: Option<ParsedSkill> = None;

    for line in lines.iter().skip(anchor + 1) {
        if line.trim_end().starts_with("└  Use --skill") {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        if !line.starts_with('│') {
            let heading = line.trim();
            if !heading.is_empty() {
                group = Some(heading.to_string());
            }
            continue;
        }

        // Check the longer (description) prefix first: a six-space line also
        // starts with the four-space name prefix.
        if let Some(description) = line.strip_prefix("│      ") {
            if let Some(skill) = current.as_mut() {
                if !skill.description.is_empty() {
                    skill.description.push('\n');
                }
                skill.description.push_str(description.trim_end());
            }
            continue;
        }
        if let Some(name) = line.strip_prefix("│    ") {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            if let Some(previous) = current.take() {
                skills.push(previous);
            }
            current = Some(ParsedSkill {
                name: name.to_string(),
                description: String::new(),
                group: group.clone(),
            });
        }
        // Guide-only `│`, blank description lines and unknown decorations are
        // ignored.
    }
    if let Some(last) = current {
        skills.push(last);
    }

    if skills.is_empty() {
        PreviewParseOutcome::Failed {
            raw: truncate_chars(&cleaned, RAW_OUTPUT_LIMIT),
        }
    } else {
        PreviewParseOutcome::Skills(skills)
    }
}

/// Strip CSI/OSC escape sequences and carriage returns from CLI output.
pub fn strip_ansi(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\r' => {}
            '\u{1b}' => match chars.peek().copied() {
                Some('[') => {
                    chars.next();
                    // CSI: parameters/intermediates until a final byte 0x40..=0x7e.
                    for inner in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&inner) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    // OSC: terminated by BEL or ST (ESC \).
                    let mut previous_escape = false;
                    for inner in chars.by_ref() {
                        if inner == '\u{7}' || (previous_escape && inner == '\\') {
                            break;
                        }
                        previous_escape = inner == '\u{1b}';
                    }
                }
                Some(_) => {
                    chars.next();
                }
                None => {}
            },
            other => output.push(other),
        }
    }
    output
}

pub fn truncate_chars(input: &str, limit: usize) -> String {
    if input.chars().count() <= limit {
        return input.to_string();
    }
    let mut truncated: String = input.chars().take(limit).collect();
    truncated.push_str("\n…（输出已截断）");
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    const SINGLE_CI: &str = include_str!("../../tests/fixtures/cli/single-skill-ci.txt");
    const SINGLE_SPINNER: &str = include_str!("../../tests/fixtures/cli/single-skill-spinner.txt");
    const MULTI_CI: &str = include_str!("../../tests/fixtures/cli/multi-skill-ci.txt");
    const MULTI_SPINNER: &str = include_str!("../../tests/fixtures/cli/multi-skill-spinner.txt");
    const GROUPED: &str = include_str!("../../tests/fixtures/cli/grouped-spinner.txt");
    const NO_SKILLS: &str = include_str!("../../tests/fixtures/cli/no-skills.txt");
    const NETWORK: &str = include_str!("../../tests/fixtures/cli/network-failure.txt");

    fn skills(outcome: PreviewParseOutcome) -> Vec<ParsedSkill> {
        match outcome {
            PreviewParseOutcome::Skills(skills) => skills,
            other => panic!("expected skills, got {other:?}"),
        }
    }

    #[test]
    fn parses_single_skill_ci_output() {
        let parsed = skills(parse_preview_output(SINGLE_CI));
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "find-skills");
        assert!(
            parsed[0]
                .description
                .contains("Helps users discover and install agent skills")
        );
        assert!(parsed[0].group.is_none());
    }

    #[test]
    fn parses_single_skill_with_spinner_and_ansi() {
        let parsed = skills(parse_preview_output(SINGLE_SPINNER));
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "find-skills");
        assert!(!parsed[0].description.contains('\u{1b}'));
        assert!(parsed[0].description.contains("installable skill"));
    }

    #[test]
    fn parses_multi_skill_output_in_discovery_order_without_paths() {
        let cases: [(&str, &[&str]); 2] = [
            (
                MULTI_CI,
                &[
                    "vercel-composition-patterns",
                    "deploy-to-vercel",
                    "vercel-react-best-practices",
                    "vercel-react-native-skills",
                    "vercel-react-view-transitions",
                    "vercel-cli-with-tokens",
                    "vercel-optimize",
                    "web-design-guidelines",
                    "writing-guidelines",
                ],
            ),
            (
                MULTI_SPINNER,
                &[
                    "vercel-composition-patterns",
                    "deploy-to-vercel",
                    "vercel-react-best-practices",
                    "vercel-react-view-transitions",
                    "vercel-optimize",
                    "web-design-guidelines",
                    "writing-guidelines",
                ],
            ),
        ];
        for (raw, expected) in cases {
            let parsed = skills(parse_preview_output(raw));
            let names: Vec<&str> = parsed.iter().map(|skill| skill.name.as_str()).collect();
            assert_eq!(names, expected);
            assert!(parsed.iter().all(|skill| !skill.description.is_empty()));
            assert!(
                parsed
                    .iter()
                    .all(|skill| !skill.description.contains("https://github.com"))
            );
        }
    }

    #[test]
    fn captures_plugin_group_headings() {
        let parsed = skills(parse_preview_output(GROUPED));
        assert_eq!(parsed.len(), 38);
        assert_eq!(parsed[0].name, "ask-matt");
        assert_eq!(parsed[0].group.as_deref(), Some("Mattpocock Skills"));
        let general = parsed
            .iter()
            .find(|skill| skill.name == "chief-of-staff")
            .expect("General group skill");
        assert_eq!(general.group.as_deref(), Some("General"));
    }

    #[test]
    fn recognizes_no_skills_marker() {
        assert_eq!(
            parse_preview_output(NO_SKILLS),
            PreviewParseOutcome::NoSkills
        );
    }

    #[test]
    fn keeps_raw_text_when_anchor_is_absent() {
        match parse_preview_output(NETWORK) {
            PreviewParseOutcome::Failed { raw } => {
                assert!(raw.contains("Failed to clone repository"));
                assert!(raw.contains("Couldn't connect to server"));
            }
            other => panic!("expected failed parse, got {other:?}"),
        }
    }

    #[test]
    fn joins_multiline_descriptions() {
        let raw = "◇  Available Skills\n│\n│    foo\n│\n│      line one\n│      line two\n│\n└  Use --skill <name> to install specific skills\n";
        let parsed = skills(parse_preview_output(raw));
        assert_eq!(parsed[0].description, "line one\nline two");
    }

    #[test]
    fn ignores_banner_and_spinner_text_before_anchor() {
        let raw = "███████╗██╗  ██╗\n┌   skills \n◒  Fetching skills…...\n◇  Found 1 skill\n◇  Available Skills\n│    real-skill\n│      real description\n└  Use --skill <name> to install specific skills\n";
        let parsed = skills(parse_preview_output(raw));
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "real-skill");
    }

    #[test]
    fn strips_csi_osc_and_carriage_returns() {
        let raw = "\u{1b}[36mname\u{1b}[39m\r\n\u{1b}]0;title\u{7}tail\u{1b}[?25l";
        assert_eq!(strip_ansi(raw), "name\ntail");
    }

    #[test]
    fn truncates_oversized_raw_fallback() {
        let long = "x".repeat(RAW_OUTPUT_LIMIT + 10);
        let truncated = truncate_chars(&long, RAW_OUTPUT_LIMIT);
        assert!(truncated.ends_with("…（输出已截断）"));
        assert!(truncated.chars().count() < long.chars().count());
    }
}
