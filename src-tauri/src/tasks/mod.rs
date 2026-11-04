//! Mutation task foundation and install preview.

pub mod batch;
pub mod conflicts;
pub(crate) mod copy_to_project;
pub mod install;
pub mod manager;
pub mod parser;
pub mod preview;
mod session;
pub mod source;
pub mod summary;
pub mod uninstall;
pub mod update;

pub use conflicts::{ConflictInfo, ConflictProvider, InventoryConflictProvider, find_conflicts};
pub use install::{
    CliSkillResult, CliSkillStatus, INSTALL_TIMEOUT, InstallOutput, InstallPlan, InstallRequest,
    InstallTarget, InstallVerification, UNIVERSAL_AGENT_ID, parse_install_output, snapshot_lock,
    validate_request, validate_skill_name, validate_skill_names, verify_install,
};
pub use manager::{
    ActiveMutationInfo, CancelAck, EventSink, MutationPermit, PREVIEW_TIMEOUT, PreviewSkill,
    PreviewSummary, RegistryChangeGuard, ShutdownReport, TaskAction, TaskEvent, TaskId,
    TaskManager, TaskManagerConfig, TaskOverall, TaskPhase, TaskResult, TaskSkillResult,
    TaskSkillStatus,
};
pub use parser::{ParsedSkill, PreviewParseOutcome, parse_preview_output};
pub use source::{SourceSpec, validate_source};
pub use uninstall::{UNINSTALL_TIMEOUT, UninstallPlan, plan_uninstall, verify_uninstall};
pub use update::{
    UpdateItem, UpdatePlan, UpdateSkip, UpdateVerification, plan_updates, verify_update,
};

#[cfg(test)]
mod timeout_matrix_tests {
    use super::*;
    use crate::timeouts::{PREVIEW_TIMEOUT as SPEC_PREVIEW, WRITE_TIMEOUT as SPEC_WRITE};
    use std::time::Duration;

    #[test]
    fn all_mutations_use_the_15_minute_write_cap() {
        assert_eq!(INSTALL_TIMEOUT, Duration::from_secs(900));
        assert_eq!(INSTALL_TIMEOUT, SPEC_WRITE);
        assert_eq!(UNINSTALL_TIMEOUT, INSTALL_TIMEOUT);
    }

    #[test]
    fn preview_uses_the_60_second_read_only_cap() {
        assert_eq!(PREVIEW_TIMEOUT, Duration::from_secs(60));
        assert_eq!(PREVIEW_TIMEOUT, SPEC_PREVIEW);
    }
}
