# Project Update check lifecycle

## Notes

Architecture candidate 01: deepen the module that owns Project Update check
submission and invalidation. The user accepted all recommended design decisions
and authorized implementation in the final shared-understanding confirmation.
Implementation and validation are complete.

## Decisions so far

- A successful Relocate or Forget invalidates every check attempt that began
  before that change. A subsequent Relocate back to the earlier Project root
  does not restore the result's validity. Switching the active Scope does not
  invalidate a check.
- An Invalidated check contributes no report, badge, Last check, or persisted
  summary. Its waiting state ends normally, without a check-failure message.
- If a Project registry change succeeds but check-state cleanup fails, preserve
  the successful Project change and show a cleanup warning. Old summaries must
  remain invisible both in the current session and after restarting the app.
- After a successful Relocate, immediately run a new full Update check when
  the new Project root is available and contains Managed skills. Do not wait
  for the previous six-hour interval.
- Persist a Project check revision. Every successful Relocate changes that
  revision, including relocation to the same root. Read a persisted Project
  summary only when its revision matches the current Project; a forgotten
  Project has no valid summary. On upgrade, discard Project summaries that
  lack a revision and check again; preserve Global check history.
- Track pending checks by Scope and Project check revision. Prevent duplicate
  checks within the same valid revision, allow independent Scopes to check,
  and let a new revision check while an Invalidated check still awaits the
  network. Completion of an old attempt must not clear a new attempt's
  waiting state.
- Capture each attempt's Scope at its start so a response cannot be placed in
  a different Scope's cache after an active-Scope switch.
- One backend module owns capture, publication, invalidation and valid-summary
  reads. The registry owns the persisted revision. Keep the network outside the
  mutation permit and the coordination lock; the frontend shares pending state
  by Scope/revision.
- Relocate and Forget return successful results with typed cleanup warnings.
  The app retains these warnings after the dialog closes. Tests exercise the
  lifecycle interface and complete app flows, including deterministic paused
  requests and cleanup failure followed by restart.

## Implementation

- `src-tauri/src/update/project_checks.rs`: coordinated lifecycle interface.
  `update/project.rs` computes reports without owning persisted summary state.
- `src/lib/update-check-lifecycle.ts`: shared check identity, deduplication,
  publication and in-place clearing of observed cache values.
- `src/hooks/use-auto-update-check.ts`: new revisions check immediately after
  the relocated inventory refresh; switching Scope preserves freshness policy.
- `src/components/project-dialogs.tsx` and `src/App.tsx`: warning transport and
  persistent-in-session feedback for successful Project changes.
- [ADR 0015](../../docs/adr/0015-project-update-check-lifecycle.md) records the
  durable revision and coordination trade-off.

## Validation

- `pnpm test`: 26 files, 215 tests passed.
- `pnpm build`: TypeScript and production build passed.
- `cargo test --quiet`: 500 tests passed; 3 real-CLI/network tests ignored by
  their default configuration.
- `cargo fmt --check` and `git diff --check`: passed.
- Backend behavior: `src-tauri/tests/project_check_lifecycle.rs` covers
  same-root Relocate, A -> B -> A, Forget, concurrent Scopes/revisions, legacy
  state, cleanup failure and restart filtering.
- Frontend behavior: `src/hooks/use-updates.test.tsx` covers Scope capture,
  shared deduplication, observed-cache clearing and late completion/error.
- App behavior: `src/App.project-check-lifecycle.test.tsx` covers inventory
  refresh before immediate checking and cleanup-warning visibility after
  Relocate/Forget dialogs close.

## Facts before implementation

- The existing check-state mutex protects file read-modify-write, but cannot
  prevent a check that started before Relocate or Forget from later storing its
  result (`update/project.rs`, `commands.rs`, `update/state.rs`).
- `last_opened_at` changes for both Relocate and Scope switching, so it cannot
  identify the lifetime against which a result was computed (`projects.rs`).
- Relocate to the same root and A -> B -> A are allowed (`projects.rs`).
- The frontend check callback uses the current render's Scope. The installed
  QueryClient implementation updates pending mutation callbacks, so an old
  Scope's response can be written to a new Scope (`use-updates.ts`).
- Relocate invalidates disabled report/time queries without clearing their
  cached values. The automatic-check hook retains the same Scope's scheduling
  anchor, and its in-flight guard can discard the request for the new root
  (`mutation.ts`, `use-auto-update-check.ts`, `use-updates.ts`).

## Fog

None; the agreed module ownership, warning transport and test surface are
implemented.
