//! Structured application error returned across the Tauri IPC boundary.
//!
//! `ErrorCode` owns the stable dotted code strings; named constructors own the
//! user-facing copy, hint and retryability. The frontend only displays the
//! normalized fields and branches on the `lock.*` family.

use serde::Serialize;

pub mod cli;
mod code;
mod constructors;

pub use cli::{CliAction, CliFailure};
pub use code::ErrorCode;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    pub retryable: bool,
}

impl AppError {
    fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            detail: None,
            hint: None,
            retryable: false,
        }
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for AppError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_missing_maps_to_env_domain_and_is_retryable() {
        let err = AppError::env_node_missing("tried: override, volta, fnm, nvm");
        assert_eq!(err.code, ErrorCode::EnvNodeMissing);
        assert!(err.retryable);
        assert!(err.message.contains("未找到 Node.js"));
        assert_eq!(
            err.detail.as_deref(),
            Some("tried: override, volta, fnm, nvm")
        );
        assert!(err.hint.is_some());
    }

    #[test]
    fn node_too_old_reports_current_version_and_requirement() {
        let err = AppError::env_node_too_old("v20.11.0", "/opt/node/bin/node");
        assert_eq!(err.code, ErrorCode::EnvNodeTooOld);
        assert!(err.message.contains("v20.11.0"));
        assert!(err.message.contains("22.20"));
        assert_eq!(
            err.detail.as_deref(),
            Some("/opt/node/bin/node（v20.11.0）")
        );
    }

    #[test]
    fn tool_errors_use_expected_codes() {
        assert_eq!(
            AppError::env_npx_missing("x").code,
            ErrorCode::EnvNpxMissing
        );
        assert_eq!(
            AppError::env_git_missing("x").code,
            ErrorCode::EnvGitMissing
        );
        assert_eq!(
            AppError::env_cli_missing("x").code,
            ErrorCode::EnvCliMissing
        );
        assert_eq!(
            AppError::env_version_unsupported("1.7.4", "1.8.0").code,
            ErrorCode::EnvVersionUnsupported
        );
        assert_eq!(AppError::cli_failed("x").code, ErrorCode::CliFailed);
        assert_eq!(
            AppError::cli_spawn_failed("x").code,
            ErrorCode::CliSpawnFailed
        );
        assert_eq!(
            AppError::cli_timeout("安装", std::time::Duration::from_secs(9)).code,
            ErrorCode::CliTimeout
        );
        assert_eq!(
            AppError::cli_exit_nonzero("x").code,
            ErrorCode::CliExitNonzero
        );
        assert_eq!(
            AppError::cli_invalid_json("x").code,
            ErrorCode::CliInvalidJson
        );
        assert_eq!(
            AppError::cli_partial_failure("x").code,
            ErrorCode::CliPartialFailure
        );
        assert_eq!(AppError::task_cancelled().code, ErrorCode::TaskCancelled);
        assert_eq!(AppError::cli_cancelled().code, ErrorCode::CliCancelled);
        assert_eq!(
            AppError::skill_ambiguous("alpha", "x").code,
            ErrorCode::SkillAmbiguous
        );
        assert_eq!(
            AppError::verify_lock_mismatch("alpha", "acme/skills", "other/repo").code,
            ErrorCode::VerifyLockMismatch
        );
        assert_eq!(
            AppError::lock_corrupt("bad json").code,
            ErrorCode::LockCorrupt
        );
        assert_eq!(
            AppError::lock_unsupported_version(2).code,
            ErrorCode::LockUnsupportedVersion
        );
        assert_eq!(
            AppError::lock_read_failed("denied").code,
            ErrorCode::LockReadFailed
        );
        assert_eq!(
            AppError::path_app_data("denied").code,
            ErrorCode::PathAppData
        );
        assert_eq!(
            AppError::skill_not_found("alpha").code,
            ErrorCode::SkillNotFound
        );
        assert_eq!(
            AppError::skill_read_failed("denied").code,
            ErrorCode::SkillReadFailed
        );
        assert_eq!(AppError::path_invalid("x").code, ErrorCode::PathInvalid);
        assert_eq!(
            AppError::path_outside_allowed_root("x").code,
            ErrorCode::PathOutsideAllowedRoot
        );
        assert_eq!(
            AppError::project_not_found("id").code,
            ErrorCode::ProjectNotFound
        );
        assert_eq!(
            AppError::project_invalid_root("x").code,
            ErrorCode::ProjectInvalidRoot
        );
        assert_eq!(
            AppError::project_root_unavailable("alpha", "x").code,
            ErrorCode::ProjectRootUnavailable
        );
        assert_eq!(
            AppError::project_duplicate(std::path::Path::new("/a"), "alpha").code,
            ErrorCode::ProjectDuplicate
        );
        assert_eq!(
            AppError::project_registry_corrupt("x").code,
            ErrorCode::ProjectRegistryCorrupt
        );
        assert_eq!(AppError::open_failed("x").code, ErrorCode::OpenFailed);
        assert_eq!(
            AppError::agent_not_attached("Claude Code").code,
            ErrorCode::AgentNotAttached
        );
        assert_eq!(AppError::agent_native("Codex").code, ErrorCode::AgentNative);
        assert_eq!(
            AppError::agent_broken("Gemini CLI", std::path::Path::new("/x")).code,
            ErrorCode::AgentBroken
        );
        assert_eq!(
            AppError::agent_dependency_conflict("Claude Code", &["Gemini CLI".to_string()]).code,
            ErrorCode::AgentDependencyConflict
        );
        assert_eq!(
            AppError::agent_detach_failed("denied").code,
            ErrorCode::AgentDetachFailed
        );
        assert_eq!(
            AppError::network_offline("x").code,
            ErrorCode::NetworkOffline
        );
        assert_eq!(
            AppError::network_rate_limited("x").code,
            ErrorCode::NetworkRateLimited
        );
        assert_eq!(AppError::network_error("x").code, ErrorCode::NetworkError);
        assert_eq!(
            AppError::agent_dependency_conflict("Claude Code", &["Qoder".to_string()]).code,
            ErrorCode::AgentDependencyConflict
        );
        assert_eq!(
            AppError::verify_canonical_present("alpha", std::path::Path::new("/a")).code,
            ErrorCode::VerifyCanonicalPresent
        );
        assert_eq!(
            AppError::verify_attachment_present("alpha", "Qoder", std::path::Path::new("/a")).code,
            ErrorCode::VerifyAttachmentPresent
        );
        assert_eq!(
            AppError::verify_lock_present("alpha").code,
            ErrorCode::VerifyLockPresent
        );
    }

    #[test]
    fn serializes_camel_case_and_omits_absent_optional_fields() {
        let err = AppError::new(ErrorCode::EnvNodeMissing, "未找到 Node.js");
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["code"], ErrorCode::EnvNodeMissing.as_str());
        assert_eq!(json["message"], "未找到 Node.js");
        assert_eq!(json["retryable"], false);
        assert!(json.get("detail").is_none());
        assert!(json.get("hint").is_none());
    }
}
