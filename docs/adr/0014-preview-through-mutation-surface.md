# Preview shares the frontend mutation lifecycle

Status: accepted

ADR 0008 extracted one frontend mutation surface but deliberately left the install preview on `task-runner.ts` because preview is read-only and drives wizard steps rather than a mutation result. `task-runner.ts` then kept a second task reducer, generation guard, cancellation path and start-error slot, almost line-for-line identical to `useMutation`. We let preview use `useMutation` with an explicit preview action: the wizard passes an `onEvent` callback for its step transitions, `setStartError` covers local validation, and the refresh plan for preview is empty. The backend `MutationSession` boundary is unchanged: preview still never enters the session, writes no snapshot and refreshes no cache. `task-runner.ts` is deleted and its two label tables move to `task-labels.ts`. This supersedes ADR 0008's choice to keep preview on `task-runner`, not its reason for keeping preview out of the mutation result vocabulary.

## Considered options

- **A generic raw-result `useMutation<T>`**: rejected because the wizard needs event-driven step transitions and a local start error, not only a typed result; the callback plus explicit action is the smaller interface.
- **A shared `useTaskStream` hook with two wrappers**: rejected because it moves the duplicate reducer into another hook instead of removing it.
- **Keeping `task-runner.ts`**: rejected because the duplicate guard and cancel paths drift exactly like the dialogs ADR 0008 consolidated.

## Consequences

- Preview keeps its own wizard steps, cancellation and start-error display, now through the same reducer and generation guard as every mutation.
- `MutationAction` gains `preview`; its refresh plan invalidates and removes nothing, so a preview never touches the query cache.
- The preview still produces a `TaskResult` with a `PreviewSummary`; it is not shown as a mutation result and never writes a backup or log.
- `task-runner.test.ts` is replaced by the existing App wizard coverage plus mutation reducer tests for `setStartError` and the empty preview refresh plan.
