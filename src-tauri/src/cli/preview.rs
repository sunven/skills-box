//! Read-only command previews derived from the same plans as execution.
use super::{CliRuntime, ScopedCli, commands::display_command};
use crate::tasks::{InstallPlan, InstallRequest, UninstallPlan, UpdatePlan};
use crate::{error::AppError, paths::AppPaths, scope::ScopeId, scope_layout::ScopeLayout};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum CliCommandRequest {
    Preview { source: String },
    Install { request: InstallRequest },
    Update { skills: Vec<String> },
    Uninstall { skill: String },
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliCommandEntry {
    pub name: String,
    pub command: Option<String>,
    pub reason: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliCommandPreview {
    pub id: String,
    pub entries: Vec<CliCommandEntry>,
    pub marketplace_cli_required: bool,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub marketplace_versions: std::collections::BTreeMap<String, String>,
}
impl CliCommandPreview {
    pub fn new(scope: &ScopeId, action: &str, entries: Vec<CliCommandEntry>) -> Self {
        let bytes =
            serde_json::to_vec(&(scope, action, &entries)).expect("serializable CLI preview");
        Self {
            id: format!("{:x}", Sha256::digest(bytes)),
            entries,
            marketplace_versions: Default::default(),
            marketplace_cli_required: false,
        }
    }
    pub fn validate(&self, expected: &str) -> Result<Vec<String>, AppError> {
        if self.id != expected {
            return Err(AppError::cli_command_changed());
        }
        Ok(self
            .entries
            .iter()
            .filter_map(|e| e.command.clone())
            .collect())
    }
}
pub fn install_preview(
    layout: &ScopeLayout,
    runtime: &CliRuntime,
    plan: &InstallPlan,
) -> CliCommandPreview {
    let spec = ScopedCli::new(runtime, layout).install(
        &plan.source,
        &plan.skills,
        &plan.agent_ids,
        crate::timeouts::WRITE_TIMEOUT,
    );
    CliCommandPreview::new(
        &layout.scope,
        "install",
        vec![CliCommandEntry {
            name: plan.skills.join("、"),
            command: Some(display_command(&spec)),
            reason: None,
        }],
    )
}
pub fn update_preview(
    layout: &ScopeLayout,
    runtime: Option<&CliRuntime>,
    plan: &UpdatePlan,
) -> CliCommandPreview {
    let entries = plan
        .items
        .iter()
        .map(|item| CliCommandEntry {
            name: item.name.clone(),
            command: Some(display_command(
                &item
                    .marketplace_plan
                    .as_ref()
                    .map(|p| p.command())
                    .unwrap_or_else(|| {
                        ScopedCli::new(runtime.expect("executable update runtime"), layout).install(
                            &item.source,
                            std::slice::from_ref(&item.name),
                            &item.agent_ids,
                            crate::timeouts::WRITE_TIMEOUT,
                        )
                    }),
            )),
            reason: None,
        })
        .chain(plan.skipped.iter().map(|skip| CliCommandEntry {
            name: skip.name.clone(),
            command: None,
            reason: Some(skip.error.message.clone()),
        }))
        .collect();
    let mut preview = CliCommandPreview::new(&layout.scope, "update", entries);
    preview.marketplace_cli_required = plan
        .skipped
        .iter()
        .any(|s| s.error.code == crate::error::ErrorCode::MarketplaceCliUnavailable);
    preview.marketplace_versions = plan
        .items
        .iter()
        .filter_map(|i| {
            i.marketplace_plan
                .as_ref()
                .map(|p| (i.name.clone(), p.detail.summary.version.clone()))
        })
        .collect();
    preview
}
pub fn uninstall_preview(
    layout: &ScopeLayout,
    runtime: Option<&CliRuntime>,
    plan: &UninstallPlan,
) -> CliCommandPreview {
    let command = runtime.map(|runtime| {
        display_command(
            &ScopedCli::new(runtime, layout).uninstall(&plan.name, crate::timeouts::WRITE_TIMEOUT),
        )
    });
    CliCommandPreview::new(
        &layout.scope,
        "uninstall",
        vec![CliCommandEntry {
            name: plan.name.clone(),
            command,
            reason: None,
        }],
    )
}
pub fn source_preview(
    paths: &AppPaths,
    layout: &ScopeLayout,
    runtime: &CliRuntime,
    source: &str,
) -> CliCommandPreview {
    let spec =
        ScopedCli::new(runtime, layout).preview(paths, source, crate::timeouts::PREVIEW_TIMEOUT);
    CliCommandPreview::new(
        &layout.scope,
        "preview",
        vec![CliCommandEntry {
            name: source.into(),
            command: Some(display_command(&spec)),
            reason: None,
        }],
    )
}

pub trait CommandPlan {
    fn needs_cli(&self) -> bool;
    fn command_preview(
        &self,
        layout: &ScopeLayout,
        runtime: Option<&CliRuntime>,
    ) -> CliCommandPreview;
}
impl CommandPlan for InstallPlan {
    fn needs_cli(&self) -> bool {
        true
    }
    fn command_preview(
        &self,
        layout: &ScopeLayout,
        runtime: Option<&CliRuntime>,
    ) -> CliCommandPreview {
        install_preview(layout, runtime.expect("install runtime"), self)
    }
}
impl CommandPlan for UpdatePlan {
    fn needs_cli(&self) -> bool {
        self.items.iter().any(|i| i.marketplace_record.is_none())
    }
    fn command_preview(
        &self,
        layout: &ScopeLayout,
        runtime: Option<&CliRuntime>,
    ) -> CliCommandPreview {
        update_preview(layout, runtime, self)
    }
}
impl CommandPlan for UninstallPlan {
    fn needs_cli(&self) -> bool {
        !self.marketplace && self.management != crate::inventory::ManagementState::Unmanaged
    }
    fn command_preview(
        &self,
        layout: &ScopeLayout,
        runtime: Option<&CliRuntime>,
    ) -> CliCommandPreview {
        uninstall_preview(layout, runtime, self)
    }
}
