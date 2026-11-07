# Batch operation preview, confirmation and retry

Status: implemented and validated (2026-11-07)
Implementation: complete
Review baseline: `677e4d3`

## Context

The user requested the next candidate after Batch target execution was
implemented and committed. This is candidate 02 from the whole-codebase
architecture review at `d5d8735`: concentrate the frontend Batch operation's
preview, confirmation and retry process. Candidate 03, Check attempt
scheduling, remains a separate future candidate.

The original report is an OS-temporary artifact:
`/var/folders/hx/3v6dd_js10lfdpq5mvfpdbth0000gn/T/architecture-review-20261107-161839.html`.
This local record preserves the candidate context independently of that file.

## Current friction

`src/components/batch-dialog.tsx` coordinates selected-target previews, Agent
counts, two-second refreshes, confirmation validity, task startup and result
history. Changes to Agent selection, uninstall mode, destination Project and
retry state require coordinated state and request-generation changes.

`src/lib/batch.ts` supplies result merge and completed-Skill projection, but
those pure functions cannot verify the dialog's asynchronous calling protocol.
`src/lib/mutation.ts::useMutation` already owns task event generations, task
start/cancel/terminal handling and query refresh. It is the existing module to
compose, not another lifecycle to reproduce.

The intended depth is a small interface for the Batch operation process that
hides this coordination. An interface exposing the existing setters and
request refs would merely relocate the current complexity and would not pass
the deletion test.

## Confirmed decisions

The user accepted both recommendations with “按推荐” on 2026-11-07:

1. One dialog-local module owns preview, confirmation validity, refresh and
   retry history, internally composing Mutation surface for task handling.
   It covers all eight existing Batch actions.
2. Uninstall mode, selected Agents, destination Project and the folder picker
   remain UI-owned choices. The module automatically invalidates previews and
   confirmations when its selection input changes; callers no longer
   coordinate invalidation steps.

The user confirmed the complete design below with “确认” on 2026-11-07,
authorizing implementation. No additional product choices were needed.

## Module interface and ownership

Implement the process as a dialog-local `useBatchOperation` hook under
`src/hooks/`, following the repository's existing hook test patterns. A
standalone state object plus another adapter layer is not needed: React owns
the dialog lifetime, and the current Mutation surface already supplies the
task lifecycle.

| Owner | Responsibilities |
| --- | --- |
| App and list selection | Fixed Batch selection and Scope, global running guard, completed-Skill selection updates and Project navigation |
| BatchDialog | Uninstall mode, selected Agents, destination Project, directory picker, presentation, warning copy and focus |
| Batch operation module | Selected-target/count previews, request identity, revision-bound confirmation, refresh coordination, start handoff, retry keys, accumulated history and result projection |
| Mutation surface | Task generations, task ID, progress, cancellation, terminal outcomes, normalized errors and query refresh |
| Rust Batch modules | Preview/revision and command truth, eligibility, authorization, task/permit ownership and action execution |

The input describes the current Scope, action, selected Skill IDs, selected
Agent IDs, optional destination Project root, available source Agents and
whether external conditions block submission. Existing running/completed
callbacks remain so App's synchronous close/Scope guard and list selection
behavior are preserved. Selection values remain owned by BatchDialog; the
module owns the protocol required to apply them safely.

The output contains read-only process state needed for rendering: preview,
Agent list/counts, loading/error/change notice, required confirmations and
their current values, submission availability, task progress and combined
result/retry summary. User commands cover changing a confirmation, reloading
the preview, starting, stopping later targets and retrying unfinished targets.
A close guard reads the current running state without exposing a mutable ref.
The directory picker remains UI-owned; its existing error display and submit
blocking must be preserved through a narrow selection-error input or command,
without exposing arbitrary process-state setters.

Keep request generations, preview IDs used for counts, result history updates
and Mutation surface reset/start coordination private. The interface must not
return `requestSeq`, `stateRef`, `setPreview`, `setHistory`, or a general
invalidation/reset operation for callers to sequence themselves.

Copy to project's available Agents come from its latest accepted preview.
Keep those preview-derived facts in the module, rather than feeding them back
through a separate UI effect. Normalize selected Agent order against that
list and retain explicitly selected Agents that disappear during retry.
Changing destination still clears the UI's selected Agents as today; clearing
the old preview, Project Agent facts, counts and confirmations belongs to the
module's handling of the changed destination.

## Identity and ordering contract

- Associate each selected-target preview with the Scope, action, selected
  Skills/Agents, Project root and retry target keys used to request it.
  Input changes must make the old preview non-submittable immediately; an
  effect alone must not leave a render where old confirmation can be reused.
- Guard successes, failures and loading completion by request identity.
  Preserve the separate identity of advisory count requests, whose Agent set
  differs from the submitted preview.
- Bind deletion and copy confirmation to the current preview revision.
  Unchanged revisions retain confirmation; changed revisions and preview
  errors clear it. Confirmation UI continues to express the same two user
  authorizations.
- Preserve the two-second polling interval and the existing joint wait for
  selected-target/count refresh responses. A count failure is ignored, but
  a pending count response still delays application of that refresh cycle.
  Decoupling these responses is a separate possible change, not part of this
  behavior-preserving refactor.
- Starting invalidates pending preview application before handing off to
  Mutation surface. Preserve the synchronous App running notification and
  reject repeated confirmation while the task is active. Stop continues to
  use Mutation surface's cancellation protocol.
- On a Batch result, merge target rows and notify completed Skill IDs once.
  On a start/terminal outcome without Batch rows, clear confirmations and
  load a new preview while preserving the existing error presentation.
- Retry retains successful rows, narrows target keys to failed/not-executed
  rows, resets the previous task through Mutation surface and requires a new
  preview and user confirmation. Result view and selection locking remain
  local to the open dialog; closing does not persist this history.
- Dispose preview polling and reject late preview work on unmount. Task
  ownership and cancellation remain with the existing Mutation surface and
  backend; unmount does not introduce a new task-cancellation path.

## Existing behavior and ownership to preserve

- Backend previews and revisions remain authoritative. Rust retains target
  eligibility, command construction, authorization and write-time revalidation.
- Mutation surface retains task generations, cancellation, terminal outcomes
  and refresh ownership; the new module must not introduce a second task reducer.
- All eight Batch actions remain supported, including Copy to project,
  Remove from Agents, Clean stale records and Restore skill.
- Complete Uninstall remains the default removal mode. Selected Agent order,
  removed Agents retained as explicit retry targets, optional Restore
  attachments and Copy to project's independent destination semantics remain.
- Selection changes invalidate confirmation. A changed preview revision resets
  deletion/copy confirmation and shows the existing changed-target notice.
- Agent-count previews are advisory. Their failures do not replace the selected
  target preview's error. Restore does not request count previews; Copy to
  project requires a chosen Project root before previewing.
  Project Agents come from the authoritative preview and feed the count
  request. Initial count requests currently use effect disposal to reject old
  responses, without a separate running/result gate; their scheduling must be
  reviewed explicitly rather than silently changed during extraction.
- Refresh runs every two seconds only while the preview is open and no task is
  running; requests within one refresh effect do not overlap. Currently the
  selected-target and count refreshes are awaited together. Decoupling their
  responses would change current timing and is not implicit in extraction.
- Starting rejects late selected-preview responses. A start without a Batch
  result clears confirmation and reloads the preview.
- Retry includes only failed or not-executed target keys, keeps earlier
  successes, requires a fresh preview/confirmation and locks selection controls.
  Preserve previous eligibility during history merge so a now-ineligible retry
  does not incorrectly clear a selected Skill.
- Keep focus, close behavior, existing labels, command display, App running
  notification and completed-Skill selection updates.

## Domain and ADR alignment

Use existing Batch operation, Batch target, Mutation surface, Scope, Project
and Agent attachment vocabulary. This is an implementation module, so no new
domain term is currently needed.

ADRs 0007, 0008, 0010 and 0014 retain pure workspace projection, dialog-local
mutation lifecycle and the shared Fake backend. ADRs 0021 and 0024–0027 retain
Batch ownership, retry/confirmation rules and individual action semantics.
No new backend contract, global store or ADR change is proposed.

## Verification gathered before implementation

- `pnpm test src/App.batch.test.tsx src/lib/batch.test.ts src/lib/mutation.lifecycle.test.tsx`:
  3 files, 35 tests passed on `677e4d3`.
- `pnpm test src/App.copy-to-project.test.tsx src/App.stale-records.test.tsx`:
  2 files, 12 tests passed on `677e4d3` (47 passing across these checks).
- The shared Fake backend supports command overrides, delayed responses,
  command recording and controlled task events. Existing `renderHook` tests
  for Mutation surface and Update check show how to use that adapter through
  a module's interface without mounting all of App.
- Existing App Batch tests cover selection/confirmation invalidation, a late
  Agent preview, start rejection, cancellation and result/retry behavior.
  They do not currently override `refresh_batch_preview` or use fake timers
  to exercise polling races. Those scenarios need focused coverage at the
  chosen interface, with UI and action behavior coverage retained.

## Implementation and acceptance plan

1. Add focused behavior coverage before moving the asynchronous coordination
   where existing tests leave gaps, particularly polling revision changes and
   the current selected/count refresh wait. Use deferred responses and fake
   timers, without wall-clock sleeps.
2. Move the process into the hook and have BatchDialog consume that same
   interface. Keep action-specific controls, directory selection, warning
   rendering and focus in the dialog. No Rust, IPC, new global store or new
   dependency is needed.
3. Test through `renderHook` with the real QueryClient, the existing Fake
   backend's typed command overrides and manual task events. Do not create
   a second backend adapter or reproduce Mutation surface's reducer tests.
4. Verify old success/error/finally responses after selection changes; count
   request isolation; non-overlapping polling; unchanged/changed revisions;
   refresh failure and recovery; start during a pending refresh; repeated
   confirmation; retry history/eligibility and removed Agent targets; and
   cleanup after unmount. Assert public state, IPC payloads and callbacks.
5. Retain App tests for controls, prompts, command display, Scope and Project
   integration, running guards and completed list selection. Replace lower
   level tests only when equivalent behavior is covered through the new
   interface; avoid keeping duplicate assertions solely to test internals.
6. Run targeted Batch/Copy/Restore/Mutation tests, the complete frontend suite,
   TypeScript/build checks and `git diff --check`. Document baseline failures
   separately if any appear. No commit or push is implicit in this design.

Acceptance requires BatchDialog to stop coordinating request generations,
preview invalidation, polling and history merging. Callers and tests must use
the same module interface, while all existing action and task semantics remain
owned by their established modules.

## Implementation and validation outcome

Implemented in `src/hooks/use-batch-operation.ts` and consumed by
`src/components/batch-dialog.tsx`:

- The hook owns preview state and request identities, advisory Agent counts,
  preview-derived Project Agents, both confirmations, polling, start handoff,
  result merging and retry keys. BatchDialog keeps its selection controls,
  directory picker, rendering and focus behavior.
- Preview state is keyed to the actual request values. A changed selection
  hides old authorization in its first render; late successes, errors and
  loading completions cannot affect the new selection. Equivalent input
  objects do not invalidate an unchanged selection.
- Mutation surface is composed internally. App's running notification remains
  synchronous, duplicate confirmation is rejected while a task is active, and
  callers use a close guard without accessing the mutation ref.
- Directory-selection errors use `reportSelectionError`; reloading or applying
  a fresh authoritative preview clears them through the same submission gate.
- Selected-target/count polling retains its two-second interval and joint
  wait. Count results are accepted only for their current count request, and
  count failures remain advisory. The existing initial count request behavior
  during execution was retained.
- `completedSkillIds` and `mergeBatchRows` moved unchanged into the hook as
  private implementation. Their two direct unit tests were replaced by a
  hook-level retry test covering partial Agent failures, only-ineligible
  Skills, retained successes, disappeared Agents and eligibility retention.
- No backend, IPC, dependency, shared Mutation surface or domain/ADR changes
  were required.

Verification:

1. Before production changes, two temporary BatchDialog tests passed against
   the original implementation: same/changed revision confirmation handling,
   and waiting for count refresh without overlapping refresh cycles. These
   tests were moved to the hook interface after extraction; the temporary
   duplicate component test file was removed.
2. `src/hooks/use-batch-operation.test.tsx` now has **20 passing tests** using
   the existing Fake backend, deferred responses, fake interval timers and
   controlled task events. They additionally cover old preview/count/refresh
   success and failure, immediate selection invalidation, refresh recovery,
   start during pending refresh, duplicate confirmation, start/terminal errors,
   retry history, destination isolation, picker errors and StrictMode/unmount.
3. `pnpm test src/hooks/use-batch-operation.test.tsx src/App.batch.test.tsx src/App.copy-to-project.test.tsx src/App.stale-records.test.tsx src/lib/mutation.lifecycle.test.tsx`:
   **5 files, 65 tests passed**.
4. `pnpm test`: **38 files, 346 tests passed**.
5. `pnpm build`: TypeScript and production Vite build passed. Vite emitted its
   non-failing advisory about a minified chunk larger than 500 kB; this change
   does not alter bundling configuration.
6. `git diff --check` passed. Architecture inspection confirmed BatchDialog no
   longer owns request generations, polling, preview invalidation or history
   merging, and it does not import the Batch IPC functions or Mutation surface.

The user authorized committing this implementation on 2026-11-07.
