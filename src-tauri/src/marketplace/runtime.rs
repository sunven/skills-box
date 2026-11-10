use super::Coordinate;
use crate::{
    error::AppError,
    paths::AppPaths,
    process::{CancelToken, CommandSpec, ProcessExecutor},
};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

pub const INSTALL_COMMAND: &str = "curl -fsSL https://skillhub-1388575217.cos.ap-guangzhou.myqcloud.com/install/install.sh | bash -s -- --cli-only";
pub const INSTRUCTIONS_URL: &str = "https://skillhub.cn/install/skillhub.md";
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub available: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub error: Option<AppError>,
    pub install_command: String,
    pub instructions_url: String,
}
#[derive(Debug, Clone)]
pub struct Runtime {
    pub program: PathBuf,
    pub env: BTreeMap<String, String>,
}
impl Runtime {
    pub fn install(&self, c: &Coordinate, version: &str, stage: &Path) -> CommandSpec {
        // Separate --namespace forces the public resolver path. @ns/slug also
        // works, but falls back to saved enterprise credentials on a 404.
        CommandSpec::new(
            &self.program,
            vec![
                "--skip-self-upgrade".into(),
                "install".into(),
                format!("{}@{version}", c.slug),
                "--namespace".into(),
                c.namespace.clone(),
                "--dir".into(),
                stage.to_string_lossy().into_owned(),
                "--json".into(),
            ],
            stage,
            self.env.clone(),
            crate::timeouts::WRITE_TIMEOUT,
        )
    }
}
pub async fn detect(paths: &AppPaths, executor: &dyn ProcessExecutor) -> (Status, Option<Runtime>) {
    let mut status = Status {
        available: false,
        path: None,
        version: None,
        error: None,
        install_command: INSTALL_COMMAND.into(),
        instructions_url: INSTRUCTIONS_URL.into(),
    };
    let mut dirs = vec![
        paths.home().join(".local/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ];
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    let Some(program) = dirs
        .iter()
        .map(|p| p.join("skillhub"))
        .find(|p| p.is_absolute() && p.is_file())
    else {
        status.error = Some(AppError::marketplace_cli_unavailable(
            "未找到 skillhub 命令",
        ));
        return (status, None);
    };
    status.path = Some(program.to_string_lossy().into_owned());
    let mut env = crate::env::child_env::build_child_env(crate::env::child_env::ChildEnvParams {
        home: paths.home(),
        node_dir: program.parent(),
        git_dir: None,
        parent: &BTreeMap::new(),
    });
    for (k, v) in [
        ("SKILLHUB_HOST", "https://api.skillhub.cn"),
        ("SKILLHUB_API_BASE", "https://api.skillhub.cn"),
        ("SKILLHUB_SKIP_SELF_UPGRADE", "true"),
        ("SKILLHUB_SKIP_WORKSPACE_SKILLS", "true"),
        ("SKILLHUB_CALLER", "none"),
    ] {
        env.insert(k.into(), v.into());
    }
    let runtime = Runtime { program, env };
    for args in [vec!["--version"], vec!["install", "--help"]] {
        let mut spec = CommandSpec::new(
            &runtime.program,
            args.iter().map(|s| s.to_string()).collect(),
            paths.home(),
            runtime.env.clone(),
            Duration::from_secs(10),
        );
        spec.cli_context = Some(crate::cli::commands::CliCommandContext::background(
            "environment",
            Some("skillhub".into()),
        ));
        match executor.run(spec, CancelToken::new()).await {
            Ok(output) if output.success() => {
                if args[0] == "--version" {
                    let v = output.stdout.trim();
                    status.version = Some(v.into());
                    let parsed = v
                        .split_whitespace()
                        .last()
                        .and_then(|s| semver::Version::parse(s).ok());
                    if parsed.is_none_or(|v| v < semver::Version::new(2026, 8, 5)) {
                        status.error = Some(AppError::marketplace_cli_unavailable(
                            "需要支持 namespace 与固定版本的 SkillHub CLI（2026.8.5 或更高）",
                        ));
                        return (status, None);
                    }
                } else if !["--namespace", "--json", "--dir"]
                    .iter()
                    .all(|s| output.stdout.contains(s))
                {
                    status.error = Some(AppError::marketplace_cli_unavailable(
                        "CLI 缺少所需安装参数",
                    ));
                    return (status, None);
                }
            }
            Ok(output) => {
                status.error = Some(AppError::marketplace_cli_unavailable(output.stderr.trim()));
                return (status, None);
            }
            Err(e) => {
                status.error = Some(AppError::marketplace_cli_unavailable(format!("{e:?}")));
                return (status, None);
            }
        }
    }
    status.available = true;
    (status, Some(runtime))
}
