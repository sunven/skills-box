//! A pack owns one mutation permit across staging, sequential publication and workflow completion.
use super::{install::InstallTarget, marketplace, publication, uninstall::UninstallPlan};
use crate::{
    attachment_target::{ResolvedAttachment, TargetContext},
    backup::{BackupAction, BackupStore},
    baseline::fingerprint_folder,
    cli::{
        CliRuntime,
        preview::{CliCommandEntry, CliCommandPreview, CommandPlan},
    },
    error::AppError,
    expert_packs::{Snapshot, records::Records, state::clean_content},
    inventory::{LocatedSkill, ManagementState},
    lock::LockEntry,
    paths::AppPaths,
    scope_layout::ScopeLayout,
};
use serde::Serialize;
use std::{fs, path::PathBuf};

#[derive(Debug, Clone)]
pub struct Unit {
    pub name: String,
    pub download: Option<marketplace::Plan>,
    pub expected: Option<LockEntry>,
    pub previous: Option<UninstallPlan>,
    pub attachments: Vec<ResolvedAttachment>,
}
#[derive(Debug, Clone)]
pub struct Plan {
    pub snapshot: Snapshot,
    pub previous_snapshot: Option<Snapshot>,
    pub layout: ScopeLayout,
    pub targets: Vec<InstallTarget>,
    pub units: Vec<Unit>,
    pub created_at: u64,
    pub nonce: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewItem {
    pub name: String,
    pub action: String,
    pub version: Option<String>,
    pub snapshot_version: Option<String>,
    pub missing_agents: Vec<String>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub snapshot: Snapshot,
    pub items: Vec<PreviewItem>,
    pub commands: CliCommandPreview,
    pub supplement: bool,
}

fn resolved_attachments(
    paths: &AppPaths,
    layout: &ScopeLayout,
    name: &str,
    targets: &[InstallTarget],
    entry: Option<LockEntry>,
) -> Result<Vec<ResolvedAttachment>, AppError> {
    let located = layout.locate(paths, name)?.unwrap_or_else(|| LocatedSkill {
        name: name.into(),
        primary_path: Some(layout.canonical_dir.join(name)),
        canonical_path: Some(layout.canonical_dir.join(name)),
        management: ManagementState::Managed,
        lock_entry: entry,
        attachments: vec![],
    });
    if located.management != ManagementState::Managed
        || located.canonical_path.as_ref() != Some(&layout.canonical_dir.join(name))
    {
        return Err(AppError::skill_conflict(name, "仅允许复用完整受管技能。"));
    }
    let agents = if layout.is_global() {
        layout.agents.clone()
    } else {
        layout.install_targets(&crate::inventory::detected_agent_dirs(paths))
    };
    let mut result = vec![];
    for target in targets
        .iter()
        .filter(|t| t.needs_attachment(&layout.canonical_dir))
    {
        if let Some(existing) = located.attachments.iter().find(|a| a.agent_id == target.id) {
            if existing.mode != crate::inventory::AttachmentMode::Link
                || fs::canonicalize(&existing.path).ok()
                    != fs::canonicalize(layout.canonical_dir.join(name)).ok()
            {
                return Err(AppError::skill_conflict(
                    name,
                    "目标已有副本、损坏或其他来源挂载。",
                ));
            }
            continue;
        }
        result.push(crate::attachment_target::resolve_publication_attach(
            &layout.canonical_dir,
            &located,
            &agents,
            &target.id,
            TargetContext::for_layout(layout),
            false,
        )?);
    }
    Ok(result)
}
impl Plan {
    pub fn build(
        paths: &AppPaths,
        layout: ScopeLayout,
        snapshot: Snapshot,
        previous_snapshot: Option<Snapshot>,
        downloads: Vec<marketplace::Plan>,
    ) -> Result<Self, AppError> {
        snapshot.validate()?;
        let (_, targets) =
            super::install::validate_selected_agents(paths, &layout, &snapshot.agents)?;
        let lock = layout.lock()?;
        let market = crate::marketplace::records::Records::read(&layout.lock_path)?;
        let workflows = Records::read(&layout.lock_path)?;
        if workflows
            .workflows
            .values()
            .any(|w| w.slug == snapshot.slug && w.name != snapshot.workflow_name)
        {
            return Err(AppError::skill_conflict(
                &snapshot.workflow_name,
                "同一专家包的旧入口名称已变化，请先单独卸载旧入口。",
            ));
        }
        let mut units = vec![];
        let mut conflicts = vec![];
        for name in snapshot.names() {
            let unit = (|| {
                let expected = lock.get(name).cloned();
                let member = snapshot.members.iter().find(|m| m.name == name);
                let download = downloads.iter().find(|p| p.name() == name).cloned();
                if let Some(entry) = &expected {
                    let same = if let Some(member) = member {
                        market
                            .skills
                            .get(name)
                            .is_some_and(|r| r.coordinate == member.coordinate)
                    } else {
                        workflows.workflows.get(name).is_some_and(|r| {
                            r.slug == snapshot.slug && r.markdown == snapshot.installed_workflow
                        })
                    };
                    if !same {
                        return Err(AppError::skill_conflict(
                            name,
                            "名称已由其他来源占用，或残留入口与此次内容不同。请先处理已有技能。",
                        ));
                    }
                    clean_content(&layout, name, &entry.skill_folder_hash)?;
                    if previous_snapshot.is_none()
                        && member.is_some_and(|m| {
                            entry.reference.as_deref() != Some(m.version.as_str())
                                || fs::read_to_string(
                                    layout.canonical_dir.join(name).join("SKILL.md"),
                                )
                                .ok()
                                .as_ref()
                                    != Some(&m.markdown)
                        })
                    {
                        return Err(AppError::batch_target_changed(
                            "成员在准备预览期间发生变化，请重新预览。",
                        ));
                    }
                } else if let Some(error) =
                    super::install::conflicts_for(paths, &layout, &[name.into()], None)?
                {
                    return Err(error);
                }
                if member.is_some() && expected.is_none() && download.is_none() {
                    return Err(AppError::verify_failed("成员缺少固定版本安装计划"));
                }
                let previous = if expected.is_some() {
                    Some(super::uninstall::plan_uninstall(
                        paths,
                        &layout,
                        &layout.scope.skill_id(name),
                    )?)
                } else {
                    None
                };
                let attachments =
                    resolved_attachments(paths, &layout, name, &targets, expected.clone())?;
                Ok::<_, AppError>(Unit {
                    name: name.into(),
                    download,
                    expected,
                    previous,
                    attachments,
                })
            })();
            match unit {
                Ok(unit) => units.push(unit),
                Err(error) => conflicts.push(format!(
                    "{name}：{} {}",
                    error.message,
                    error.detail.unwrap_or_default()
                )),
            }
        }
        if !conflicts.is_empty() {
            return Err(AppError::skill_conflict(
                &snapshot.workflow_name,
                conflicts.join("\n"),
            ));
        }
        let plan = Self {
            snapshot,
            previous_snapshot,
            layout,
            targets,
            units,
            created_at: crate::time::now_ms(),
            nonce: uuid::Uuid::new_v4().to_string(),
        };
        plan.check(paths, &plan.layout)?;
        Ok(plan)
    }
    pub fn check_scope(&self, layout: &ScopeLayout) -> Result<(), AppError> {
        layout.ensure_mutation_ready()?;
        layout.lock()?;
        if self.layout.scope != layout.scope
            || self.layout.root != layout.root
            || self.layout.project.as_ref().map(|p| &p.check_revision)
                != layout.project.as_ref().map(|p| &p.check_revision)
        {
            return Err(AppError::batch_target_changed(
                "项目位置已变化，请重新预览专家包。",
            ));
        }
        if let Some(root) = layout.project_root() {
            crate::project_path::resolve(
                &layout.canonical_dir,
                root,
                crate::project_path::Existence::AllowMissing,
            )
            .map_err(|e| AppError::path_invalid(format!("{e:?}")))?;
        }
        if fs::symlink_metadata(&layout.canonical_dir).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(AppError::path_invalid("技能目录不能是符号链接"));
        }
        Ok(())
    }
    pub fn check_unit(
        &self,
        paths: &AppPaths,
        layout: &ScopeLayout,
        unit: &Unit,
    ) -> Result<(), AppError> {
        self.check_scope(layout)?;
        let (_, targets) =
            super::install::validate_selected_agents(paths, layout, &self.snapshot.agents)?;
        if targets != self.targets {
            return Err(AppError::batch_target_changed(
                "Agent 目标已变化，请重新预览。",
            ));
        }
        if layout.lock()?.get(&unit.name) != unit.expected.as_ref() {
            return Err(AppError::cli_command_changed());
        }
        if let Some(previous) = &unit.previous {
            if &super::uninstall::plan_uninstall(paths, layout, &layout.scope.skill_id(&unit.name))?
                != previous
            {
                return Err(AppError::batch_target_changed(
                    "技能挂载已变化，请重新预览。",
                ));
            }
            clean_content(
                layout,
                &unit.name,
                &unit.expected.as_ref().unwrap().skill_folder_hash,
            )?;
        } else if let Some(error) =
            super::install::conflicts_for(paths, layout, std::slice::from_ref(&unit.name), None)?
        {
            return Err(error);
        }
        let current = resolved_attachments(
            paths,
            layout,
            &unit.name,
            &self.targets,
            unit.expected.clone(),
        )?;
        if current.len() != unit.attachments.len()
            || current.iter().zip(&unit.attachments).any(|(a, b)| {
                a.agent != b.agent
                    || a.target != b.target
                    || a.canonical != b.canonical
                    || a.target_state != b.target_state
            })
        {
            return Err(AppError::batch_target_changed(
                "目标挂载路径已变化，请重新预览。",
            ));
        }
        if let Some(download) = &unit.download {
            download.check(paths, layout)?;
        }
        Ok(())
    }
    pub fn check(&self, paths: &AppPaths, layout: &ScopeLayout) -> Result<(), AppError> {
        self.check_scope(layout)?;
        if Records::read(&layout.lock_path)?
            .packs
            .get(&self.snapshot.slug)
            != self.previous_snapshot.as_ref()
        {
            return Err(AppError::cli_command_changed());
        }
        let (_, targets) =
            super::install::validate_selected_agents(paths, layout, &self.snapshot.agents)?;
        if targets != self.targets {
            return Err(AppError::cli_command_changed());
        }
        for unit in &self.units {
            self.check_unit(paths, layout, unit)?;
        }
        Ok(())
    }
    pub fn preview(&self) -> Preview {
        Preview {
            snapshot: self.snapshot.clone(),
            supplement: self.previous_snapshot.is_some(),
            commands: self.command_preview(&self.layout, None),
            items: self
                .units
                .iter()
                .map(|unit| PreviewItem {
                    name: unit.name.clone(),
                    action: if unit.expected.is_none() {
                        "install"
                    } else if unit
                        .attachments
                        .iter()
                        .any(|a| a.target_state == crate::attachment_target::TargetState::Absent)
                    {
                        "attach"
                    } else {
                        "reuse"
                    }
                    .into(),
                    version: unit
                        .download
                        .as_ref()
                        .map(|d| d.detail.summary.version.clone())
                        .or_else(|| unit.expected.as_ref().and_then(|e| e.reference.clone())),
                    snapshot_version: self
                        .snapshot
                        .members
                        .iter()
                        .find(|m| m.name == unit.name)
                        .map(|m| m.version.clone()),
                    missing_agents: if unit.expected.is_none() {
                        self.snapshot.agents.clone()
                    } else {
                        unit.attachments
                            .iter()
                            .filter(|a| {
                                a.target_state == crate::attachment_target::TargetState::Absent
                            })
                            .map(|a| a.agent.id.clone())
                            .collect()
                    },
                })
                .collect(),
        }
    }
}
impl CommandPlan for Plan {
    fn needs_cli(&self) -> bool {
        false
    }
    fn command_preview(&self, layout: &ScopeLayout, _: Option<&CliRuntime>) -> CliCommandPreview {
        CliCommandPreview::new(
            &layout.scope,
            &format!("expert-pack:{}", self.nonce),
            self.units
                .iter()
                .map(|u| CliCommandEntry {
                    name: u.name.clone(),
                    command: u
                        .download
                        .as_ref()
                        .map(|p| crate::cli::commands::display_command(&p.command())),
                    reason: Some(
                        if u.expected.is_some() {
                            "复用已有内容并补齐挂载"
                        } else if u.download.is_some() {
                            "暂存并校验确认版本"
                        } else {
                            "所有成员就绪后发布工作流"
                        }
                        .into(),
                    ),
                })
                .collect(),
        )
    }
}

pub(super) async fn run(
    paths: AppPaths,
    ctx: super::session::SessionContext,
    plan: Plan,
) -> super::manager::FinishOutcome {
    use super::manager::{
        FinishOutcome, TaskAction, TaskEvent, TaskOverall, TaskPhase, TaskResult, TaskSkillResult,
        TaskSkillStatus,
    };
    let total = plan.units.len() as u32;
    let mut rows: Vec<_> = plan
        .units
        .iter()
        .map(|u| TaskSkillResult {
            name: u.name.clone(),
            status: TaskSkillStatus::Skipped,
            error: None,
            backup_path: None,
        })
        .collect();
    let mut warnings = vec![];
    let mut logged = vec![false; rows.len()];
    let operation_log = crate::logs::OperationLog::for_scope(&paths, &plan.layout.scope);
    let mut backup_path = None;
    let preparation = (|| {
        plan.check(&paths, &plan.layout)?;
        let backup = BackupStore::for_layout(&paths, &plan.layout).create(
            BackupAction::Install,
            &plan.snapshot.workflow_name,
            None,
            &[],
            &plan.snapshot.agents,
        )?;
        (ctx.record_snapshot)(&backup.path);
        backup_path = Some(paths.display(&backup.path));
        plan.check_scope(&plan.layout)?;
        let mut records = Records::read(&plan.layout.lock_path)?;
        if records.packs.get(&plan.snapshot.slug) != plan.previous_snapshot.as_ref() {
            return Err(AppError::cli_command_changed());
        }
        records
            .packs
            .insert(plan.snapshot.slug.clone(), plan.snapshot.clone());
        records.save(&plan.layout.lock_path)
    })();
    let mut stopped = preparation.is_err();
    if let Err(e) = preparation {
        rows[0].status = TaskSkillStatus::Failed;
        rows[0].error = Some(e);
    }
    let mut staged: Vec<Option<(PathBuf, String)>> = vec![None; plan.units.len()];
    // Download phase is cancellable and cannot publish any skill.
    for (i, unit) in plan.units.iter().enumerate() {
        if stopped || ctx.cancel.is_cancelled() {
            break;
        }
        let Some(download) = &unit.download else {
            continue;
        };
        (ctx.sink)(TaskEvent::progress(
            &ctx.task_id,
            TaskPhase::RunningCli,
            format!("正在暂存并校验 {}…", unit.name),
            i as u32,
            total,
        ));
        let mut command = download.command();
        if !ctx
            .expected_commands
            .as_ref()
            .is_some_and(|c| c.contains(&crate::cli::commands::display_command(&command)))
        {
            rows[i].status = TaskSkillStatus::Failed;
            rows[i].error = Some(AppError::cli_command_changed());
            stopped = true;
            break;
        }
        command.cli_context = Some(crate::cli::commands::CliCommandContext::task(
            &ctx.task_id,
            ctx.envelope.scope.clone(),
        ));
        let result = match ctx.executor.run(command, ctx.cancel.clone()).await {
            Ok(output) => marketplace::validate_staged(download, &output),
            Err(e) => Err(crate::error::cli::classify_exec_error(
                crate::error::cli::CliAction::Install,
                e,
            )),
        };
        match result {
            Ok(root) => match fingerprint_folder(&root) {
                Ok(hash) => staged[i] = Some((root, hash.content_hash)),
                Err(e) => {
                    rows[i].status = TaskSkillStatus::Failed;
                    rows[i].error = Some(AppError::skill_fingerprint_failed(e));
                    stopped = true;
                }
            },
            Err(e) => {
                rows[i].status = TaskSkillStatus::Failed;
                rows[i].error = Some(e);
                stopped = true;
            }
        }
    }
    for (i, unit) in plan.units.iter().enumerate() {
        if stopped || ctx.cancel.is_cancelled() {
            break;
        }
        // Workflow is the last unit. Re-observe every member including its original targets.
        if i + 1 == plan.units.len() {
            match crate::expert_packs::state::observe(&paths, &plan.layout, &plan.snapshot) {
                Ok(state)
                    if state.skills.iter().take(i).all(|s| {
                        s.present && !s.modified && !s.conflict && s.missing_agents.is_empty()
                    }) => {}
                Ok(_) => {
                    rows[i].error = Some(AppError::verify_failed(
                        "成员或必要挂载未就绪，工作流未执行。",
                    ));
                    break;
                }
                Err(e) => {
                    rows[i].status = TaskSkillStatus::Failed;
                    rows[i].error = Some(e);
                    break;
                }
            }
        }
        (ctx.sink)(TaskEvent::progress(
            &ctx.task_id,
            TaskPhase::Verifying,
            format!("正在安装 {}…", unit.name),
            i as u32,
            total,
        ));
        // Keep cancellation accepted; writes settle this unit before observing the token.
        let worker_paths = paths.clone();
        let worker_plan = plan.clone();
        let worker_unit = unit.clone();
        let root = staged[i].clone();
        let record = ctx.record_snapshot.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            let layout = &worker_plan.layout;
            if Records::read(&layout.lock_path)
                .ok()
                .and_then(|r| r.packs.get(&worker_plan.snapshot.slug).cloned())
                .as_ref()
                != Some(&worker_plan.snapshot)
            {
                return (
                    Err(AppError::batch_target_changed(
                        "专家包快照在执行期间变化，已停止后续项。",
                    )),
                    None,
                    true,
                );
            }
            if let Err(e) = worker_plan.check_unit(&worker_paths, layout, &worker_unit) {
                let critical = worker_plan.check_scope(layout).is_err();
                return (Err(e), None, critical);
            }
            let prepared = (|| {
                let previous = worker_unit.previous.as_ref();
                let snapshot = BackupStore::for_layout(&worker_paths, layout).create(
                    BackupAction::Install,
                    &worker_unit.name,
                    previous.and_then(|p| p.canonical.as_deref()),
                    &previous.map(|p| p.backup_attachments()).unwrap_or_default(),
                    &worker_plan.snapshot.agents,
                )?;
                record(&snapshot.path);
                Ok::<_, AppError>(snapshot)
            })();
            let backup = match prepared {
                Ok(backup) => backup,
                Err(e) => return (Err(e), None, true),
            };
            let result = (|| {
                worker_plan.check_unit(&worker_paths, layout, &worker_unit)?;
                if let Some(download) = &worker_unit.download {
                    let (root, expected_hash) = root.as_ref().unwrap();
                    if fingerprint_folder(root)
                        .map_err(AppError::skill_fingerprint_failed)?
                        .content_hash
                        != *expected_hash
                    {
                        return Err(AppError::verify_failed("暂存内容已变化，成员未发布。"));
                    }
                    Ok(publication::publish(
                        &worker_paths,
                        publication::Publication::Marketplace {
                            plan: download,
                            root,
                        },
                    ))
                } else if worker_unit.name == worker_plan.snapshot.workflow_name {
                    Ok(publication::publish(
                        &worker_paths,
                        publication::Publication::Workflow {
                            plan: &worker_plan,
                            unit: &worker_unit,
                        },
                    ))
                } else {
                    Ok(publication::Outcome::from(publication::attach(
                        &worker_paths,
                        layout,
                        &worker_unit.name,
                        &worker_plan.targets,
                        &|| worker_plan.check_scope(layout),
                    )))
                }
            })();
            let critical = worker_plan.check_scope(layout).is_err()
                || Records::read(&layout.lock_path).is_err();
            (result, Some(worker_paths.display(&backup.path)), critical)
        })
        .await;
        let (result, backup, critical) = match outcome {
            Ok(outcome) => outcome,
            Err(e) => (Err(AppError::task_internal("专家包发布", e)), None, true),
        };
        stopped = critical;
        if backup.is_some() {
            backup_path = backup.clone();
        }
        rows[i].backup_path = backup.clone();
        let publication = result.unwrap_or_else(|error| Err(error).into());
        warnings.extend(publication.warnings);
        match publication.result {
            Ok(()) => rows[i].status = TaskSkillStatus::Installed,
            Err(e) => {
                rows[i].status = TaskSkillStatus::Failed;
                rows[i].error = Some(e);
            }
        }
        logged[i] = true;
        let _ = operation_log.append(&crate::logs::OperationEntry::new(
            "install",
            &unit.name,
            plan.snapshot.agents.clone(),
            super::update::status_label(rows[i].status),
            backup,
            rows[i].error.clone(),
        ));
        (ctx.sink)(TaskEvent::progress(
            &ctx.task_id,
            TaskPhase::Verifying,
            format!("已处理 {} / {total} 项", i + 1),
            (i + 1) as u32,
            total,
        ));
    }
    for (i, row) in rows.iter().enumerate().filter(|(i, _)| !logged[*i]) {
        let _ = operation_log.append(&crate::logs::OperationEntry::new(
            "install",
            &row.name,
            plan.snapshot.agents.clone(),
            if row.status == TaskSkillStatus::Skipped {
                "not_executed"
            } else {
                super::update::status_label(rows[i].status)
            },
            row.backup_path.clone(),
            row.error.clone(),
        ));
    }
    let succeeded = rows
        .iter()
        .filter(|r| r.status == TaskSkillStatus::Installed)
        .count();
    let complete = crate::expert_packs::state::observe(&paths, &plan.layout, &plan.snapshot)
        .is_ok_and(|p| p.complete);
    let overall = if ctx.cancel.is_cancelled() {
        TaskOverall::Cancelled
    } else if succeeded == rows.len() && complete {
        TaskOverall::Success
    } else if succeeded > 0 {
        TaskOverall::Partial
    } else {
        TaskOverall::Failed
    };
    FinishOutcome::Completed(
        TaskResult {
            action: TaskAction::Install,
            scope: ctx.envelope.scope,
            overall,
            skills: rows,
            warnings,
            backup_path,
            log_path: Some(paths.display(operation_log.path())),
            started_at: ctx.envelope.started_at,
            finished_at_epoch_ms: crate::time::now_ms(),
            preview: None,
            batch: None,
        },
        format!("专家包安装已结束，{succeeded} 项成功。"),
    )
}

#[cfg(test)]
mod tests;
