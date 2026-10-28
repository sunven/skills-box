//! PATH discovery and tool probing.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use semver::Version;

use crate::agents::{DetectionContext, detect_agents};
use crate::cli::CliRuntime;
use crate::error::AppError;
use crate::paths::AppPaths;
use crate::process::{
    CancelToken, CommandSpec, ExecError, ProcessExecutor, tokio_executor::TokioExecutor,
};

use super::ResolvedEnvironment;
use super::child_env::{ChildEnvParams, build_child_env};
use super::lock_health::{self, LockStatus};
use super::{CliStatus, EnvReport, PathSource, ToolState, ToolStatus};

/// Node locations derived from environment variables and macOS conventions.
/// Tests construct this directly so Homebrew/system/login-shell stages can be
/// exercised inside a temp directory.
#[derive(Debug, Clone)]
pub struct SearchEnv {
    pub volta_home: Option<PathBuf>,
    pub fnm_bases: Vec<PathBuf>,
    pub nvm_dir: Option<PathBuf>,
    pub homebrew_nodes: Vec<PathBuf>,
    pub system_nodes: Vec<PathBuf>,
    pub login_shell: PathBuf,
    pub git_candidates: Vec<PathBuf>,
}

impl SearchEnv {
    pub fn from_values(home: &Path, vars: &BTreeMap<String, String>) -> Self {
        let volta_home = non_empty(vars, "VOLTA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".volta"));

        let mut fnm_bases = Vec::new();
        if let Some(dir) = non_empty(vars, "FNM_DIR") {
            fnm_bases.push(PathBuf::from(dir));
        }
        fnm_bases.push(home.join("Library/Application Support/fnm"));
        if let Some(dir) = non_empty(vars, "XDG_DATA_HOME") {
            fnm_bases.push(Path::new(dir).join("fnm"));
        }
        fnm_bases.push(home.join(".local/share/fnm"));
        fnm_bases.push(home.join(".fnm"));
        dedupe_paths(&mut fnm_bases);

        let nvm_dir = if let Some(dir) = non_empty(vars, "NVM_DIR") {
            PathBuf::from(dir)
        } else if home.join(".nvm").exists() {
            home.join(".nvm")
        } else if let Some(dir) = non_empty(vars, "XDG_CONFIG_HOME") {
            let candidate = Path::new(dir).join("nvm");
            if candidate.exists() {
                candidate
            } else {
                home.join(".nvm")
            }
        } else {
            home.join(".nvm")
        };

        Self {
            volta_home: Some(volta_home),
            fnm_bases,
            nvm_dir: Some(nvm_dir),
            homebrew_nodes: vec![
                PathBuf::from("/opt/homebrew/bin/node"),
                PathBuf::from("/usr/local/bin/node"),
            ],
            system_nodes: vec![
                PathBuf::from("/usr/local/bin/node"),
                PathBuf::from("/usr/bin/node"),
            ],
            login_shell: non_empty(vars, "SHELL")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/bin/zsh")),
            git_candidates: vec![
                PathBuf::from("/opt/homebrew/bin/git"),
                PathBuf::from("/usr/local/bin/git"),
                PathBuf::from("/usr/bin/git"),
            ],
        }
    }
}

#[derive(Debug, Clone)]
pub struct EnvResolverConfig {
    pub search: SearchEnv,
    pub node_override: Option<PathBuf>,
    pub parent_env: BTreeMap<String, String>,
    pub pinned_cli_version: String,
    pub min_node_version: Version,
    pub probe_timeout: Duration,
    pub login_shell_timeout: Duration,
}

impl EnvResolverConfig {
    pub fn from_process(paths: &AppPaths) -> Self {
        let parent_env = capture_process_env();
        let max = crate::timeouts::PROBE_TIMEOUT;
        Self {
            search: SearchEnv::from_values(paths.home(), &parent_env),
            node_override: node_override_from_env(&parent_env, cfg!(debug_assertions)),
            parent_env,
            pinned_cli_version: "1.7.4".to_string(),
            min_node_version: Version::new(22, 20, 0),
            probe_timeout: max,
            login_shell_timeout: crate::timeouts::LOGIN_SHELL_TIMEOUT,
        }
    }
}

pub struct EnvResolver {
    paths: AppPaths,
    executor: Arc<dyn ProcessExecutor>,
    config: EnvResolverConfig,
}

impl EnvResolver {
    pub fn new(
        paths: AppPaths,
        executor: Arc<dyn ProcessExecutor>,
        config: EnvResolverConfig,
    ) -> Self {
        Self {
            paths,
            executor,
            config,
        }
    }

    pub fn from_process(paths: AppPaths) -> Self {
        let config = EnvResolverConfig::from_process(&paths);
        Self::new(paths, Arc::new(TokioExecutor), config)
    }

    pub async fn resolve(&self) -> Result<EnvReport, AppError> {
        self.resolve_detailed()
            .await
            .map(|resolved| resolved.report)
    }

    pub async fn resolve_detailed(&self) -> Result<ResolvedEnvironment, AppError> {
        self.paths
            .ensure_dirs()
            .map_err(|error| AppError::path_app_data(error.to_string()))?;

        let detection_env = self.base_child_env(None, None);
        let detection =
            DetectionContext::new(self.paths.home(), self.paths.cli_cwd(), detection_env);
        let agents = detect_agents(&detection);
        let lock = lock_health::inspect(&self.paths);

        let override_mode = self.config.node_override.is_some();
        let static_candidates = self.static_node_candidates();
        let mut node_result = self.search_nodes(static_candidates.clone()).await;
        let mut warnings = Vec::new();
        let mut login_dirs: Vec<PathBuf> = Vec::new();

        if !override_mode && node_result.npx.is_none() {
            let login = self.login_shell_paths().await;
            if let Some(warning) = login.warning {
                warnings.push(warning);
            }
            login_dirs = login.dirs;
            if !login_dirs.is_empty() {
                let candidates = self.login_node_candidates(&login_dirs, &static_candidates);
                if !candidates.is_empty() {
                    let login_result = self.search_nodes(candidates).await;
                    let login_better = login_result.npx.is_some()
                        || (node_result.node.is_none() && login_result.node.is_some())
                        || (node_result.node.is_none()
                            && node_result.failure.is_none()
                            && login_result.failure.is_some());
                    if login_better {
                        let mut merged = login_result;
                        let mut combined = std::mem::take(&mut node_result.warnings);
                        combined.append(&mut merged.warnings);
                        merged.warnings = combined;
                        node_result = merged;
                    } else {
                        node_result.warnings.extend(login_result.warnings);
                    }
                }
            }
        }
        warnings.extend(node_result.warnings.clone());

        let (node, node_issue) = self.node_status(&node_result);
        let (npx, npx_issue) = self.npx_status(&node_result);
        let git = self.resolve_git(&login_dirs).await;
        let (cli, cli_issue) = self.resolve_cli(&node_result, git.info.as_ref()).await;

        let mut issues = Vec::new();
        issues.extend(node_issue);
        issues.extend(npx_issue);
        if let Some(issue) = git.issue {
            issues.push(issue);
        }
        issues.extend(cli_issue);
        if let Some(error) = lock.error.clone() {
            issues.push(error);
        }
        if lock.status == LockStatus::Unreadable && lock.error.is_none() {
            issues.push(AppError::lock_read_failed("无法读取 lock 文件"));
        }

        let mutations_enabled = issues.is_empty();
        let cli_runtime = if cli.available {
            match (&node_result.node, &node_result.npx) {
                (Some(node), Some(npx)) => {
                    let node_dir = node.path.parent().unwrap_or(Path::new("/")).to_path_buf();
                    let git_dir = git
                        .info
                        .as_ref()
                        .and_then(|info| info.path.parent().map(Path::to_path_buf));
                    Some(CliRuntime {
                        npx_path: npx.path.clone(),
                        node_dir: node_dir.clone(),
                        git_dir: git_dir.clone(),
                        cwd: self.paths.cli_cwd(),
                        home: self.paths.home().to_path_buf(),
                        env: self.base_child_env(Some(&node_dir), git_dir.as_deref()),
                        pinned_version: self.config.pinned_cli_version.clone(),
                    })
                }
                _ => None,
            }
        } else {
            None
        };
        let report = EnvReport {
            ready: mutations_enabled,
            mutations_enabled,
            path_source: node_result.source.unwrap_or(PathSource::Unresolved),
            path_source_detail: node_result.source_detail.clone(),
            node,
            npx,
            git: git.status,
            cli,
            agents,
            lock,
            interrupted: crate::interrupted::inspect(&self.paths),
            issues,
            warnings,
            checked_at_epoch_ms: epoch_ms(),
            pinned_cli_version: self.config.pinned_cli_version.clone(),
        };
        Ok(ResolvedEnvironment {
            report,
            cli_runtime,
        })
    }

    fn base_child_env(
        &self,
        node_dir: Option<&Path>,
        git_dir: Option<&Path>,
    ) -> BTreeMap<String, String> {
        build_child_env(ChildEnvParams {
            home: self.paths.home(),
            node_dir,
            git_dir,
            parent: &self.config.parent_env,
        })
    }

    fn static_node_candidates(&self) -> Vec<NodeCandidate> {
        if let Some(override_path) = &self.config.node_override {
            return vec![NodeCandidate {
                path: override_path.clone(),
                source: PathSource::Override,
                detail: "用户指定（SKILLS_BOX_NODE）".to_string(),
            }];
        }

        let mut candidates = Vec::new();
        if let Some(volta_home) = &self.config.search.volta_home {
            candidates.push(NodeCandidate {
                path: volta_home.join("bin/node"),
                source: PathSource::Volta,
                detail: "Volta".to_string(),
            });
        }
        for base in &self.config.search.fnm_bases {
            candidates.push(NodeCandidate {
                path: base.join("aliases/default/bin/node"),
                source: PathSource::Fnm,
                detail: "fnm 默认别名".to_string(),
            });
        }
        if let Some(nvm_dir) = &self.config.search.nvm_dir {
            candidates.extend(nvm_candidates(nvm_dir));
        }
        for path in &self.config.search.homebrew_nodes {
            candidates.push(NodeCandidate {
                path: path.clone(),
                source: PathSource::Homebrew,
                detail: "Homebrew".to_string(),
            });
        }
        for path in &self.config.search.system_nodes {
            candidates.push(NodeCandidate {
                path: path.clone(),
                source: PathSource::System,
                detail: "系统路径".to_string(),
            });
        }
        dedupe_candidates(&mut candidates);
        candidates
    }

    fn login_node_candidates(
        &self,
        dirs: &[PathBuf],
        static_candidates: &[NodeCandidate],
    ) -> Vec<NodeCandidate> {
        let known: HashSet<&Path> = static_candidates
            .iter()
            .map(|candidate| candidate.path.as_path())
            .collect();
        let mut candidates = Vec::new();
        for dir in dirs {
            if dir.as_os_str().is_empty() {
                continue;
            }
            let path = dir.join("node");
            if known.contains(path.as_path()) {
                continue;
            }
            candidates.push(NodeCandidate {
                path,
                source: PathSource::LoginShell,
                detail: format!("登录 shell PATH（{}）", dir.display()),
            });
        }
        dedupe_candidates(&mut candidates);
        candidates
    }

    async fn login_shell_paths(&self) -> LoginShellOutcome {
        let mut env = self.base_child_env(None, None);
        env.insert(
            "PATH".to_string(),
            "/usr/bin:/bin:/usr/sbin:/sbin".to_string(),
        );
        let spec = CommandSpec::new(
            &self.config.search.login_shell,
            vec![
                "-ilc".to_string(),
                "printf '\\n__SKILLS_BOX_PATH__%s\\n' \"$PATH\"".to_string(),
            ],
            self.paths.home(),
            env,
            self.config.login_shell_timeout,
        );

        match self.executor.run(spec, CancelToken::new()).await {
            Ok(output) if output.success() => match parse_login_path(&output.stdout) {
                Some(dirs) if !dirs.is_empty() => LoginShellOutcome {
                    dirs,
                    warning: Some("已通过登录 shell 回退解析 PATH。".to_string()),
                },
                _ => LoginShellOutcome {
                    dirs: Vec::new(),
                    warning: Some("登录 shell 未返回可解析的 PATH。".to_string()),
                },
            },
            Ok(_) => LoginShellOutcome {
                dirs: Vec::new(),
                warning: Some("登录 shell 返回非零退出码，已跳过回退。".to_string()),
            },
            Err(ExecError::Timeout(_)) => LoginShellOutcome {
                dirs: Vec::new(),
                warning: Some("登录 shell 回退超时，已跳过。".to_string()),
            },
            Err(_) => LoginShellOutcome {
                dirs: Vec::new(),
                warning: Some("登录 shell 回退失败，已跳过。".to_string()),
            },
        }
    }

    async fn search_nodes(&self, candidates: Vec<NodeCandidate>) -> NodeSearchResult {
        let mut result = NodeSearchResult::default();
        let mut seen = HashSet::new();

        for candidate in candidates {
            if !seen.insert(candidate.path.clone()) {
                continue;
            }
            if !is_executable_file(&candidate.path) {
                if candidate.source == PathSource::Override {
                    result.record_failure(NodeFailure {
                        kind: NodeFailureKind::OverrideInvalid,
                        path: Some(candidate.path.clone()),
                        version: None,
                        source: Some(PathSource::Override),
                        source_detail: Some(candidate.detail.clone()),
                        detail: format!("{} 不存在或不可执行", self.paths.display(&candidate.path)),
                    });
                }
                continue;
            }

            match self.probe_node(&candidate.path).await {
                NodeProbe::Ok(version) => {
                    let node = NodeInfo {
                        path: candidate.path.clone(),
                        version: version.clone(),
                    };
                    if result.node.is_none() {
                        result.node = Some(node.clone());
                        result.source = Some(candidate.source);
                        result.source_detail = Some(format!("{}（{}）", candidate.detail, version));
                    }

                    let node_dir = candidate.path.parent().unwrap_or(Path::new("/"));
                    let npx_path = node_dir.join("npx");
                    if !is_executable_file(&npx_path) {
                        result.record_failure(NodeFailure {
                            kind: NodeFailureKind::NpxMissing,
                            path: Some(npx_path.clone()),
                            version: None,
                            source: Some(candidate.source),
                            source_detail: Some(format!("{}（{}）", candidate.detail, version)),
                            detail: format!("{} 不存在", self.paths.display(&npx_path)),
                        });
                        continue;
                    }

                    match self.probe_npx(&npx_path, node_dir).await {
                        Ok(npx_version) => {
                            result.node = Some(node);
                            result.npx = Some(NpxInfo {
                                path: npx_path,
                                version: npx_version.clone(),
                            });
                            result.source = Some(candidate.source);
                            result.source_detail =
                                Some(format!("{}（{}）", candidate.detail, version));
                            return result;
                        }
                        Err(detail) => {
                            result.record_failure(NodeFailure {
                                kind: NodeFailureKind::NpxProbeFailed,
                                path: Some(npx_path),
                                version: None,
                                source: Some(candidate.source),
                                source_detail: Some(format!("{}（{}）", candidate.detail, version)),
                                detail,
                            });
                        }
                    }
                }
                NodeProbe::TooOld(version) => {
                    result.warnings.push(format!(
                        "已跳过版本过低的 Node：{version}（{}）",
                        self.paths.display(&candidate.path)
                    ));
                    if result.node.is_none() {
                        result.record_failure(NodeFailure {
                            kind: NodeFailureKind::TooOld,
                            path: Some(candidate.path.clone()),
                            version: Some(version.clone()),
                            source: Some(candidate.source),
                            source_detail: Some(format!(
                                "{}（{}，版本过低）",
                                candidate.detail, version
                            )),
                            detail: format!("{}（{version}）", self.paths.display(&candidate.path)),
                        });
                    }
                }
                NodeProbe::Timeout(detail) => {
                    if result.node.is_none() {
                        result.record_failure(NodeFailure {
                            kind: NodeFailureKind::ProbeTimeout,
                            path: Some(candidate.path.clone()),
                            version: None,
                            source: Some(candidate.source),
                            source_detail: Some(format!("{}（探测超时）", candidate.detail)),
                            detail,
                        });
                    }
                }
                NodeProbe::Failed(detail) => {
                    if result.node.is_none() {
                        result.record_failure(NodeFailure {
                            kind: NodeFailureKind::ProbeFailed,
                            path: Some(candidate.path.clone()),
                            version: None,
                            source: Some(candidate.source),
                            source_detail: Some(format!("{}（探测失败）", candidate.detail)),
                            detail,
                        });
                    }
                }
            }
        }

        result
    }

    async fn probe_node(&self, path: &Path) -> NodeProbe {
        let spec = CommandSpec::new(
            path,
            vec!["--version".to_string()],
            self.paths.cli_cwd(),
            self.base_child_env(None, None),
            self.config.probe_timeout,
        );
        match self.executor.run(spec, CancelToken::new()).await {
            Ok(output) if output.success() => match parse_node_version(output.stdout_trimmed()) {
                Some(version) if version >= self.config.min_node_version => {
                    NodeProbe::Ok(format!("v{version}"))
                }
                Some(version) => NodeProbe::TooOld(format!("v{version}")),
                None => NodeProbe::Failed(format!(
                    "无法解析 node --version 输出：{:?}",
                    output.stdout_trimmed()
                )),
            },
            Ok(output) => NodeProbe::Failed(format!(
                "node --version 退出码 {:?}：{}",
                output.code,
                output.stderr_trimmed()
            )),
            Err(ExecError::Timeout(timeout)) => {
                NodeProbe::Timeout(format!("node --version 超时（{timeout:?}）"))
            }
            Err(error) => NodeProbe::Failed(error.to_string()),
        }
    }

    async fn probe_npx(&self, npx: &Path, node_dir: &Path) -> Result<String, String> {
        let spec = CommandSpec::new(
            npx,
            vec!["--version".to_string()],
            self.paths.cli_cwd(),
            self.base_child_env(Some(node_dir), None),
            self.config.probe_timeout,
        );
        match self.executor.run(spec, CancelToken::new()).await {
            Ok(output) if output.success() => first_nonempty_line(&output.stdout)
                .map(str::to_string)
                .ok_or_else(|| "npx --version 输出为空".to_string()),
            Ok(output) => Err(format!(
                "npx --version 退出码 {:?}：{}",
                output.code,
                output.stderr_trimmed()
            )),
            Err(ExecError::Timeout(timeout)) => Err(format!("npx --version 超时（{timeout:?}）")),
            Err(error) => Err(error.to_string()),
        }
    }

    fn node_status(&self, result: &NodeSearchResult) -> (ToolStatus, Option<AppError>) {
        if let Some(node) = &result.node {
            return (
                ToolStatus {
                    found: true,
                    path: Some(self.paths.display(&node.path)),
                    version: Some(node.version.clone()),
                    status: ToolState::Ok,
                    detail: None,
                },
                None,
            );
        }

        let Some(failure) = &result.failure else {
            let detail = "已尝试：用户覆盖、Volta、fnm、nvm、Homebrew、系统路径与登录 shell。";
            return (
                ToolStatus {
                    found: false,
                    path: None,
                    version: None,
                    status: ToolState::Missing,
                    detail: Some(detail.to_string()),
                },
                Some(AppError::env_node_missing(detail)),
            );
        };

        let path = failure.path.as_ref().map(|path| self.paths.display(path));
        match failure.kind {
            NodeFailureKind::TooOld => {
                let version = failure.version.clone().unwrap_or_default();
                let path_display = path.clone().unwrap_or_default();
                (
                    ToolStatus {
                        found: true,
                        path,
                        version: Some(version.clone()),
                        status: ToolState::TooOld,
                        detail: Some(failure.detail.clone()),
                    },
                    Some(AppError::env_node_too_old(&version, &path_display)),
                )
            }
            NodeFailureKind::ProbeTimeout => (
                ToolStatus {
                    found: true,
                    path,
                    version: None,
                    status: ToolState::Error,
                    detail: Some(failure.detail.clone()),
                },
                Some(AppError::env_node_probe_timeout(failure.detail.clone())),
            ),
            NodeFailureKind::ProbeFailed => (
                ToolStatus {
                    found: true,
                    path,
                    version: None,
                    status: ToolState::Error,
                    detail: Some(failure.detail.clone()),
                },
                Some(AppError::env_node_probe_failed(failure.detail.clone())),
            ),
            NodeFailureKind::OverrideInvalid => (
                ToolStatus {
                    found: false,
                    path,
                    version: None,
                    status: ToolState::Error,
                    detail: Some(failure.detail.clone()),
                },
                Some(AppError::env_override_invalid(failure.detail.clone())),
            ),
            NodeFailureKind::NpxMissing | NodeFailureKind::NpxProbeFailed => {
                // A valid node is present in every npx failure path.
                (
                    ToolStatus::missing("Node 已找到，但 npx 不可用"),
                    Some(AppError::env_npx_missing(failure.detail.clone())),
                )
            }
        }
    }

    fn npx_status(&self, result: &NodeSearchResult) -> (ToolStatus, Option<AppError>) {
        if let Some(npx) = &result.npx {
            return (
                ToolStatus {
                    found: true,
                    path: Some(self.paths.display(&npx.path)),
                    version: Some(npx.version.clone()),
                    status: ToolState::Ok,
                    detail: None,
                },
                None,
            );
        }
        let Some(node) = &result.node else {
            return (ToolStatus::not_checked("Node 未就绪，未检测 npx"), None);
        };

        let node_dir = node.path.parent().unwrap_or(Path::new("/"));
        let expected = node_dir.join("npx");
        match result.failure.as_ref().map(|failure| failure.kind) {
            Some(NodeFailureKind::NpxProbeFailed) => {
                let detail = result
                    .failure
                    .as_ref()
                    .map(|failure| failure.detail.clone())
                    .unwrap_or_else(|| "npx 无法执行".to_string());
                (
                    ToolStatus {
                        found: true,
                        path: Some(self.paths.display(&expected)),
                        version: None,
                        status: ToolState::Error,
                        detail: Some(detail.clone()),
                    },
                    Some(AppError::env_npx_missing(detail)),
                )
            }
            _ => {
                let detail = format!("{} 不存在", self.paths.display(&expected));
                (
                    ToolStatus {
                        found: false,
                        path: Some(self.paths.display(&expected)),
                        version: None,
                        status: ToolState::Missing,
                        detail: Some(detail.clone()),
                    },
                    Some(AppError::env_npx_missing(detail)),
                )
            }
        }
    }

    async fn resolve_git(&self, extra_dirs: &[PathBuf]) -> GitOutcome {
        let mut candidates = self.config.search.git_candidates.clone();
        for dir in extra_dirs {
            candidates.push(dir.join("git"));
        }
        dedupe_paths(&mut candidates);

        let mut failure_detail = None;
        for candidate in candidates {
            if !is_executable_file(&candidate) {
                continue;
            }
            let spec = CommandSpec::new(
                &candidate,
                vec!["--version".to_string()],
                self.paths.cli_cwd(),
                self.base_child_env(None, None),
                self.config.probe_timeout,
            );
            match self.executor.run(spec, CancelToken::new()).await {
                Ok(output) if output.success() => {
                    let version = parse_git_version(output.stdout_trimmed())
                        .unwrap_or_else(|| output.stdout_trimmed().to_string());
                    return GitOutcome {
                        info: Some(GitInfo {
                            path: candidate.clone(),
                        }),
                        status: ToolStatus {
                            found: true,
                            path: Some(self.paths.display(&candidate)),
                            version: Some(version),
                            status: ToolState::Ok,
                            detail: None,
                        },
                        issue: None,
                    };
                }
                Ok(output) => {
                    failure_detail = Some(format!(
                        "git --version 退出码 {:?}：{}",
                        output.code,
                        output.stderr_trimmed()
                    ));
                }
                Err(error) => failure_detail = Some(error.to_string()),
            }
        }

        let detail = failure_detail.unwrap_or_else(|| {
            "已检查 /opt/homebrew/bin、/usr/local/bin、/usr/bin 与登录 shell PATH。".to_string()
        });
        GitOutcome {
            info: None,
            status: ToolStatus {
                found: false,
                path: None,
                version: None,
                status: if detail.starts_with("已检查") {
                    ToolState::Missing
                } else {
                    ToolState::Error
                },
                detail: Some(detail.clone()),
            },
            issue: Some(AppError::env_git_missing(detail)),
        }
    }

    async fn resolve_cli(
        &self,
        node_result: &NodeSearchResult,
        git: Option<&GitInfo>,
    ) -> (CliStatus, Option<AppError>) {
        let pinned = self.config.pinned_cli_version.clone();
        let Some(node) = &node_result.node else {
            return (
                CliStatus {
                    available: false,
                    version: None,
                    pinned_version: pinned,
                    status: ToolState::NotChecked,
                    detail: Some("Node 未就绪，未检测 skills CLI".to_string()),
                },
                None,
            );
        };
        let Some(npx) = &node_result.npx else {
            return (
                CliStatus {
                    available: false,
                    version: None,
                    pinned_version: pinned,
                    status: ToolState::NotChecked,
                    detail: Some("npx 未就绪，未检测 skills CLI".to_string()),
                },
                None,
            );
        };

        let node_dir = node.path.parent().unwrap_or(Path::new("/"));
        let git_dir = git.and_then(|info| info.path.parent());
        let env = self.base_child_env(Some(node_dir), git_dir);
        let spec = CommandSpec::new(
            &npx.path,
            vec![
                "-y".to_string(),
                format!("skills@{pinned}"),
                "--version".to_string(),
            ],
            self.paths.cli_cwd(),
            env,
            self.config.probe_timeout,
        );

        let error_status = |detail: String| CliStatus {
            available: false,
            version: None,
            pinned_version: pinned.clone(),
            status: ToolState::Error,
            detail: Some(detail),
        };

        match self.executor.run(spec, CancelToken::new()).await {
            Ok(output) if output.success() => {
                let version = first_nonempty_line(&output.stdout)
                    .map(str::to_string)
                    .unwrap_or_default();
                if version == pinned {
                    (
                        CliStatus {
                            available: true,
                            version: Some(version),
                            pinned_version: pinned,
                            status: ToolState::Ok,
                            detail: None,
                        },
                        None,
                    )
                } else {
                    let detail = format!("期望 skills@{pinned}，实际输出 {version:?}");
                    (
                        error_status(detail.clone()),
                        Some(AppError::env_cli_missing(detail)),
                    )
                }
            }
            Ok(output) => {
                let detail = if output.stderr_trimmed().is_empty() {
                    format!("skills --version 退出码 {:?}", output.code)
                } else {
                    output.stderr_trimmed().to_string()
                };
                (
                    error_status(detail.clone()),
                    Some(AppError::env_cli_missing(detail)),
                )
            }
            Err(ExecError::Timeout(timeout)) => {
                let detail = format!("skills CLI 探测超时（{timeout:?}）");
                (
                    error_status(detail.clone()),
                    Some(AppError::env_cli_missing(detail)),
                )
            }
            Err(error) => {
                let detail = error.to_string();
                (
                    error_status(detail.clone()),
                    Some(AppError::env_cli_missing(detail)),
                )
            }
        }
    }
}

#[derive(Debug, Clone)]
struct NodeCandidate {
    path: PathBuf,
    source: PathSource,
    detail: String,
}

#[derive(Debug, Clone)]
struct NodeInfo {
    path: PathBuf,
    version: String,
}

#[derive(Debug, Clone)]
struct NpxInfo {
    path: PathBuf,
    version: String,
}

#[derive(Debug, Clone)]
struct GitInfo {
    path: PathBuf,
}

struct GitOutcome {
    info: Option<GitInfo>,
    status: ToolStatus,
    issue: Option<AppError>,
}

#[derive(Debug, Default)]
struct NodeSearchResult {
    node: Option<NodeInfo>,
    npx: Option<NpxInfo>,
    source: Option<PathSource>,
    source_detail: Option<String>,
    failure: Option<NodeFailure>,
    warnings: Vec<String>,
}

impl NodeSearchResult {
    fn record_failure(&mut self, failure: NodeFailure) {
        let replace = self
            .failure
            .as_ref()
            .map(|current| failure.kind.priority() > current.kind.priority())
            .unwrap_or(true);
        if replace {
            self.failure = Some(failure);
        }
        // A validated node owns the reported source. Before that, follow the
        // highest-priority failure so `pathSource` matches the node status.
        if self.node.is_none()
            && let Some(failure) = &self.failure
            && failure.source.is_some()
        {
            self.source = failure.source;
            self.source_detail = failure.source_detail.clone();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeFailureKind {
    NpxMissing,
    NpxProbeFailed,
    ProbeFailed,
    ProbeTimeout,
    TooOld,
    OverrideInvalid,
}

impl NodeFailureKind {
    fn priority(self) -> u8 {
        match self {
            Self::NpxMissing => 0,
            Self::NpxProbeFailed => 1,
            Self::ProbeFailed => 2,
            Self::ProbeTimeout => 3,
            Self::TooOld => 4,
            Self::OverrideInvalid => 5,
        }
    }
}

#[derive(Debug, Clone)]
struct NodeFailure {
    kind: NodeFailureKind,
    path: Option<PathBuf>,
    version: Option<String>,
    source: Option<PathSource>,
    source_detail: Option<String>,
    detail: String,
}

enum NodeProbe {
    Ok(String),
    TooOld(String),
    Timeout(String),
    Failed(String),
}

struct LoginShellOutcome {
    dirs: Vec<PathBuf>,
    warning: Option<String>,
}

pub fn capture_process_env() -> BTreeMap<String, String> {
    std::env::vars_os()
        .filter_map(|(key, value)| {
            let key = key.into_string().ok()?;
            let value = value.into_string().ok()?;
            Some((key, value))
        })
        .collect()
}

/// `SKILLS_BOX_NODE` is a debug-only override, mirroring `SKILLS_BOX_ROOT`.
pub fn node_override_from_env(vars: &BTreeMap<String, String>, debug: bool) -> Option<PathBuf> {
    if !debug {
        return None;
    }
    non_empty(vars, "SKILLS_BOX_NODE").map(PathBuf::from)
}

fn nvm_candidates(nvm_dir: &Path) -> Vec<NodeCandidate> {
    let versions_dir = nvm_dir.join("versions/node");
    let mut versions: Vec<(Version, PathBuf)> = std::fs::read_dir(&versions_dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let version = Version::parse(name.trim_start_matches('v')).ok()?;
            Some((version, entry.path().join("bin/node")))
        })
        .collect();
    versions.sort_by(|left, right| right.0.cmp(&left.0));

    let alias = std::fs::read_to_string(nvm_dir.join("alias/default"))
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let mut candidates = Vec::new();
    if let Some(alias) = &alias {
        let prefix = alias.trim_start_matches('v');
        if prefix
            .chars()
            .next()
            .map(|character| character.is_ascii_digit())
            .unwrap_or(false)
            && let Some((_, path)) = versions
                .iter()
                .find(|(version, _)| version_matches_alias(version, prefix))
        {
            candidates.push(NodeCandidate {
                path: path.clone(),
                source: PathSource::Nvm,
                detail: format!("nvm 默认别名 {alias}"),
            });
        }
    }
    for (version, path) in versions {
        candidates.push(NodeCandidate {
            path,
            source: PathSource::Nvm,
            detail: format!("nvm 版本 v{version}"),
        });
    }
    candidates
}

fn version_matches_alias(version: &Version, alias: &str) -> bool {
    let alias_parts: Vec<&str> = alias.split('.').collect();
    let version_parts = [
        version.major.to_string(),
        version.minor.to_string(),
        version.patch.to_string(),
    ];
    if alias_parts.is_empty() || alias_parts.len() > 3 {
        return false;
    }
    alias_parts
        .iter()
        .zip(version_parts.iter())
        .all(|(alias_part, version_part)| alias_part == version_part)
}

fn parse_node_version(raw: &str) -> Option<Version> {
    let line = first_nonempty_line(raw)?;
    Version::parse(line.trim_start_matches('v')).ok()
}

fn parse_git_version(raw: &str) -> Option<String> {
    let line = first_nonempty_line(raw)?;
    line.strip_prefix("git version ")
        .and_then(|rest| rest.split_whitespace().next())
        .map(str::to_string)
        .or_else(|| Some(line.to_string()))
}

fn first_nonempty_line(raw: &str) -> Option<&str> {
    raw.lines().map(str::trim).find(|line| !line.is_empty())
}

fn parse_login_path(raw: &str) -> Option<Vec<PathBuf>> {
    let value = raw.lines().rev().find_map(|line| {
        line.split_once("__SKILLS_BOX_PATH__")
            .map(|(_, path)| path.trim())
    })?;
    let mut dirs: Vec<PathBuf> = value
        .split(':')
        .filter(|part| !part.is_empty())
        .map(PathBuf::from)
        .collect();
    dedupe_paths(&mut dirs);
    Some(dirs)
}

fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
}

fn dedupe_paths(paths: &mut Vec<PathBuf>) {
    let mut seen = HashSet::new();
    paths.retain(|path| seen.insert(path.clone()));
}

fn dedupe_candidates(candidates: &mut Vec<NodeCandidate>) {
    let mut seen = HashSet::new();
    candidates.retain(|candidate| seen.insert(candidate.path.clone()));
}

fn non_empty<'a>(vars: &'a BTreeMap<String, String>, key: &str) -> Option<&'a str> {
    vars.get(key)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::codes;
    use crate::process::fake::{ScriptedExecutor, ScriptedResponse};
    use std::collections::BTreeMap;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    struct Fixture {
        _tmp: tempfile::TempDir,
        paths: AppPaths,
        search: SearchEnv,
    }

    impl Fixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path().join("home");
            fs::create_dir_all(&home).unwrap();
            let paths = AppPaths::new(home, tmp.path().join("app-data"));
            let search = SearchEnv {
                volta_home: None,
                fnm_bases: Vec::new(),
                nvm_dir: None,
                homebrew_nodes: Vec::new(),
                system_nodes: Vec::new(),
                login_shell: PathBuf::from("/bin/zsh"),
                git_candidates: Vec::new(),
            };
            Self {
                _tmp: tmp,
                paths,
                search,
            }
        }

        fn config(&self) -> EnvResolverConfig {
            EnvResolverConfig {
                search: self.search.clone(),
                node_override: None,
                parent_env: BTreeMap::new(),
                pinned_cli_version: "1.7.4".to_string(),
                min_node_version: Version::new(22, 20, 0),
                probe_timeout: Duration::from_millis(100),
                login_shell_timeout: Duration::from_millis(50),
            }
        }

        fn resolver(&self, executor: Arc<ScriptedExecutor>) -> EnvResolver {
            EnvResolver::new(self.paths.clone(), executor, self.config())
        }
    }

    fn make_executable(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "#!/bin/sh\n").unwrap();
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    fn make_node_bin(dir: &Path) -> PathBuf {
        let node = dir.join("node");
        make_executable(&node);
        make_executable(&dir.join("npx"));
        node
    }

    fn make_git(dir: &Path) -> PathBuf {
        let git = dir.join("git");
        make_executable(&git);
        git
    }

    fn standard_executor(
        node_path: &Path,
        node_version: &str,
        npx_version: &str,
        cli_version: &str,
        git_path: &Path,
    ) -> ScriptedExecutor {
        standard_executor_repeated(
            node_path,
            node_version,
            npx_version,
            cli_version,
            git_path,
            1,
        )
    }

    fn standard_executor_repeated(
        node_path: &Path,
        node_version: &str,
        npx_version: &str,
        cli_version: &str,
        git_path: &Path,
        repeats: usize,
    ) -> ScriptedExecutor {
        let _ = git_path;
        let mut executor = ScriptedExecutor::new();
        for _ in 0..repeats {
            executor = executor
                .expect(
                    &node_path.display().to_string(),
                    &["--version"],
                    ScriptedResponse::ok(format!("{node_version}\n")),
                )
                .expect(
                    "npx",
                    &["skills@1.7.4"],
                    ScriptedResponse::ok(format!("{cli_version}\n")),
                )
                .expect(
                    "npx",
                    &["--version"],
                    ScriptedResponse::ok(format!("{npx_version}\n")),
                )
                .expect(
                    "git",
                    &["--version"],
                    ScriptedResponse::ok("git version 2.54.0 (Apple Git-157)\n"),
                );
        }
        executor
    }

    fn login_rule(executor: ScriptedExecutor, path: &str) -> ScriptedExecutor {
        executor.expect(
            "zsh",
            &["-ilc"],
            ScriptedResponse::ok(format!("\n__SKILLS_BOX_PATH__{path}\n")),
        )
    }

    fn issue_codes(report: &EnvReport) -> Vec<&str> {
        report
            .issues
            .iter()
            .map(|issue| issue.code.as_str())
            .collect()
    }

    #[tokio::test]
    async fn discovers_volta_before_nvm() {
        let mut fixture = Fixture::new();
        let volta_node = make_node_bin(&fixture._tmp.path().join("volta/bin"));
        let nvm_node = make_node_bin(&fixture._tmp.path().join("nvm/versions/node/v24.15.0/bin"));
        let git = make_git(&fixture._tmp.path().join("git-bin"));
        fixture.search.volta_home = Some(fixture._tmp.path().join("volta"));
        fixture.search.nvm_dir = Some(fixture._tmp.path().join("nvm"));
        fixture.search.git_candidates = vec![git.clone()];

        let executor = Arc::new(standard_executor(
            &volta_node,
            "v22.20.0",
            "11.12.1",
            "1.7.4",
            &git,
        ));
        let report = fixture.resolver(executor.clone()).resolve().await.unwrap();

        assert_eq!(report.path_source, PathSource::Volta);
        assert_eq!(
            report.node.path.as_deref(),
            Some(fixture.paths.display(&volta_node).as_str())
        );
        assert!(report.mutations_enabled);
        assert!(!executor.calls().iter().any(|call| call.program == nvm_node));
    }

    #[tokio::test]
    async fn uses_fnm_default_alias() {
        let mut fixture = Fixture::new();
        let node = make_node_bin(&fixture._tmp.path().join("fnm/aliases/default/bin"));
        let git = make_git(&fixture._tmp.path().join("git-bin"));
        fixture.search.fnm_bases = vec![fixture._tmp.path().join("fnm")];
        fixture.search.git_candidates = vec![git.clone()];

        let executor = Arc::new(standard_executor(
            &node, "v24.15.0", "11.12.1", "1.7.4", &git,
        ));
        let report = fixture.resolver(executor).resolve().await.unwrap();
        assert_eq!(report.path_source, PathSource::Fnm);
        assert_eq!(report.node.version.as_deref(), Some("v24.15.0"));
    }

    #[tokio::test]
    async fn nvm_default_alias_resolves_to_highest_matching_version() {
        let mut fixture = Fixture::new();
        let nvm = fixture._tmp.path().join("nvm");
        fs::create_dir_all(nvm.join("alias")).unwrap();
        fs::write(nvm.join("alias/default"), "24\n").unwrap();
        let high = make_node_bin(&nvm.join("versions/node/v24.15.0/bin"));
        let low = make_node_bin(&nvm.join("versions/node/v24.9.0/bin"));
        let _too_old = make_node_bin(&nvm.join("versions/node/v20.11.0/bin"));
        let git = make_git(&fixture._tmp.path().join("git-bin"));
        fixture.search.nvm_dir = Some(nvm);
        fixture.search.git_candidates = vec![git.clone()];

        let executor = Arc::new(
            standard_executor(&high, "v24.15.0", "11.12.1", "1.7.4", &git)
                .expect(
                    &low.display().to_string(),
                    &["--version"],
                    ScriptedResponse::ok("v24.9.0\n"),
                )
                .expect(
                    &fixture
                        ._tmp
                        .path()
                        .join("nvm/versions/node/v20.11.0/bin/node")
                        .display()
                        .to_string(),
                    &["--version"],
                    ScriptedResponse::ok("v20.11.0\n"),
                ),
        );
        let report = fixture.resolver(executor).resolve().await.unwrap();

        assert_eq!(report.path_source, PathSource::Nvm);
        assert_eq!(
            report.node.path.as_deref(),
            Some(fixture.paths.display(&high).as_str())
        );
        assert_eq!(report.node.version.as_deref(), Some("v24.15.0"));
        assert!(
            report
                .path_source_detail
                .as_deref()
                .unwrap()
                .contains("默认别名 24")
        );
    }

    #[tokio::test]
    async fn nvm_without_alias_uses_highest_version() {
        let mut fixture = Fixture::new();
        let nvm = fixture._tmp.path().join("nvm");
        let highest = make_node_bin(&nvm.join("versions/node/v24.2.0/bin"));
        let mid = make_node_bin(&nvm.join("versions/node/v23.1.0/bin"));
        let low = make_node_bin(&nvm.join("versions/node/v20.11.0/bin"));
        let git = make_git(&fixture._tmp.path().join("git-bin"));
        fixture.search.nvm_dir = Some(nvm);
        fixture.search.git_candidates = vec![git.clone()];

        let executor = Arc::new(
            standard_executor(&highest, "v24.2.0", "11.12.1", "1.7.4", &git)
                .expect(
                    &mid.display().to_string(),
                    &["--version"],
                    ScriptedResponse::ok("v23.1.0\n"),
                )
                .expect(
                    &low.display().to_string(),
                    &["--version"],
                    ScriptedResponse::ok("v20.11.0\n"),
                ),
        );
        let report = fixture.resolver(executor).resolve().await.unwrap();

        assert_eq!(report.path_source, PathSource::Nvm);
        assert_eq!(report.node.version.as_deref(), Some("v24.2.0"));
    }

    #[tokio::test]
    async fn homebrew_and_system_stages_are_used() {
        for (source, use_homebrew) in [(PathSource::Homebrew, true), (PathSource::System, false)] {
            let mut fixture = Fixture::new();
            let bin = fixture._tmp.path().join("platform/bin");
            let node = make_node_bin(&bin);
            let git = make_git(&fixture._tmp.path().join("git-bin"));
            if use_homebrew {
                fixture.search.homebrew_nodes = vec![node.clone()];
            } else {
                fixture.search.system_nodes = vec![node.clone()];
            }
            fixture.search.git_candidates = vec![git.clone()];

            let executor = Arc::new(standard_executor(
                &node, "v24.15.0", "11.12.1", "1.7.4", &git,
            ));
            let report = fixture.resolver(executor).resolve().await.unwrap();
            assert_eq!(report.path_source, source);
        }
    }

    #[tokio::test]
    async fn bounded_login_shell_fallback_finds_node() {
        let fixture = Fixture::new();
        let login_bin = fixture._tmp.path().join("login/bin");
        let node = make_node_bin(&login_bin);
        let git = make_git(&fixture._tmp.path().join("git-bin"));

        let executor = Arc::new(login_rule(
            standard_executor(&node, "v24.15.0", "11.12.1", "1.7.4", &git),
            &login_bin.display().to_string(),
        ));
        let mut config = fixture.config();
        // Use an empty PATH after the login-bin so only the fixture node is found.
        config.search.git_candidates = vec![git];
        let resolver = EnvResolver::new(fixture.paths.clone(), executor, config);
        let report = resolver.resolve().await.unwrap();

        assert_eq!(report.path_source, PathSource::LoginShell);
        assert_eq!(
            report.node.path.as_deref(),
            Some(fixture.paths.display(&node).as_str())
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("登录 shell"))
        );
    }

    #[tokio::test]
    async fn node_below_minimum_is_reported_as_too_old() {
        let mut fixture = Fixture::new();
        let node = make_node_bin(&fixture._tmp.path().join("fnm/aliases/default/bin"));
        fixture.search.fnm_bases = vec![fixture._tmp.path().join("fnm")];

        let executor = Arc::new(login_rule(
            ScriptedExecutor::new().expect(
                &node.display().to_string(),
                &["--version"],
                ScriptedResponse::ok("v20.11.0\n"),
            ),
            &fixture._tmp.path().join("empty").display().to_string(),
        ));
        let report = fixture.resolver(executor).resolve().await.unwrap();

        assert_eq!(report.node.status, ToolState::TooOld);
        assert_eq!(report.node.version.as_deref(), Some("v20.11.0"));
        assert!(!report.mutations_enabled);
        assert!(issue_codes(&report).contains(&codes::ENV_NODE_TOO_OLD));
        assert!(report.issues[0].message.contains("22.20"));
    }

    #[tokio::test]
    async fn missing_node_is_reported_distinctly() {
        let fixture = Fixture::new();
        let empty = fixture._tmp.path().join("empty");
        fs::create_dir_all(&empty).unwrap();
        let executor = Arc::new(login_rule(
            ScriptedExecutor::new(),
            &empty.display().to_string(),
        ));
        let report = fixture.resolver(executor).resolve().await.unwrap();

        assert_eq!(report.node.status, ToolState::Missing);
        assert_eq!(report.path_source, PathSource::Unresolved);
        assert!(issue_codes(&report).contains(&codes::ENV_NODE_MISSING));
        assert!(!report.mutations_enabled);
    }

    #[tokio::test]
    async fn missing_npx_is_reported_separately_from_node() {
        let mut fixture = Fixture::new();
        let bin = fixture._tmp.path().join("fnm/aliases/default/bin");
        let node = bin.join("node");
        make_executable(&node);
        fixture.search.fnm_bases = vec![fixture._tmp.path().join("fnm")];

        let executor = Arc::new(login_rule(
            ScriptedExecutor::new().expect(
                &node.display().to_string(),
                &["--version"],
                ScriptedResponse::ok("v24.15.0\n"),
            ),
            &fixture._tmp.path().join("empty").display().to_string(),
        ));
        let report = fixture.resolver(executor).resolve().await.unwrap();

        assert_eq!(report.node.status, ToolState::Ok);
        assert_eq!(report.npx.status, ToolState::Missing);
        assert_eq!(report.cli.status, ToolState::NotChecked);
        assert!(issue_codes(&report).contains(&codes::ENV_NPX_MISSING));
        assert!(!report.mutations_enabled);
    }

    #[tokio::test]
    async fn missing_git_is_reported_while_node_stays_usable() {
        let mut fixture = Fixture::new();
        let node = make_node_bin(&fixture._tmp.path().join("fnm/aliases/default/bin"));
        fixture.search.fnm_bases = vec![fixture._tmp.path().join("fnm")];

        let executor = Arc::new(standard_executor(
            &node,
            "v24.15.0",
            "11.12.1",
            "1.7.4",
            Path::new("/nonexistent/git"),
        ));
        let report = fixture.resolver(executor).resolve().await.unwrap();

        assert_eq!(report.node.status, ToolState::Ok);
        assert_eq!(report.git.status, ToolState::Missing);
        assert!(issue_codes(&report).contains(&codes::ENV_GIT_MISSING));
        assert!(!report.mutations_enabled);
    }

    #[tokio::test]
    async fn unavailable_cli_is_reported_with_stderr_detail() {
        let mut fixture = Fixture::new();
        let node = make_node_bin(&fixture._tmp.path().join("fnm/aliases/default/bin"));
        let git = make_git(&fixture._tmp.path().join("git-bin"));
        fixture.search.fnm_bases = vec![fixture._tmp.path().join("fnm")];
        fixture.search.git_candidates = vec![git.clone()];

        let executor = Arc::new(
            ScriptedExecutor::new()
                .expect(
                    &node.display().to_string(),
                    &["--version"],
                    ScriptedResponse::ok("v24.15.0\n"),
                )
                .expect(
                    "npx",
                    &["skills@1.7.4"],
                    ScriptedResponse::fail(1, "network down"),
                )
                .expect("npx", &["--version"], ScriptedResponse::ok("11.12.1\n"))
                .expect(
                    "git",
                    &["--version"],
                    ScriptedResponse::ok("git version 2.54.0\n"),
                ),
        );
        let report = fixture.resolver(executor).resolve().await.unwrap();

        assert_eq!(report.cli.status, ToolState::Error);
        assert!(!report.cli.available);
        assert!(issue_codes(&report).contains(&codes::ENV_CLI_MISSING));
        let issue = report
            .issues
            .iter()
            .find(|issue| issue.code == codes::ENV_CLI_MISSING)
            .unwrap();
        assert!(issue.detail.as_deref().unwrap().contains("network down"));
    }

    #[tokio::test]
    async fn explicit_override_wins_and_does_not_fall_back() {
        let mut fixture = Fixture::new();
        let override_node = make_node_bin(&fixture._tmp.path().join("override/bin"));
        let volta_node = make_node_bin(&fixture._tmp.path().join("volta/bin"));
        let git = make_git(&fixture._tmp.path().join("git-bin"));
        fixture.search.volta_home = Some(fixture._tmp.path().join("volta"));
        fixture.search.git_candidates = vec![git.clone()];

        let executor = Arc::new(standard_executor(
            &override_node,
            "v24.15.0",
            "11.12.1",
            "1.7.4",
            &git,
        ));
        let mut config = fixture.config();
        config.node_override = Some(override_node.clone());
        let resolver = EnvResolver::new(fixture.paths.clone(), executor.clone(), config);
        let report = resolver.resolve().await.unwrap();

        assert_eq!(report.path_source, PathSource::Override);
        assert_eq!(
            report.node.path.as_deref(),
            Some(fixture.paths.display(&override_node).as_str())
        );
        assert!(
            !executor
                .calls()
                .iter()
                .any(|call| call.program == volta_node)
        );
    }

    #[tokio::test]
    async fn invalid_override_reports_without_falling_back() {
        let mut fixture = Fixture::new();
        let _volta_node = make_node_bin(&fixture._tmp.path().join("volta/bin"));
        fixture.search.volta_home = Some(fixture._tmp.path().join("volta"));

        let missing = fixture._tmp.path().join("missing/bin/node");
        let executor = Arc::new(ScriptedExecutor::new());
        let mut config = fixture.config();
        config.node_override = Some(missing);
        let resolver = EnvResolver::new(fixture.paths.clone(), executor.clone(), config);
        let report = resolver.resolve().await.unwrap();

        assert_eq!(report.path_source, PathSource::Override);
        assert_eq!(report.node.status, ToolState::Error);
        assert!(issue_codes(&report).contains(&codes::ENV_OVERRIDE_INVALID));
        assert_eq!(executor.call_count(), 0, "no fallback probes for override");
    }

    #[tokio::test]
    async fn path_source_follows_highest_priority_failure() {
        let mut fixture = Fixture::new();
        let volta_node = make_node_bin(&fixture._tmp.path().join("volta/bin"));
        let fnm_node = make_node_bin(&fixture._tmp.path().join("fnm/aliases/default/bin"));
        fixture.search.volta_home = Some(fixture._tmp.path().join("volta"));
        fixture.search.fnm_bases = vec![fixture._tmp.path().join("fnm")];

        let executor = Arc::new(login_rule(
            ScriptedExecutor::new()
                .expect(
                    &volta_node.display().to_string(),
                    &["--version"],
                    ScriptedResponse::fail(1, "bad node"),
                )
                .expect(
                    &fnm_node.display().to_string(),
                    &["--version"],
                    ScriptedResponse::ok("v20.11.0\n"),
                ),
            &fixture._tmp.path().join("empty").display().to_string(),
        ));
        let report = fixture.resolver(executor).resolve().await.unwrap();

        assert_eq!(report.node.status, ToolState::TooOld);
        assert_eq!(report.path_source, PathSource::Fnm);
        assert!(
            report
                .path_source_detail
                .as_deref()
                .unwrap()
                .contains("fnm")
        );
    }

    #[tokio::test]
    async fn node_probe_timeout_is_reported() {
        let mut fixture = Fixture::new();
        let node = make_node_bin(&fixture._tmp.path().join("fnm/aliases/default/bin"));
        fixture.search.fnm_bases = vec![fixture._tmp.path().join("fnm")];

        let executor = Arc::new(login_rule(
            ScriptedExecutor::new().expect(
                &node.display().to_string(),
                &["--version"],
                ScriptedResponse::timeout(),
            ),
            &fixture._tmp.path().join("empty").display().to_string(),
        ));
        let report = fixture.resolver(executor).resolve().await.unwrap();

        assert_eq!(report.node.status, ToolState::Error);
        assert!(issue_codes(&report).contains(&codes::ENV_NODE_PROBE_TIMEOUT));
    }

    #[tokio::test]
    async fn cli_probe_receives_node_path_minimal_env_and_app_data_cwd() {
        let mut fixture = Fixture::new();
        let node = make_node_bin(&fixture._tmp.path().join("fnm/aliases/default/bin"));
        let git = make_git(&fixture._tmp.path().join("git-bin"));
        fixture.search.fnm_bases = vec![fixture._tmp.path().join("fnm")];
        fixture.search.git_candidates = vec![git.clone()];

        let executor = Arc::new(standard_executor(
            &node, "v24.15.0", "11.12.1", "1.7.4", &git,
        ));
        let mut config = fixture.config();
        config.parent_env = [
            ("GITHUB_TOKEN", "secret"),
            ("XDG_STATE_HOME", "/tmp/state"),
            ("CURSOR_AGENT", "1"),
        ]
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
        let resolver = EnvResolver::new(fixture.paths.clone(), executor.clone(), config);
        resolver.resolve().await.unwrap();

        let cli_call = executor
            .calls()
            .into_iter()
            .find(|call| call.args.iter().any(|arg| arg.contains("skills@1.7.4")))
            .expect("CLI probe must run");
        let node_dir = node.parent().unwrap();
        assert_eq!(cli_call.program, node_dir.join("npx"));
        assert_eq!(cli_call.cwd, fixture.paths.cli_cwd());
        assert!(cli_call.env["PATH"].starts_with(&format!("{}:", node_dir.display())));
        assert_eq!(cli_call.env["DISABLE_TELEMETRY"], "1");
        assert_eq!(cli_call.env["DO_NOT_TRACK"], "1");
        assert_eq!(cli_call.env["CI"], "true");
        assert_eq!(cli_call.env["GIT_TERMINAL_PROMPT"], "0");
        assert_eq!(
            cli_call.env["HOME"],
            fixture.paths.home().display().to_string()
        );
        assert!(!cli_call.env.contains_key("GITHUB_TOKEN"));
        assert!(!cli_call.env.contains_key("XDG_STATE_HOME"));
        assert!(!cli_call.env.contains_key("CURSOR_AGENT"));
    }

    #[tokio::test]
    async fn report_includes_detected_agents_and_lock_health() {
        let mut fixture = Fixture::new();
        let node = make_node_bin(&fixture._tmp.path().join("fnm/aliases/default/bin"));
        let git = make_git(&fixture._tmp.path().join("git-bin"));
        fixture.search.fnm_bases = vec![fixture._tmp.path().join("fnm")];
        fixture.search.git_candidates = vec![git.clone()];
        fs::create_dir_all(fixture.paths.home().join(".claude")).unwrap();
        fs::create_dir_all(fixture.paths.agents_root()).unwrap();
        fs::write(
            fixture.paths.lock_path(),
            r#"{"version":3,"skills":{"alpha":{},"beta":{}}}"#,
        )
        .unwrap();

        let executor = Arc::new(standard_executor(
            &node, "v24.15.0", "11.12.1", "1.7.4", &git,
        ));
        let report = fixture.resolver(executor).resolve().await.unwrap();

        assert!(report.agents.iter().any(|agent| agent.id == "claude-code"));
        assert_eq!(report.lock.status, LockStatus::Ok);
        assert_eq!(report.lock.skill_count, Some(2));
        assert!(report.ready);
        assert!(report.issues.is_empty());
    }

    #[tokio::test]
    async fn environment_service_caches_until_refresh() {
        let mut fixture = Fixture::new();
        let node = make_node_bin(&fixture._tmp.path().join("fnm/aliases/default/bin"));
        let git = make_git(&fixture._tmp.path().join("git-bin"));
        fixture.search.fnm_bases = vec![fixture._tmp.path().join("fnm")];
        fixture.search.git_candidates = vec![git.clone()];

        let executor = Arc::new(standard_executor_repeated(
            &node, "v24.15.0", "11.12.1", "1.7.4", &git, 2,
        ));
        let service = super::super::EnvironmentService::new(EnvResolver::new(
            fixture.paths.clone(),
            executor.clone(),
            fixture.config(),
        ));

        let first = service.get(false).await.unwrap();
        assert_eq!(executor.call_count(), 4);
        let second = service.get(false).await.unwrap();
        assert_eq!(executor.call_count(), 4, "second read must hit the cache");
        assert_eq!(first.checked_at_epoch_ms, second.checked_at_epoch_ms);

        let refreshed = service.get(true).await.unwrap();
        assert_eq!(executor.call_count(), 8, "refresh must re-run every probe");
        assert_eq!(refreshed.node.version.as_deref(), Some("v24.15.0"));
    }

    #[tokio::test]
    async fn detailed_resolution_exposes_absolute_cli_runtime() {
        let mut fixture = Fixture::new();
        let node = make_node_bin(&fixture._tmp.path().join("fnm/aliases/default/bin"));
        let git = make_git(&fixture._tmp.path().join("git-bin"));
        fixture.search.fnm_bases = vec![fixture._tmp.path().join("fnm")];
        fixture.search.git_candidates = vec![git.clone()];

        let executor = Arc::new(standard_executor(
            &node, "v24.15.0", "11.12.1", "1.7.4", &git,
        ));
        let resolved = fixture.resolver(executor).resolve_detailed().await.unwrap();
        let runtime = resolved.cli_runtime.expect("CLI runtime should be present");
        let node_dir = node.parent().unwrap();

        assert_eq!(runtime.npx_path, node_dir.join("npx"));
        assert_eq!(runtime.node_dir, node_dir);
        assert_eq!(runtime.git_dir.as_deref(), git.parent());
        assert_eq!(runtime.cwd, fixture.paths.cli_cwd());
        assert_eq!(runtime.home, fixture.paths.home());
        assert_eq!(runtime.pinned_version, "1.7.4");
        assert_eq!(runtime.env["CI"], "true");
        assert!(
            runtime.env["PATH"].starts_with(&format!("{}:", node_dir.display())),
            "node bin must lead PATH"
        );
        assert_eq!(resolved.report.pinned_cli_version, "1.7.4");
    }

    #[tokio::test]
    async fn cli_runtime_surfaces_the_environment_issue_when_unavailable() {
        let fixture = Fixture::new();
        let empty = fixture._tmp.path().join("empty");
        fs::create_dir_all(&empty).unwrap();
        let executor = Arc::new(login_rule(
            ScriptedExecutor::new(),
            &empty.display().to_string(),
        ));
        let service = super::super::EnvironmentService::new(fixture.resolver(executor));
        let error = service.cli_runtime().await.unwrap_err();
        assert_eq!(error.code, codes::ENV_NODE_MISSING);
    }

    #[test]
    fn override_env_is_debug_only() {
        let vars = BTreeMap::from([(
            "SKILLS_BOX_NODE".to_string(),
            "/opt/node/bin/node".to_string(),
        )]);
        assert_eq!(
            node_override_from_env(&vars, true),
            Some(PathBuf::from("/opt/node/bin/node"))
        );
        assert_eq!(node_override_from_env(&vars, false), None);
    }

    #[test]
    fn parses_login_path_marker_and_dedupes() {
        let dirs =
            parse_login_path("shell noise\n__SKILLS_BOX_PATH__/a/bin:/b/bin:/a/bin:/usr/bin\n")
                .unwrap();
        assert_eq!(
            dirs,
            vec![
                PathBuf::from("/a/bin"),
                PathBuf::from("/b/bin"),
                PathBuf::from("/usr/bin")
            ]
        );
        assert!(parse_login_path("no marker").is_none());
    }
}
