# Shared Check attempt scheduling

Status: implemented and validated (2026-11-07)
Implementation: complete
Review baseline: `677e4d3`
Date: 2026-11-07

## Context and confirmed scope

This is candidate 03 from the whole-codebase architecture review. At the start
of this candidate, candidate 01, Batch target execution, was committed.
Candidate 02, Batch operation preview, confirmation and retry, was implemented
and validated but uncommitted; its working-tree changes were preserved.

The original report is an OS-temporary artifact:
`/var/folders/hx/3v6dd_js10lfdpq5mvfpdbth0000gn/T/architecture-review-20261107-161839.html`.

The user accepted the recommended ownership on 2026-11-07 with “按推荐”:
one internal Check attempt module owns source budgeting, bounded concurrency,
one deadline, unfinished-source completion, report sorting and statistics.
Callers provide completed cache results and sources that still need checking.
Only the latter consume the source budget. Cache admission, fingerprint
evaluation and publication remain with their existing owners.

The user confirmed the detailed design below with “确认” on 2026-11-07,
authorizing implementation.

## Friction and intended depth

`update/mod.rs::check` / `run_network` and
`update/project.rs::check` / `run_groups` each maintain the same protocol:
source admission, pending/completed tracking, bounded concurrency, one absolute
deadline, timeout completion, and report aggregation. The duplicated protocol
allows changes in scheduling or report interpretation to drift between Scopes.

The new module hides this entire protocol behind one asynchronous interface.
Deleting it would put budgeting, scheduling and report assembly back into both
callers: it passes the deletion test. Shared protocol tests provide leverage;
Scope-specific tests retain locality for cache and fingerprint behavior.

## Module interface and ownership

Add a private `update/attempt.rs` module. Its conceptual interface is:

```text
run(prepared_check, scheduling_limits, executor) -> UpdateCheckReport
```

- `prepared_check` contains the caller-captured `checked_at_epoch_ms`,
  uncheckable Skill results, and an ordered list of prepared source groups.
- Each prepared source is either `Ready(SourceOutcome)` or `Pending(Group)`.
  A Ready result is complete and performs no upstream lookup. Group is opaque
  to the scheduler and retains the original Scope-specific target/cache data.
- `scheduling_limits` contains the existing `max_sources`, `concurrency` and
  `deadline_ms` values. It introduces no new user configuration.
- The executor adapter evaluates an admitted group, maps a skipped group, and
  maps a timed-out group into the same owned `SourceOutcome` shape: one source
  result, its Skill results, and warnings.
- The runner owns the attempt-local executor by value. Each production adapter
  owns its attempt-local `SourceArchives` and borrows the existing checker for
  its other dependencies. The runner releases active futures and the executor
  before returning the report.

Global and Project supply two concrete executor adapters. Global keeps its
existing tree/content dispatch; Project keeps content evaluation. Tests supply
controlled work through the same seam. Keep the interface internal to
`update`; do not introduce dynamic registration or a general task framework.

| Owner | Responsibilities |
| --- | --- |
| Global / Project checker | Read the appropriate Lock, interpret selection, validate targets, group by source identity and ref, classify Ready/Pending, capture the report timestamp |
| Check attempt module | Apply the Pending-source budget, schedule work, track completion, fill skipped/timeout results through the adapter, aggregate the report |
| Global / Project executor adapter | Evaluate a group, preserve its cache/fingerprint-specific result fields, own attempt-local acquisition resources |
| SourceArchives | Resolve/acquire archives, share acquisition within an attempt, maintain disk cache and acquisition cancellation |
| Existing publication owners | Global full-check state persistence; Project revision validation, invalidation and persistence |

The caller must no longer maintain a second pending/completed set, timeout
loop, source-budget loop, or report sorting/counting pass. Move the duplicated
outcome shape into the shared module. Keep target types and source normalization
with their current owners; the scheduler need not inspect fingerprints or
reconstruct source identity.

## Behavior contract

### Admission and execution

1. Callers supply unique groups in their existing `GroupKey` order. A budget
   unit is one `(source identity, ref)` group. Distinct refs count separately;
   generic Git source-path case continues to distinguish identities.
2. Global fresh GitHub tree results are Ready and consume no budget. Global
   generic Git and all Project groups are Pending because they must resolve
   the current commit before consulting the archive cache. An archive cache
   hit therefore still consumes a source slot.
3. Admit the first `max_sources` Pending groups; produce Skipped results for
   the rest. `max_sources = 0` remains valid. Ready groups cannot consume or
   displace a Pending group's slot.
4. Clamp concurrency and deadline duration to at least one, as today. Start
   one absolute deadline after local preparation and skipped-result mapping,
   immediately before scheduling. Queue waiting and asynchronous group work
   share that deadline; completion of a group never resets it.
5. Collect completed work as it finishes, preserving successes alongside
   failures. At the deadline, every admitted but unfinished group, including
   queued groups, gets exactly one timeout result. Groups excluded by the
   source limit remain Skipped.
6. Keep failures local to their source. Existing fallback fields remain
   adapter-owned: Global tree timeout/skipped results can retain prior state,
   timestamps and truncation; content results retain their current semantics.

### Resource lifetime

All group futures and SourceArchives instances belong to this one invocation.
Do not spawn detached group tasks or move SourceArchives into a persistent
checker field. Its acquisitions map retains shared futures, so dropping only
the buffered stream is insufficient to release unfinished downloads.

On normal completion or timeout, drop the active stream and the owned executor
before returning the report for publication. Dropping the overall Check attempt
future must also drop both. Preserve the existing Git worker's cancellation
signal and cleanup behavior; do not add a new cancellation protocol.

Project revision invalidation continues to control publication through
`project_checks`; Relocate/Forget do not acquire a new immediate network-cancel
behavior in this refactor.

### Report and persistence

- Preserve aggregation phases: Ready results, Skipped results, completed work
  in completion order, then timed-out groups in original group order.
- Sort Skills by name and sources by ASCII-lowercased source followed by ref,
  using the existing stable sorting behavior. Do not add a new tie-breaker for
  generic Git paths that differ only in case.
- Derive cached/skipped counts from source statuses. Preserve the source-limit
  warning and apply only the existing adjacent `warnings.dedup()` behavior.
- Report `last_success_at_epoch_ms` is the maximum source success timestamp.
  `checked_at_epoch_ms` is captured after reading the Lock, as today, rather
  than at report completion. Cached data keeps its old success timestamp;
  request/download timestamps remain with existing evaluators/acquisition.
- Preserve empty selection, empty Lock, uncheckable-only and cache-only report
  behavior, including the existing distinction between full and selected checks.
- Global full-check summary persistence remains after report construction.
  Project publication still checks the persisted revision after computation;
  an Invalidated check returns normally without a report or summary. Existing
  persistence warnings are appended by those publication owners.

## Constraints

This is a behavior-preserving internal Rust refactor. Keep IPC, frontend,
public checker constructors/configuration, fingerprint algorithms, defaults,
cache formats and disk paths unchanged. Candidate 02 is outside this diff.

ADRs 0009, 0012 and 0015 remain authoritative. `Check attempt` already exists
in `GLOSSARY.md`; this design introduces no new domain term or ADR decision.

## Verification and implementation sequence

Baseline completed before production edits:

- `cargo test update:: --lib`: 115 passed (the filter also matches related
  app-update and task-update tests).
- `cargo test --test project_check_lifecycle`: 7 passed.

1. Add focused characterization coverage through the existing checker
   interfaces for cache admission versus source budgets, especially Global
   Ready results at zero budget and Project cached archives at zero budget.
   Run those tests before refactoring.
2. Introduce the attempt interface and migrate both callers. Test the shared
   interface with controlled group work: mixed Ready/Pending inputs, bounded
   concurrency, zero limits, one deadline across queued work, completed-result
   preservation, timeout/caller-drop cleanup, and report aggregation. Include
   observable executor-resource release so a surviving acquisition owner is
   caught without asserting private scheduler data structures.
3. Retain checker tests for cache/fingerprint mapping, mixed tree/content
   groups, source identity/ref grouping and external report contracts. Existing
   SourceArchives/Git cancellation and Project lifecycle tests remain valuable
   because they cover their own seams. Replace any newly redundant pure
   scheduler tests only after equivalent coverage exists at the shared interface.
4. Run update tests and Project lifecycle tests, then the Rust suite, formatting
   and lint checks appropriate to the final change. Do not add dependencies
   solely to express this refactor or its tests.

Previously reproduced baseline limitations: the host's `/Applications/ZCode.app`
causes `agents::tests::detects_home_roots_in_catalog_order` and
`one_hundred_skill_fixture_meets_the_inventory_budget` to fail; strict Clippy
flags the pre-existing `tasks/preview.rs::source_preview` argument count.
Report these separately if they recur, and verify the remaining suite with
only those known exclusions. Do not repair unrelated code in this candidate.

Completion requires both callers to use the shared protocol, the new ownership
and behavior tests to pass, and candidate 02's changes to remain intact. Commit
only on a subsequent explicit user request.

## Implementation result

Implemented the shared protocol in `src-tauri/src/update/attempt.rs`. The single
`run` interface accepts prepared results/work, scheduling limits and an owned
`GroupExecutor`. Ready outcomes are boxed to keep the input enum compact.
Global and Project each provide an attempt-local adapter owning SourceArchives.
The buffered work and adapter are released before report assembly/publication.

Both callers now delegate source budgeting and report construction, including
empty selections, to this interface. Their duplicate schedulers, completion
sets, outcome definitions and orphaned `Group::key` methods were removed.
Ordered pending slots are sufficient to track completion because callers
already supply unique groups in source-identity/ref order.

Target validation, source grouping, tree/content evaluation, cache acquisition
and publication remain with their prior owners. No IPC, frontend, dependencies,
domain documentation or persistence-format changes were needed. Candidate 02's
existing working-tree changes were preserved.

## Completed validation

- Two cache/source-budget characterization tests passed against the original
  implementation before the production refactor. They now cover the migrated
  checker interfaces, including Global stale-cache fallback on skipped rows.
- Eight tests in `update/attempt/tests.rs` exercise the shared interface:
  Ready/Pending budgeting and aggregation phases; bounded/out-of-order work and
  source failures; the single deadline across active and queued work; caller
  cancellation; zero concurrency/deadline; zero budget; empty/uncheckable-only
  attempts; and report ordering/timestamps. Resource-release assertions cover
  successful completion, timeout and caller cancellation.
- Existing checker, SourceArchives and Git acquisition tests were retained to
  verify their own cache/fingerprint/cancellation behavior through their seams.
- `cargo test update:: --lib`: 125 passed.
- `cargo test --test project_check_lifecycle`: 7 passed.
- Final Rust suite with only the two recorded host-dependent tests excluded:
  687 passed, 0 failed, 5 opt-in tests ignored, 2 filtered out.
- `cargo clippy --all-targets -- -D warnings -A clippy::too_many_arguments`:
  passed, retaining only the previously recorded lint exception.
- `cargo fmt --check` and `git diff --check`: passed.

The user authorized committing this implementation on 2026-11-07.
