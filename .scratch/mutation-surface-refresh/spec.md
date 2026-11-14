# Mutation surface refresh

Status: confirmed by the user on 2026-11-14.
Date: 2026-11-14.
Implementation: complete; see [implementation record](implementation.md).
Review baseline: `54695da`.

## Findings

The architecture report's second candidate identified a shallow refresh-plan
interface: `mutationRefreshPlan` returns invalidate/remove keys, while
`refreshMutationQueries` adds report patches, lifecycle changes, Discovery
invalidation, Expert pack invalidation and environment rechecks separately.
The plan has one production caller and its tests do not describe all effects.

Current code was inspected and four behaviors reproduced with a real
in-memory QueryClient in a temporary test file, removed after execution:

1. Preview settlement invalidates local Expert packs despite its empty plan.
   This contradicts ADR-0014's established no-cache-effects rule.
2. Project Forget hooks supply Global Scope; a rejected native promise has no
   result Scope. Refresh therefore removes Global inventory/report/detail
   caches, while the intended Project caches remain.
3. Successful Forget removes its planned Project caches but invalidates and
   retains that Project's local Expert pack query. An active observer can then
   refetch a Project that no longer exists.
4. A pending native action that rejects after the hook's Scope changes refreshes
   the latest rendered Scope. The submitted Scope remains unrefreshed because
   settlement reads `optionsRef.current`. Action and Skill selection also come
   from the latest options.

Evidence: `src/lib/mutation.ts` refresh policy and `useMutation.settle`,
`src/components/project-dialogs.tsx` hook options and result construction,
`src/lib/mutation.test.ts` plan assertions, and
`src/lib/mutation.lifecycle.test.tsx` lifecycle coverage.

The four existing suites below pass all **42 tests**, while the four temporary
probes confirm the mismatches above:

```sh
pnpm exec vitest run src/lib/mutation.test.ts src/lib/mutation.lifecycle.test.tsx src/hooks/use-updates.test.tsx src/App.project-check-lifecycle.test.tsx
```

## Confirmed scope and behavior

Address all four findings while concentrating the complete refresh policy.

- Preview never patches, invalidates or removes queries, triggers environment
  rechecks, or starts/invalidates an Update check, including failed and cancelled
  terminals. Its wizard event handling remains as defined by ADR-0014.
- Capture the refresh action, Scope, Skill id and environment callback at the
  start of each operation. A returned result Scope still takes precedence;
  fallback context comes from submission, not a later render. Generation guards
  still discard superseded completions.
- Relocate and Forget supply their known target Project Scope before execution.
  Add obtains its new Project Scope from a successful result as today.
- Only confirmed successful Forget removes the forgotten Project's caches and
  invalidates its Update check lifecycle. Cleanup includes local Expert pack
  observations, Skill content and Stale lock entry caches, in addition to the
  existing planned Scope caches. It must not schedule a refetch of those removed
  queries.
- Failed Forget preserves cached data and refreshes the submitted Project's
  observations plus shared registry state. It neither clears Global caches nor
  invalidates a still-registered Project's check revision as though forgotten.
- Other non-preview terminal outcomes retain their existing refresh behavior,
  including partial writes and failures. Successful Update rows are marked
  current before report invalidation; Clean stale records and Restore retain
  their successful-row cleanup and Restore's targeted Update check.

## Ownership and interface

One internal Mutation refresh module owns every refresh decision and effect:
cache patches, invalidation, removal, environment callback, and coordination
with the existing Update check lifecycle. `useMutation` supplies the captured
operation context and terminal result through one refresh entry point.

Any action-to-query helper is private to this implementation; callers and tests
use the complete behavior rather than a separately exported partial plan.
An exhaustive action switch may remain an implementation detail. The design
requires no generic refresh engine, new dependency adapter or global store.

The existing Update check lifecycle continues to own revision checks, attempts
and stale-result rejection. Mutation refresh calls that interface without moving
its internals. Dialogs retain confirmation and wizard state; the Mutation
surface retains its task and promise drivers, cancellation and generation
guards. Project dialogs only provide their actual known target Scope.

Invalidation continues to be scheduled without waiting for query refetches.
Task launch rejection does not refresh; cancellation acknowledgements wait for
the terminal event; reset/close and superseded completions do not refresh.
Standalone Forget expert pack retains its existing action flow.

Depth comes from one complete refresh interface, locality from keeping policy
and effects together, and leverage from applying and testing the same behavior
for both existing drivers. A real QueryClient is the local test dependency.

## Verification plan

Seed real Global and multiple Project queries, then observe their data,
invalidation and removal through the refresh interface and Mutation surface.
Avoid mocking away QueryClient invalidation/removal.

1. Preview success, failure, cancellation and reset leave all seeded queries
   untouched, including Expert packs, Discovery, reports and content.
2. All action branches preserve their intended refresh coverage, target Scope
   and per-Skill specificity. Manage actions retain their environment recheck.
3. Failed Project Forget preserves Global and other Projects' data. Successful
   Forget removes the target caches, invalidates its check lifecycle and does
   not refetch the target's Expert pack/content queries.
4. While a task or promise is pending, change rendered Scope/Skill options.
   Result Scope takes precedence when provided; failure/null results use the
   submitted context. Superseded results still produce no refresh.
5. Partial/cancelled Updates patch only successful updated rows; failed rows
   remain unchanged. Clean/Restore results retain their cleanup and targeted
   check effects. Relocate retains revision invalidation order.
6. Preserve real observer/lifecycle tests where asynchronous queries matter;
   replace redundant key-list assertions once equivalent behavior is covered.

Run focused Mutation/Project/Expert pack tests, the full frontend suite,
TypeScript/build checks, and `git diff --check`. Backend mutation contracts and
Skill publication are outside this frontend change.

## Existing decisions

ADRs 0008 and 0014 already assign refresh ownership and exclude Preview cache
effects. ADR-0015 preserves Project revision validity; ADR-0031 requires Pack
observations to reflect Skill mutations. The fixes enforce those decisions.
The user confirmed all four observed problems and the complete ownership/test
plan above on 2026-11-14.
