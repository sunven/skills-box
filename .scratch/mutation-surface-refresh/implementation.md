# Mutation surface refresh implementation

Status: complete.
Date: 2026-11-14.
Review baseline: `54695da`.

## Changes

- `src/lib/mutation-refresh.ts` owns the complete terminal refresh behavior:
  report patches, cache invalidation/removal, environment rechecks and Update
  check coordination. The exported partial refresh plan has been removed.
- `useMutation` captures action, Scope, Skill selection and environment callback
  at submission and passes that context through both drivers. Returned Scope
  still takes precedence. Existing generation and cancellation guards remain.
- Preview returns before any refresh effect. Successful Forget invalidates the
  Project check lifecycle and removes its observations, including local Expert
  packs, Skill content and Stale lock entries, without scheduling their refetch.
  Failed Forget keeps data and the current revision while refreshing the target
  Project and shared registry observations.
- Project Relocate and Forget dialogs provide their selected Project Scope
  before executing, so rejected promises refresh the correct Scope.
- QueryClient state and active observers replace mocked refresh methods and
  partial key-list assertions. Regression coverage includes both drivers,
  context changes, Preview terminals, reset/supersession, Project dialog errors,
  pending Update checks, partial/cancelled Update rows, and Clean/Restore.

## Verification

- Before implementation, the new regression tests produced 18 expected failures
  and 18 passes, demonstrating the four reported defects and callback capture.
- Focused Mutation, Project, Expert pack and Batch suites: 141 tests passed.
- Full frontend suite (`pnpm test`): 467 tests passed across 46 files.
- Production build (`pnpm build`, including TypeScript): passed. Vite reports a
  bundle-size warning for the main JavaScript chunk over 500 kB.
- `git diff --check`: passed.

No backend contracts or files changed. Existing ADRs already define the behavior
enforced here; no new domain decision or glossary term was required.
