# Scope CLI execution

## Notes

Architecture candidate 03 from the whole-codebase review. Candidates 01 and 02
are complete and committed as `030c320` and `33bb4be`. The user requested the
next candidate and accepted the recommended scope. Implementation and
validation are complete.

## Facts before implementation

- `tasks/install.rs:68` stores `scope`, `cwd` and `global` in `InstallPlan`.
  `validate_request` derives them from ScopeLayout at lines 113-119.
- `tasks/update.rs:40` stores the same facts in `UpdateItem`; planning derives
  them at lines 224-230, and `install_plan` copies them again at lines 65-75.
  Production construction is currently consistent; no wrong-Scope execution
  bug has been established.
- Install's command uses the plan's cwd/global (`install.rs:483`), while
  snapshot and verification use `InstallAction.layout`. Its baseline uses
  `plan.scope` (`install.rs:929`). Update's command also uses the item instead
  of its action's layout (`update.rs:469`); its backup and verification use
  the layout.
- Uninstall does not duplicate cwd/global in its plan, but its command
  interface treats `Option<cwd>` as the global/project selector
  (`uninstall.rs:158`, `415`). Its baseline uses `plan.scope` (`450`).
- Preview's command interface accepts independent cwd/global
  (`cli.rs:44`, `manager.rs:1022`). Global Preview uses `paths.cli_cwd()`,
  while Global mutations fall back to `runtime.cwd`. Project commands all
  use the resolved root. The normal runtime and app paths agree, but adapters
  can distinguish the two global cwd sources.
- ADRs 0001 and 0011 define the pinned CLI and Scope execution rules. ADR 0003
  keeps Mutation session execution independent of Preview and native actions;
  ADR 0016 keeps permit/registration/marker ownership in TaskManager.

## Agreed scope

- Concentrate CLI Scope rules in one module used by Install, Update,
  Uninstall and Preview. Callers supply the resolved ScopeLayout and semantic
  operation data rather than independent cwd/global values.
- Remove redundant Scope execution facts from InstallPlan and UpdateItem.
  Action-local bookkeeping uses the same layout as the CLI command, including
  Baseline fingerprint writes. Retain actual operation data: source, skill
  names, Install targets, attachment sets and update assessments.
- Preserve behavior: exact argument ordering, pinned version, timeout and
  child environment; Global mutations use runtime.cwd, Global Preview uses
  the app's isolated cwd, and Project commands use the resolved Project root
  without `-g`.
- Preserve the public TaskManager/IPC interface, events/results, cancellation
  windows, marker policy and native extra-agent work. Preview shares command
  construction, not the Mutation session lifecycle.
- Use the existing ProcessExecutor seam and its real/fake adapters. No new
  executor trait or test-only planning interface is needed.

## Test surface

- Existing exact argv tests cover Global/Project Install, Update, Uninstall
  and Preview. Replace removed plan-field assertions with command observations
  through the same interface used by real actions.
- Existing public TaskManager tests for Project Update and Uninstall already
  observe executor commands, real temporary Canonical store/Lock entry changes
  and Scope-isolated Backup snapshots. Keep those behavior locks.
- Extend missing coverage where it tests the real call path, especially
  Project Install's command, verification and baseline Scope agreement and
  the distinction between the two Global cwd sources.
- Preserve the existing pinned real-CLI sandbox fixtures. They use handcrafted
  argv and therefore validate upstream CLI semantics, not the app's command
  construction interface; deterministic app-path tests must cover that seam.
- Run the affected tests, backend full suite, formatting and diff checks after
  implementation. No frontend change is planned.

## Implementation

- `src-tauri/src/cli/scoped.rs` owns the pinned command prefix, exact
  operation arguments, Scope flag, cwd, child environment and timeout. The
  same ScopedCli interface is used by all real command callers and tests.
- InstallPlan and UpdateItem no longer carry scope/cwd/global; UninstallPlan
  also no longer carries a redundant Scope. Install and Uninstall baseline
  bookkeeping now uses their action's layout, as Update already did.
- The old command helpers and ScopeLayout's unused CLI cwd helper are removed.
  UpdateItem's InstallPlan conversion remains for shared attachment verification
  and native Extra agent linking, rather than command construction.
- `src-tauri/src/tasks/manager/tests/scoped_cli.rs` exercises Project Install
  success, cross-Scope verification refusal and Preview through public
  TaskManager entry points. It reuses existing execution adapters.
- ADR 0017 records the ownership decision; ADR 0011 links to it, and the
  matching code-review follow-up checklist item is complete.

## Validation

- Four added tests cover command Scope/cwd selection, Project Install's
  command/verification/backup/baseline agreement, refusal to accept a Global
  Skill as Project installation evidence, and Preview's public read-only
  lifecycle. Existing exact command and scoped mutation tests remain intact.
- `cargo test --quiet scoped`: 19 passed. Backend `cargo test --quiet`:
  511 passed, 3 real-CLI/network tests ignored by default.
- CLI tests remain inline under `#[cfg(test)]`, matching the static audit's
  product-source convention. The full security audit passes unchanged.
- `cargo fmt --check` and `git diff --check` passed.

## Fog

None; the recommended scope is confirmed.
