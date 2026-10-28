//! Structured application error returned across the Tauri IPC boundary.
//!
//! The frontend only ever displays the normalized `message`; `detail` carries
//! diagnostic output (for example CLI stderr) and `hint` carries an actionable
//! next step for the user.

use serde::Serialize;

/// Error code constants grouped by domain. Keep the string values stable —
/// the frontend and tests may branch on them.
pub mod codes {
    pub const ENV_NODE_MISSING: &str = "env.node_missing";
    pub const ENV_NODE_TOO_OLD: &str = "env.node_too_old";
    pub const ENV_NODE_PROBE_FAILED: &str = "env.node_probe_failed";
    pub const ENV_NODE_PROBE_TIMEOUT: &str = "env.node_probe_timeout";
    pub const ENV_OVERRIDE_INVALID: &str = "env.override_invalid";
    pub const ENV_NPX_MISSING: &str = "env.npx_missing";
    pub const ENV_GIT_MISSING: &str = "env.git_missing";
    pub const ENV_CLI_MISSING: &str = "env.cli_missing";
    pub const LOCK_CORRUPT: &str = "lock.corrupt";
    pub const LOCK_UNSUPPORTED_VERSION: &str = "lock.unsupported_version";
    pub const LOCK_READ_FAILED: &str = "lock.read_failed";
    pub const SKILL_NOT_FOUND: &str = "skill.not_found";
    pub const SKILL_READ_FAILED: &str = "skill.read_failed";
    pub const PATH_APP_DATA: &str = "path.app_data";
    pub const PATH_INVALID: &str = "path.invalid";
    pub const PATH_OUTSIDE_ALLOWED_ROOT: &str = "path.outside_allowed_root";
    pub const OPEN_FAILED: &str = "open.failed";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    pub retryable: bool,
}

impl AppError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
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

    pub fn env_node_missing(detail: impl Into<String>) -> Self {
        Self::new(
            codes::ENV_NODE_MISSING,
            "未找到 Node.js。Skills Box 需要 Node 22.20 或更高版本。",
        )
        .detail(detail)
        .hint("请安装 Node.js 22.20 或更高版本，然后重新检测。")
        .retryable(true)
    }

    pub fn env_node_too_old(version: &str, path: &str) -> Self {
        Self::new(
            codes::ENV_NODE_TOO_OLD,
            format!("Node 版本过低（当前 {version}），需要 22.20 或更高版本。"),
        )
        .detail(format!("{path}（{version}）"))
        .hint("请升级 Node.js 到 22.20 或更高版本，然后重新检测。")
        .retryable(true)
    }

    pub fn env_node_probe_failed(detail: impl Into<String>) -> Self {
        Self::new(
            codes::ENV_NODE_PROBE_FAILED,
            "Node 版本探测失败，无法确认运行环境。",
        )
        .detail(detail)
        .hint("请确认该 Node 可正常执行 `node --version`，然后重新检测。")
        .retryable(true)
    }

    pub fn env_node_probe_timeout(detail: impl Into<String>) -> Self {
        Self::new(codes::ENV_NODE_PROBE_TIMEOUT, "Node 版本探测超时。")
            .detail(detail)
            .hint("请检查 Node 安装是否卡住，然后重新检测。")
            .retryable(true)
    }

    pub fn env_override_invalid(detail: impl Into<String>) -> Self {
        Self::new(
            codes::ENV_OVERRIDE_INVALID,
            "指定的 Node 路径不可用，已停止回退探测。",
        )
        .detail(detail)
        .hint("请检查 SKILLS_BOX_NODE 是否指向可执行的 Node 22.20 或更高版本。")
    }

    pub fn env_npx_missing(detail: impl Into<String>) -> Self {
        Self::new(codes::ENV_NPX_MISSING, "未找到 npx。")
            .detail(detail)
            .hint("npx 随 Node.js 一起提供，请检查 Node.js 安装是否完整。")
    }

    pub fn env_git_missing(detail: impl Into<String>) -> Self {
        Self::new(codes::ENV_GIT_MISSING, "未找到 git。")
            .detail(detail)
            .hint("请安装 Xcode Command Line Tools（xcode-select --install）或 git。")
    }

    pub fn env_cli_missing(detail: impl Into<String>) -> Self {
        Self::new(
            codes::ENV_CLI_MISSING,
            "skills CLI 不可用（npx skills@1.7.4）。",
        )
        .detail(detail)
        .hint("首次运行需要联网下载 skills@1.7.4，请检查网络后重新检测。")
        .retryable(true)
    }

    pub fn lock_corrupt(detail: impl Into<String>) -> Self {
        Self::new(codes::LOCK_CORRUPT, "Skill lock 文件损坏，无法解析。")
            .detail(detail)
            .hint("请检查 ~/.agents/.skill-lock.json，或从备份恢复后再试。")
    }

    pub fn lock_unsupported_version(version: u64) -> Self {
        Self::new(
            codes::LOCK_UNSUPPORTED_VERSION,
            format!("Skill lock 版本不受支持（v{version}），需要 v3。"),
        )
        .detail(format!("version = {version}"))
        .hint("请使用 skills@1.7.4 重建 lock，或从备份恢复。")
    }

    pub fn lock_read_failed(detail: impl Into<String>) -> Self {
        Self::new(codes::LOCK_READ_FAILED, "无法读取 Skill lock 文件。")
            .detail(detail)
            .hint("请检查 ~/.agents/.skill-lock.json 的权限。")
    }

    pub fn path_app_data(detail: impl Into<String>) -> Self {
        Self::new(codes::PATH_APP_DATA, "无法创建应用数据目录。")
            .detail(detail)
            .hint("请检查磁盘空间与目录权限，然后重新检测。")
    }

    pub fn skill_not_found(name: &str) -> Self {
        Self::new(
            codes::SKILL_NOT_FOUND,
            format!("找不到 skill「{name}」的目录。"),
        )
        .hint("请刷新库存后重试；如果目录已被外部删除，列表会自动更新。")
    }

    pub fn skill_read_failed(detail: impl Into<String>) -> Self {
        Self::new(codes::SKILL_READ_FAILED, "读取 skill 详情失败。")
            .detail(detail)
            .hint("请检查该 skill 目录的权限后重试。")
            .retryable(true)
    }

    pub fn path_invalid(detail: impl Into<String>) -> Self {
        Self::new(codes::PATH_INVALID, "无效的 skill 标识或路径。").detail(detail)
    }

    pub fn path_outside_allowed_root(detail: impl Into<String>) -> Self {
        Self::new(
            codes::PATH_OUTSIDE_ALLOWED_ROOT,
            "路径不在允许的 skill 目录内，已拒绝打开。",
        )
        .detail(detail)
        .hint("只允许打开 canonical store 或已知 agent 目录中的 skill。")
    }

    pub fn open_failed(detail: impl Into<String>) -> Self {
        Self::new(codes::OPEN_FAILED, "无法打开文件或目录。")
            .detail(detail)
            .hint("请检查系统默认应用与文件权限。")
            .retryable(true)
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
        assert_eq!(err.code, codes::ENV_NODE_MISSING);
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
        assert_eq!(err.code, codes::ENV_NODE_TOO_OLD);
        assert!(err.message.contains("v20.11.0"));
        assert!(err.message.contains("22.20"));
        assert_eq!(
            err.detail.as_deref(),
            Some("/opt/node/bin/node（v20.11.0）")
        );
    }

    #[test]
    fn tool_errors_use_expected_codes() {
        assert_eq!(AppError::env_npx_missing("x").code, codes::ENV_NPX_MISSING);
        assert_eq!(AppError::env_git_missing("x").code, codes::ENV_GIT_MISSING);
        assert_eq!(AppError::env_cli_missing("x").code, codes::ENV_CLI_MISSING);
        assert_eq!(AppError::lock_corrupt("bad json").code, codes::LOCK_CORRUPT);
        assert_eq!(
            AppError::lock_unsupported_version(2).code,
            codes::LOCK_UNSUPPORTED_VERSION
        );
        assert_eq!(
            AppError::lock_read_failed("denied").code,
            codes::LOCK_READ_FAILED
        );
        assert_eq!(AppError::path_app_data("denied").code, codes::PATH_APP_DATA);
        assert_eq!(
            AppError::skill_not_found("alpha").code,
            codes::SKILL_NOT_FOUND
        );
        assert_eq!(
            AppError::skill_read_failed("denied").code,
            codes::SKILL_READ_FAILED
        );
        assert_eq!(AppError::path_invalid("x").code, codes::PATH_INVALID);
        assert_eq!(
            AppError::path_outside_allowed_root("x").code,
            codes::PATH_OUTSIDE_ALLOWED_ROOT
        );
        assert_eq!(AppError::open_failed("x").code, codes::OPEN_FAILED);
    }

    #[test]
    fn serializes_camel_case_and_omits_absent_optional_fields() {
        let err = AppError::new("env.node_missing", "未找到 Node.js");
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["code"], "env.node_missing");
        assert_eq!(json["message"], "未找到 Node.js");
        assert_eq!(json["retryable"], false);
        assert!(json.get("detail").is_none());
        assert!(json.get("hint").is_none());
    }
}
