//! Mutation task foundation and install preview.

pub mod conflicts;
pub mod manager;
pub mod parser;
pub mod source;

pub use conflicts::{ConflictInfo, ConflictProvider, InventoryConflictProvider, find_conflicts};
pub use manager::{
    CancelAck, EventSink, MutationPermit, PREVIEW_TIMEOUT, PreviewSkill, PreviewSummary, TaskEvent,
    TaskId, TaskManager, TaskManagerConfig, TaskOverall, TaskPhase, TaskResult, TaskSkillResult,
    TaskSkillStatus,
};
pub use parser::{ParsedSkill, PreviewParseOutcome, parse_preview_output};
pub use source::{SourceSpec, validate_source};
