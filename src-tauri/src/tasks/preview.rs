//! The shared read-only CLI seam. Scope conflicts and task events belong to
//! callers; catalog discovery uses only the actual CLI list and its parser.

use std::time::Duration;

use super::parser::{PreviewParseOutcome, parse_preview_output};
use crate::cli::{CliRuntimeProvider, ScopedCli};
use crate::error::{
    AppError,
    cli::{self, CliAction},
};
use crate::paths::AppPaths;
use crate::process::{CancelToken, ExecOutput, ProcessExecutor};
use crate::scope_layout::ScopeLayout;

pub const CATALOG_CONTRACT: &str = "skills@1.7.4/preview-v2";

pub async fn source_preview(
    runtime: &dyn CliRuntimeProvider,
    executor: &dyn ProcessExecutor,
    paths: &AppPaths,
    layout: &ScopeLayout,
    source: &str,
    timeout: Duration,
    cancel: CancelToken,
) -> Result<(ExecOutput, PreviewParseOutcome), AppError> {
    let runtime = tokio::select! {
        result = runtime.cli_runtime() => result?,
        _ = cancel.cancelled() => return Err(AppError::task_cancelled()),
    };
    if cancel.is_cancelled() {
        return Err(AppError::task_cancelled());
    }
    let mut spec = ScopedCli::new(&runtime, layout).preview(paths, source, timeout);
    // Authentication is confined to the native GitHub API client, including
    // when an injected runtime happens to contain credentials.
    spec.env.remove("GITHUB_TOKEN");
    spec.env.remove("GH_TOKEN");
    let output = executor
        .run(spec, cancel.clone())
        .await
        .map_err(|error| cli::classify_exec_error(CliAction::Preview, error))?;
    if cancel.is_cancelled() {
        return Err(AppError::task_cancelled());
    }
    let parsed = parse_preview_output(&output.stdout);
    Ok((output, parsed))
}
