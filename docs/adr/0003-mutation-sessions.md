# Mutation sessions own the protocol

Status: accepted

Install, update and uninstall each re-implement the same ordering-sensitive protocol inside `TaskManager`: initial phase, runtime resolution, preflight snapshot/backup, cancellation seal, CLI execution, `ExecError` mapping, verification, baseline/log bookkeeping and one finish outcome. We extract that protocol into a `MutationSession` driven by a crate-private `MutationAction` trait. The session owns the unit loop, runtime resolution, cancellation windows, executor invocation, `ExecError` mapping, snapshot recording and the `TaskResult` envelope; each action owns its plan, units, preflight content, argv, completion (parse, filesystem verification, baseline, operation log), per-unit events and failure/cancel bookkeeping. Install and uninstall are one-unit sessions; update is one unit per update candidate. Hooks are sync; the session wraps `preflight` and `complete` in `spawn_blocking`. Preview and native attach/detach/extra-agent actions stay outside because they do not share the mutation protocol. This is a behavior-preserving refactor: Tauri commands, `TaskEvent` sequences, `TaskResult` shape and the frontend contract do not change.

## Considered options

- **Per-action flows with shared primitives**: rejected because the ordering of seal, snapshot and verification would remain in each action; the deletion test scatters the protocol back across callers.
- **Session-owned events and failure policy**: rejected because install, update and uninstall emit per-row progress at different points relative to bookkeeping, and their infrastructure failures produce three different result shapes (empty task failure, continue with a failed row, completed with a failed row). Encoding those timings in a profile widens the interface more than a reporter plus failure hook.
- **Enum dispatch inside one session**: rejected because it centralizes the text of three flows into one large match without giving tests a fake adapter; three real actions justify a trait seam.
- **Moving the registry, permit or marker into the session**: rejected because preview and native actions also need the single-writer registry and marker; the session returns one `FinishOutcome` and `TaskManager` keeps `finish`.

## Consequences

- `TaskManager` keeps validation, permit, task registration, `begin_marker`, `finish`, preview, native actions and shutdown; the session touches them only through narrow callbacks (`record_snapshot`, `set_cancellable`).
- The protocol can be tested once with a fake `MutationAction`; existing `TaskManager` and action tests remain behavior locks.
- `ExecError` is mapped once, in the session; actions receive an `AppError` and return a failure disposition. `classify_install_failure` moves out of `manager.rs`, removing update's dependency on the task manager.
- Update's between-unit cancellation and uninstall's completed-with-failed-row result are preserved through `SessionProfile` and `FailureDisposition`, not re-derived by the session.
- `preview` retains its own runtime/`ExecError` handling for now; the mutation side goes from four copies to one.
- Migration is incremental (install, uninstall, update); the old `run_*` functions are deleted only after each action's tests pass.
