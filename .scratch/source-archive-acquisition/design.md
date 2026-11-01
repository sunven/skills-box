# Source archive acquisition

Status: implemented and validated (2026-11-01)

Different commits previously waited behind one archive map lock during download.
Source archive acquisition now hides commit resolution, reuse, cache decisions
and parsing behind one interface. Different commits can download concurrently
within the Project Update check's existing concurrency limit.

## Interface and ownership

One `SourceArchives` instance belongs to one Check attempt. Its crate-private
interface has construction with the existing dependencies and one operation:

```rust
fn new(
    client: Arc<dyn ArchiveClient>,
    cache: CommitArchiveCache,
    clock: Arc<dyn Clock>,
    cache_ttl_ms: u64,
    limits: ArchiveLimits,
) -> SourceArchives;

async fn acquire(
    &self,
    source: &str,
    reference: Option<&str>,
) -> Result<AcquiredArchive, AcquisitionFailure>;

struct AcquiredArchive {
    archive: Arc<SourceArchive>,
    fetched_at_epoch_ms: u64,
    cached: bool,
}

struct AcquisitionFailure {
    error: AppError,
    previous_fetched_at: Option<u64>,
}
```

Source/ref normalization and Lock entry support checks remain in target
collection. The acquisition interface returns only what fingerprint comparison
and report construction need; commit, cache paths and sharing state remain
private implementation details.

`ProjectUpdateService::run_groups` constructs the attempt's module and keeps
source concurrency, pending/completed groups and the existing deadline.
Each group calls `acquire`, compares its Skill fingerprints, and builds its
outcome before it is considered complete. `ProjectUpdateChecks` keeps revision
validation and publication.

Place the acquisition implementation under `update/archive/acquisition.rs`,
with crate-private exports from `archive.rs`. Remove the archive map,
`load_or_download`, `ArchiveEntry` and `ArchiveFailure` from `project.rs` after
integration. Existing cache and parser implementation remains behind the
acquisition module's interface.

## Implementation and lifetime

Each call resolves its Source/ref to the latest commit. A short map lock finds
or registers a shared acquisition future for that commit; the call awaits the
future after releasing the lock. Use the existing `futures::Shared` support.
The map remains keyed only by commit, matching existing memory and disk reuse.

Shared futures capture independent client/cache/clock dependencies, never the
module containing their map. This prevents a holding cycle. They start no
detached worker. Dropping one waiter allows another waiter for the same commit
to continue the same acquisition. Dropping the entire Check attempt releases
the map and its waiting futures, releasing unfinished network futures. This is
the lifetime consequence of per-attempt sharing, rather than a new cancellation
mechanism exposed at the interface.

Synchronous cache reads, parsing, persistence and fingerprint calculation keep
their current execution context. The timer cannot preempt synchronous work;
the existing deadline is not a strict CPU deadline. Archive limits, cache
format and the number of retained archives stay as they are.

## Behavior contracts

- Success and failure are shared within an attempt. A failed acquisition is
  retried only in a later attempt, with no new failure written to disk.
- Fresh disk hits preserve the recorded time and return `cached = true`.
- New downloads record the clock before downloading. Their first acquisition
  returns `cached = false`; later successful reuse returns `cached = true`.
- Failure, including reused failure, is reported with `cached = false`.
- Fresh metadata with an unreadable payload is not a cache hit and contributes
  no previous success time. Stale metadata retains its previous success time
  when downloading fails. Existing stale files are retained.
- A cache write failure does not fail an otherwise successful acquisition.
- Commit-resolution failure carries no previous success time. Incomplete
  groups at the deadline keep the current timeout outcome.
- Independent attempts and Scopes have independent acquisition lifetimes.
  Relocate and Forget retain existing revision validation after computation.

## Seam, depth and tests

Reuse the `ArchiveClient` seam with production and scripted test adapters.
Use real temporary directories for the disk cache. No new dependency trait is
needed. The acquisition interface is the test surface: tests observe results,
download entry, network-future release and persisted cache contents.

Add deterministic tests that hold downloads behind explicit gates:

1. Different commits enter download before either is released, proving actual
   concurrency rather than relying on elapsed-time thresholds.
2. Same-commit callers share one download and both its success and failure.
3. Dropping the first waiter lets a second waiter finish the original download.
4. Dropping an attempt releases unfinished network work; a new attempt retries
   independently. A complete Project check deadline also releases downloads.
5. Fresh/stale/corrupt cache, write failure, limits, cached flags and timestamps
   preserve the existing contracts through the acquisition interface.

Keep complete Project check tests for grouping, source limits, fingerprint
results, source failure isolation and deadline behavior, and lifecycle tests
for revision publication. Parser tests still earn their place through parser
behavior. Avoid adding tests for the private shared map's representation.

Verification: focused acquisition and Project check tests, complete Rust tests,
`cargo fmt --check` and `git diff --check`. This change touches Rust and domain
documentation; the frontend contract is preserved.

## Validation results

The different-commit regression test first failed against the original
implementation, then passed after extraction. All twelve acquisition tests and
thirteen Project checker tests pass, including concurrent failure reuse,
independent attempt lifetimes and deadline release. All seven Project check
lifecycle tests also pass.

The remaining Rust suite passes with 494 unit tests and 32 integration tests;
three real-CLI tests retain their existing ignored status. The full suite has
two host-dependent failures in unchanged Agent detection fixtures:
`agents::tests::detects_home_roots_in_catalog_order` and
`one_hundred_skill_fixture_meets_the_inventory_budget`. Both count an extra
Agent because the vendored catalog includes `/Applications/ZCode.app`, which
exists on this machine even when the tests use temporary homes. The first
failure also reproduces in isolation. No Agent detection code or fixtures were
changed. The successful remaining-suite command is:

```sh
cargo test --quiet -- --skip agents::tests::detects_home_roots_in_catalog_order --skip one_hundred_skill_fixture_meets_the_inventory_budget
```

`cargo fmt --check` and `git diff --check` pass.

## Design trade-offs

The Source/ref interface has greater depth than an explicit-commit interface:
the current caller does not use the commit, so exposing it adds ordering
knowledge without a real caller benefit. A pure batch-return interface would
move fingerprint calculation outside the current complete-group deadline.
Keeping individual acquisition and existing group orchestration preserves
that behavior while concentrating archive protocol locality.

ADRs 0012, 0013 and 0015 continue to apply. This design does not require a new
ADR: fingerprint and publication decisions remain as recorded, and the
acquisition implementation can be revised behind the same interface.
