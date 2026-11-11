//! Native attach: create a skill's attachment in one more agent without
//! reinstalling it. Inverse of detach; the canonical store and lock entry stay
//! untouched.
//!
//! Only managed skills with a canonical folder can be attached. Copy-only and
//! unmanaged skills, universal/shared agents, broken links and agents that
//! already have the skill are rejected; the frontend never offers them. Every
//! path is resolved on the server from the inventory and guarded: the target
//! must be exactly one entry inside that agent's skills directory and the
//! directory itself may not be a symlink.

use serde::Serialize;
use std::fs;

use crate::attachment_target::{self, AgentDirectoryState, TargetContext};
use crate::error::AppError;
use crate::logs::{OperationEntry, OperationLog};
use crate::paths::AppPaths;
use crate::scope_layout::ScopeLayout;

const ATTACH_ACTION: &str = "attach";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachResult {
    pub skill_id: String,
    pub agent_id: String,
    pub agent_name: String,
    pub attachment_path: String,
    pub canonical_path: String,
    pub log_path: String,
}

pub fn attach_agent(
    paths: &AppPaths,
    layout: &ScopeLayout,
    skill_id: &str,
    agent_id: &str,
) -> Result<AttachResult, AppError> {
    attach_agent_checked(paths, layout, skill_id, agent_id, &|| Ok(()))
}

pub(crate) fn attach_agent_checked(
    paths: &AppPaths,
    layout: &ScopeLayout,
    skill_id: &str,
    agent_id: &str,
    before_write: &dyn Fn() -> Result<(), AppError>,
) -> Result<AttachResult, AppError> {
    attach_checked(paths, layout, skill_id, agent_id, before_write, false)
}

/// Installation publication may link a universal agent's separate global
/// directory. The standalone Attach/Detach eligibility remains unchanged.
pub(crate) fn attach_publication_checked(
    paths: &AppPaths,
    layout: &ScopeLayout,
    skill_id: &str,
    agent_id: &str,
    before_write: &dyn Fn() -> Result<(), AppError>,
) -> Result<AttachResult, AppError> {
    attach_checked(paths, layout, skill_id, agent_id, before_write, true)
}

fn attach_checked(
    paths: &AppPaths,
    layout: &ScopeLayout,
    skill_id: &str,
    agent_id: &str,
    before_write: &dyn Fn() -> Result<(), AppError>,
    publication: bool,
) -> Result<AttachResult, AppError> {
    let name = layout.scope.parse_skill_id(skill_id)?.to_string();
    super::tasks::install::validate_skill_name(&name)?;
    // A corrupt lock for this scope blocks every mutation, including attach.
    layout.ensure_mutation_ready()?;

    let located = layout
        .locate(paths, &name)?
        .ok_or_else(|| AppError::skill_not_found(skill_id))?;
    // Project scope attaches against the spec §3.5 target set: globally
    // detected catalog agents plus configured extras, even when the project
    // directory does not exist yet (it is created below).
    let targets = if layout.is_global() {
        layout.agents.clone()
    } else {
        layout.install_targets(&crate::inventory::detected_agent_dirs(paths))
    };
    let resolved = if publication {
        attachment_target::resolve_publication_attach(
            &layout.canonical_dir,
            &located,
            &targets,
            agent_id,
            TargetContext::for_layout(layout),
            true,
        )?
    } else {
        attachment_target::resolve_attach(
            &layout.canonical_dir,
            &located,
            &targets,
            agent_id,
            TargetContext::for_layout(layout),
        )?
    };
    let agent = &resolved.agent;
    let canonical = &resolved.canonical;
    let target = &resolved.target;

    if matches!(resolved.directory_state, AgentDirectoryState::Missing(_)) {
        fs::create_dir_all(&agent.skills_dir).map_err(|error| {
            AppError::agent_attach_failed(format!(
                "无法创建 {}：{error}",
                agent.skills_dir.display()
            ))
        })?;
    }
    // The resolver checked the target; keep a final check after the directory
    // may have been created so a race still produces the historical message.
    if fs::symlink_metadata(target).is_ok() {
        return Err(AppError::agent_attach_failed(format!(
            "{} 已存在，已停止（不覆盖现有内容）。",
            target.display()
        )));
    }

    before_write()?;
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(canonical, target).map_err(|error| {
            AppError::agent_attach_failed(format!("无法创建符号链接 {}：{error}", target.display()))
        })?;
    }

    let created = fs::symlink_metadata(target)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false);
    if !created {
        return Err(AppError::agent_attach_failed(format!(
            "创建后未找到符号链接：{}",
            target.display()
        )));
    }

    let log = OperationLog::for_scope(paths, &layout.scope);
    let log_path = paths.display(log.path());
    let _ = log.append(&OperationEntry::new(
        ATTACH_ACTION,
        skill_id,
        vec![agent_id.to_string()],
        "attached",
        None,
        None,
    ));
    Ok(AttachResult {
        skill_id: skill_id.to_string(),
        agent_id: agent_id.to_string(),
        agent_name: agent.display_name.clone(),
        attachment_path: paths.display(target),
        canonical_path: paths.display(canonical),
        log_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use crate::inventory::AgentDir;
    use std::path::PathBuf;

    fn attach_agent(
        paths: &AppPaths,
        agents: &[AgentDir],
        skill_id: &str,
        agent_id: &str,
    ) -> Result<AttachResult, AppError> {
        let layout = ScopeLayout::global(paths, agents.to_vec());
        super::attach_agent(paths, &layout, skill_id, agent_id)
    }

    struct Fixture {
        _tmp: tempfile::TempDir,
        paths: AppPaths,
        agents: Vec<AgentDir>,
        claude: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path().join("home");
            let paths = AppPaths::new(&home, tmp.path().join("app-data"));
            paths.ensure_dirs().unwrap();
            let canonical = paths.canonical_skills_dir().join("alpha");
            fs::create_dir_all(&canonical).unwrap();
            fs::write(
                canonical.join("SKILL.md"),
                "---\nname: alpha\ndescription: alpha\n---\n",
            )
            .unwrap();
            fs::create_dir_all(home.join(".claude/skills")).unwrap();
            fs::write(
                paths.lock_path(),
                serde_json::json!({
                    "version": 3,
                    "skills": {
                        "alpha": {
                            "source": "acme/skills",
                            "sourceType": "github",
                            "sourceUrl": "https://github.com/acme/skills.git",
                            "skillPath": "skills/alpha/SKILL.md",
                            "skillFolderHash": "0123456789abcdef0123456789abcdef01234567",
                            "installedAt": "2026-01-01T00:00:00.000Z",
                            "updatedAt": "2026-01-01T00:00:00.000Z"
                        }
                    }
                })
                .to_string(),
            )
            .unwrap();
            let claude = home.join(".claude/skills");
            let agents = vec![AgentDir {
                id: "claude-code".to_string(),
                display_name: "Claude Code".to_string(),
                universal: false,
                skills_dir: claude.clone(),
            }];
            Self {
                _tmp: tmp,
                paths,
                agents,
                claude,
            }
        }
    }

    #[test]
    fn attaches_managed_skill_with_a_symlink_and_rejects_repeats() {
        let fixture = Fixture::new();
        let result = attach_agent(&fixture.paths, &fixture.agents, "alpha", "claude-code").unwrap();
        assert_eq!(result.agent_name, "Claude Code");
        let link = fixture.claude.join("alpha");
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::canonicalize(&link).unwrap(),
            fs::canonicalize(fixture.paths.canonical_skills_dir().join("alpha")).unwrap()
        );

        let error =
            attach_agent(&fixture.paths, &fixture.agents, "alpha", "claude-code").unwrap_err();
        assert_eq!(error.code, ErrorCode::AgentAlreadyAttached);
    }

    #[test]
    fn rejects_unmanaged_unknown_and_occupied_targets() {
        let fixture = Fixture::new();
        fs::remove_file(fixture.paths.lock_path()).unwrap();
        let error =
            attach_agent(&fixture.paths, &fixture.agents, "alpha", "claude-code").unwrap_err();
        assert_eq!(error.code, ErrorCode::SkillUnmanaged);

        let fixture = Fixture::new();
        let error = attach_agent(&fixture.paths, &fixture.agents, "alpha", "nope").unwrap_err();
        assert_eq!(error.code, ErrorCode::AgentNotDetected);

        let fixture = Fixture::new();
        // A plain file in the agent directory is invisible to the inventory
        // but still occupies the attachment path; attach must not overwrite it.
        fs::write(fixture.claude.join("alpha"), "noise").unwrap();
        let error =
            attach_agent(&fixture.paths, &fixture.agents, "alpha", "claude-code").unwrap_err();
        assert_eq!(error.code, ErrorCode::AgentAttachFailed);
    }

    #[test]
    fn rejects_universal_agents_and_copy_only_skills() {
        let fixture = Fixture::new();
        let canonical = fixture.paths.canonical_skills_dir();
        let agents = vec![AgentDir {
            id: "codex".to_string(),
            display_name: "Codex".to_string(),
            universal: true,
            skills_dir: canonical.clone(),
        }];
        let error = attach_agent(&fixture.paths, &agents, "alpha", "codex").unwrap_err();
        assert_eq!(error.code, ErrorCode::AgentNative);

        // Copy-only beta: a lock entry and an agent copy, no canonical folder.
        let copy = fixture.claude.join("beta");
        fs::create_dir_all(&copy).unwrap();
        fs::write(copy.join("SKILL.md"), "---\nname: beta\n---\n").unwrap();
        let raw = fs::read_to_string(fixture.paths.lock_path())
            .unwrap()
            .replace("\"alpha\"", "\"beta\"");
        fs::write(fixture.paths.lock_path(), raw).unwrap();
        fs::remove_dir_all(canonical.join("alpha")).unwrap();

        let error =
            attach_agent(&fixture.paths, &fixture.agents, "beta", "claude-code").unwrap_err();
        assert_eq!(error.code, ErrorCode::SkillCopyOnly);
    }

    #[test]
    fn project_attach_links_into_the_project_and_creates_a_missing_extra_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let root = tmp.path().join("projects/app");
        fs::create_dir_all(&home).unwrap();
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        paths.ensure_dirs().unwrap();
        fs::write(
            paths.agents_config_path(),
            r#"{
                "version": 1,
                "agents": [
                    {"id": "acme", "displayName": "Acme", "skillsDir": "~/.acme/skills", "projectSkillsDir": ".acme/skills"}
                ]
            }"#,
        )
        .unwrap();
        let canonical = root.join(".agents/skills/alpha");
        fs::create_dir_all(&canonical).unwrap();
        fs::write(
            canonical.join("SKILL.md"),
            "---\nname: alpha\ndescription: alpha\n---\n",
        )
        .unwrap();
        fs::write(
            root.join("skills-lock.json"),
            serde_json::json!({
                "version": 1,
                "skills": {
                    "alpha": {
                        "source": "acme/skills",
                        "sourceType": "github",
                        "sourceUrl": "https://github.com/acme/skills.git",
                        "skillPath": "skills/alpha/SKILL.md",
                        "computedHash": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        let project = crate::projects::Project {
            id: uuid::Uuid::new_v4(),
            root,
            display_name: "app".to_string(),
            added_at: 1,
            last_opened_at: 1,
            check_revision: "initial".into(),
        };
        let layout = ScopeLayout::project(&paths, project);
        let skill_id = layout.scope.skill_id("alpha");
        let extra_dir = layout.root.join(".acme/skills");
        assert!(!extra_dir.exists());

        let result = super::attach_agent(&paths, &layout, &skill_id, "acme").unwrap();
        assert_eq!(result.agent_name, "Acme");
        let link = extra_dir.join("alpha");
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::canonicalize(&link).unwrap(),
            fs::canonicalize(layout.canonical_dir.join("alpha")).unwrap()
        );
        assert!(
            !paths.canonical_skills_dir().exists(),
            "global canonical store untouched"
        );
    }
}
