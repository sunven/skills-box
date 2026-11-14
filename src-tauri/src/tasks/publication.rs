//! Local Skill publication. Callers own preparation, backups and task lifetime.
use super::{expert_packs, install::InstallTarget, marketplace};
use crate::{
    baseline::{BaselineStore, fingerprint_folder},
    error::AppError,
    expert_packs::records::{Records as PackRecords, WorkflowInstallation},
    inventory::AttachmentMode,
    marketplace::records::Records as MarketplaceRecords,
    paths::AppPaths,
    scope_layout::ScopeLayout,
};
use std::{fs, path::Path};

pub(super) enum Publication<'a> {
    Marketplace {
        plan: &'a marketplace::Plan,
        root: &'a Path,
    },
    Workflow {
        plan: &'a expert_packs::Plan,
        unit: &'a expert_packs::Unit,
    },
}

#[derive(Debug)]
pub(super) struct Outcome {
    pub result: Result<(), AppError>,
    pub warnings: Vec<AppError>,
}

impl From<Result<(), AppError>> for Outcome {
    fn from(result: Result<(), AppError>) -> Self {
        Self {
            result,
            warnings: vec![],
        }
    }
}

pub(super) fn publish(paths: &AppPaths, publication: Publication<'_>) -> Outcome {
    let mut warnings = vec![];
    let result = publish_inner(paths, &publication, &mut warnings);
    Outcome { result, warnings }
}

fn publish_inner(
    paths: &AppPaths,
    publication: &Publication<'_>,
    warnings: &mut Vec<AppError>,
) -> Result<(), AppError> {
    let (layout, name, targets, prefix) = match publication {
        Publication::Marketplace { plan, .. } => {
            let registry = crate::projects::ProjectRegistry::new(paths.clone());
            let layout = ScopeLayout::resolve_detected(paths, &registry, &plan.layout.scope)?;
            plan.check(paths, &layout)?;
            (layout, plan.name(), &plan.targets, ".marketplace-")
        }
        Publication::Workflow { plan, unit } => {
            // Reused workflows need only their missing attachments; their
            // independent source record and baseline must not be rewritten.
            if unit.expected.is_some() {
                return attach(paths, &plan.layout, &unit.name, &plan.targets, &|| {
                    plan.check_scope(&plan.layout)
                });
            }
            (
                plan.layout.clone(),
                unit.name.as_str(),
                &plan.targets,
                ".pack-",
            )
        }
    };
    fs::create_dir_all(&layout.canonical_dir)
        .map_err(|e| AppError::verify_failed(e.to_string()))?;
    // Prepare on the target volume before replacing anything. No-overwrite
    // renames retain the existing publication and backup recovery semantics.
    let stage = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(&layout.canonical_dir)
        .map_err(|e| AppError::verify_failed(e.to_string()))?;
    let candidate = stage.path().join("candidate");
    match publication {
        Publication::Marketplace { root, .. } => crate::backup::copy_tree(root, &candidate),
        Publication::Workflow { plan, .. } => fs::create_dir(&candidate).and_then(|()| {
            fs::write(
                candidate.join("SKILL.md"),
                &plan.snapshot.installed_workflow,
            )
        }),
    }
    .map_err(|e| AppError::verify_failed(e.to_string()))?;

    let canonical = layout.canonical_dir.join(name);
    match publication {
        Publication::Marketplace { plan, .. } => {
            plan.check(paths, &layout)?;
            if plan.previous.is_some() {
                crate::fsutil::rename_directory_new(&canonical, &stage.path().join("previous"))
                    .map_err(|e| AppError::verify_failed(e.to_string()))?;
            }
        }
        Publication::Workflow { plan, unit } => plan.check_unit(paths, &layout, unit)?,
    }
    crate::fsutil::rename_directory_new(&candidate, &canonical)
        .map_err(|e| AppError::verify_failed(e.to_string()))?;
    let fingerprint = fingerprint_folder(&canonical).map_err(AppError::skill_fingerprint_failed)?;
    let installed = match publication {
        Publication::Marketplace { plan, .. } => {
            let installed = plan.installation(fingerprint.content_hash.clone());
            let mut records = MarketplaceRecords::read(&layout.lock_path)?;
            if records.skills.get(name) != plan.previous.as_ref() {
                return Err(AppError::verify_failed(
                    "安装记录在发布期间变化，已保留备份。",
                ));
            }
            records.skills.insert(name.into(), installed.clone());
            records.save(&layout.lock_path)?;
            Some(installed)
        }
        Publication::Workflow { plan, .. } => {
            let mut records = PackRecords::read(&layout.lock_path)?;
            records.workflows.insert(
                name.into(),
                WorkflowInstallation {
                    name: name.into(),
                    slug: plan.snapshot.slug.clone(),
                    markdown: plan.snapshot.installed_workflow.clone(),
                    content_hash: fingerprint.content_hash.clone(),
                    installed_at: httpdate::fmt_http_date(std::time::SystemTime::now()),
                },
            );
            records.save(&layout.lock_path)?;
            None
        }
    };
    // Baselines describe the published content even if attachments later fail.
    // A warning survives any subsequent terminal failure in the same outcome.
    if let Err(error) =
        BaselineStore::for_scope(paths, &layout.scope).record(name, &canonical, &fingerprint, true)
    {
        warnings.push(crate::baseline::write_warning(error));
    }

    if let Publication::Marketplace { plan, .. } = publication
        && let Some(previous) = &plan.previous_paths
    {
        for attachment in &previous.attachments {
            if attachment.mode == AttachmentMode::Copy {
                fs::remove_dir_all(&attachment.path)
                    .map_err(|e| AppError::agent_attach_failed(e.to_string()))?;
            }
        }
    }
    // The old-installation checks above cannot run after its source record has
    // changed. Attachments retain each caller's narrower write-time guard.
    attach(paths, &layout, name, targets, &|| match publication {
        Publication::Marketplace { .. } => layout.ensure_mutation_ready(),
        Publication::Workflow { plan, .. } => plan.check_scope(&layout),
    })?;
    if let Some(installed) = installed
        && (MarketplaceRecords::read(&layout.lock_path)?
            .skills
            .get(name)
            != Some(&installed)
            || crate::marketplace::original_name(
                &fs::read_to_string(canonical.join("SKILL.md"))
                    .map_err(|e| AppError::verify_failed(e.to_string()))?,
            )? != name)
    {
        return Err(AppError::verify_failed("市场安装后校验失败"));
    }
    Ok(())
}

pub(super) fn attach(
    paths: &AppPaths,
    layout: &ScopeLayout,
    name: &str,
    targets: &[InstallTarget],
    before_write: &dyn Fn() -> Result<(), AppError>,
) -> Result<(), AppError> {
    let canonical = layout.canonical_dir.join(name);
    for target in targets
        .iter()
        .filter(|t| t.needs_attachment(&layout.canonical_dir))
    {
        let destination = target.skills_dir.join(name);
        if fs::symlink_metadata(&destination).is_ok_and(|m| m.file_type().is_symlink())
            && fs::canonicalize(&destination).ok() == fs::canonicalize(&canonical).ok()
        {
            continue;
        }
        crate::attach::attach_publication_checked(
            paths,
            layout,
            &layout.scope.skill_id(name),
            &target.id,
            before_write,
        )?;
    }
    Ok(())
}
