//! Each host owns its native evidence, eligibility, commands and effect checks.
//! Coordination sees snapshots and command results without interpreting native JSON.
mod claude;
mod codex;
mod shared;
#[cfg(test)]
mod tests;
use super::{
    contracts::Contract,
    model::*,
    transport::{Output, Runtime},
};
use std::path::PathBuf;

pub(super) struct Observation {
    pub snapshot: Snapshot,
    evidence: Evidence,
}
enum Evidence {
    Codex(codex::NativeEvidence),
    Claude(claude::NativeEvidence),
}
pub(super) struct Prepared {
    pub arguments: Vec<String>,
    pub fingerprint: String,
    pub consequences: Vec<String>,
    pub affected_plugins: Vec<String>,
}
pub(super) struct CommandResult {
    pub success: bool,
    pub diagnostic: String,
    pub response_accepted: bool,
    output: Output,
}
impl CommandResult {
    fn new(r: &Request, output: Output) -> Self {
        let response_accepted = match r.host {
            Host::Codex => codex::response_matches(r, &output),
            Host::Claude => claude::response_matches(r, &output),
        };
        Self {
            success: output.success,
            diagnostic: output.diagnostic.clone(),
            response_accepted,
            output,
        }
    }
}
impl Host {
    pub(super) fn defaults(self) -> Contract {
        match self {
            Self::Codex => codex::defaults(),
            Self::Claude => claude::defaults(),
        }
    }
    pub(super) fn probe_sequence(self) -> Vec<Action> {
        match self {
            Self::Codex => codex::probe_sequence(),
            Self::Claude => claude::probe_sequence(),
        }
    }
    pub(super) fn check_context(self, context: &ResolvedContext) -> Result<(), String> {
        match self {
            Self::Codex => codex::validate_context(context),
            Self::Claude => Ok(()),
        }
    }
    pub(super) fn observe(
        self,
        rt: &Runtime,
        capabilities: Vec<Capability>,
        projects: &[PathBuf],
        context: ResolvedContext,
    ) -> Result<Observation, String> {
        let mut obs = match self {
            Self::Codex => codex::observe(rt, capabilities, projects, context),
            Self::Claude => claude::observe(rt, capabilities, projects, context),
        }?;
        obs.project_action_blocks();
        Ok(obs)
    }
}
impl Observation {
    fn check(&self, r: &Request) -> Result<(), String> {
        match &self.evidence {
            Evidence::Codex(e) => codex::check(r, &self.snapshot, e),
            Evidence::Claude(e) => claude::check(r, &self.snapshot, e),
        }
    }
    pub fn prepare(&self, r: &Request) -> Result<Prepared, String> {
        self.check(r)?;
        let (arguments, fingerprint, (consequences, affected_plugins)) = match &self.evidence {
            Evidence::Codex(_) => (
                codex::arguments(r),
                codex::confirmation_fingerprint(r, &self.snapshot)?,
                codex::consequences(r, &self.snapshot),
            ),
            Evidence::Claude(_) => (
                claude::arguments(r),
                claude::confirmation_fingerprint(r, &self.snapshot)?,
                claude::consequences(r, &self.snapshot),
            ),
        };
        Ok(Prepared {
            arguments,
            fingerprint,
            consequences,
            affected_plugins,
        })
    }
    pub fn execute(&self, rt: &Runtime, r: &Request) -> Result<CommandResult, String> {
        self.check(r)?;
        let output = match &self.evidence {
            Evidence::Codex(e) => codex::execute(rt, r, e),
            Evidence::Claude(e) => claude::execute(rt, r, e),
        }?;
        Ok(CommandResult::new(r, output))
    }
    pub fn verify(&self, r: &Request, after: &Observation, out: &CommandResult) -> bool {
        match (&self.evidence, &after.evidence) {
            (Evidence::Codex(_), Evidence::Codex(_)) => {
                codex::postcondition(r, &self.snapshot, &after.snapshot, &out.output)
            }
            (Evidence::Claude(before), Evidence::Claude(next)) => claude::postcondition(
                r,
                &self.snapshot,
                &after.snapshot,
                &out.output,
                before,
                next,
            ),
            _ => false,
        }
    }
    fn project_action_blocks(&mut self) {
        let request = |action, target| Request {
            host: self.snapshot.host,
            action,
            context: self.snapshot.context.target.clone(),
            target,
            source: String::new(),
            source_kind: String::new(),
            reference: String::new(),
            sparse_paths: vec![],
        };
        let blocks: Vec<_> = self
            .snapshot
            .plugins
            .iter()
            .map(|p| {
                let mut blocks = std::collections::BTreeMap::new();
                for action in [
                    Action::Install,
                    Action::Enable,
                    Action::Disable,
                    Action::Update,
                    Action::Reinstall,
                    Action::Uninstall,
                ] {
                    if let Err(reason) = self.check(&request(action, p.id.clone())) {
                        blocks.insert(
                            serde_json::to_value(action)
                                .unwrap()
                                .as_str()
                                .unwrap()
                                .into(),
                            reason,
                        );
                    }
                }
                blocks
            })
            .collect();
        let market_blocks: Vec<_> = self
            .snapshot
            .markets
            .iter()
            .map(|m| {
                (
                    self.check(&request(Action::SyncMarket, m.name.clone()))
                        .err(),
                    self.check(&request(Action::RemoveMarket, m.name.clone()))
                        .err(),
                )
            })
            .collect();
        for (plugin, blocks) in self.snapshot.plugins.iter_mut().zip(blocks) {
            plugin.action_blocks = blocks;
        }
        for (market, (sync, remove)) in self.snapshot.markets.iter_mut().zip(market_blocks) {
            market.sync_blocked_reason = sync;
            market.remove_blocked_reason = remove;
        }
    }
}
