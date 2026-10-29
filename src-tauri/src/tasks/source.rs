//! Validation and normalization for install sources.
//!
//! v1 only accepts public GitHub root repositories — `owner/repo` or
//! `https://github.com/owner/repo` — and always installs the default branch.
//! Everything else (subdirectories, refs, GitLab, SSH, local paths, Notion)
//! is rejected before any CLI process is spawned.

use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSpec {
    pub normalized: String,
}

pub fn validate_source(input: &str) -> Result<SourceSpec, AppError> {
    let value = input.trim();
    if value.is_empty() {
        return Err(AppError::source_missing());
    }
    if value.chars().any(char::is_control) {
        return Err(AppError::source_invalid("来源包含控制字符"));
    }
    if value.contains('#') {
        return Err(AppError::source_unsupported_ref());
    }
    if is_local_path(value) {
        return Err(AppError::source_unsupported_provider("本地路径"));
    }
    if value.starts_with("git@")
        || value.starts_with("ssh://")
        || value.starts_with("git://")
        || value.starts_with("git+ssh://")
    {
        return Err(AppError::source_unsupported_provider("SSH / Git"));
    }

    if let Some(rest) = value.strip_prefix("https://") {
        let (host, path) = match rest.split_once('/') {
            Some((host, path)) => (host, path),
            None => (rest, ""),
        };
        let host_lower = host.to_ascii_lowercase();
        return match host_lower.as_str() {
            "github.com" | "www.github.com" => parse_github_path(path),
            host if host.contains("gitlab") => Err(AppError::source_unsupported_provider("GitLab")),
            host if host.contains("notion") => Err(AppError::source_unsupported_provider("Notion")),
            "" => Err(AppError::source_invalid("URL 缺少主机名")),
            other => Err(AppError::source_unsupported_provider(other)),
        };
    }
    if value.starts_with("http://") {
        return Err(AppError::source_unsupported_provider(
            "HTTP（仅支持 HTTPS）",
        ));
    }
    if value.contains("://") {
        return Err(AppError::source_unsupported_provider("非 HTTPS 协议"));
    }

    let lower = value.to_ascii_lowercase();
    if lower.starts_with("gitlab.com/")
        || lower.starts_with("www.gitlab.com/")
        || lower.starts_with("gitlab:")
    {
        return Err(AppError::source_unsupported_provider("GitLab"));
    }
    if lower.starts_with("notion.so/")
        || lower.starts_with("www.notion.so/")
        || lower.starts_with("notion.site/")
        || lower.starts_with("www.notion.site/")
        || lower.starts_with("notion:")
    {
        return Err(AppError::source_unsupported_provider("Notion"));
    }
    if lower.starts_with("github.com/") || lower.starts_with("www.github.com/") {
        return Err(AppError::source_invalid("GitHub URL 缺少 https://"));
    }

    parse_shorthand(value)
}

fn is_local_path(value: &str) -> bool {
    value == "."
        || value == ".."
        || value.starts_with("./")
        || value.starts_with("../")
        || value.starts_with(".\\")
        || value.starts_with("..\\")
        || value.starts_with('/')
        || value.starts_with('~')
        || value.contains('\\')
        || value
            .as_bytes()
            .get(1)
            .is_some_and(|second| *second == b':' && value.as_bytes()[0].is_ascii_alphabetic())
}

fn parse_github_path(path: &str) -> Result<SourceSpec, AppError> {
    if path.contains('?') {
        return Err(AppError::source_invalid("不支持 URL 查询参数"));
    }
    let cleaned = path.trim_end_matches('/');
    let cleaned = cleaned.strip_suffix(".git").unwrap_or(cleaned);
    if cleaned.contains('@') {
        return Err(AppError::source_invalid("不支持 @skill 语法"));
    }
    parse_owner_repo(cleaned)
}

fn parse_shorthand(value: &str) -> Result<SourceSpec, AppError> {
    if value.contains('@') {
        return Err(AppError::source_invalid("不支持 @skill 语法"));
    }
    if value.contains(':') {
        return Err(AppError::source_invalid("来源包含无效字符"));
    }
    if value.contains('?') {
        return Err(AppError::source_invalid("来源包含查询参数"));
    }
    let cleaned = value.trim_end_matches('/');
    let cleaned = cleaned.strip_suffix(".git").unwrap_or(cleaned);
    parse_owner_repo(cleaned)
}

fn parse_owner_repo(value: &str) -> Result<SourceSpec, AppError> {
    let segments: Vec<&str> = value.split('/').collect();
    if segments.iter().any(|segment| segment.is_empty()) {
        return Err(AppError::source_invalid("仓库路径包含空段"));
    }
    if segments.len() > 2 {
        return Err(AppError::source_unsupported_subdir());
    }
    if segments.len() != 2 {
        return Err(AppError::source_invalid("需要 owner/repo 两段"));
    }
    let owner = segments[0];
    let repo = segments[1];
    if !owner
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric())
        || !is_valid_component(owner)
    {
        return Err(AppError::source_invalid("owner 格式无效"));
    }
    if repo.starts_with('-') || !is_valid_component(repo) || repo == "." || repo == ".." {
        return Err(AppError::source_invalid("repo 格式无效"));
    }
    Ok(SourceSpec {
        normalized: format!("{owner}/{repo}"),
    })
}

fn is_valid_component(component: &str) -> bool {
    !component.is_empty()
        && component.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    fn normalized(input: &str) -> String {
        validate_source(input).unwrap().normalized
    }

    fn code(input: &str) -> ErrorCode {
        validate_source(input).unwrap_err().code
    }

    #[test]
    fn accepts_shorthand_and_normalizes_whitespace_and_trailing_slashes() {
        assert_eq!(normalized("vercel-labs/skills"), "vercel-labs/skills");
        assert_eq!(normalized("  vercel-labs/skills  "), "vercel-labs/skills");
        assert_eq!(normalized("vercel-labs/skills/"), "vercel-labs/skills");
        assert_eq!(normalized("vercel-labs/skills.git"), "vercel-labs/skills");
    }

    #[test]
    fn accepts_https_github_urls_and_normalizes_them_to_owner_repo() {
        assert_eq!(
            normalized("https://github.com/vercel-labs/skills"),
            "vercel-labs/skills"
        );
        assert_eq!(
            normalized("https://github.com/vercel-labs/skills/"),
            "vercel-labs/skills"
        );
        assert_eq!(
            normalized("https://github.com/vercel-labs/skills.git"),
            "vercel-labs/skills"
        );
    }

    #[test]
    fn rejects_empty_and_whitespace_only_sources() {
        assert_eq!(code(""), ErrorCode::SourceMissing);
        assert_eq!(code("   "), ErrorCode::SourceMissing);
    }

    #[test]
    fn rejects_subdirectories() {
        assert_eq!(code("owner/repo/sub"), ErrorCode::SourceUnsupportedSubdir);
        assert_eq!(
            code("https://github.com/owner/repo/tree/main/sub"),
            ErrorCode::SourceUnsupportedSubdir
        );
    }

    #[test]
    fn rejects_refs_and_skill_filters() {
        assert_eq!(code("owner/repo#main"), ErrorCode::SourceUnsupportedRef);
        assert_eq!(
            code("https://github.com/owner/repo#v1"),
            ErrorCode::SourceUnsupportedRef
        );
        assert_eq!(code("owner/repo@skill"), ErrorCode::SourceInvalid);
    }

    #[test]
    fn rejects_non_github_providers_with_explicit_errors() {
        assert_eq!(
            code("https://gitlab.com/group/repo"),
            ErrorCode::SourceUnsupportedProvider
        );
        assert_eq!(
            code("gitlab.com/group/repo"),
            ErrorCode::SourceUnsupportedProvider
        );
        assert_eq!(
            code("git@github.com:owner/repo.git"),
            ErrorCode::SourceUnsupportedProvider
        );
        assert_eq!(
            code("ssh://git@github.com/owner/repo.git"),
            ErrorCode::SourceUnsupportedProvider
        );
        assert_eq!(
            code("https://www.notion.so/page"),
            ErrorCode::SourceUnsupportedProvider
        );
        assert_eq!(
            code("notion.site/page"),
            ErrorCode::SourceUnsupportedProvider
        );
    }

    #[test]
    fn rejects_local_paths() {
        for input in [
            ".",
            "..",
            "./skills",
            "../skills",
            "/tmp/skills",
            "~/skills",
        ] {
            assert_eq!(
                code(input),
                ErrorCode::SourceUnsupportedProvider,
                "{input} should be rejected"
            );
        }
    }

    #[test]
    fn rejects_malformed_github_inputs() {
        assert_eq!(code("https://github.com/owner"), ErrorCode::SourceInvalid);
        assert_eq!(
            code("https://github.com/owner/repo?tab=readme"),
            ErrorCode::SourceInvalid
        );
        assert_eq!(
            code("http://github.com/owner/repo"),
            ErrorCode::SourceUnsupportedProvider
        );
        assert_eq!(code("github.com/owner/repo"), ErrorCode::SourceInvalid);
        assert_eq!(code("owner"), ErrorCode::SourceInvalid);
        assert_eq!(code("owner//repo"), ErrorCode::SourceInvalid);
        assert_eq!(code("-owner/repo"), ErrorCode::SourceInvalid);
        assert_eq!(code("owner/-repo"), ErrorCode::SourceInvalid);
        assert_eq!(code("owner/re po"), ErrorCode::SourceInvalid);
    }

    #[test]
    fn rejects_shell_metacharacters_instead_of_letting_them_reach_a_process() {
        for input in [
            "owner/repo;rm -rf /",
            "owner/repo && echo pwned",
            "owner/repo | cat",
            "owner/repo$(id)",
            "owner/`id`",
            "owner/repo\nrm -rf /",
            "owner/repo\t--force",
            "owner/repo\0",
            "https://github.com/owner/repo;id",
            "https://github.com/owner/repo`id`",
        ] {
            let error = validate_source(input).unwrap_err();
            assert_eq!(error.code, ErrorCode::SourceInvalid, "{input:?}");
        }
    }
}
