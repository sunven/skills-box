# One mutation surface for task state, results and refresh

Status: accepted

The install, update and uninstall dialogs each duplicated the same task lifecycle (progress, cancel windows, terminal result, failure, backup/log reveal and cache invalidation), while attach, detach and extra-agent management each re-implemented pending/error state and their own invalidation lists. We introduce `src/lib/mutation.ts`: one normalized `MutationState` (`MutationResultView`, `MutationRow`, `MutationOutcome`) and one `useMutation` hook that runs either a `taskEventDriver` (CLI tasks stream `TaskEvent`s) or a `promiseDriver` (native commands return one promise). The hook owns the event-generation guard, task-id cancellation, progress/terminal transitions, error fallbacks and the refresh policy. `mutationRefreshKeys` maps each action to its query prefixes; `refreshMutationQueries` also patches updated rows to `current` in the update report and routes manage add/remove through the injected environment recheck. `task-runner.ts` shrinks to the low-level task-event driver and shared labels; the install preview keeps using it because it is read-only and drives wizard steps rather than a mutation result.

Shared UI lives in `src/components/mutation/`: `MutationProgress`, `MutationError` and a configurable `MutationResult` (test id prefix, copy, backup label, log/reveal visibility, retry mode, retry callbacks, warnings test id). Dialogs keep only their confirmation, selection and wizard state. The Rust `TaskAction`/`TaskResult`/`TaskEvent` contract and IPC remain unchanged; normalization is a frontend projection. Preview and confirmed exit stay outside the mutation surface.

## Considered options

- **Keep per-dialog runners and only share the result JSX**: rejected because the invalidation, report patch and error fallbacks drift independently, which is where the duplication actually hurt.
- **Change the backend task contract to a generic mutation result**: rejected; it would churn the Rust task protocol and tests for a frontend modeling problem.
- **A generic async hook without refresh ownership**: rejected because every caller would still repeat its own invalidation sets and the update-report patch.
- **A React context/store for mutations**: rejected; mutation lifecycles are dialog-local and a provider would hide the driver boundary rather than simplify it.

## Consequences

- Install, update and uninstall keep identical cancellation windows, test ids and result vocabulary (`已安装`/`已更新`/`已卸载`) while sharing one lifecycle.
- Attach, detach and extra-agent add/remove share the same failure/partial result surface and keep their success notices and close behavior.
- Refresh happens on every terminal task outcome and every settled native promise; manage add/remove additionally trigger the environment recheck.
- Update still patches successful rows to current without starting an automatic check; attach/detach invalidate only the affected skill.
- The progress component is shared with the read-only preview, so preview keeps its behavior without entering the mutation surface.
