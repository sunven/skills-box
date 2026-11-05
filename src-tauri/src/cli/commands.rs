//! Copyable command text and session-only records of actual CLI invocations.
use crate::process::{CancelToken, CommandSpec, ExecError, ExecOutput, ProcessExecutor};
use crate::scope::ScopeId;
use async_trait::async_trait;
use serde::Serialize;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliCommandContext {
    pub origin: String,
    pub scope: ScopeId,
    pub task_id: Option<String>,
    pub source: Option<String>,
}

impl CliCommandContext {
    pub fn task(task_id: &str, scope: ScopeId) -> Self {
        Self {
            origin: "task".into(),
            scope,
            task_id: Some(task_id.into()),
            source: None,
        }
    }
    pub fn background(origin: &str, source: Option<String>) -> Self {
        Self {
            origin: origin.into(),
            scope: ScopeId::Global,
            task_id: None,
            source,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliCommandRecord {
    pub id: String,
    pub command: String,
    pub context: CliCommandContext,
    pub status: String,
    pub started_at: u64,
}

type Listener = Arc<dyn Fn(CliCommandRecord) + Send + Sync>;
#[derive(Default)]
pub struct CliCommandStore {
    records: Mutex<Vec<CliCommandRecord>>,
    listener: Mutex<Option<Listener>>,
}
impl CliCommandStore {
    pub fn records(&self) -> Vec<CliCommandRecord> {
        self.records
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
    pub fn listen(&self, listener: Listener) {
        *self.listener.lock().unwrap_or_else(|p| p.into_inner()) = Some(listener);
    }
    fn publish(&self, record: CliCommandRecord) {
        {
            let mut records = self.records.lock().unwrap_or_else(|p| p.into_inner());
            if records.iter().any(|old| {
                (old.id == record.id && old.status != "running" && record.status == "running")
                    || (record.context.origin != "task"
                        && old.context == record.context
                        && old.started_at > record.started_at)
            }) {
                return;
            }
            records.retain(|old| {
                old.id != record.id
                    && (record.context.origin == "task" || old.context != record.context)
            });
            records.push(record.clone());
        }
        let listener = self
            .listener
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Some(listener) = listener {
            listener(record);
        }
    }
}

pub fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_./:@%+=,-".contains(c))
    {
        value.into()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}
pub fn display_command(spec: &CommandSpec) -> String {
    let argv = std::iter::once(shell_quote(&spec.program.to_string_lossy()))
        .chain(spec.args.iter().map(|arg| shell_quote(arg)))
        .collect::<Vec<_>>()
        .join(" ");
    format!("cd {} && {argv}", shell_quote(&spec.cwd.to_string_lossy()))
}

pub struct ObservedExecutor {
    pub executor: Arc<dyn ProcessExecutor>,
    pub commands: Arc<CliCommandStore>,
}
#[async_trait]
impl ProcessExecutor for ObservedExecutor {
    async fn run(&self, spec: CommandSpec, cancel: CancelToken) -> Result<ExecOutput, ExecError> {
        let Some(context) = spec.cli_context.clone() else {
            return self.executor.run(spec, cancel).await;
        };
        if cancel.is_cancelled() {
            return Err(ExecError::Cancelled);
        }
        let mut record = CliCommandRecord {
            id: uuid::Uuid::new_v4().to_string(),
            command: display_command(&spec),
            context,
            status: "running".into(),
            started_at: crate::time::now_ms(),
        };
        self.commands.publish(record.clone());
        let output = self.executor.run(spec, cancel).await;
        record.status = match &output {
            Ok(output) if output.success() => "succeeded",
            Err(ExecError::Cancelled) => "cancelled",
            _ => "failed",
        }
        .into();
        self.commands.publish(record);
        output
    }
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
