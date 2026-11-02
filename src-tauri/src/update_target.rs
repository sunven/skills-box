//! Resolve a lock entry to the source and ref that update checking and
//! execution share. Source selection and ref safety are decided once here;
//! callers keep their own diagnostics and CLI argument building.

use crate::lock::LockEntry;
use crate::tasks::source::{SourceKind, SourceSpec, validate_source};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateTarget {
    pub name: String,
    pub source: String,
    pub reference: Option<String>,
}

impl UpdateTarget {
    /// Normalized source, with `#ref` appended only when the lock pinned one.
    pub fn cli_source(&self) -> String {
        match self.reference.as_deref() {
            Some(reference) => format!("{}#{reference}", self.source),
            None => self.source.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateTargetError {
    Source { detail: String },
    Reference { detail: String },
}

impl UpdateTargetError {
    pub fn detail(&self) -> &str {
        match self {
            Self::Source { detail } | Self::Reference { detail } => detail,
        }
    }
}

/// Normalize only the repository source. Ref validity is deliberately not
/// checked so callers that display a stale lock can keep a usable source.
pub fn resolve_source(entry: &LockEntry) -> Result<String, UpdateTargetError> {
    resolve_spec(entry).map(|spec| spec.normalized)
}

pub fn supported_type(source_type: &str) -> bool {
    ["github", "git", "gitlab"]
        .iter()
        .any(|kind| source_type.eq_ignore_ascii_case(kind))
}

fn resolve_spec(entry: &LockEntry) -> Result<SourceSpec, UpdateTargetError> {
    if !supported_type(&entry.source_type) {
        return Err(UpdateTargetError::Source {
            detail: source_detail(entry),
        });
    }
    let github = entry.source_type.eq_ignore_ascii_case("github");
    let candidates = if github {
        [entry.source.as_str(), entry.source_url.as_str()]
    } else {
        [entry.source_url.as_str(), entry.source.as_str()]
    };
    for candidate in candidates {
        if let Ok(spec) = validate_source(candidate) {
            if (github && spec.kind == SourceKind::GitHub)
                || (!github && spec.kind == SourceKind::Git)
            {
                return Ok(spec);
            }
        }
    }
    Err(UpdateTargetError::Source {
        detail: source_detail(entry),
    })
}

pub fn resolve(entry: &LockEntry) -> Result<UpdateTarget, UpdateTargetError> {
    Ok(UpdateTarget {
        name: entry.name.clone(),
        source: resolve_source(entry)?,
        reference: normalize_reference(entry.reference.as_deref())?,
    })
}

fn source_detail(entry: &LockEntry) -> String {
    format!(
        "source={:?} sourceType={:?} sourceUrl={:?}",
        entry.source, entry.source_type, entry.source_url
    )
}

fn normalize_reference(raw: Option<&str>) -> Result<Option<String>, UpdateTargetError> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let reference = raw.trim();
    if reference.is_empty() || reference.eq_ignore_ascii_case("HEAD") {
        return Ok(None);
    }
    if !valid_ref(reference) {
        return Err(UpdateTargetError::Reference {
            detail: format!("lock ref 不是安全的分支/标签名：{reference:?}"),
        });
    }
    Ok(Some(reference.to_string()))
}

fn valid_ref(reference: &str) -> bool {
    !reference.is_empty()
        && reference.len() <= 200
        && !reference.starts_with('-')
        && !reference.starts_with('/')
        && !reference.ends_with('/')
        && !reference.contains("..")
        && reference.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | '/')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        source: &str,
        source_type: &str,
        source_url: &str,
        reference: Option<&str>,
    ) -> LockEntry {
        LockEntry {
            name: "alpha".to_string(),
            source: source.to_string(),
            source_type: source_type.to_string(),
            source_url: source_url.to_string(),
            reference: reference.map(str::to_string),
            skill_path: Some("skills/alpha/SKILL.md".to_string()),
            skill_folder_hash: "0123456789abcdef0123456789abcdef01234567".to_string(),
            installed_at: Some("2026-01-01T00:00:00.000Z".to_string()),
            updated_at: Some("2026-01-01T00:00:00.000Z".to_string()),
            plugin_name: None,
            source_base_url: None,
            well_known_digest: None,
        }
    }

    #[test]
    fn normalizes_source_from_source_or_source_url() {
        let shorthand = entry("acme/skills", "github", "", None);
        assert_eq!(resolve_source(&shorthand).unwrap(), "acme/skills");

        let url = entry("", "GitHub", "https://github.com/Acme/Skills.git", None);
        assert_eq!(resolve_source(&url).unwrap(), "Acme/Skills");

        let fallback = entry(
            "not a repo",
            "github",
            "https://github.com/acme/skills",
            None,
        );
        assert_eq!(resolve_source(&fallback).unwrap(), "acme/skills");
    }

    #[test]
    fn resolves_git_and_gitlab_urls_and_preserves_safe_refs() {
        for source_type in ["git", "gitlab"] {
            let target = resolve(&entry(
                "old/slug",
                source_type,
                "https://cnb.cool/cnb/skills/cnb-skill",
                Some("dev/next"),
            ))
            .unwrap();
            assert_eq!(
                target.cli_source(),
                "https://cnb.cool/cnb/skills/cnb-skill.git#dev/next"
            );
            assert_eq!(
                resolve_source(&entry(
                    "https://gitlab.com/group/sub/Skills.git",
                    source_type,
                    "",
                    None
                ))
                .unwrap(),
                "https://gitlab.com/group/sub/Skills.git"
            );
            assert!(resolve(&entry("owner/repo", source_type, "", None)).is_err());
            assert!(resolve(&entry("ssh://git@host/repo", source_type, "", None)).is_err());
        }
        assert!(resolve(&entry("https://cnb.cool/group/repo", "github", "", None)).is_err());
        assert!(resolve(&entry("https://github.com/owner/repo", "git", "", None)).is_err());
    }

    #[test]
    fn rejects_non_github_sources_with_lock_context() {
        let error = resolve_source(&entry("/tmp/local", "local", "", None)).unwrap_err();
        assert!(matches!(error, UpdateTargetError::Source { .. }));
        assert!(error.detail().contains("sourceType"));

        let error = resolve(&entry("not a repo", "github", "", None)).unwrap_err();
        assert!(matches!(error, UpdateTargetError::Source { .. }));
    }

    #[test]
    fn normalizes_and_validates_refs_with_the_existing_rules() {
        let target = resolve(&entry("acme/skills", "github", "", Some(" dev "))).unwrap();
        assert_eq!(target.reference.as_deref(), Some("dev"));
        assert_eq!(target.cli_source(), "acme/skills#dev");

        for reference in [None, Some(""), Some("HEAD"), Some(" head ")] {
            let target = resolve(&entry("acme/skills", "github", "", reference)).unwrap();
            assert_eq!(target.reference, None);
            assert_eq!(target.cli_source(), "acme/skills");
        }

        for reference in ["--all", "/dev", "dev/", "../dev", "a b"] {
            let error = resolve(&entry("acme/skills", "github", "", Some(reference))).unwrap_err();
            assert!(
                matches!(error, UpdateTargetError::Reference { .. }),
                "{reference:?}"
            );
        }

        let too_long = "a".repeat(201);
        let error = resolve(&entry("acme/skills", "github", "", Some(&too_long))).unwrap_err();
        assert!(matches!(error, UpdateTargetError::Reference { .. }));
    }

    #[test]
    fn source_only_resolution_ignores_ref_validity_for_stale_conflicts() {
        let source = resolve_source(&entry("acme/skills", "github", "", Some("--all"))).unwrap();
        assert_eq!(source, "acme/skills");
    }
}
