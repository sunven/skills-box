# Batch target execution

Status: implemented and validated; pre-existing verification exceptions recorded below (2026-11-07)
Implementation: complete
Review baseline: `d5d8735`

## Problem and intended result

`tasks/manager/batch.rs::run` combines Batch operation scheduling with eight
actions' execution and state transitions. It maintains Copy to project's
`copied` / `copy_failed` sets and advances Clean stale records / Restore skill's
expected Lock document. `RestoreAction` calls back into
`manager::batch::verify_target`. A change to one action therefore requires
understanding the outer loop as well as the action.

Introduce one internal Batch target execution module whose lifetime is one
Batch operation. Its depth comes from hiding action-specific authorization,
execution, state advancement and outcome interpretation behind a small
interface. The outer loop retains ordered scheduling, observing stop requests
between targets, and assembling the public batch result. It does not inspect
Copy or stale-record state, or classify action errors.

## Confirmed interview decisions

1. **State ownership:** one execution module instance owns action state for the
   entire batch. The user selected the recommendation for Q1.
2. **Coverage:** all eight Batch actions use it: Update, Uninstall, Remove from
   Agents, Attach, Detach, Copy to project, Clean stale records and Restore
   skill. Migrate incrementally while preserving each action's rules. The user
   selected the recommendation for Q2.
3. **Outcome interpretation:** the execution module returns an interpreted
   target outcome and whether later targets must stop. It preserves current
   progress, warnings, backups and public results. The user accepted the
   recommendation for Q3.
4. **Testing:** an internal, controllable test adapter exercises scheduling,
   stopping and aggregation independently. Existing temporary-filesystem
   integration tests continue to validate authorization, backups, writes and
   Scope isolation. Migrate redundant scheduling tests. The user accepted the
   recommendation for Q4.

## Ownership and interface contract

| Owner | Responsibilities |
| --- | --- |
| TaskManager and scoped-mutation module | Permit, resolved Scope lifetime, registration, interruption marker, shutdown coordination, final publication |
| Batch operation loop | Fixed target order, initial eligibility, stop observation between targets, cumulative progress and public result assembly |
| Batch target execution module | Action dependencies, write-time authorization, action dispatch, snapshot observation, post-write state advancement, interpreted target outcome and stop disposition |
| Existing action implementations | Their specific eligibility, backup strategy, filesystem/CLI writes, verification and bookkeeping |
| Mutation session | Existing CLI execution protocol and its established native Uninstall support |

The internal interface binds the confirmed plan, Scope and execution
dependencies once per batch. Each target is processed serially through that
same interface. The caller receives the target's final contribution to the
batch: its row, warnings, backup, progress contribution, existing task-skill
projection where applicable, and continuation disposition. Represent distinct
outcome categories explicitly rather than a freely combinable collection of
booleans. Rust type names and method layout remain implementation choices;
these types do not become a new public IPC contract.

Copy publication/dependency state and the expected stale Lock document are
private to the implementation. Restore's observed document is per-target
temporary state; it must be validated before advancing the expected document.
The outer loop never reads or mutates these values. Existing action code can
continue receiving read-only snapshots of the current plan.

Keep target verification outside TaskManager's orchestration so Restore and
the executor can use it without an action-to-manager dependency. Preserve
fresh Agent detection, Project identity/revision checks, error precedence and
the repeated validation points. Moving the guard must not collapse distinct
action authorization policies.

The production adapter composes existing Mutation session and native action
paths. A test adapter at this internal seam can pause a target and return
specified outcomes. Both are consumed by the same scheduling implementation.
Do not expose a test adapter through public TaskManager methods or impose
MutationAction on standalone native Attach / Detach.

## Observable behavior to preserve

These are existing behaviors, not new product decisions. In particular,
similarly named statuses have different observable contributions today.

| Situation | Batch row | TaskResult.skills | completed | Per-target progress |
| --- | --- | --- | --- | --- |
| Ineligible in the confirmed preview | Skipped, no error | No entry | No increment | None |
| Eligible target after stop/cancellation | NotExecuted, no error | No entry | No increment | None |
| Copy failed, so its later attachment cannot run | NotExecuted with dependency reason | No entry | Increment | None for that attachment |
| Target changes during an attempted operation | Skipped with error and possible backup | Entry | Increment | Preparing and completion progress |
| Ordinary attempted failure | Failed | Entry | Increment | Existing sequence; later targets continue |
| Failure that stops the batch | Failed; later eligible targets NotExecuted | Entry for attempted target | Increment for attempted target | Existing sequence |

Additional ordering and lifetime constraints:

- A stop request waits for the current target's write and verification. Keep
  its independent cancellation token and the existing between-target stop
  observation. Worker completion still governs permit lifetime.
- Stale-record advancement runs before finalizing the row and progress. It
  may replace an apparently successful outcome or an earlier error with a
  failure and force later targets to stop.
- Failed Restore execution can already have changed the Lock and files.
  Preserve the captured document, actual-state diagnostics and backups; never
  treat an arbitrary new document as the next authorized state.
- Snapshot observation is independent of successful return. A failed target
  can retain a backup; the row uses its latest recorded snapshot and the batch
  uses the last row with a backup.
- Inner task events keep their ordering, target label and the current batch
  completed/total values. Preserve command-preview agreement checks and
  actual-invocation observation.
- The batch ends with `FinishOutcome::Completed` containing the batch result.
  Explicit cancellation takes precedence for overall Cancelled. An error that
  stops later targets still yields Partial or Failed according to retained
  successful rows.
- Preserve warnings collected from inner Completed outcomes and the existing
  first-row-error fallback for unsuccessful session results.
- Successful writes, Project registration, retry scope and confirmation rules
  remain as established by the existing actions and ADRs.

## Implementation scope

Expected production changes are confined to the Rust task modules:

- Add the internal Batch target execution module and its module declaration.
- Reduce `tasks/manager/batch.rs` to its existing preview/start responsibilities
  plus generic scheduling and result assembly.
- Relocate shared Batch target verification to an appropriate task module;
  update Restore's use and keep action-specific checks in their current owners.
- Move the cross-target Copy and stale-document state into execution.
- Remove orchestration and forwarding code made redundant by the new ownership.
  Keep required MutationAction forwarding private where preserving the
  existing protocol still requires it; do not widen Mutation session merely to
  eliminate forwarding lines.

No frontend, serialization, command construction, preview/retry contract,
general filesystem abstraction or generic deletion protocol is required.

## Verification and acceptance

1. **Record behavior before moving it.** Run existing Batch tests and add
   focused behavior locks for the table's currently implicit distinctions:
   dependency-blocked attachments, execution-time skips, snapshot retention
   on failure, and stale advancement overriding an earlier outcome.
2. **Exercise the internal scheduling interface.** Use deterministic pauses
   and scripted outcomes for confirmed order, ordinary failure continuation,
   fatal stopping, explicit stop during an active target, and result/progress
   aggregation. Do not depend on sleeps or simulate CLI output for these tests.
3. **Exercise production execution.** Retain temporary-filesystem coverage for
   all eight actions, post-backup revalidation, Scope isolation, partial writes,
   Copy dependencies, and Lock document advancement. Existing CLI/runtime
   adapters remain available for actual action execution tests.
4. **Keep ownership checks.** Run scoped-mutation lifetime and shutdown tests,
   existing task/session tests, the full Rust suite, formatting/lint checks
   appropriate to the changed files, and `git diff --check`. Real-network or
   real-CLI tests retain their existing opt-in policy.
5. **Check the architecture result.** The outer loop contains no action dispatch,
   action-specific state advancement or error-code classification. Restore no
   longer depends on `manager::batch`. Production and scheduling tests cross
   the same internal seam. Remove redundant tests only after equivalent
   observable behavior is covered; retain integration tests with filesystem
   or ownership assertions.

## ADR and domain alignment

ADRs 0003, 0013, 0016 and 0021 retain their session, permit and batch ownership.
ADRs 0024, 0025, 0026 and 0027 retain Copy to project, Remove from Agents,
command observation and stale-record authorization semantics. No existing
decision needs to be reopened.

Batch target and Batch operation already exist in GLOSSARY.md. The execution
module is an implementation concept, so no new glossary term is needed.
The local design record captures this reversible internal seam choice; a new
ADR is not required at this stage.

## Confirmation and validation status

The user confirmed the four decisions and the complete design, then authorized
implementation on 2026-11-07.

Implemented:

- `tasks/batch_execution.rs` owns one `BatchExecution` per operation, covering
  all eight actions, Copy dependencies, stale Lock advancement, target
  verification, snapshots, warnings and continuation decisions.
- `TargetExecutor::execute` is the shared production/test interface.
  `TargetCompletion` distinguishes dependency-blocked targets from attempts;
  the latter carries the task-skill status, warnings and continuation decision.
- `manager/batch.rs::run_targets` schedules confirmed targets, observes
  cancellation between calls, and assembles progress and results. It has no
  action dispatch, action-specific state or error-code classification.
- Shared `verify_target` now lives in `tasks/batch.rs`; Restore no longer
  calls `manager::batch`. The verification body, `CheckedAction` protocol and
  `unit_outcome` interpretation were preserved apart from relocated references.
- Five scripted scheduling tests cover order, ordinary failures, skips,
  dependencies, warnings, backups, fatal stopping and cancellation. Three
  integration tests lock down Copy dependency progress and stale advancement
  overriding either failure or apparent success. The existing post-backup
  revalidation test now uses `BatchExecution` instead of constructing its
  private `CheckedAction`. Existing integration tests retain their distinct
  filesystem, action-classification and permit-lifetime assertions.

Validation (commands run from `src-tauri` unless noted):

- Before moving production code, the original 38 Batch tests passed; two new
  behavior tests also passed against the original implementation (40 total).
- `cargo test --quiet --lib tasks::manager::batch`: **46 passed**, including
  41 production integration tests and 5 scripted scheduling tests.
- The full test run exposed two pre-existing environment-dependent failures:
  `agents::tests::detects_home_roots_in_catalog_order` and
  `one_hundred_skill_fixture_meets_the_inventory_budget`. Both assume only
  fixture Agents are detected, but this machine has `/Applications/ZCode.app`.
  Both failures were reproduced in a separate, unmodified archive of
  baseline `d5d8735`.
- `cargo test --quiet --no-fail-fast -- --skip agents::tests::detects_home_roots_in_catalog_order --skip one_hundred_skill_fixture_meets_the_inventory_budget`:
  **677 passed** (645 unit tests and 32 integration tests); the two reproduced
  failures were filtered out, and 5 existing real-CLI/network tests remained
  ignored. This includes the task/session, scoped-mutation lifetime and
  shutdown coverage.
- Strict `cargo clippy --all-targets -- -D warnings` is blocked by the existing
  `clippy::too_many_arguments` finding in `tasks/preview.rs::source_preview`,
  also reproduced on unmodified `d5d8735`. After fixing the new module's
  redundant borrows, `cargo clippy --all-targets -- -D warnings -A clippy::too_many_arguments`
  passed. No lint suppression was added to source code.
- `cargo fmt --check` and repository-root `git diff --check` passed.

The unrelated Agent-detection tests and preview function are unchanged.
