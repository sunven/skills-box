//! Deterministic executor used by unit tests.
//!
//! Rules match on the program file name (or an exact path when the pattern
//! contains `/`) plus required argument substrings, and are consumed in
//! registration order. Any unmatched command fails as a spawn error, which
//! makes unexpected external calls obvious in tests.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;

use super::{CommandSpec, ExecError, ExecOutput, ProcessExecutor};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptedResponse {
    Output {
        code: i32,
        stdout: String,
        stderr: String,
    },
    Timeout,
    SpawnError(String),
}

impl ScriptedResponse {
    pub fn ok(stdout: impl Into<String>) -> Self {
        Self::Output {
            code: 0,
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    pub fn fail(code: i32, stderr: impl Into<String>) -> Self {
        Self::Output {
            code,
            stdout: String::new(),
            stderr: stderr.into(),
        }
    }

    pub fn timeout() -> Self {
        Self::Timeout
    }

    pub fn spawn_error(message: impl Into<String>) -> Self {
        Self::SpawnError(message.into())
    }
}

#[derive(Debug, Clone)]
struct ScriptedRule {
    program: String,
    args_contain: Vec<String>,
    response: ScriptedResponse,
}

impl ScriptedRule {
    fn matches(&self, spec: &CommandSpec) -> bool {
        let program_matches = if self.program.is_empty() {
            true
        } else if self.program.contains('/') {
            spec.program == Path::new(&self.program)
        } else {
            spec.program
                .file_name()
                .map(|name| name.to_string_lossy() == self.program)
                .unwrap_or(false)
        };
        program_matches
            && self
                .args_contain
                .iter()
                .all(|required| spec.args.iter().any(|arg| arg.contains(required)))
    }
}

#[derive(Debug, Default)]
pub struct ScriptedExecutor {
    rules: Mutex<VecDeque<ScriptedRule>>,
    calls: Mutex<Vec<CommandSpec>>,
}

impl ScriptedExecutor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a response for a program (file name or exact path) whose args
    /// contain every substring in `args_contain`.
    pub fn expect(
        mut self,
        program: &str,
        args_contain: &[&str],
        response: ScriptedResponse,
    ) -> Self {
        self.rules
            .get_mut()
            .expect("fresh mutex")
            .push_back(ScriptedRule {
                program: program.to_string(),
                args_contain: args_contain.iter().map(|arg| arg.to_string()).collect(),
                response,
            });
        self
    }

    /// Register a response matching any command.
    pub fn expect_any(self, response: ScriptedResponse) -> Self {
        self.expect("", &[], response)
    }

    pub fn calls(&self) -> Vec<CommandSpec> {
        self.calls.lock().expect("calls mutex").clone()
    }

    pub fn call_count(&self) -> usize {
        self.calls.lock().expect("calls mutex").len()
    }
}

#[async_trait]
impl ProcessExecutor for ScriptedExecutor {
    async fn run(&self, spec: CommandSpec) -> Result<ExecOutput, ExecError> {
        self.calls.lock().expect("calls mutex").push(spec.clone());
        let response = {
            let mut rules = self.rules.lock().expect("rules mutex");
            let index = rules.iter().position(|rule| rule.matches(&spec));
            index.and_then(|index| rules.remove(index))
        };
        let Some(rule) = response else {
            return Err(ExecError::Spawn {
                program: spec.program.display().to_string(),
                message: "no scripted response".to_string(),
            });
        };
        match rule.response {
            ScriptedResponse::Output {
                code,
                stdout,
                stderr,
            } => Ok(ExecOutput {
                code: Some(code),
                signal: None,
                stdout,
                stderr,
                duration: Duration::ZERO,
            }),
            ScriptedResponse::Timeout => Err(ExecError::Timeout(spec.timeout)),
            ScriptedResponse::SpawnError(message) => Err(ExecError::Spawn {
                program: spec.program.display().to_string(),
                message,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn spec(program: &str, args: &[&str]) -> CommandSpec {
        CommandSpec::new(
            program,
            args.iter().map(|arg| arg.to_string()).collect(),
            "/tmp",
            BTreeMap::new(),
            Duration::from_secs(5),
        )
    }

    #[tokio::test]
    async fn scripts_success_failure_timeout_and_spawn_error() {
        let executor = ScriptedExecutor::new()
            .expect("node", &["--version"], ScriptedResponse::ok("v24.15.0\n"))
            .expect("git", &["--version"], ScriptedResponse::fail(1, "no git"))
            .expect("npx", &["skills@1.7.4"], ScriptedResponse::timeout())
            .expect("zsh", &["-ilc"], ScriptedResponse::spawn_error("boom"));

        let node = executor
            .run(spec("/fake/bin/node", &["--version"]))
            .await
            .unwrap();
        assert_eq!(node.stdout_trimmed(), "v24.15.0");

        let git = executor
            .run(spec("/usr/bin/git", &["--version"]))
            .await
            .unwrap();
        assert_eq!(git.code, Some(1));
        assert_eq!(git.stderr_trimmed(), "no git");

        let npx = executor
            .run(spec("/fake/bin/npx", &["-y", "skills@1.7.4", "--version"]))
            .await
            .unwrap_err();
        assert!(matches!(npx, ExecError::Timeout(_)));

        let shell = executor
            .run(spec("/bin/zsh", &["-ilc", "printf x"]))
            .await
            .unwrap_err();
        assert!(matches!(shell, ExecError::Spawn { .. }));
    }

    #[tokio::test]
    async fn records_calls_and_rejects_unscripted_commands() {
        let executor = ScriptedExecutor::new().expect_any(ScriptedResponse::ok("x"));
        executor.run(spec("/bin/echo", &["hi"])).await.unwrap();
        assert_eq!(executor.call_count(), 1);
        assert_eq!(executor.calls()[0].program, Path::new("/bin/echo"));

        let empty = ScriptedExecutor::new();
        let error = empty.run(spec("/bin/echo", &[])).await.unwrap_err();
        assert!(matches!(error, ExecError::Spawn { .. }));
    }
}
