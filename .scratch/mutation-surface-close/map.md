# Mutation surface close and cancel protocol

## Notes

Architecture review candidate 02: deepen the existing Mutation surface so that
dialogs no longer own task cancellation and reset ordering. Candidate 01 was
implemented and committed as `bdb30a8`; this exploration starts at that commit.
The user selected the next candidate and accepted both first-round recommendations.
Three interface alternatives were compared. The user confirmed the recommended
design; implementation was completed and validated on 2026-11-01.

## Facts

- `useMutation` owns task/promise execution, result normalization, refresh and
  generation guards for start, task events and reset. A stale launch that returns
  a taskId after reset already triggers best-effort cancellation.
- `reset()` invalidates the generation and resolves its pending start promise
  with `null`; it does not cancel an already known taskId.
- Before this change, `cancel()` did not guard its acknowledgement, rejection/error
  or final cancelling-state update against the run generation. A live-hook test
  reproduced a rejected reply attaching an error to a later run. Active-generation
  and task-identity checks now protect every reply path, including terminal runs.
- A live-hook test also reproduced a missing cancellation when a taskId had
  returned before React rendered it. The existing reducer now updates its state
  ref synchronously so close/cancel decisions retain the latest task identity.
- The Install wizard now calls the grouped `requestClose` instead of decoding
  task state and directly invoking `cancelTask`. It closes preview immediately.
  It closes a running
  install only in `preparing`; a null phase before the first event also blocks
  closure. Cancellation during close is best effort, without awaiting a reply.
- Update and uninstall prohibit closure throughout a running task. Explicit
  cancellation leaves the dialog open for its terminal result. Native promise
  actions also prohibit closure while pending and have no task cancellation.
- Update/uninstall now route every close trigger through `requestClose`, while
  keeping explicit cancellation separate. Their launch guards and the unchanged
  attach flow retain `stateRef`; the Install wizard no longer reads it.
- ADRs 0008 and 0014 retain frontend lifecycle ownership and preview's distinct
  wizard/result vocabulary. ADR 0003 retains backend cancellation windows.
- Focused baseline passed: five test files, 39 tests (`mutation`, `App.install`,
  `App.install-exec`, `App.update-exec`, `App.uninstall`). Existing tests cover
  preview Escape, install writing locks, explicit preparation cancellation,
  between-Skill update cancellation and result refresh. Eighteen new live-hook
  tests cover the asynchronous close and cancel lifecycle through its interface.

## Decisions so far

- Candidate 02 is selected for exploration.
- Preserve the existing close policies, including immediate best-effort preview
  and preparing-install closure, blocked installation before the first event and
  during writing, and blocked update/uninstall/native closure while running.
- Include generation isolation for stale cancellation replies, verified through
  the live hook. Preserve Rust IPC, wizard steps, result vocabulary and refresh
  policy.
- [Recommended design](design.md): add `canClose()` and
  `requestClose(companions?)` to the existing handle, keep fixed action policies
  inside the module, and preserve the Install wizard's cancellation priority.
  The user confirmed this interface, migration scope and verification plan.

## Verification

- Live-hook regressions failed before the relevant implementation changes, then
  passed: stale cancel errors on new and terminal runs, grouped reset, and a
  known taskId lost before React rendered it.
- Focused validation: six test files / 56 tests passed.
- Complete frontend validation: 27 test files / 237 tests passed.
- TypeScript check, `npm run build` and `git diff --check` passed.
- [Design](design.md) records the implemented interface and exact validation.

## Fog

- No unresolved design or implementation questions.
