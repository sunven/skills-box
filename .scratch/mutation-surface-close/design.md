# Mutation surface close and cancel protocol

Status: implemented and validated (2026-11-01)

The Install wizard previously read mutable task state and directly cancelled
tasks before resetting two Mutation handles. The Mutation surface now owns
the complete close protocol alongside launch, events and refresh. Cancellation
replies now share the generation protection used by launch and task events.

The first design round confirmed preservation of current close policies and
inclusion of stale cancellation-reply isolation. This document recommends the
implemented interface and records its behavior and verification contracts.

## Interface

The existing `MutationHandle` exposes two additional operations:

```ts
interface MutationHandle {
  // Existing state, stateRef, start, cancel, reset and setStartError remain.
  canClose: () => boolean;
  requestClose: (companions?: readonly MutationHandle[]) => boolean;
}
```

Both operations have stable callback identities and use the latest internal
state. `canClose()` is a read-only query for this handle. `requestClose()` makes
a synchronous close decision for this handle followed by any companions. It
returns `false` without side effects if any participant prohibits closure.

When closure is permitted, the module selects the first running participant,
requests best-effort cancellation of its known taskId, and synchronously resets
every participant. The pending `start()` promises resolve with `null`. The
operation returns `true` without waiting for the cancellation reply. This is
the Install wizard's existing installation-before-preview priority, rather
than a guarantee that backend cancellation succeeds.

The parent `onClose` callback stays with the dialog. Steps, selections, business
confirmation and result vocabulary remain with their current callers.

```ts
const closeWizard = () => {
  if (installMutation.requestClose([previewMutation])) onClose();
};

useModalFocus({ open, panelRef, onClose: closeWizard });
// Close button disabled: !installMutation.canClose().
// Preview never blocks closure, so the installation permission is sufficient.

const closeDialog = () => {
  if (mutation.requestClose()) onClose();
};
useModalFocus({
  open,
  panelRef,
  onClose: closeDialog,
  canClose: mutation.canClose,
});
```

## Close policies and ownership

The module interprets its existing action; no new close-policy configuration
is needed for currently fixed action behavior.

| Action and state | Close behavior |
| --- | --- |
| Any idle or terminal handle | Allow and reset |
| Running task preview | Allow immediate best-effort cancellation and reset |
| Running task installation in `preparing` | Allow immediate best-effort cancellation and reset |
| Running installation before the first event (`phase === null`) | Block |
| Running installation after preparing | Block |
| Running update, uninstall or native promise | Block |

Explicit `cancel()` stays separate from closure. It does not reset or close the
dialog; its terminal task event still produces the existing result and refresh.
An explicit cancel without a known taskId remains a no-op. A close without a
known taskId invalidates the generation, so a subsequently returned taskId is
cancelled by the existing stale-launch path.

`reset()` remains state-only. Opening a dialog, changing wizard selections and
other existing reset callers do not gain implicit task cancellation. Closing
keeps the existing suppression of old task events and resulting refresh after
reset. Normal terminal task and settled native-promise refresh remains unchanged.

## Implementation and lifetime

Implement the operations within `src/lib/mutation.ts`; no provider, task module,
background lifecycle or IPC adapter is added. Group closure uses the real two
handles already present in the Install wizard, with no new group hook or
registration mechanism.

Keep current state available synchronously inside the module as it transitions,
including launch, taskId, progress, terminal, cancellation and reset. A close
decision must not depend on whether React has rendered a queued transition yet.
Reuse the existing reducer and internal state ref instead of introducing a
second state machine. Retain the public `stateRef` for current launch guards
and unchanged callers; removing it entirely is outside this close refactor.

An explicit cancellation captures the current generation and task identity.
Every acknowledgement/rejection, catch and finally update checks that the same
run is still active. Replies from a reset, replaced or terminal run cannot set
the new run's error or clear its cancelling flag. Current-run cancellation
rejection and failure still surface the same messages. Repeated cancellation
while one is pending remains a no-op.

Keep the existing late-launch best-effort cancellation. No automatic unmount
cleanup or backend cancellation-window change is introduced. Backend rejection
after its cancellation seal remains possible when a preparing-install close
is requested; the accepted UX is immediate best-effort closure.

## Caller migration

- Install wizard: remove direct `cancelTask` calls and phase/taskId decoding
  from its close callback. Use the grouped request and permission for the
  existing close button and writing lock; keep preview/install steps intact.
- Update and uninstall: route Escape, overlay, header and footer/result closure
  through the single-handle request. Keep explicit cancel controls and launch
  guards, as well as confirmation and result UI.
- Other native dialogs keep their current flows. The default running policy
  remains compatible with their existing promise adapter. `useModalFocus`
  continues to own keyboard focus, trapping and restoration.
- Keep refresh mapping, TaskEvent/TaskResult types, Rust and IPC unchanged.

## Verification

Use the live `useMutation` hook with a QueryClient provider and the existing
Fake backend. Gate launch and cancellation replies with explicit promises so
tests control the interleaving. Observe the public close decision, state,
`start()` result, cancellation calls and query refresh; do not test a private
controller or map representation.

The live-hook tests cover:

1. Preparing installation and preview close immediately, reset and resolve their
   starts with `null`, even while a cancellation reply is held.
2. Installation before its first event and during writing, plus update/uninstall
   while running, refuse closure without cancellation or resetting a companion.
3. The wizard's two handles reset together after a permitted close, retaining
   installation-before-preview cancellation priority.
4. Closing before launch returns a taskId cancels the late task; old task events
   cannot restore state or advance the new wizard session.
5. An already returned taskId is remembered before a React render, so immediate
   closure does not miss known-task cancellation.
6. Cancellation rejection and error from run A after reset/start B cannot alter
   B. A's finally cannot clear B's own pending cancellation flag. Late replies
   after a terminal event cannot add an error to the terminal result.
7. Normal explicit cancellation still waits for a task event, preserves result
   and refresh behavior, and reports current-run rejection/failure.
8. A running native promise blocks close; normal settlement retains its result
   and refresh behavior.

Retain the existing App coverage for preview Escape and focus restoration,
install writing locks and preparation cancellation, and update cancellation
between Skills with completed results. Replace the redundant reducer-only
cancel/reset test once the live-hook cancellation coverage is established;
normalization and refresh-policy tests continue to cover their own contracts.

Run focused hook/App tests, the complete frontend suite, `npm run build` and
`git diff --check`. The focused baseline at `bdb30a8` is five files / 39 tests.

## Validation results

The live-hook cancellation test first failed against the original implementation:
a rejected cancel reply from run A attached its error to run B after reset and
restart. Another test reproduced a cancellation error attached to an already
completed result. Both now pass with active-generation and task-identity guards.

A further regression test failed when a taskId had returned but React had not
rendered it: closing reset the run without issuing cancellation. Synchronous
state-ref transitions now preserve that identity and the test passes.

- Live-hook lifecycle suite: 18 tests passed, covering close permissions, grouped
  reset and cancellation priority, late launch/events, stale rejection/error/
  finally updates, normal cancellation/refresh and native promise settlement.
- Focused mutation and App suites: six files / 56 tests passed. The old
  reducer-only cancel/reset test was replaced by live-hook coverage.
- Complete frontend suite: 27 files / 237 tests passed.
- TypeScript check and `npm run build` passed. Vite reported its chunk-size
  advisory for the main bundle; the build completed successfully.
- `git diff --check` passed. Changes are limited to the existing Mutation module,
  its tests, the three selected dialogs and these design records.

## Alternatives and recommendation

Three designs were compared for depth, locality and seam placement:

1. Per-handle `canClose` and `requestClose(companions)`: two operations on the
   existing module hide phase, task identity, cancellation and reset ordering.
2. Declarative close policies plus a group helper: makes the same operations
   configurable, but adds an option that could disagree with the action and
   adds a separate coordination entry point.
3. A dedicated close-group hook: gives the wizard a short call site but makes
   single-handle dialogs use a group and introduces a lifecycle wrapper for one
   currently complex caller.

Recommend the first design, keeping its group behavior limited to the current
wizard protocol. The existing task/promise drivers and Fake backend are real
adapters at the current seam. ADRs 0003, 0008 and 0014 continue to apply; no new
domain term or ADR is required.
