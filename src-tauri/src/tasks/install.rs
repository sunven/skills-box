//! Install execution: request validation, the pinned CLI command, its
//! `--json` result parser, the pre-spawn lock snapshot, and the post-install
//! verifier.
//!
//! The CLI's `installed` status is never trusted on its own: after the process
//! exits we check the canonical folder, the lock key, and every selected
//! non-universal agent attachment with `lstat`. A silent symlink-to-copy
//! fallback is reported as a warning instead of a failure.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;

use crate::agents::{DetectionContext, catalog, resolve_global_skills_dir};
use crate::cli::CliRuntime;
use crate::env::capture_process_env;
use crate::env::child_env::{ChildEnvParams, build_child_env};
use crate::error::AppError;
use crate::inventory::{AgentDir, ManagementState};
use crate::lock::LockFile;
use crate::paths::AppPaths;
use crate::process::CommandSpec;
use crate::security;

use super::conflicts::find_conflicts;
use super::manager::{TaskSkillResult, TaskSkillStatus};
use super::source::validate_source;

pub const INSTALL_TIMEOUT: Duration = crate::timeouts::WRITE_TIMEOUT;
pub const UNIVERSAL_AGENT_ID: &str = "universal";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallRequest {
    pub source: String,
    pub skills: Vec<String>,
    pub agents: Vec<String>,
    /// Explicit user retry of failed items from a previous partial install.
    /// Only a same-source managed/stale lock entry may be overwritten; plain
    /// installs keep rejecting every same-name conflict.
    #[serde(default)]
    pub retry: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallTarget {
    pub id: String,
    pub display_name: String,
    pub universal: bool,
    pub skills_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPlan {
    /// Normalized `owner/repo`.
    pub source: String,
    pub skills: Vec<String>,
    /// Explicit `-a` ids in request order, with `universal` appended last.
    pub agent_ids: Vec<String>,
    /// Selected agents resolved to absolute skills directories.
    pub targets: Vec<InstallTarget>,
    /// True only for an explicit retry request (see [`InstallRequest::retry`]).
    pub retry: bool,
}

impl InstallPlan {
    pub fn total(&self) -> u32 {
        self.skills.len() as u32
    }
}

/// Validate a full install request server-side. The frontend supplies only
/// semantic values (source, skill names, agent ids); everything else is
/// resolved here.
pub fn validate_request(
    paths: &AppPaths,
    request: &InstallRequest,
) -> Result<InstallPlan, AppError> {
    let source = validate_source(&request.source)?;
    let skills = validate_skill_names(&request.skills)?;
    let (agent_ids, targets) = validate_agents(paths, &request.agents)?;
    Ok(InstallPlan {
        source: source.normalized,
        skills,
        agent_ids,
        targets,
        retry: request.retry,
    })
}

/// Reject the CLI flags and wildcard forms the app must never send, plus
/// path-like, control-character or shell-metacharacter names. The executor
/// never uses a shell, so the metacharacter rule is defense in depth against a
/// malicious repository controlling a skill folder name.
pub fn validate_skill_names(raw: &[String]) -> Result<Vec<String>, AppError> {
    let mut seen = HashSet::new();
    let mut names = Vec::new();
    for value in raw {
        let name = value.trim();
        validate_skill_name(name)?;
        if seen.insert(name.to_string()) {
            names.push(name.to_string());
        }
    }
    if names.is_empty() {
        return Err(AppError::skill_invalid_name("至少需要一个 skill 名。"));
    }
    Ok(names)
}

/// The single skill-name rule shared by install/update and the native
/// detach/uninstall entry points. `value` must already be the exact name (no
/// trimming) so an inventory id can never resolve to a different directory.
pub fn validate_skill_name(value: &str) -> Result<(), AppError> {
    let invalid = value.is_empty()
        || value.len() > 255
        || value == "*"
        || value == "."
        || value == ".."
        || value.starts_with('-')
        || value.starts_with('*')
        || value.contains(['/', '\\', '\0'])
        || value.chars().any(char::is_control)
        || value.chars().any(is_shell_metacharacter);
    if invalid {
        return Err(AppError::skill_invalid_name(format!(
            "无效的 skill 名：{value:?}"
        )));
    }
    Ok(())
}

fn is_shell_metacharacter(character: char) -> bool {
    matches!(
        character,
        ';' | '|'
            | '&'
            | '$'
            | '`'
            | '<'
            | '>'
            | '"'
            | '\''
            | '('
            | ')'
            | '*'
            | '?'
            | '!'
            | '{'
            | '}'
            | '['
            | ']'
            | '~'
            | '#'
    )
}

fn validate_agents(
    paths: &AppPaths,
    raw: &[String],
) -> Result<(Vec<String>, Vec<InstallTarget>), AppError> {
    let mut seen = HashSet::new();
    let mut ids = Vec::new();
    for value in raw {
        let id = value.trim();
        let invalid = id.is_empty()
            || id.len() > 64
            || id == "*"
            || id.starts_with('-')
            || !id.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
            });
        if invalid {
            return Err(AppError::agent_invalid(format!(
                "无效的 agent 标识：{value:?}"
            )));
        }
        if id == UNIVERSAL_AGENT_ID {
            continue;
        }
        let definition = catalog()
            .agents
            .iter()
            .find(|definition| definition.id == id)
            .ok_or_else(|| AppError::agent_invalid(format!("未知 agent：{id}")))?;
        if definition.global_skills_dir.is_none() {
            return Err(AppError::agent_unsupported_global(&definition.display_name));
        }
        if seen.insert(definition.id.clone()) {
            ids.push(definition.id.clone());
        }
    }

    let detected = crate::inventory::detected_agent_dirs(paths);
    let mut targets = Vec::new();
    for id in ids {
        let target = detected
            .iter()
            .find(|agent| agent.id == id)
            .ok_or_else(|| {
                let display_name = catalog()
                    .agents
                    .iter()
                    .find(|definition| definition.id == id)
                    .map(|definition| definition.display_name.as_str())
                    .unwrap_or(id.as_str());
                AppError::agent_not_detected(display_name)
            })?;
        targets.push(InstallTarget {
            id: target.id.clone(),
            display_name: target.display_name.clone(),
            universal: target.universal,
            skills_dir: target.skills_dir.clone(),
        });
    }

    let agent_ids = targets
        .iter()
        .map(|target| target.id.clone())
        .chain(std::iter::once(UNIVERSAL_AGENT_ID.to_string()))
        .collect();
    Ok((agent_ids, targets))
}

/// Resolve known agent ids to absolute global skills directories without their
/// config roots existing. Install validation additionally requires detection;
/// this helper exists for verification fixtures and future callers.
pub fn resolve_agent_dirs(paths: &AppPaths, ids: &[String]) -> Vec<AgentDir> {
    let parent = capture_process_env();
    let env = build_child_env(ChildEnvParams {
        home: paths.home(),
        node_dir: None,
        git_dir: None,
        parent: &parent,
    });
    let context = DetectionContext::new(paths.home(), paths.cli_cwd(), env);
    ids.iter()
        .filter_map(|id| {
            let definition = catalog()
                .agents
                .iter()
                .find(|definition| &definition.id == id)?;
            let skills_dir = resolve_global_skills_dir(definition, &context)?;
            Some(AgentDir {
                id: definition.id.clone(),
                display_name: definition.display_name.clone(),
                universal: definition.universal,
                skills_dir,
            })
        })
        .collect()
}

/// Reject same-name managed/copy-only/unmanaged skills before spawning. The
/// CLI would otherwise overwrite them silently.
///
/// `retry_source` is only `Some(owner/repo)` for an explicit retry of a
/// previous partial install: a managed (including stale-lock) entry from the
/// same source may be overwritten, because this is exactly the skill that the
/// earlier attempt partially wrote. Every other duplicate stays rejected.
pub fn conflicts_for(
    paths: &AppPaths,
    skills: &[String],
    retry_source: Option<&str>,
) -> Result<Option<AppError>, AppError> {
    let conflicts = find_conflicts(paths, skills)?;
    for name in skills {
        if let Some(conflict) = conflicts.get(name) {
            let retryable_managed = retry_source.is_some()
                && conflict.management == ManagementState::Managed
                && conflict.source.as_deref() == retry_source;
            if retryable_managed {
                continue;
            }
            let management = match conflict.management {
                ManagementState::Managed => "已安装",
                ManagementState::CopyOnly => "仅副本",
                ManagementState::Unmanaged => "未受管",
            };
            let source = conflict.source.as_deref().unwrap_or("未知");
            return Ok(Some(AppError::skill_conflict(
                name,
                format!("{management}；现有来源：{source}"),
            )));
        }
    }
    Ok(None)
}

/// `npx -y skills@1.7.4 add <source> -g -s <names> -a <ids> -a universal -y --json`
pub fn install_command(runtime: &CliRuntime, plan: &InstallPlan, timeout: Duration) -> CommandSpec {
    let mut args = vec![
        "-y".to_string(),
        format!("skills@{}", runtime.pinned_version),
        "add".to_string(),
        plan.source.clone(),
        "-g".to_string(),
    ];
    for name in &plan.skills {
        args.push("-s".to_string());
        args.push(name.clone());
    }
    for id in &plan.agent_ids {
        args.push("-a".to_string());
        args.push(id.clone());
    }
    args.push("-y".to_string());
    args.push("--json".to_string());
    CommandSpec::new(
        &runtime.npx_path,
        args,
        &runtime.cwd,
        runtime.env.clone(),
        timeout,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliSkillStatus {
    Installed,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliSkillResult {
    pub name: Option<String>,
    pub status: CliSkillStatus,
    pub error: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallOutput {
    Results(Vec<CliSkillResult>),
    /// A fatal preflight object without a `name` (clone failure, no SKILL.md,
    /// invalid agents, ...).
    Fatal(String),
    /// Not a JSON array, an empty array, or an unknown entry shape.
    Unrecognized(String),
}

pub fn parse_install_output(raw: &str) -> InstallOutput {
    let value: serde_json::Value = match serde_json::from_str(raw.trim()) {
        Ok(value) => value,
        Err(error) => {
            return InstallOutput::Unrecognized(format!("CLI 输出不是 JSON：{error}"));
        }
    };
    let Some(entries) = value.as_array() else {
        return InstallOutput::Unrecognized("CLI 输出不是 JSON 数组。".to_string());
    };
    if entries.is_empty() {
        return InstallOutput::Unrecognized("CLI 返回了空结果。".to_string());
    }

    let mut results = Vec::new();
    for entry in entries {
        let Some(object) = entry.as_object() else {
            return InstallOutput::Unrecognized("CLI 结果包含非对象条目。".to_string());
        };
        let Some(status) = object.get("status").and_then(serde_json::Value::as_str) else {
            return InstallOutput::Unrecognized("CLI 结果缺少 status。".to_string());
        };
        let name = object
            .get("name")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        let error = object
            .get("error")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        let reason = object
            .get("reason")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);

        match status {
            "installed" => {
                if name.is_none() {
                    return InstallOutput::Unrecognized("installed 结果缺少 name。".to_string());
                }
                results.push(CliSkillResult {
                    name,
                    status: CliSkillStatus::Installed,
                    error: None,
                    reason: None,
                });
            }
            "skipped" => {
                if name.is_none() {
                    return InstallOutput::Unrecognized("skipped 结果缺少 name。".to_string());
                }
                results.push(CliSkillResult {
                    name,
                    status: CliSkillStatus::Skipped,
                    error: None,
                    reason,
                });
            }
            "failed" => {
                if name.is_none() {
                    return InstallOutput::Fatal(
                        error.unwrap_or_else(|| {
                            "skills CLI 预检失败（未提供错误信息）。".to_string()
                        }),
                    );
                }
                results.push(CliSkillResult {
                    name,
                    status: CliSkillStatus::Failed,
                    error,
                    reason: None,
                });
            }
            other => {
                return InstallOutput::Unrecognized(format!("CLI 返回了未知状态：{other}"));
            }
        }
    }
    InstallOutput::Results(results)
}

/// Copy `~/.agents/.skill-lock.json` into `backups/<ms>-install-<task>/`
/// before the CLI runs. A missing lock is a normal fresh-machine state and is
/// recorded as absent.
pub fn snapshot_lock(paths: &AppPaths, task_id: &str) -> Result<PathBuf, AppError> {
    let directory = paths
        .backups_dir()
        .join(format!(
            "{}-install-{}",
            now_ms(),
            security::safe_component(task_id)
        ));
    fs::create_dir_all(&directory)
        .map_err(|error| AppError::backup_failed(format!("{}：{error}", directory.display())))?;
    let source = paths.lock_path();
    let target = directory.join("skill-lock.json");
    match fs::copy(&source, &target) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::write(
                directory.join("lock-absent.txt"),
                b"skill lock did not exist before install\n",
            )
            .map_err(|error| {
                AppError::backup_failed(format!("{}：{error}", directory.display()))
            })?;
        }
        Err(error) => {
            return Err(AppError::backup_failed(format!(
                "{}：{error}",
                source.display()
            )));
        }
    }
    Ok(directory)
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InstallVerification {
    pub results: Vec<TaskSkillResult>,
    pub warnings: Vec<AppError>,
}

/// Verify every requested skill after the CLI exits. `installed` entries are
/// checked on disk; `failed`/`skipped` entries are authoritative for their own
/// outcome. Filesystem checks are best-effort and never panic.
pub fn verify_install(
    paths: &AppPaths,
    plan: &InstallPlan,
    cli_results: &[CliSkillResult],
) -> InstallVerification {
    let by_name: HashMap<&str, &CliSkillResult> = cli_results
        .iter()
        .filter_map(|result| result.name.as_deref().map(|name| (name, result)))
        .collect();
    let lock = LockFile::read(&paths.lock_path());
    let mut verification = InstallVerification::default();

    for name in &plan.skills {
        let entry = by_name.get(name.as_str());
        let outcome = match entry {
            None => Err(AppError::verify_failed(format!(
                "CLI 未返回 skill「{name}」的结果。"
            ))),
            Some(result) => match result.status {
                CliSkillStatus::Installed => {
                    verify_installed(paths, plan, &lock, name, &mut verification.warnings)
                }
                CliSkillStatus::Skipped => Err(AppError::skill_not_in_source(
                    name,
                    result
                        .reason
                        .clone()
                        .unwrap_or_else(|| "CLI 跳过了该 skill。".to_string()),
                )),
                CliSkillStatus::Failed => Err(AppError::skill_install_failed(
                    name,
                    result
                        .error
                        .clone()
                        .unwrap_or_else(|| "CLI 报告安装失败。".to_string()),
                )),
            },
        };

        let (status, error) = match outcome {
            Ok(()) => (TaskSkillStatus::Installed, None),
            Err(error) if error.code == crate::error::codes::SKILL_NOT_IN_SOURCE => {
                (TaskSkillStatus::Skipped, Some(error))
            }
            Err(error) => (TaskSkillStatus::Failed, Some(error)),
        };
        verification.results.push(TaskSkillResult {
            name: name.clone(),
            status,
            error,
            backup_path: None,
        });
    }
    verification
}

fn verify_installed(
    paths: &AppPaths,
    plan: &InstallPlan,
    lock: &Result<LockFile, AppError>,
    name: &str,
    warnings: &mut Vec<AppError>,
) -> Result<(), AppError> {
    let canonical = paths.canonical_skills_dir().join(name);
    match fs::symlink_metadata(&canonical) {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {}
        _ => return Err(AppError::verify_canonical_missing(name, &canonical)),
    }

    match lock {
        Ok(lock) => match lock.get(name) {
            None => return Err(AppError::verify_lock_missing(name)),
            Some(entry) => {
                // Update sources may carry `#ref`; the lock stores the base
                // repository. A different base means the CLI wrote a lock
                // entry for an unexpected repository.
                let expected = plan.source.split('#').next().unwrap_or(&plan.source);
                if entry.source != expected {
                    return Err(AppError::verify_lock_mismatch(
                        name,
                        expected,
                        &entry.source,
                    ));
                }
            }
        },
        Err(error) => {
            return Err(AppError::verify_failed(format!(
                "lock 无法读取：{}",
                error.message
            )));
        }
    }

    let canonical_resolved = fs::canonicalize(&canonical).unwrap_or_else(|_| canonical.clone());
    for target in plan.targets.iter().filter(|target| !target.universal) {
        let attachment = target.skills_dir.join(name);
        match fs::symlink_metadata(&attachment) {
            Err(_) => {
                return Err(AppError::verify_attachment_missing(
                    name,
                    &target.display_name,
                    &attachment,
                ));
            }
            Ok(metadata) if metadata.file_type().is_symlink() => {
                match fs::canonicalize(&attachment) {
                    Ok(resolved) if resolved == canonical_resolved => {}
                    Ok(resolved) => {
                        return Err(AppError::verify_attachment_mode(
                            name,
                            &target.display_name,
                            &attachment,
                            format!("符号链接指向 {}", resolved.display()),
                        ));
                    }
                    Err(error) => {
                        return Err(AppError::verify_attachment_mode(
                            name,
                            &target.display_name,
                            &attachment,
                            format!("符号链接无法解析：{error}"),
                        ));
                    }
                }
            }
            Ok(metadata) if metadata.file_type().is_dir() => {
                warnings.push(AppError::verify_copy_fallback(
                    name,
                    &target.display_name,
                    &attachment,
                ));
            }
            Ok(_) => {
                return Err(AppError::verify_attachment_mode(
                    name,
                    &target.display_name,
                    &attachment,
                    "既不是符号链接也不是目录",
                ));
            }
        }
    }
    Ok(())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::codes;
    use std::collections::BTreeMap;
    use std::path::Path;

    const SUCCESS: &str = include_str!("../../tests/fixtures/cli/install-success.txt");
    const PARTIAL: &str = include_str!("../../tests/fixtures/cli/install-partial.txt");
    const COPY: &str = include_str!("../../tests/fixtures/cli/install-copy.txt");
    const FAILED: &str = include_str!("../../tests/fixtures/cli/install-failed.txt");
    const EMPTY: &str = include_str!("../../tests/fixtures/cli/install-empty.txt");
    const FATAL_CLONE: &str = include_str!("../../tests/fixtures/cli/install-fatal-clone.txt");
    const NO_SKILLS: &str = include_str!("../../tests/fixtures/cli/install-noskills.txt");

    fn runtime() -> CliRuntime {
        CliRuntime {
            npx_path: PathBuf::from("/node/bin/npx"),
            node_dir: PathBuf::from("/node/bin"),
            git_dir: Some(PathBuf::from("/usr/bin")),
            cwd: PathBuf::from("/app-data/cli-cwd"),
            home: PathBuf::from("/Users/tester"),
            env: BTreeMap::from([
                ("CI".to_string(), "true".to_string()),
                ("DISABLE_TELEMETRY".to_string(), "1".to_string()),
            ]),
            pinned_version: crate::cli::PINNED_CLI_VERSION.to_string(),
        }
    }

    fn request(source: &str, skills: &[&str], agents: &[&str]) -> InstallRequest {
        InstallRequest {
            source: source.to_string(),
            skills: skills.iter().map(|value| value.to_string()).collect(),
            agents: agents.iter().map(|value| value.to_string()).collect(),
            retry: false,
        }
    }

    fn fixture() -> (tempfile::TempDir, AppPaths) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(home.join(".qoder")).unwrap();
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        (tmp, paths)
    }

    fn plan(skills: &[&str], agents: &[&str]) -> InstallPlan {
        InstallPlan {
            source: "acme/skills".to_string(),
            skills: skills.iter().map(|value| value.to_string()).collect(),
            agent_ids: agents
                .iter()
                .map(|value| value.to_string())
                .chain(std::iter::once(UNIVERSAL_AGENT_ID.to_string()))
                .collect(),
            targets: vec![InstallTarget {
                id: "qoder".to_string(),
                display_name: "Qoder".to_string(),
                universal: false,
                skills_dir: PathBuf::from("/Users/tester/.qoder/skills"),
            }],
            retry: false,
        }
    }

    fn write_lock(paths: &AppPaths, names: &[&str]) {
        let mut skills = serde_json::Map::new();
        for name in names {
            skills.insert(
                (*name).to_string(),
                serde_json::json!({
                    "source": "acme/skills",
                    "sourceType": "github",
                    "sourceUrl": "https://github.com/acme/skills.git",
                    "skillPath": format!("skills/{name}/SKILL.md"),
                    "skillFolderHash": "0123456789abcdef0123456789abcdef01234567",
                    "installedAt": "2026-01-01T00:00:00.000Z",
                    "updatedAt": "2026-01-01T00:00:00.000Z"
                }),
            );
        }
        fs::create_dir_all(paths.agents_root()).unwrap();
        fs::write(
            paths.lock_path(),
            serde_json::json!({ "version": 3, "skills": skills }).to_string(),
        )
        .unwrap();
    }

    fn install_state(paths: &AppPaths, name: &str, agent_dir: Option<&Path>, copy: bool) {
        let canonical = paths.canonical_skills_dir().join(name);
        fs::create_dir_all(&canonical).unwrap();
        fs::write(canonical.join("SKILL.md"), format!("# {name}\n")).unwrap();
        if let Some(agent_dir) = agent_dir {
            fs::create_dir_all(agent_dir).unwrap();
            let attachment = agent_dir.join(name);
            if copy {
                fs::create_dir_all(&attachment).unwrap();
            } else {
                #[cfg(unix)]
                std::os::unix::fs::symlink(&canonical, &attachment).unwrap();
            }
        }
    }

    fn verification_plan(home: &Path, skills: &[&str], agents: &[&str]) -> InstallPlan {
        let ids: Vec<String> = agents.iter().map(|value| value.to_string()).collect();
        let targets = resolve_agent_dirs(&AppPaths::new(home, home.join("app-data")), &ids)
            .into_iter()
            .map(|agent| InstallTarget {
                id: agent.id,
                display_name: agent.display_name,
                universal: agent.universal,
                skills_dir: agent.skills_dir,
            })
            .collect();
        InstallPlan {
            source: "acme/skills".to_string(),
            skills: skills.iter().map(|value| value.to_string()).collect(),
            agent_ids: agents
                .iter()
                .map(|value| value.to_string())
                .chain(std::iter::once(UNIVERSAL_AGENT_ID.to_string()))
                .collect(),
            targets,
            retry: false,
        }
    }

    #[test]
    fn install_command_is_exact_and_never_uses_forbidden_flags() {
        let plan = plan(&["find-skills", "to-spec"], &["claude-code"]);
        let spec = install_command(&runtime(), &plan, Duration::from_secs(900));
        assert_eq!(spec.program, PathBuf::from("/node/bin/npx"));
        assert_eq!(
            spec.args,
            vec![
                "-y",
                "skills@1.7.4",
                "add",
                "acme/skills",
                "-g",
                "-s",
                "find-skills",
                "-s",
                "to-spec",
                "-a",
                "claude-code",
                "-a",
                "universal",
                "-y",
                "--json"
            ]
        );
        assert_eq!(spec.cwd, PathBuf::from("/app-data/cli-cwd"));
        assert_eq!(spec.env["CI"], "true");
        assert_eq!(spec.timeout, Duration::from_secs(900));
        for forbidden in ["--all", "--copy", "*", "update", "check", "remove", "list"] {
            assert!(
                !spec.args.iter().any(|arg| arg == forbidden),
                "{forbidden} must never be part of install"
            );
        }
        assert_eq!(spec.args[2], "add");
        assert_eq!(spec.args.last().map(String::as_str), Some("--json"));
        assert_eq!(
            spec.args.iter().filter(|arg| *arg == "-a").count(),
            2,
            "exactly one -a per selected agent plus universal"
        );
    }

    #[test]
    fn validation_normalizes_source_dedupes_and_appends_universal() {
        let (_tmp, paths) = fixture();
        let plan = validate_request(
            &paths,
            &request(
                "https://github.com/acme/skills/",
                &["alpha", "alpha", "beta"],
                &["qoder", "universal", "qoder"],
            ),
        )
        .unwrap();
        assert_eq!(plan.source, "acme/skills");
        assert_eq!(plan.skills, vec!["alpha", "beta"]);
        assert_eq!(plan.agent_ids, vec!["qoder", "universal"]);
        assert_eq!(plan.targets.len(), 1);
        assert_eq!(
            plan.targets[0].skills_dir,
            paths.home().join(".qoder/skills")
        );
    }

    #[test]
    fn validation_rejects_copy_all_wildcard_and_flag_like_names() {
        let (_tmp, paths) = fixture();
        for forbidden in [
            "--copy", "--all", "*", "-s", "a/b", "..", "", "alpha;rm", "$(id)", "`id`", "a|b",
            "a&b", "a>b", "a<b", "a\"b", "a'b", "a(b)", "a*b", "a?b", "a{b}", "a[b]", "a~b", "a#b",
        ] {
            let error = validate_request(&paths, &request("acme/skills", &[forbidden], &["qoder"]))
                .unwrap_err();
            assert_eq!(error.code, codes::SKILL_INVALID_NAME, "{forbidden:?}");
        }
        let error = validate_request(&paths, &request("acme/skills", &[], &["qoder"])).unwrap_err();
        assert_eq!(error.code, codes::SKILL_INVALID_NAME);
    }

    #[test]
    fn validation_rejects_unknown_wildcard_and_unsupported_agents() {
        let (_tmp, paths) = fixture();
        for unknown in [
            "*",
            "--all",
            "not-an-agent",
            "-a",
            "qoder;rm",
            "../qoder",
            "qoder/../../x",
            "$(id)",
        ] {
            let error = validate_request(&paths, &request("acme/skills", &["alpha"], &[unknown]))
                .unwrap_err();
            assert_eq!(error.code, codes::AGENT_INVALID, "{unknown}");
        }
        let error = validate_request(
            &paths,
            &request("acme/skills", &["alpha"], &["promptscript"]),
        )
        .unwrap_err();
        assert_eq!(error.code, codes::AGENT_UNSUPPORTED_GLOBAL);
        let error =
            validate_request(&paths, &request("acme/skills", &["alpha"], &["eve"])).unwrap_err();
        assert_eq!(error.code, codes::AGENT_UNSUPPORTED_GLOBAL);
    }

    #[test]
    fn validation_rejects_agent_that_is_not_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        let error =
            validate_request(&paths, &request("acme/skills", &["alpha"], &["qoder"])).unwrap_err();
        assert_eq!(error.code, codes::AGENT_NOT_DETECTED);
    }

    #[test]
    fn parser_reads_success_partial_failures_and_fatal_errors() {
        let InstallOutput::Results(results) = parse_install_output(SUCCESS) else {
            panic!("success must parse");
        };
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name.as_deref(), Some("find-skills"));
        assert_eq!(results[0].status, CliSkillStatus::Installed);

        // `mode: "copy"` is informational only; mount truth comes from lstat.
        let InstallOutput::Results(results) = parse_install_output(COPY) else {
            panic!("copy fixture must parse");
        };
        assert_eq!(results[0].status, CliSkillStatus::Installed);

        let InstallOutput::Results(results) = parse_install_output(PARTIAL) else {
            panic!("partial must parse");
        };
        assert_eq!(results[0].status, CliSkillStatus::Skipped);
        assert_eq!(
            results[0].reason.as_deref(),
            Some("No matching skill found in source")
        );
        assert_eq!(results[1].status, CliSkillStatus::Installed);

        let InstallOutput::Results(results) = parse_install_output(FAILED) else {
            panic!("per-skill failure must parse");
        };
        assert_eq!(results[0].status, CliSkillStatus::Failed);
        assert!(
            results[0]
                .error
                .as_deref()
                .unwrap()
                .contains("PromptScript")
        );

        let InstallOutput::Fatal(error) = parse_install_output(FATAL_CLONE) else {
            panic!("fatal clone failure must be fatal");
        };
        assert!(error.contains("Failed to clone"));

        let InstallOutput::Fatal(error) = parse_install_output(NO_SKILLS) else {
            panic!("no-skills preflight must be fatal");
        };
        assert!(error.contains("No valid skills"));

        assert!(matches!(
            parse_install_output(EMPTY),
            InstallOutput::Unrecognized(_)
        ));
        assert!(matches!(
            parse_install_output("garbage"),
            InstallOutput::Unrecognized(_)
        ));
        assert!(matches!(
            parse_install_output(r#"[{"status":"installed"}]"#),
            InstallOutput::Unrecognized(_)
        ));
        assert!(matches!(
            parse_install_output(r#"[{"name":"x","status":"weird"}]"#),
            InstallOutput::Unrecognized(_)
        ));
    }

    #[test]
    fn verifier_checks_canonical_lock_and_agent_link() {
        let (tmp, paths) = fixture();
        let plan = verification_plan(paths.home(), &["find-skills"], &["qoder"]);
        let agent_dir = tmp.path().join("home/.qoder/skills");
        install_state(&paths, "find-skills", Some(&agent_dir), false);
        write_lock(&paths, &["find-skills"]);

        let result = verify_install(&paths, &plan, &parse_results(SUCCESS));
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].status, TaskSkillStatus::Installed);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn verifier_fails_missing_canonical_attachment_and_lock() {
        let (tmp, paths) = fixture();
        let plan = verification_plan(paths.home(), &["find-skills"], &["qoder"]);
        let cli = parse_results(SUCCESS);

        let result = verify_install(&paths, &plan, &cli);
        assert_eq!(result.results[0].status, TaskSkillStatus::Failed);
        assert_eq!(
            result.results[0].error.as_ref().unwrap().code,
            codes::VERIFY_CANONICAL_MISSING
        );

        let agent_dir = tmp.path().join("home/.qoder/skills");
        install_state(&paths, "find-skills", None, false);
        write_lock(&paths, &["find-skills"]);
        let result = verify_install(&paths, &plan, &cli);
        assert_eq!(
            result.results[0].error.as_ref().unwrap().code,
            codes::VERIFY_ATTACHMENT_MISSING
        );

        let _ = fs::remove_dir_all(&agent_dir);
        fs::create_dir_all(&agent_dir).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(paths.home().join("missing"), agent_dir.join("find-skills"))
            .unwrap();
        let result = verify_install(&paths, &plan, &cli);
        assert_eq!(
            result.results[0].error.as_ref().unwrap().code,
            codes::VERIFY_ATTACHMENT_MODE
        );
        #[cfg(unix)]
        fs::remove_file(agent_dir.join("find-skills")).unwrap();

        let result = verify_install(&paths, &plan, &cli);
        assert_eq!(
            result.results[0].error.as_ref().unwrap().code,
            codes::VERIFY_ATTACHMENT_MISSING
        );

        install_state(&paths, "find-skills", Some(&agent_dir), false);
        fs::remove_file(paths.lock_path()).unwrap();
        let result = verify_install(&paths, &plan, &cli);
        assert_eq!(
            result.results[0].error.as_ref().unwrap().code,
            codes::VERIFY_LOCK_MISSING
        );
    }

    #[test]
    fn verifier_reports_copy_fallback_as_a_warning() {
        let (tmp, paths) = fixture();
        let plan = verification_plan(paths.home(), &["find-skills"], &["qoder"]);
        let agent_dir = tmp.path().join("home/.qoder/skills");
        install_state(&paths, "find-skills", Some(&agent_dir), true);
        write_lock(&paths, &["find-skills"]);

        let result = verify_install(&paths, &plan, &parse_results(SUCCESS));
        assert_eq!(result.results[0].status, TaskSkillStatus::Installed);
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(result.warnings[0].code, codes::VERIFY_COPY_FALLBACK);
    }

    #[test]
    fn verifier_marks_skipped_and_cli_failed_entries_without_checking_disk() {
        let (_tmp, paths) = fixture();
        let plan = verification_plan(paths.home(), &["no-such-skill", "find-skills"], &["qoder"]);
        let result = verify_install(&paths, &plan, &parse_results(PARTIAL));
        assert_eq!(result.results[0].name, "no-such-skill");
        assert_eq!(result.results[0].status, TaskSkillStatus::Skipped);
        assert_eq!(
            result.results[0].error.as_ref().unwrap().code,
            codes::SKILL_NOT_IN_SOURCE
        );
        assert_eq!(result.results[1].status, TaskSkillStatus::Failed);
        assert_eq!(
            result.results[1].error.as_ref().unwrap().code,
            codes::VERIFY_CANONICAL_MISSING
        );
    }

    #[test]
    fn retry_permits_only_same_source_managed_or_stale_names() {
        let (_tmp, paths) = fixture();
        install_state(&paths, "alpha", None, false);
        write_lock(&paths, &["alpha"]);
        // Plain duplicate is always rejected.
        let error = conflicts_for(&paths, &["alpha".to_string()], None)
            .unwrap()
            .expect("plain duplicate must conflict");
        assert_eq!(error.code, codes::SKILL_CONFLICT);
        // A retry from the same source may overwrite the managed entry.
        assert!(
            conflicts_for(&paths, &["alpha".to_string()], Some("acme/skills"))
                .unwrap()
                .is_none()
        );
        // A different source stays a conflict even for a retry.
        let error = conflicts_for(&paths, &["alpha".to_string()], Some("other/repo"))
            .unwrap()
            .expect("different source must conflict");
        assert_eq!(error.code, codes::SKILL_CONFLICT);

        // Stale lock entries reserve the name too, but a same-source retry is
        // the partial-install case this flag exists for.
        fs::remove_dir_all(paths.canonical_skills_dir()).unwrap();
        let error = conflicts_for(&paths, &["alpha".to_string()], None)
            .unwrap()
            .expect("stale lock must conflict");
        assert_eq!(error.code, codes::SKILL_CONFLICT);
        assert!(
            conflicts_for(&paths, &["alpha".to_string()], Some("acme/skills"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn verifier_rejects_a_lock_entry_from_a_different_source() {
        let (tmp, paths) = fixture();
        let plan = verification_plan(paths.home(), &["find-skills"], &["qoder"]);
        let agent_dir = tmp.path().join("home/.qoder/skills");
        install_state(&paths, "find-skills", Some(&agent_dir), false);
        write_lock(&paths, &["find-skills"]);
        let raw = fs::read_to_string(paths.lock_path())
            .unwrap()
            .replace("acme/skills", "other/repo");
        fs::write(paths.lock_path(), raw).unwrap();

        let result = verify_install(&paths, &plan, &parse_results(SUCCESS));
        assert_eq!(result.results[0].status, TaskSkillStatus::Failed);
        assert_eq!(
            result.results[0].error.as_ref().unwrap().code,
            codes::VERIFY_LOCK_MISMATCH
        );
    }

    fn parse_results(raw: &str) -> Vec<CliSkillResult> {
        match parse_install_output(raw) {
            InstallOutput::Results(results) => results,
            other => panic!("expected results, got {other:?}"),
        }
    }
}
