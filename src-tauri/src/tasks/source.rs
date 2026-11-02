//! Validation and normalization for install sources.
//!
//! Public HTTPS root repositories and GitHub `owner/repo` shorthand.
//! Generic URLs end in `.git` so the pinned CLI treats them as Git sources.

use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSpec {
    pub normalized: String,
    pub kind: SourceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    GitHub,
    Git,
}

impl SourceSpec {
    /// GitHub paths are case insensitive; other Git services may distinguish them.
    pub fn identity(&self) -> String {
        match self.kind {
            SourceKind::GitHub => self.normalized.to_ascii_lowercase(),
            SourceKind::Git => self.normalized.clone(),
        }
    }
}

pub fn source_identity(source: &str) -> String {
    validate_source(source)
        .map(|spec| spec.identity())
        .unwrap_or_else(|_| source.to_string())
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

    if value
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"))
    {
        return parse_https(value);
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

fn parse_https(value: &str) -> Result<SourceSpec, AppError> {
    let mut url =
        reqwest::Url::parse(value).map_err(|_| AppError::source_invalid("HTTPS URL 格式无效"))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AppError::source_invalid("仓库 URL 不支持内嵌凭据"));
    }
    if url.query().is_some() {
        return Err(AppError::source_invalid("不支持 URL 查询参数"));
    }
    let host = url
        .host_str()
        .ok_or_else(|| AppError::source_invalid("URL 缺少主机名"))?;
    if matches!(
        host,
        "notion.so"
            | "www.notion.so"
            | "notion.site"
            | "www.notion.site"
            | "raw.githubusercontent.com"
    ) {
        return Err(AppError::source_unsupported_provider(host));
    }
    let path = value[8..]
        .split_once('/')
        .map(|(_, path)| path)
        .unwrap_or("");
    if matches!(host, "github.com" | "www.github.com") {
        if url.port().is_some() {
            return Err(AppError::source_invalid("GitHub URL 不支持自定义端口"));
        }
        return parse_github_path(path);
    }
    let cleaned = path.trim_end_matches('/');
    let segments: Vec<_> = cleaned.split('/').collect();
    if segments
        .iter()
        .any(|segment| !is_valid_component(segment) || matches!(*segment, "." | ".."))
    {
        return Err(AppError::source_invalid("仓库路径包含空段或无效字符"));
    }
    if segments
        .windows(2)
        .any(|pair| pair == ["-", "tree"] || pair == ["-", "blob"])
        || (host == "gitee.com" && segments.len() >= 4 && matches!(segments[2], "tree" | "blob"))
    {
        return Err(AppError::source_unsupported_subdir());
    }
    // The pinned parser recognizes these hosts with unanchored regexes.
    // Do not let an embedded host name redirect a generic source elsewhere.
    if value.contains("github.com/") || (host != "gitlab.com" && value.contains("gitlab.com/")) {
        return Err(AppError::source_invalid("来源会被 CLI 识别为另一 Git 主机"));
    }
    let git_path = if cleaned.ends_with(".git") {
        cleaned.to_string()
    } else {
        format!("{cleaned}.git")
    };
    url.set_path(&format!("/{git_path}"));
    Ok(SourceSpec {
        normalized: url.to_string(),
        kind: SourceKind::Git,
    })
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
        kind: SourceKind::GitHub,
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
    fn accepts_https_git_sources_with_nested_paths_and_ports() {
        for input in [
            "https://cnb.cool/cnb/skills/cnb-skill",
            "https://cnb.cool/cnb/skills/cnb-skill/",
            "https://cnb.cool/cnb/skills/cnb-skill.git",
        ] {
            assert_eq!(
                normalized(input),
                "https://cnb.cool/cnb/skills/cnb-skill.git"
            );
        }
        assert_eq!(
            normalized("https://gitlab.com/group/subgroup/Skills"),
            "https://gitlab.com/group/subgroup/Skills.git"
        );
        assert_eq!(
            normalized("https://GIT.example.com:8443/Group/Skills"),
            "https://git.example.com:8443/Group/Skills.git"
        );
        assert_eq!(
            normalized("https://gitee.com/owner/repo"),
            "https://gitee.com/owner/repo.git"
        );
        assert_eq!(
            source_identity("https://git.example.com/Group/Skills"),
            "https://git.example.com/Group/Skills.git"
        );
        assert_ne!(
            source_identity("https://git.example.com/Group/Skills"),
            source_identity("https://git.example.com/group/skills")
        );
    }

    #[test]
    fn rejects_malformed_or_ambiguous_git_urls_before_spawn() {
        for input in [
            "https://git.example.com",
            "https://user:secret@git.example.com/repo",
            "https://git.example.com/a/../repo",
            "https://git.example.com/a//repo",
            "https://git.example.com/repo?x=1",
            "https://git.example.com/re po",
            "https://git.example.com/%2e%2e/repo",
            "https://git.example.com/github.com/owner/repo",
            "https://evilgitlab.com/owner/repo",
        ] {
            assert_eq!(code(input), ErrorCode::SourceInvalid, "{input}");
        }
        assert_eq!(
            code("https://gitlab.com/group/repo/-/tree/main/skill"),
            ErrorCode::SourceUnsupportedSubdir
        );
        assert_eq!(
            code("https://gitee.com/group/repo/tree/main"),
            ErrorCode::SourceUnsupportedSubdir
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
    fn rejects_unsupported_providers_with_explicit_errors() {
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
