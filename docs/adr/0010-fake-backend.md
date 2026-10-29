# One fake backend behind App tests and the preview

Status: accepted

Every App-level suite re-implemented a Tauri command switch: ten suites covered 2–11 commands each, four task suites copied channel emission and event-index bookkeeping, and the preview harness omitted `get_update_check_state`, `start_uninstall` and `detach_agent`. Command strings lived only as literals inside `api.ts`, so adding a command could not be detected until a button happened to call it. The preview and tests also drifted in their detail, directory and file responses.

We make the IPC command surface explicit: `API_COMMANDS` and `ApiCommand` live in `src/lib/api.ts`, and every `invoke` uses the constant. `src/test/fake-backend.ts` implements a `FakeHandlers` record keyed by `ApiCommand`, with a `FakeCommandContract` supplying each command's args/result types. `createFakeBackend` owns default data (healthy environment, empty snapshot, derived detail, file/tree defaults, empty update report), a mutable snapshot for native attach/detach/add/remove, recorded calls, task handles with per-channel event ordering, command overrides, and `dispatch()` for overrides that fall through after an injected failure. `autoTasks` drives preview/install/update/uninstall to completion for the preview only; tests emit task events manually.

All App suites now use one backend per test; the preview passes its existing rich fixtures as the data adapter. `clearMocks()` moved to the global test setup. A coverage test asserts every `API_COMMANDS` value has a handler and that command names are unique; missing handlers also fail typecheck.

## Considered options

- **Scan `api.ts` source for invoke literals**: rejected as the primary contract; the typed constant makes the command name and the fake handler part of one compile-time graph.
- **Keep a separate preview mock**: rejected; preview drift was the motivating failure, and it now exercises the same every-command surface.
- **A fully stateful fake that applies task results to the snapshot**: rejected; successful/partial/cancelled task result matrices belong in explicit test events, and auto-applying them would hide the refresh behavior under test.
- **Error maps plus script queues plus a generic hook**: rejected in favor of data factories that may throw and typed per-command overrides with `dispatch()`.
- **Shared render/query helpers as well**: deferred; this change unifies the backend, not the DOM query vocabulary.

## Consequences

- Adding an API command requires adding it to `API_COMMANDS` and the fake handlers; TypeScript and the coverage test fail otherwise.
- App suites no longer contain command switches, manual `runCallback` indexes or per-suite `clearMocks` hooks.
- The preview covers the full command surface, including uninstall, detach and persisted update-check state.
- Native writes update the fake snapshot; CLI task completion remains explicit and manual in tests.
- Production code changes only the command-name constants; no IPC, payload or UI behavior changes.
