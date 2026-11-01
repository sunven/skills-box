# Source archive acquisition

## Notes

Architecture review at `7f3688f`, candidate 01: deepen Project Update check
Source archive acquisition. The user selected this candidate, accepted the
recommended decisions from both design rounds and confirmed implementation.
Implementation was completed and validated on 2026-11-01.

## Decisions so far

- Concentrate acquisition, reuse, freshness, parsing, persistence and failure
  history in one deep module, and remove serialization of downloads for
  different commits. Preserve report, six-hour cache and timeout semantics.
- Share in-flight acquisition only within one Check attempt. Independent
  Scopes and attempts retain independent lifetimes. Continue to share the
  existing disk cache across attempts.
- Use a per-attempt Source archive module with `acquire(source, ref)`. Hide
  commit resolution, shared acquisition, freshness, parsing, limits,
  persistence and acquisition failure history behind that interface.
- Keep source grouping, concurrency slots, target fingerprint comparison,
  report construction and the complete-group deadline in the Project Update
  check module. Keep publication in ProjectUpdateChecks.
- Keep synchronous cache reads, parsing and writes in their existing execution
  context. Do not introduce background workers whose work outlives a dropped
  Check attempt.
- [Design](design.md) records the interface, lifetime consequences, behavior
  contracts and verification plan derived from these decisions.

## Facts

- Before this change, the archive map lock was held across download and parsing,
  serializing different commits. `SourceArchives` now registers shared futures
  under a short lock and awaits outside it, allowing different commits to
  download concurrently.
- Defaults: 4 source groups in flight, 20 sources, 30-second group deadline,
  60-second request timeout; compressed and retained-content limits are
  64 MiB and 256 MiB per archive respectively.
- Each attempt retains completed archives to its end. Cache reads,
  parsing and writes run synchronously inside the async check. Cancellation
  drops network futures; no detached work is started.
- Success and failure are both reused by commit within an attempt. Failed
  acquisition does not persist a new cache entry. The next attempt can retry.
- Fresh cache hits retain their timestamp; successful downloads record the
  clock before downloading. Cache write failure does not fail the check.
- ADRs 0012 and 0015 retain fingerprint and publication ownership. The network
  stays outside the mutation permit and Project coordination lock.

## Verification

- The different-commit regression test failed against the original implementation
  and passes after extraction. Twelve acquisition tests and thirteen Project
  checker tests cover sharing, cancellation, cache history, limits, fingerprint
  results and deadlines.
- Rust validation passed 494 unit tests and 32 integration tests, including all
  seven Project check lifecycle tests. Three real-CLI tests remain ignored by
  the existing suite.
- Full Rust validation encounters two unchanged host-dependent failures:
  `agents::tests::detects_home_roots_in_catalog_order` and
  `one_hundred_skill_fixture_meets_the_inventory_budget`. The vendored catalog
  detects the installed `/Applications/ZCode.app` in addition to each test's
  temporary-home agents. The first also fails when run alone. The remaining
  suite passes when only these two tests are excluded; Agent detection code,
  catalog and fixtures were not changed.
- `cargo fmt --check` and `git diff --check` pass.

## Fog

- No unresolved implementation or design questions.
