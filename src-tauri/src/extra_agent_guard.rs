//! Guards and cleanup for extra-agent definitions that participate in project
//! scopes (spec §4.9, §7.5).
//!
//! Removing an extra must never leave invisible project attachments behind, and
//! changing its project directory must not silently change where existing
//! attachments point. Both operations therefore inspect every registered
//! project first; an unavailable or unreadable project always counts as "may
//! still hold attachments" and blocks the change.

use crate::agents::extras::{self, ExtraProjectDetachment, RemoveExtraAgentResult};
use crate::attachment_target::{DetachEligibility, detach_eligibility};
use crate::error::AppError;
use crate::inventory;
use crate::paths::AppPaths;
use crate::projects::{ProjectEntry, ProjectRegistry};
use crate::scope_layout::ScopeLayout;

/// One registered project and the skills attached to the extra inside it.
struct ProjectScan {
    index: usize,
    layout: ScopeLayout,
    skills: Vec<String>,
}

fn project_label(entry: &ProjectEntry) -> String {
    let name = &entry.project.display_name;
    let root = entry.project.root.display();
    match &entry.unavailable_reason {
        Some(reason) => format!("{name}（{root}）：{reason}"),
        None => format!("{name}（{root}）"),
    }
}

/// Scan one project for attachments of `agent_id`. A project lock that cannot
/// be read is an error: the caller must treat it as "cannot prove absent".
///
/// `detachable_only` is used for removal: only rows that `detach_agent` will
/// accept are collected, so a read-only (broken/unmanaged/copy-only) row never
/// aborts cleanup after other mounts were already removed.
fn scan_project(
    paths: &AppPaths,
    project: &crate::projects::Project,
    agent_id: &str,
    detachable_only: bool,
) -> Result<(ScopeLayout, Vec<String>), AppError> {
    let layout = ScopeLayout::project(paths, project.clone());
    let catalog = layout.catalog(paths)?;
    let mut skills = Vec::new();
    for name in &catalog.names {
        if detachable_only {
            let Some(located) = inventory::locate_in_catalog(&catalog, name) else {
                continue;
            };
            let Some(attachment) = located
                .attachments
                .iter()
                .find(|attachment| attachment.agent_id == agent_id)
            else {
                continue;
            };
            let eligibility = detach_eligibility(
                located.management,
                located.canonical_path.is_some(),
                false,
                Some(attachment.mode),
            );
            if eligibility == DetachEligibility::Detachable {
                skills.push(name.clone());
            }
            continue;
        }
        let attached = catalog
            .aggregates
            .get(name)
            .map(|aggregate| {
                aggregate
                    .attachments
                    .iter()
                    .any(|attachment| attachment.agent_id == agent_id)
            })
            .unwrap_or(false);
        if attached {
            skills.push(name.clone());
        }
    }
    Ok((layout, skills))
}

/// Block changing or clearing `projectSkillsDir` while any registered project
/// may still hold an attachment. Unavailable projects always block; available
/// projects block when a scan finds an attachment (or cannot be read).
pub fn ensure_project_dir_change_allowed(
    paths: &AppPaths,
    projects: &ProjectRegistry,
    agent_id: &str,
    current: Option<&str>,
    next: Option<&str>,
) -> Result<(), AppError> {
    if current == next {
        return Ok(());
    }
    let list = projects.list();
    let mut blockers = Vec::new();
    for entry in &list.projects {
        if !entry.available {
            blockers.push(format!(
                "{}：项目不可用，无法确认挂载",
                project_label(entry)
            ));
            continue;
        }
        match scan_project(paths, &entry.project, agent_id, false) {
            Ok((_, skills)) if !skills.is_empty() => blockers.push(format!(
                "{}：仍有 {} 个挂载（{}）",
                project_label(entry),
                skills.len(),
                skills.join("、")
            )),
            Ok(_) => {}
            Err(error) => blockers.push(format!(
                "{}：无法检查挂载（{}）",
                project_label(entry),
                error.message
            )),
        }
    }
    if blockers.is_empty() {
        Ok(())
    } else {
        Err(AppError::extra_agent_project_dir_blocked(&blockers))
    }
}

/// Detach the extra from the global scope and every registered project, then
/// remove its definition. Any unavailable or unreadable project blocks before
/// the first mutation so no partial cleanup is performed.
pub fn remove_extra_agent(
    paths: &AppPaths,
    projects: &ProjectRegistry,
    agent_id: &str,
) -> Result<RemoveExtraAgentResult, AppError> {
    let extra =
        extras::find(paths, agent_id).ok_or_else(|| AppError::extra_agent_not_found(agent_id))?;
    if extra.builtin {
        return Err(AppError::extra_agent_builtin(&extra.display_name));
    }
    let list = projects.list();
    let unavailable: Vec<String> = list
        .projects
        .iter()
        .filter(|entry| !entry.available)
        .map(project_label)
        .collect();
    if !unavailable.is_empty() {
        return Err(AppError::extra_agent_remove_blocked(&unavailable));
    }

    // Pre-scan both scopes so a failed read aborts before any unlink.
    let scans: Vec<ProjectScan> = list
        .projects
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let (layout, skills) =
                scan_project(paths, &entry.project, agent_id, true).map_err(|error| {
                    AppError::extra_agent_remove_blocked(&[format!(
                        "{}：无法检查挂载（{}）",
                        project_label(entry),
                        error.message
                    )])
                })?;
            Ok(ProjectScan {
                index,
                layout,
                skills,
            })
        })
        .collect::<Result<_, AppError>>()?;

    let agents = inventory::detected_agent_dirs(paths);
    let global_layout = ScopeLayout::global(paths, agents.clone());
    let snapshot = inventory::read_with_agents(paths, &agents)?;
    let global_skills: Vec<String> = snapshot
        .skills
        .iter()
        .filter(|skill| {
            skill
                .attachments
                .iter()
                .any(|attachment| attachment.agent_id == agent_id && attachment.detachable)
        })
        .map(|skill| skill.name.clone())
        .collect();

    let mut detached = Vec::new();
    for name in global_skills {
        let result = crate::detach::detach_agent(paths, &global_layout, &name, agent_id)?;
        detached.push(result.skill_id);
    }

    let mut project_detachments = Vec::new();
    for scan in scans {
        if scan.skills.is_empty() {
            continue;
        }
        let scope = scan.layout.scope.clone();
        let mut names = Vec::new();
        for name in scan.skills {
            let skill_id = scope.skill_id(&name);
            let result = crate::detach::detach_agent(paths, &scan.layout, &skill_id, agent_id)?;
            names.push(result.skill_id);
        }
        let entry = &list.projects[scan.index];
        project_detachments.push(ExtraProjectDetachment {
            project_id: entry.project.id.to_string(),
            project_name: entry.project.display_name.clone(),
            detached: names,
        });
    }

    extras::remove_user_agent(paths, agent_id)?;
    Ok(RemoveExtraAgentResult {
        id: agent_id.to_string(),
        display_name: extra.display_name,
        detached,
        projects: project_detachments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::AppPaths;
    use crate::projects::Project;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    struct Fixture {
        tmp: tempfile::TempDir,
        paths: AppPaths,
        registry: Arc<ProjectRegistry>,
        project: Project,
    }

    fn write_file(path: &Path, body: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn skill_md(name: &str) -> String {
        format!("---\nname: {name}\ndescription: {name}\n---\n")
    }

    fn write_global_lock(paths: &AppPaths, name: &str) {
        write_file(
            &paths.lock_path(),
            &serde_json::json!({
                "version": 3,
                "skills": {
                    name: {
                        "source": "acme/skills",
                        "sourceType": "github",
                        "sourceUrl": "https://github.com/acme/skills.git",
                        "skillPath": format!("skills/{name}/SKILL.md"),
                        "skillFolderHash": "0123456789abcdef0123456789abcdef01234567",
                        "installedAt": "2026-01-01T00:00:00.000Z",
                        "updatedAt": "2026-01-01T00:00:00.000Z"
                    }
                }
            })
            .to_string(),
        );
    }

    fn write_project_lock(root: &Path, name: &str) {
        write_file(
            &root.join("skills-lock.json"),
            &serde_json::json!({
                "version": 1,
                "skills": {
                    name: {
                        "source": "acme/skills",
                        "sourceType": "github",
                        "sourceUrl": "https://github.com/acme/skills.git",
                        "skillPath": format!("skills/{name}/SKILL.md"),
                        "computedHash": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    }
                }
            })
            .to_string(),
        );
    }

    impl Fixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path().join("home");
            fs::create_dir_all(home.join(".acme/skills")).unwrap();
            let paths = AppPaths::new(&home, tmp.path().join("app-data"));
            paths.ensure_dirs().unwrap();
            write_file(
                &paths.agents_config_path(),
                r#"{
                    "version": 1,
                    "agents": [
                        {"id": "acme", "displayName": "Acme", "skillsDir": "~/.acme/skills", "projectSkillsDir": ".acme/skills"}
                    ]
                }"#,
            );
            let global = paths.canonical_skills_dir().join("alpha");
            write_file(&global.join("SKILL.md"), &skill_md("alpha"));
            write_global_lock(&paths, "alpha");

            let root = tmp.path().join("projects/app");
            let canonical = root.join(".agents/skills/alpha");
            write_file(&canonical.join("SKILL.md"), &skill_md("alpha"));
            write_project_lock(&root, "alpha");
            let registry = Arc::new(ProjectRegistry::new(paths.clone()));
            let project = registry.add(root.to_str().unwrap()).unwrap().project;
            Self {
                tmp,
                paths,
                registry,
                project,
            }
        }

        fn attach_global(&self) {
            let agents = inventory::detected_agent_dirs(&self.paths);
            let layout = ScopeLayout::global(&self.paths, agents);
            crate::attach::attach_agent(&self.paths, &layout, "alpha", "acme").unwrap();
        }

        fn attach_project(&self) {
            let layout = ScopeLayout::project(&self.paths, self.project.clone());
            let skill_id = layout.scope.skill_id("alpha");
            crate::attach::attach_agent(&self.paths, &layout, &skill_id, "acme").unwrap();
        }

        fn project_root(&self) -> PathBuf {
            self.project.root.clone()
        }
    }

    #[test]
    fn removal_detaches_global_and_project_mounts_then_removes_the_definition() {
        let fixture = Fixture::new();
        fixture.attach_global();
        fixture.attach_project();
        let global_link = fixture.paths.home().join(".acme/skills/alpha");
        let project_link = fixture.project_root().join(".acme/skills/alpha");
        assert!(fs::symlink_metadata(&global_link).is_ok());
        assert!(fs::symlink_metadata(&project_link).is_ok());

        let result = remove_extra_agent(&fixture.paths, &fixture.registry, "acme").unwrap();
        assert_eq!(result.id, "acme");
        assert_eq!(result.detached, vec!["alpha".to_string()]);
        assert_eq!(result.projects.len(), 1);
        assert_eq!(
            result.projects[0].project_id,
            fixture.project.id.to_string()
        );
        assert_eq!(
            result.projects[0].detached,
            vec![format!("project:{}/alpha", fixture.project.id)]
        );
        assert!(fs::symlink_metadata(&global_link).is_err());
        assert!(fs::symlink_metadata(&project_link).is_err());
        assert!(extras::find(&fixture.paths, "acme").is_none());
    }

    #[test]
    fn unavailable_project_blocks_removal_and_lists_the_project() {
        let fixture = Fixture::new();
        fixture.attach_global();
        let gone = fixture.tmp.path().join("projects/gone");
        fs::create_dir_all(&gone).unwrap();
        fixture.registry.add(gone.to_str().unwrap()).unwrap();
        fs::remove_dir_all(&gone).unwrap();
        let global_link = fixture.paths.home().join(".acme/skills/alpha");
        assert!(fs::symlink_metadata(&global_link).is_ok());

        let error = remove_extra_agent(&fixture.paths, &fixture.registry, "acme").unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::ExtraProjectBlocked);
        assert!(
            error.detail.as_deref().unwrap_or_default().contains("gone"),
            "{error:?}"
        );
        // Nothing was detached and the definition still exists.
        assert!(fs::symlink_metadata(&global_link).is_ok());
        assert!(extras::find(&fixture.paths, "acme").is_some());
    }

    #[test]
    fn changing_or_clearing_the_project_dir_blocks_while_a_mount_exists() {
        let fixture = Fixture::new();
        fixture.attach_project();
        for next in [Some(".acme2/skills"), None] {
            let error = ensure_project_dir_change_allowed(
                &fixture.paths,
                &fixture.registry,
                "acme",
                Some(".acme/skills"),
                next,
            )
            .unwrap_err();
            assert_eq!(error.code, crate::error::ErrorCode::ExtraProjectBlocked);
            assert!(
                error.detail.as_deref().unwrap_or_default().contains("app"),
                "{error:?}"
            );
        }
        // Same value is never a change and needs no guard.
        ensure_project_dir_change_allowed(
            &fixture.paths,
            &fixture.registry,
            "acme",
            Some(".acme/skills"),
            Some(".acme/skills"),
        )
        .unwrap();
    }

    #[test]
    fn unavailable_project_blocks_project_dir_changes_even_without_a_known_mount() {
        let fixture = Fixture::new();
        let gone = fixture.tmp.path().join("projects/gone");
        fs::create_dir_all(&gone).unwrap();
        fixture.registry.add(gone.to_str().unwrap()).unwrap();
        fs::remove_dir_all(&gone).unwrap();

        let error = ensure_project_dir_change_allowed(
            &fixture.paths,
            &fixture.registry,
            "acme",
            None,
            Some(".acme/skills"),
        )
        .unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::ExtraProjectBlocked);
        assert!(
            error.detail.as_deref().unwrap_or_default().contains("gone"),
            "{error:?}"
        );
    }

    #[test]
    fn project_dir_change_is_allowed_when_no_project_holds_a_mount() {
        let fixture = Fixture::new();
        ensure_project_dir_change_allowed(
            &fixture.paths,
            &fixture.registry,
            "acme",
            Some(".acme/skills"),
            Some(".acme2/skills"),
        )
        .unwrap();
        ensure_project_dir_change_allowed(
            &fixture.paths,
            &fixture.registry,
            "acme",
            Some(".acme/skills"),
            None,
        )
        .unwrap();
    }
}
