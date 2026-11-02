//! Semantic error constructors: the taxonomy owns code, copy, hint and
//! retryability; call sites only provide contextual detail.

use super::{AppError, ErrorCode};

impl AppError {
    pub fn discovery_read_failed() -> Self {
        Self::new(
            ErrorCode::DiscoveryReadFailed,
            "发现历史无法读取，原文件已保留。",
        )
        .hint("请修复或恢复 app data 中的 discovery.json 后重启应用。")
    }
    pub fn discovery_write_failed() -> Self {
        Self::new(
            ErrorCode::DiscoveryWriteFailed,
            "发现记录保存失败，本次操作未保存。",
        )
        .retryable(true)
    }

    pub fn update_state_cleanup_failed(detail: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::UpdateStateCleanupFailed,
            "项目变更已保存，但旧更新检查状态未能清理。",
        )
        .detail(detail)
        .hint("旧状态已失效，不会显示；可稍后重试更新检查。")
    }

    pub fn env_node_missing(detail: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::EnvNodeMissing,
            "未找到 Node.js。Skills Box 需要 Node 22.20 或更高版本。",
        )
        .detail(detail)
        .hint("请安装 Node.js 22.20 或更高版本，然后重新检测。")
        .retryable(true)
    }

    pub fn env_node_too_old(version: &str, path: &str) -> Self {
        Self::new(
            ErrorCode::EnvNodeTooOld,
            format!("Node 版本过低（当前 {version}），需要 22.20 或更高版本。"),
        )
        .detail(format!("{path}（{version}）"))
        .hint("请升级 Node.js 到 22.20 或更高版本，然后重新检测。")
        .retryable(true)
    }

    pub fn env_node_probe_failed(detail: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::EnvNodeProbeFailed,
            "Node 版本探测失败，无法确认运行环境。",
        )
        .detail(detail)
        .hint("请确认该 Node 可正常执行 `node --version`，然后重新检测。")
        .retryable(true)
    }

    pub fn env_node_probe_timeout(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::EnvNodeProbeTimeout, "Node 版本探测超时。")
            .detail(detail)
            .hint("请检查 Node 安装是否卡住，然后重新检测。")
            .retryable(true)
    }

    pub fn env_override_invalid(detail: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::EnvOverrideInvalid,
            "指定的 Node 路径不可用，已停止回退探测。",
        )
        .detail(detail)
        .hint("请检查 SKILLS_BOX_NODE 是否指向可执行的 Node 22.20 或更高版本。")
    }

    pub fn env_npx_missing(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::EnvNpxMissing, "未找到 npx。")
            .detail(detail)
            .hint("npx 随 Node.js 一起提供，请检查 Node.js 安装是否完整。")
    }

    pub fn env_git_missing(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::EnvGitMissing, "未找到 git。")
            .detail(detail)
            .hint("请安装 Xcode Command Line Tools（xcode-select --install）或 git。")
    }

    pub fn env_cli_missing(detail: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::EnvCliMissing,
            "skills CLI 不可用（npx skills@1.7.4）。",
        )
        .detail(detail)
        .hint("首次运行需要联网下载 skills@1.7.4，请检查网络后重新检测。")
        .retryable(true)
    }

    /// The pinned CLI answered with a different version than `skills@1.7.4`.
    /// This is an environment incompatibility, not a transient network error.
    pub fn env_version_unsupported(expected: &str, actual: &str) -> Self {
        Self::new(
            ErrorCode::EnvVersionUnsupported,
            format!("skills CLI 版本不受支持（期望 {expected}，实际 {actual}）。"),
        )
        .detail(format!("expected = {expected}, actual = {actual}"))
        .hint("请确认 npx 解析到 skills@1.7.4；本应用固定使用该版本。")
        .retryable(true)
    }

    pub fn lock_corrupt(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::LockCorrupt, "Skill lock 文件损坏，无法解析。")
            .detail(detail)
            .hint("请检查 ~/.agents/.skill-lock.json，或从备份恢复后再试。")
    }

    pub fn lock_unsupported_version(version: u64) -> Self {
        Self::new(
            ErrorCode::LockUnsupportedVersion,
            format!("Skill lock 版本不受支持（v{version}），需要 v3。"),
        )
        .detail(format!("version = {version}"))
        .hint("请使用 skills@1.7.4 重建 lock，或从备份恢复。")
    }

    pub fn lock_read_failed(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::LockReadFailed, "无法读取 Skill lock 文件。")
            .detail(detail)
            .hint("请检查 ~/.agents/.skill-lock.json 的权限。")
    }

    pub fn path_app_data(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::PathAppData, "无法创建应用数据目录。")
            .detail(detail)
            .hint("请检查磁盘空间与目录权限，然后重新检测。")
    }

    pub fn skill_not_found(name: &str) -> Self {
        Self::new(
            ErrorCode::SkillNotFound,
            format!("找不到 skill「{name}」的目录。"),
        )
        .hint("请刷新库存后重试；如果目录已被外部删除，列表会自动更新。")
    }

    /// The inventory id resolves to more than one physical location with
    /// conflicting content, so no single folder can be chosen safely.
    pub fn skill_ambiguous(name: &str, detail: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::SkillAmbiguous,
            format!("skill「{name}」对应多个位置，无法安全判断操作目标。"),
        )
        .detail(detail)
        .hint("请先手动整理该 skill 的 canonical 与 agent 副本，再刷新库存。")
    }

    pub fn skill_read_failed(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::SkillReadFailed, "读取 skill 详情失败。")
            .detail(detail)
            .hint("请检查该 skill 目录的权限后重试。")
            .retryable(true)
    }

    pub fn skill_read_timeout(what: &str, timeout: std::time::Duration) -> Self {
        Self::new(
            ErrorCode::SkillReadTimeout,
            format!("读取{what}超时（{} 秒）。", timeout.as_secs()),
        )
        .hint("请检查磁盘或网络状态，然后重新加载。")
        .retryable(true)
    }

    pub fn skill_unmanaged(name: &str) -> Self {
        Self::new(
            ErrorCode::SkillUnmanaged,
            format!("skill「{name}」没有 lock 条目，不可更新或卸载（断开亦不可用）。"),
        )
        .hint("未受管 skill 只能只读展示或手动处理，请刷新库存确认状态。")
    }

    pub fn skill_copy_only(name: &str) -> Self {
        Self::new(
            ErrorCode::SkillCopyOnly,
            format!("skill「{name}」只有 agent 副本，没有共享目录，无法更新或断开。"),
        )
        .hint("v1 不支持 copy-only 更新或断开；请先卸载，再重新安装。")
    }

    pub fn skill_source_unresolvable(name: &str, detail: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::SkillSourceUnresolvable,
            format!("无法从 lock 解析 skill「{name}」的来源，已跳过更新。"),
        )
        .detail(detail)
        .hint("只支持公开 HTTPS Git 来源；可重新安装该 skill 以恢复来源记录。")
    }

    pub fn update_local_modified(name: &str, detail: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::UpdateLocalModified,
            format!("skill「{name}」存在本地改动，更新前需要确认。"),
        )
        .detail(detail)
        .hint("更新会覆盖本地改动；操作前已要求确认并创建备份。")
    }

    pub fn update_copy_to_symlink(name: &str, agents: &str) -> Self {
        Self::new(
            ErrorCode::UpdateCopyToSymlink,
            format!("skill「{name}」在 {agents} 上是副本，更新后将转为符号链接。"),
        )
        .detail(format!("更新前已备份这些副本目录：{agents}；可自行恢复。"))
    }

    pub fn path_invalid(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::PathInvalid, "无效的 skill 标识或路径。").detail(detail)
    }

    pub fn path_outside_allowed_root(detail: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::PathOutsideAllowedRoot,
            "路径不在允许的 skill 目录内，已拒绝打开。",
        )
        .detail(detail)
        .hint("只允许打开 canonical store、已知 agent 目录或已注册项目根中的 skill。")
    }

    pub fn project_not_found(id: &str) -> Self {
        Self::new(ErrorCode::ProjectNotFound, format!("找不到项目「{id}」。"))
            .hint("请刷新项目列表；该项目可能已被遗忘。")
    }

    pub fn project_invalid_root(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::ProjectInvalidRoot, "不能使用该项目目录。")
            .detail(detail)
            .hint("请选择已存在、可读且不是 /、home 或全局 canonical store 的目录。")
    }

    pub fn project_root_unavailable(name: &str, detail: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::ProjectRootUnavailable,
            format!("项目「{name}」的路径不可用。"),
        )
        .detail(detail)
        .hint("请重新定位项目，或将其从列表中遗忘。")
    }

    pub fn project_duplicate(root: &std::path::Path, name: &str) -> Self {
        Self::new(
            ErrorCode::ProjectDuplicate,
            format!("目录已注册为项目「{name}」。"),
        )
        .detail(root.display().to_string())
        .hint("同一个目录只能注册一次；嵌套与重叠的不同目录仍可分别添加。")
    }

    pub fn project_registry_corrupt(detail: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::ProjectRegistryCorrupt,
            "项目注册表不可用，已停止写入。",
        )
        .detail(detail)
        .hint("原文件未被覆盖；请修复或移除应用数据中的 projects.json 后重试。")
    }

    /// Project locks are v1; the global `lock.unsupported_version` copy names
    /// the wrong version and path, so project scopes get their own code.
    pub fn project_lock_unsupported_version(version: u64) -> Self {
        Self::new(
            ErrorCode::ProjectLockUnsupported,
            format!("项目 Skill lock 版本不受支持（v{version}），需要 v1。"),
        )
        .detail(format!("version = {version}"))
        .hint("请使用 skills@1.7.4 在项目根重新生成 skills-lock.json。")
    }

    /// Same `lock.corrupt` family as the global lock so the frontend keeps one
    /// lock-error branch, with project-specific copy and hint.
    pub fn project_lock_corrupt(detail: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::LockCorrupt,
            "项目 Skill lock 文件损坏，无法解析。",
        )
        .detail(detail)
        .hint("请检查项目根目录的 skills-lock.json，或从备份恢复后再试。")
    }

    pub fn project_lock_read_failed(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::LockReadFailed, "无法读取项目 Skill lock 文件。")
            .detail(detail)
            .hint("请检查项目根目录 skills-lock.json 的权限。")
    }

    pub fn open_failed(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::OpenFailed, "无法打开文件或目录。")
            .detail(detail)
            .hint("请检查系统默认应用与文件权限。")
            .retryable(true)
    }

    /// External links are restricted to absolute https URLs. Retrying the same
    /// input cannot succeed, so this is not retryable.
    pub fn open_url_rejected(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::OpenFailed, "仅允许通过 opener 打开 https 链接。")
            .detail(detail)
            .hint("http、file、javascript、data、ssh、mailto 与相对路径都会被拒绝。")
            .retryable(false)
    }

    pub fn source_missing() -> Self {
        Self::new(
            ErrorCode::SourceMissing,
            "请输入公开 HTTPS Git 仓库，或 GitHub owner/repo。",
        )
        .hint("示例：vercel-labs/skills 或 https://cnb.cool/cnb/skills/cnb-skill")
    }

    pub fn source_invalid(detail: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::SourceInvalid,
            "来源格式无效。请输入 HTTPS Git 仓库地址或 GitHub owner/repo。",
        )
        .detail(detail)
        .hint("只接受公开 HTTPS Git 根仓库，且只安装默认分支。")
    }

    pub fn source_unsupported_subdir() -> Self {
        Self::new(
            ErrorCode::SourceUnsupportedSubdir,
            "仅支持公开 HTTPS Git 根仓库，不支持网页子目录链接。",
        )
        .hint("请去掉仓库后面的子目录，例如 owner/repo。")
    }

    pub fn source_unsupported_ref() -> Self {
        Self::new(
            ErrorCode::SourceUnsupportedRef,
            "仅安装默认分支，不支持 #ref（分支、标签或提交）。",
        )
        .hint("请去掉 #ref 后再预览。")
    }

    pub fn source_unsupported_provider(provider: &str) -> Self {
        Self::new(
            ErrorCode::SourceUnsupportedProvider,
            format!("不支持该来源（{provider}），仅支持公开 HTTPS Git 仓库。"),
        )
        .hint("请使用 HTTPS Git 仓库地址，或 GitHub owner/repo。")
    }

    pub fn source_preview_failed(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::SourcePreviewFailed, "无法解析仓库预览。")
            .detail(detail)
            .hint("可手动输入 skill 名，或重试预览。")
            .retryable(true)
    }

    pub fn network_fetch_failed(detail: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::NetworkFetchFailed,
            "无法访问仓库，请检查网络后重试。",
        )
        .detail(detail)
        .hint("确认仓库存在且可公开访问，然后重试预览。")
        .retryable(true)
    }

    /// Update checks never degrade to "up to date": a failed source keeps its
    /// previous result and surfaces this error instead.
    pub fn network_offline(detail: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::NetworkOffline,
            "无法连接 GitHub，已保留上次检查结果。",
        )
        .detail(detail)
        .hint("请检查网络或代理设置，然后再次手动检查更新。")
        .retryable(true)
    }

    pub fn network_rate_limited(detail: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::NetworkRateLimited,
            "GitHub 检查额度已用尽，已保留上次检查结果。",
        )
        .detail(detail)
        .hint("可在设置中配置 GitHub token，或等待额度恢复后再次手动检查更新。")
        .retryable(true)
    }

    pub fn network_auth_failed() -> Self {
        Self::new(ErrorCode::NetworkAuthFailed, "GitHub token 无效或已失效。")
            .hint("请在设置中替换 token；如果使用 GITHUB_TOKEN，请修改启动环境并重启应用。")
    }

    pub fn settings_invalid_token() -> Self {
        Self::new(ErrorCode::SettingsInvalidToken, "Token 格式无效。")
            .hint("请输入不含空白或控制字符的 GitHub token，长度不超过 4,096 字符。")
    }

    pub fn settings_read_failed() -> Self {
        Self::new(ErrorCode::SettingsReadFailed, "无法读取 GitHub 认证配置。")
            .hint("请在设置页重新保存 token 或删除已保存的配置。")
    }

    pub fn settings_write_failed() -> Self {
        Self::new(
            ErrorCode::SettingsWriteFailed,
            "无法保存 GitHub 认证配置，原配置已保留。",
        )
        .hint("请检查应用数据目录的权限后重试。")
        .retryable(true)
    }

    pub fn network_error(detail: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::NetworkError,
            "检查更新失败，已保留上次检查结果。",
        )
        .detail(detail)
        .hint("请确认仓库仍公开可访问，然后再次手动检查更新。")
        .retryable(true)
    }

    pub fn cli_failed(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::CliFailed, "skills CLI 执行失败。")
            .detail(detail)
            .hint("请检查 Node、npx 与 git 环境，然后重试。")
            .retryable(true)
    }

    pub fn cli_spawn_failed(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::CliSpawnFailed, "无法启动 skills CLI 进程。")
            .detail(detail)
            .hint("请检查 Node、npx 与 git 环境，然后重新检测。")
            .retryable(true)
    }

    pub fn cli_timeout(what: &str, timeout: std::time::Duration) -> Self {
        Self::new(
            ErrorCode::CliTimeout,
            format!("{what}超时（{} 秒）。", timeout.as_secs()),
        )
        .hint("请检查网络或磁盘状态，然后重试。")
        .retryable(true)
    }

    pub fn cli_exit_nonzero(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::CliExitNonzero, "skills CLI 以非零退出码结束。")
            .detail(detail)
            .hint("请查看详细信息中的 CLI 输出，处理后重试。")
            .retryable(true)
    }

    pub fn cli_invalid_json(detail: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::CliInvalidJson,
            "skills CLI 返回了无法解析的 JSON。",
        )
        .detail(detail)
        .hint("CLI 输出格式可能已变化；请确认固定使用 skills@1.7.4 后重试。")
        .retryable(true)
    }

    pub fn cli_partial_failure(detail: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::CliPartialFailure,
            "skills CLI 报告部分操作失败。",
        )
        .detail(detail)
        .hint("请查看详细信息中的剩余文件或条目标识，处理后重试。")
        .retryable(true)
    }

    /// The CLI process group was terminated because the task was cancelled or
    /// the application is shutting down. Task-level results still use
    /// `task.cancelled`; this code is what the operation log records for the
    /// interrupted CLI row.
    pub fn cli_cancelled() -> Self {
        Self::new(ErrorCode::CliCancelled, "skills CLI 进程已取消。").retryable(true)
    }

    pub fn task_busy() -> Self {
        Self::new(
            ErrorCode::TaskBusy,
            "已有变更任务正在执行，请等待完成后再试。",
        )
        .retryable(true)
    }

    pub fn task_not_found() -> Self {
        Self::new(ErrorCode::TaskNotFound, "任务不存在或已结束。").retryable(false)
    }

    pub fn task_cannot_cancel() -> Self {
        Self::new(
            ErrorCode::TaskCannotCancel,
            "任务已结束或已进入不可取消阶段。",
        )
        .retryable(false)
    }

    /// Terminal marker for a task the user cancelled.
    pub fn task_cancelled() -> Self {
        Self::new(ErrorCode::TaskCancelled, "任务已取消。").retryable(true)
    }

    pub fn task_shutting_down() -> Self {
        Self::new(
            ErrorCode::TaskShuttingDown,
            "应用正在退出，无法开始新的任务。",
        )
        .retryable(false)
    }

    pub fn skill_conflict(name: &str, detail: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::SkillConflict,
            format!("同名 skill「{name}」已存在，无法安装。"),
        )
        .detail(detail)
        .hint("请先卸载或重命名现有 skill，再重新安装。")
    }

    pub fn skill_invalid_name(detail: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::SkillInvalidName,
            "skill 名无效，请使用单个名称（不能以 - 或 * 开头）。",
        )
        .detail(detail)
        .hint("从仓库预览中选择 skill，或手动输入不带路径的名称。")
    }

    pub fn skill_not_in_source(name: &str, detail: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::SkillNotInSource,
            format!("来源中未找到 skill「{name}」。"),
        )
        .detail(detail)
        .hint("请重新预览仓库并选择存在的 skill。")
        .retryable(true)
    }

    pub fn skill_install_failed(name: &str, detail: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::SkillInstallFailed,
            format!("skill「{name}」安装失败。"),
        )
        .detail(detail)
        .hint("可以单独重试该 skill。")
        .retryable(true)
    }

    pub fn agent_invalid(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::AgentInvalid, "agent 标识无效。").detail(detail)
    }

    pub fn agent_unsupported_global(name: &str) -> Self {
        Self::new(
            ErrorCode::AgentUnsupportedGlobal,
            format!("agent「{name}」不支持全局安装。"),
        )
        .hint("请取消选择该 agent，或使用其项目级安装方式。")
    }

    pub fn agent_not_detected(name: &str) -> Self {
        Self::new(
            ErrorCode::AgentNotDetected,
            format!("未检测到 agent「{name}」，无法安装。"),
        )
        .hint("请刷新环境检测后重新选择 agent。")
        .retryable(true)
    }

    pub fn agent_not_attached(name: &str) -> Self {
        Self::new(
            ErrorCode::AgentNotAttached,
            format!("agent「{name}」没有挂载这个 skill，无法断开。"),
        )
        .hint("请刷新库存后重试。")
    }

    /// Universal/shared agents read the canonical store directly; there is no
    /// per-agent attachment to remove.
    pub fn agent_native(name: &str) -> Self {
        Self::new(
            ErrorCode::AgentNative,
            format!("agent「{name}」共享 canonical store，不提供断开。"),
        )
        .detail("universal / native 挂载没有独立目录，移除它不会改变该 agent 可见的 skill。")
        .hint("如需移除该 skill，请使用「彻底卸载」。")
    }

    pub fn agent_broken(name: &str, path: &std::path::Path) -> Self {
        Self::new(
            ErrorCode::AgentBroken,
            format!("agent「{name}」的挂载是断链，只读提示，不提供断开。"),
        )
        .detail(path.display().to_string())
        .hint("v1 不修复或清理断链；请手动检查该 agent 目录。")
    }

    /// A copy attachment is referenced by another agent's symlink, so removing
    /// it would leave dangling links. Shared by detach and uninstall: both
    /// block instead of rewriting other agents' attachments.
    pub fn agent_dependency_conflict(agent: &str, dependents: &[String]) -> Self {
        Self::new(
            ErrorCode::AgentDependencyConflict,
            format!("agent「{agent}」上的 copy 副本仍被其他 agent 的符号链接引用，已阻止操作。"),
        )
        .detail(format!("引用该副本的挂载：{}", dependents.join("；")))
        .hint("请先在相关 agent 中断开这些挂载，再重试。")
    }

    pub fn agent_detach_failed(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::AgentDetachFailed, "断开 agent 挂载失败。")
            .detail(detail)
            .hint("请检查该 agent 目录的权限后重试；canonical 与 lock 未被修改。")
            .retryable(true)
    }

    pub fn agent_already_attached(name: &str) -> Self {
        Self::new(
            ErrorCode::AgentAlreadyAttached,
            format!("agent「{name}」已经挂载了这个 skill。"),
        )
        .hint("请刷新库存后重试。")
    }

    pub fn agent_attach_failed(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::AgentAttachFailed, "创建 agent 挂载失败。")
            .detail(detail)
            .hint("请检查该 agent 目录的权限后重试；canonical 与 lock 未被修改。")
            .retryable(true)
    }

    pub fn extra_agent_invalid(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::ExtraInvalid, "额外 agent 定义无效。")
            .detail(detail)
            .hint("id 只能包含字母、数字、`-`、`_`、`.`，skills 目录必须是 `~/` 开头或绝对路径。")
    }

    pub fn extra_agent_duplicate(id: &str) -> Self {
        Self::new(
            ErrorCode::ExtraDuplicate,
            format!("agent 标识「{id}」已被占用。"),
        )
        .hint("请换一个 id；内置 catalog 与已有额外 agent 的 id 不可复用。")
    }

    pub fn extra_agent_builtin(name: &str) -> Self {
        Self::new(
            ErrorCode::ExtraBuiltin,
            format!("内置额外 agent「{name}」不可删除。"),
        )
        .hint("如果该产品未安装，它会自动标记为「未检测到」并从安装目标中隐藏。")
    }

    pub fn extra_agent_not_found(id: &str) -> Self {
        Self::new(
            ErrorCode::ExtraNotFound,
            format!("未找到额外 agent「{id}」。"),
        )
        .hint("请刷新后重试。")
    }

    pub fn extra_agent_config(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::ExtraConfig, "保存额外 agent 配置失败。")
            .detail(detail)
            .hint("请检查应用数据目录的权限后重试。")
            .retryable(true)
    }

    pub fn extra_agent_remove_blocked(projects: &[String]) -> Self {
        Self::new(
            ErrorCode::ExtraProjectBlocked,
            "无法删除额外 agent：仍有项目不可用。",
        )
        .detail(projects.join("\n"))
        .hint("请先重新定位或遗忘这些项目，再删除该 agent；它的项目挂载尚未被清理。")
    }

    pub fn extra_agent_project_dir_blocked(projects: &[String]) -> Self {
        Self::new(
            ErrorCode::ExtraProjectBlocked,
            "无法修改额外 agent 的项目目录：项目可能仍保留挂载。",
        )
        .detail(projects.join("\n"))
        .hint("请先在相关项目中卸载该 agent 的挂载；不可用项目需先重新定位或遗忘。")
    }

    pub fn backup_failed(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::BackupFailed, "无法创建操作前的备份快照。")
            .detail(detail)
            .hint("请检查应用数据目录与磁盘空间后重试。")
            .retryable(true)
    }

    pub fn verify_failed(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::VerifyFailed, "安装后校验失败。")
            .detail(detail)
            .hint("可以单独重试该 skill。")
            .retryable(true)
    }

    pub fn verify_canonical_missing(name: &str, path: &std::path::Path) -> Self {
        Self::new(
            ErrorCode::VerifyCanonicalMissing,
            format!("共享目录中缺少 skill「{name}」。"),
        )
        .detail(path.display().to_string())
        .hint("CLI 报告安装成功，但 canonical 目录不存在；请重试。")
        .retryable(true)
    }

    pub fn verify_lock_missing(name: &str) -> Self {
        Self::new(
            ErrorCode::VerifyLockMissing,
            format!("lock 中缺少 skill「{name}」。"),
        )
        .hint("CLI 可能未能写入 lock；请重试安装。")
        .retryable(true)
    }

    /// The CLI wrote a lock entry whose source differs from the requested one.
    pub fn verify_lock_mismatch(name: &str, expected: &str, actual: &str) -> Self {
        Self::new(
            ErrorCode::VerifyLockMismatch,
            format!("skill「{name}」的 lock 来源与本次操作不一致。"),
        )
        .detail(format!("期望 {expected}，实际 {actual}"))
        .hint("请检查 CLI 输出；确认来源无误后可重新安装以修正 lock。")
        .retryable(true)
    }

    pub fn verify_attachment_missing(name: &str, agent: &str, path: &std::path::Path) -> Self {
        Self::new(
            ErrorCode::VerifyAttachmentMissing,
            format!("agent「{agent}」缺少 skill「{name}」的挂载。"),
        )
        .detail(path.display().to_string())
        .hint("请检查该 agent 目录的权限后重试。")
        .retryable(true)
    }

    pub fn verify_attachment_mode(
        name: &str,
        agent: &str,
        path: &std::path::Path,
        detail: impl Into<String>,
    ) -> Self {
        Self::new(
            ErrorCode::VerifyAttachmentMode,
            format!("agent「{agent}」的挂载模式异常（{name}）。"),
        )
        .detail(format!("{}：{}", path.display(), detail.into()))
        .hint("挂载应为指向 canonical 的符号链接；请检查后重试。")
        .retryable(true)
    }

    pub fn verify_copy_fallback(name: &str, agent: &str, path: &std::path::Path) -> Self {
        Self::new(
            ErrorCode::VerifyCopyFallback,
            format!("「{name}」在 agent「{agent}」上是副本而非符号链接。"),
        )
        .detail(path.display().to_string())
        .hint("CLI 可能静默回退为 copy；建议检查该 agent 目录的写权限。")
    }

    /// Post-uninstall verification: the CLI exited but the shared folder is
    /// still on disk. Never reported as success.
    pub fn verify_canonical_present(name: &str, path: &std::path::Path) -> Self {
        Self::new(
            ErrorCode::VerifyCanonicalPresent,
            format!("卸载后 canonical 目录仍然存在（{name}）。"),
        )
        .detail(path.display().to_string())
        .hint("CLI 退出成功但未删除共享目录；请手动检查后重试。")
    }

    /// Post-uninstall verification: an agent attachment still exists.
    pub fn verify_attachment_present(name: &str, agent: &str, path: &std::path::Path) -> Self {
        Self::new(
            ErrorCode::VerifyAttachmentPresent,
            format!("卸载后 agent「{agent}」的挂载仍然存在（{name}）。"),
        )
        .detail(path.display().to_string())
        .hint("CLI 可能未能删除该挂载；请检查目录权限后重试。")
    }

    /// Post-uninstall verification: the lock key survived the removal.
    pub fn verify_lock_present(name: &str) -> Self {
        Self::new(
            ErrorCode::VerifyLockPresent,
            format!("卸载后 lock 中仍保留 skill「{name}」。"),
        )
        .hint("CLI 可能未清理 lock 条目；请检查后再重试。")
    }
    pub fn task_internal(context: &str, error: impl std::fmt::Display) -> Self {
        Self::new(
            ErrorCode::TaskInternal,
            format!("校验{context}请求失败：{error}"),
        )
    }

    pub fn skill_read_failed_context(context: &str, error: impl std::fmt::Display) -> Self {
        Self::new(ErrorCode::SkillReadFailed, format!("读取{context}失败。"))
            .detail(error.to_string())
            .retryable(true)
    }

    pub fn skill_dir_not_found(path: &std::path::Path, error: impl std::fmt::Display) -> Self {
        Self::new(ErrorCode::SkillNotFound, "skill 目录不存在。")
            .detail(format!("{}（{error}）", path.display()))
    }

    pub fn skill_fingerprint_failed(error: impl std::fmt::Display) -> Self {
        Self::new(ErrorCode::SkillReadFailed, "无法计算 skill 目录指纹。").detail(error.to_string())
    }

    pub fn baseline_dir_failed(error: impl std::fmt::Display) -> Self {
        Self::new(ErrorCode::BaselineWriteFailed, "无法创建基线指纹目录。")
            .detail(error.to_string())
            .hint("请检查应用数据目录的权限。")
    }

    pub fn baseline_serialize_failed(error: impl std::fmt::Display) -> Self {
        Self::new(ErrorCode::BaselineWriteFailed, "无法序列化基线指纹。").detail(error.to_string())
    }

    pub fn baseline_write_failed(error: impl std::fmt::Display) -> Self {
        Self::new(ErrorCode::BaselineWriteFailed, "无法写入基线指纹。")
            .detail(error.to_string())
            .hint("请检查应用数据目录的权限与磁盘空间。")
    }

    pub fn baseline_save_failed(error: impl std::fmt::Display) -> Self {
        Self::new(ErrorCode::BaselineWriteFailed, "无法保存基线指纹。")
            .detail(error.to_string())
            .hint("请检查应用数据目录的权限与磁盘空间。")
    }

    pub fn baseline_warning(error: AppError) -> Self {
        Self::new(
            ErrorCode::BaselineWriteFailed,
            "操作成功，但无法刷新本地基线指纹。",
        )
        .detail(error.message)
    }

    pub fn baseline_clear_failed(error: impl std::fmt::Display) -> Self {
        Self::new(
            ErrorCode::BaselineClearFailed,
            "卸载成功，但无法清除本地基线指纹。",
        )
        .detail(error.to_string())
    }

    pub fn log_write_failed(path: &std::path::Path, error: impl std::fmt::Display) -> Self {
        Self::new(ErrorCode::LogWriteFailed, "无法写入操作日志。")
            .detail(format!("{}：{error}", path.display()))
    }
}
