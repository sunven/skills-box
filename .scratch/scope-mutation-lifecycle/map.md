# Scope Mutation task preparation and lifetime

## Notes

Architecture candidate 02 from the whole-codebase review. Candidate 01 was
implemented and committed as `030c320`; the user requested the next candidate.
The user confirmed the recommended scope, including both planning lifetime
fixes, and authorized implementation. Implementation and validation are
complete.

## Facts before implementation

- Install, Update and Uninstall separately acquire the mutation permit, resolve
  their Scope layout, run blocking planning, register a task and move the permit
  into session execution (`tasks/manager.rs:697`, `889`, `943`).
- During blocking planning, these three permits remain in the requesting async
  future. Dropping that future releases the permit while the already-started
  blocking worker continues using its captured layout. Tokio does not abort a
  blocking worker that has already started.
- Native Attach and Detach already move their permits into the blocking worker
  (`tasks/manager.rs:767`, `789`), but each still assembles acquisition and Scope
  resolution independently.
- Planning happens before task registration. `shutdown_mutations` can return
  with no active task during planning; the planning result can subsequently
  register and start a task because registration does not reject shutdown
  (`tasks/manager.rs:553`, `714`, `905`, `959`).
- Update validates names before acquisition; Install validates its request after
  acquisition. Existing error precedence is intentional, especially ADR 0013's
  busy-before-invalid-install behavior.
- Relocate and Forget use `begin_registry_change` in their commands. Preview,
  reads and Update checks remain outside the mutation permit, as required by
  ADRs 0003, 0013 and 0015.
- The existing ConflictProvider is used only by Preview. Install planning calls
  `validate_request` and `conflicts_for` directly. Runtime/process adapters can
  pause execution after planning, but do not provide a planning pause.

## Agreed scope

- Deepen the Scope Mutation task module inside TaskManager for Install, Update,
  Uninstall, Attach and Detach. Acquisition must precede Scope resolution; the
  module owns the permit through actual planning/execution completion.
- Do not expose a `(permit, layout)` pair whose lifetime callers must assemble.
  Keep the protocol behind the module's internal interface; permit and registry
  ownership stay in TaskManager, preserving ADR 0003.
- Fix both planning lifetime gaps: a dropped start request must not release the
  permit while its worker is still running, and a planning result must not
  register/start a new Mutation task after shutdown begins.
- Preserve current public TaskManager/IPC interfaces, TaskEvent/TaskResult
  shapes, error priority, cancellation windows, marker policy, CLI arguments and
  native actions' independence from Node/npx.
- Keep Extra agent configuration actions and Project registry changes on their
  existing permit paths. Preview, reads and Update checks retain their existing
  lifecycles. No frontend changes are needed.

## Test surface

- Existing public TaskManager behavior locks cover single-writer refusal,
  registry guard sharing, execution cancellation, marker cleanup and native
  Detach without CLI/runtime use.
- Verify that a held registry guard makes a Skill mutation return `task.busy`
  before resolving an invalid Project Scope, then allows work after release.
- Exercise planning, execution and completion through the module interface;
  cover planning error, request interruption, shutdown and normal/cancelled
  completion. Pauses must use deterministic synchronization.
- Reuse the execution adapters and temporary filesystem. Any planning callback
  seam stays internal and is the same interface used by the real planners;
  avoid exposing a test-only planner in the public TaskManager interface.

## Implementation

- `src-tauri/src/tasks/manager/scoped_mutation.rs` owns permit acquisition,
  Scope resolution, blocking preparation and session lifetime. The three CLI
  mutation paths use its owned plan; native Attach and Detach use its blocking
  worker entry point.
- `TaskManager::register_mutation` checks shutdown while holding the task-map
  lock, preventing late registration after planning.
- Public interfaces and execution behavior are preserved. ADR 0016 records the
  ownership decision; ADR 0013 now points to the enforced acquisition order and
  its observable regression coverage.

## Validation

- Seven deterministic tests in
  `src-tauri/src/tasks/manager/scoped_mutation/tests.rs` cover all five entry
  points, error priority, abandoned planning/native work, planning failures,
  shutdown and cancellation. Pauses use synchronization rather than sleeps.
- Restoring the old lifetime and shutdown behavior made the corresponding two
  regression tests fail; both fixes were restored before final validation.
- Backend `cargo test --quiet`: 507 passed, 3 real-CLI/network tests ignored by
  default. `cargo fmt --check` and `git diff --check` passed.

## Fog

None; the recommended scope is confirmed.
