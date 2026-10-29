//! CLI output classification shared by install, update, uninstall and preview.
//!
//! Classification turns stdout/stderr/exit into a `CliFailure`; the action
//! mapper owns the existing action-specific error codes and copy. `ExecError`
//! mapping lives here too so preview and the mutation session share one map.

use crate::process::{ExecError, ExecOutput};

use super::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliAction {
    Install,
    Update,
    Uninstall,
    Preview,
}

impl CliAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Install => "安装",
            Self::Update => "更新",
            Self::Uninstall => "卸载",
            Self::Preview => "预览",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliFailure {
    Network { detail: String },
    Auth { detail: String },
    ExitNonzero { detail: String },
    Partial { detail: String },
    InvalidJson { detail: String },
}

/// Classify add/update/preview output. `detail` is the CLI JSON fatal payload
/// or preview stdout; `stderr` is appended as before.
pub fn classify_add_failure(detail: &str, stderr: &str) -> CliFailure {
    let combined = if stderr.trim().is_empty() {
        detail.trim().to_string()
    } else if detail.trim().is_empty() {
        stderr.trim().to_string()
    } else {
        format!("{}\n{}", detail.trim(), stderr.trim())
    };
    let detail = truncate_detail(&combined);
    let lower = detail.to_lowercase();
    if lower.contains("failed to clone")
        || lower.contains("unable to access")
        || lower.contains("couldn't connect")
        || lower.contains("failed to connect")
        || lower.contains("could not resolve")
        || lower.contains("network is unreachable")
    {
        CliFailure::Network { detail }
    } else if lower.contains("authentication failed") {
        CliFailure::Auth { detail }
    } else {
        CliFailure::ExitNonzero { detail }
    }
}

/// Classify `skills remove` output. `None` means the text itself reports no
/// failure; disk verification still decides success afterwards.
pub fn classify_remove_failure(output: &ExecOutput) -> Option<CliFailure> {
    let combined = if output.stderr_trimmed().is_empty() {
        output.stdout_trimmed().to_string()
    } else if output.stdout_trimmed().is_empty() {
        output.stderr_trimmed().to_string()
    } else {
        format!("{}\n{}", output.stdout_trimmed(), output.stderr_trimmed())
    };
    let lower = combined.to_lowercase();
    let failed = !output.success()
        || lower.contains("failed to remove")
        || lower.contains("could not remove");
    if !failed {
        return None;
    }
    let detail = truncate_detail(&combined);
    if !output.success() {
        if detail.is_empty() {
            Some(CliFailure::ExitNonzero {
                detail: "skills remove 以非零退出码结束且未输出详情。".to_string(),
            })
        } else {
            Some(CliFailure::ExitNonzero { detail })
        }
    } else if detail.is_empty() {
        Some(CliFailure::InvalidJson {
            detail: "skills remove 未返回任何输出。".to_string(),
        })
    } else {
        Some(CliFailure::Partial { detail })
    }
}

impl CliFailure {
    pub fn to_app_error(self, action: CliAction) -> AppError {
        match self {
            Self::Network { detail } => AppError::network_fetch_failed(detail),
            Self::Auth { detail } => match action {
                CliAction::Preview => {
                    AppError::source_preview_failed(format!("仓库不存在或无法公开访问：{detail}"))
                }
                CliAction::Install | CliAction::Update | CliAction::Uninstall => {
                    AppError::cli_exit_nonzero(detail)
                }
            },
            Self::ExitNonzero { detail } => match action {
                CliAction::Install | CliAction::Update if detail.is_empty() => {
                    AppError::cli_exit_nonzero("skills CLI 以非零退出码结束且未输出详情。")
                }
                CliAction::Install
                | CliAction::Update
                | CliAction::Uninstall
                | CliAction::Preview => AppError::cli_exit_nonzero(detail),
            },
            Self::Partial { detail } => AppError::cli_partial_failure(detail),
            Self::InvalidJson { detail } => AppError::cli_invalid_json(detail),
        }
    }
}

pub fn classify_exec_error(action: CliAction, error: ExecError) -> AppError {
    match error {
        ExecError::Cancelled => AppError::cli_cancelled(),
        ExecError::Timeout(timeout) => AppError::cli_timeout(action.label(), timeout),
        ExecError::Spawn { program, message } => {
            AppError::cli_spawn_failed(format!("无法启动 {program}: {message}"))
        }
        ExecError::Io(message) => AppError::cli_failed(message),
    }
}

fn truncate_detail(value: &str) -> String {
    value.chars().take(16 * 1024).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use std::time::Duration;

    #[test]
    fn add_failures_classify_network_auth_and_exit() {
        assert!(matches!(
            classify_add_failure("fatal: unable to access repository", ""),
            CliFailure::Network { .. }
        ));
        assert!(matches!(
            classify_add_failure("Authentication failed for repo", ""),
            CliFailure::Auth { .. }
        ));
        let plain = classify_add_failure("no matching skill", "");
        assert_eq!(
            plain.to_app_error(CliAction::Install).code,
            ErrorCode::CliExitNonzero
        );
        let auth = classify_add_failure("authentication failed", "");
        assert_eq!(
            auth.clone().to_app_error(CliAction::Preview).code,
            ErrorCode::SourcePreviewFailed
        );
        assert_eq!(
            auth.to_app_error(CliAction::Update).code,
            ErrorCode::CliExitNonzero
        );
        assert_eq!(
            CliFailure::ExitNonzero {
                detail: String::new()
            }
            .to_app_error(CliAction::Install)
            .detail
            .as_deref(),
            Some("skills CLI 以非零退出码结束且未输出详情。")
        );
        assert_eq!(
            CliFailure::InvalidJson {
                detail: "bad output".to_string()
            }
            .to_app_error(CliAction::Uninstall)
            .code,
            ErrorCode::CliInvalidJson
        );
    }

    #[test]
    fn remove_failures_preserve_partial_invalid_and_exit() {
        let output = |code: i32, stdout: &str, stderr: &str| ExecOutput {
            code: Some(code),
            signal: None,
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            duration: Duration::ZERO,
        };
        assert_eq!(classify_remove_failure(&output(0, "removed", "")), None);
        assert!(matches!(
            classify_remove_failure(&output(1, "", "")),
            Some(CliFailure::ExitNonzero { .. })
        ));
        assert!(matches!(
            classify_remove_failure(&output(0, "could not remove alpha", "")),
            Some(CliFailure::Partial { .. })
        ));
        assert_eq!(classify_remove_failure(&output(0, "", "")), None);
    }

    #[test]
    fn exec_errors_map_once_with_the_action_label() {
        let timeout = classify_exec_error(
            CliAction::Update,
            ExecError::Timeout(Duration::from_secs(5)),
        );
        assert_eq!(timeout.code, ErrorCode::CliTimeout);
        assert!(timeout.message.contains("更新"));
        let spawn = classify_exec_error(
            CliAction::Install,
            ExecError::Spawn {
                program: "npx".to_string(),
                message: "not found".to_string(),
            },
        );
        assert_eq!(spawn.code, ErrorCode::CliSpawnFailed);
        assert_eq!(
            classify_exec_error(CliAction::Preview, ExecError::Cancelled).code,
            ErrorCode::CliCancelled
        );
    }
}
