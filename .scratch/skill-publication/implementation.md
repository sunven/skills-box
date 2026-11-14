# Skill publication implementation

Date: 2026-11-14.
Status: complete; validation exceptions below.

## Result

`src-tauri/src/tasks/publication.rs` owns the local publication sequence for
Marketplace Install/Update, downloaded Pack members and Pack workflows. A
concrete `Publication` enum carries the two source-specific plans; the module
performs same-volume staging, existing write-time checks, Canonical store
publication, source record persistence, one verified baseline attempt,
attachment replacement/creation and the existing completion verification.

`Outcome` carries the terminal result and accumulated warnings independently.
Marketplace Install, Update and Expert pack task results now preserve a baseline
warning even when a later attachment step fails. Baseline writes precede both
removal of previous Copy attachments and creation of new attachments, and their
failure allows subsequent work to continue.

Marketplace's duplicate baseline write in Update is removed; Git Update keeps
its existing baseline recording. Pack member and workflow reuse share the
attachment implementation while retaining their content, source records and
baselines. The old duplicated publication and attachment loops were removed.

The interface uses the existing plan checks and strict attachment gate. Backup,
Scope ownership, staging-download verification, Pack readiness/order,
cancellation and task aggregation stay with their established owners. There
are no new dependencies or public task/IPC types.

## Verification

- Before refactoring: all 7 Marketplace and 16 Expert pack task tests passed.
- Added caller-level failure matrices cover Marketplace Install/Update and Pack
  member/workflow publication in both Global and Project Scope: baseline write
  failure, attachment failure, both failures, and successful publication. They
  assert task warnings/errors, source records, baseline contents, retained
  canonical content and attachments, and actual successful/failed snapshot
  cleanup with Backup retention disabled. Marketplace Update's baseline
  warning appears once, while its existing Copy-to-symlink warning survives.
- Source record write failures are exercised for both source kinds and both
  Scopes. Already-published files remain; baseline and attachments are not
  written. The workflow case also checks the failed task row and backup.
- Expanded Pack supplement coverage verifies missing member and workflow
  attachments are recreated without rewriting baseline or source-record bytes.
- Existing Git Update baseline, path/conflict, staging, cancellation, Scope
  lifetime, backup and Pack partial-success tests continue to pass.
- `cargo fmt --check` and `git diff --check` passed.

Across the Rust suite, **774 distinct tests passed**, **2 existing tests failed**
because this host has `/Applications/ZCode.app`, and **13 tests remained
ignored** under their existing opt-in policies. No real-network/real-CLI
ignored test was enabled.

`cargo test` first stopped at
`agents::tests::detects_home_roots_in_catalog_order`: the fixture expects four
Agents, while the existing catalog also detects the host's ZCode application.
An isolated exact rerun reproduced the failure. Both the test and catalog are
unchanged from HEAD.

Running with that test excluded passed all other **741 unit tests** and exposed
the same fixture assumption in
`performance_fixture::one_hundred_skill_fixture_meets_the_inventory_budget`
(five detected Agents rather than four). The Project performance test passed.
This failure happens before its performance assertions. Its test and detection
implementation were not changed.

The remaining integration binaries were then run explicitly to avoid those
unrelated failures preventing coverage:

- `extra_agents_fixture`: 2 passed.
- `project_read_model`: 12 passed.
- `security_audit`: 10 passed.
- `project_check_lifecycle`: 7 passed.
- `sandbox_real_cli`: 1 passed, 3 ignored.
- `generic_git_sources`, `skillhub_marketplace_real_cli` and
  `stale_restore_real_cli`: their 4 network/real-CLI tests remained ignored.
- Doc tests: no tests defined, command passed.

Permission-failure tests use real temporary directories and restore permissions
before assertions. As with existing filesystem tests, they skip permission
cases if a privileged process can bypass the restriction; this run used an
unprivileged macOS process.
