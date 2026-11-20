# Skill publication: common local facts and validation

Date: 2026-11-20.
Status: implemented and verified.

## Problem and intended result

SkillHub and ClawHub each retain the same confirmed local facts and implement
nearly identical publication checks in `tasks/marketplace.rs:87` and
`tasks/marketplace/clawhub.rs:67`. The existing publication module calls their
`MarketplaceInstallation::check` implementation before publication and again
after preparing the candidate, but delegates the common local rules back to
the source adapters.

Deepen the existing Skill publication module by giving it ownership of the
common confirmed local facts and their revalidation. Source adapters retain
source acquisition and verification. This extends the implemented
[Skill publication design](../skill-publication/spec.md); its publication,
baseline, attachment and partial-failure behavior remains unchanged.

## Confirmed decisions

The user accepted both recommendations on 2026-11-20:

1. **Q1 — Behavior preservation:** change ownership of the common rules while
   retaining error precedence, reconfirmation conditions, cancellation and
   partial-success behavior. Record independently discovered problems
   separately rather than broadening this refactor.
2. **Q2 — Data and behavior together:** the publication module owns the common
   confirmed local facts and their validation. Both source plans retain one
   common publication plan, rather than independently maintaining those facts
   and merely calling a shared validation function.

## Ownership and scope

| Owner | Responsibilities |
| --- | --- |
| Skill publication module | Common local plan: Skill identity, Marketplace coordinate, confirmed Scope layout, selected Agent attachment targets, previous Lock entry and previous physical/content facts; common revalidation; existing local publication sequence and outcome |
| SkillHub adapter | CLI runtime and command, staging directory, confirmed release content, source-specific staging checks, downloaded-content verification, source record construction |
| ClawHub adapter | Confirmed reference, archive or GitHub acquisition, moderation and content verification, staging availability and checks, source diagnostics and record construction |
| Mutation session and task ownership | Existing write permit and worker lifetime, task registration, backup protocol, cancellation, execution ordering, interruption handling and task outcomes |
| Expert pack execution | Pack snapshot, member readiness and reuse, download preparation, workflow rules, per-member continuation/stop decisions and aggregate result |

The common publication plan is an internal implementation detail, not a new
domain term or a public IPC contract. Concrete Rust names and representation
remain implementation choices. Keep the interface small; avoid replacing the
duplicated fields with a larger list of independently coordinated accessors.
The existing SkillHub and ClawHub adapters justify the seam. No generic
filesystem adapter, new dependency, or frontend change is required.

Covered callers are SkillHub and ClawHub Install, single Update and Update all,
and Expert pack missing members that already use a SkillHub download plan.
Plan construction sites, fixtures and affected assertions migrate together.

Existing Expert pack member reuse and Pack workflow checks remain with their
current owner. Reuse only adds missing attachments and must not become an
installation or rewrite content, records or baselines. Git execution retains
its existing protocol. Marketplace Batch Update currently rejects this source
and directs users to single Update or Update all; that behavior is preserved.

## Revalidation timing and error precedence

Rule ownership changes; these existing checkpoints all remain:

1. Preview validates whether the proposed installation or update is eligible.
2. Start revalidates against the Scope resolved under mutation ownership.
3. Preflight checks the target before creating its Backup snapshot.
4. `before_write` checks again after backup and the cancellation seal.
5. Publication resolves the current Scope and validates before preparation.
6. Publication validates again after copying the candidate and before moving
   the old canonical directory.

The source plan's combined check preserves its current order:

- Source-specific staging check.
- Scope mutation readiness and lock readability.
- Duplicate Marketplace coordinate under a different local name.
- Confirmed Scope, Project root and Project revision equality.
- Previous Marketplace record equality.
- For Update, previous physical paths and current content fingerprint;
  otherwise, new-installation name conflicts.
- For a new Install only, selected Agents and strict publication attachment
  eligibility.
- Project canonical path containment and canonical-directory symlink rejection.

SkillHub always has a staging directory; ClawHub preview may not yet have
staged content. Preserve their distinct staging errors. These directory checks
are not a replacement for source-specific content verification.

UpdateAction's existing early Scope readiness check remains before the source
plan check. Update may retain attachments to Agents no longer detected;
new-Install detection requirements must not be applied to Update.

After publishing a new record, previous-record revalidation is no longer
valid. Preserve the existing narrower attachment write guards and final
record/name verification. Publication continues to use same-volume candidate
preparation and no-overwrite replacement. It introduces no rollback or retry.

## Verification and acceptance

The interface is the test surface. Exercise common local checks through the
publication module with real temporary directories and records, rather than
only making a fake check fail on its second call.

Acceptance checks:

1. Both source adapters retain successful Global and Project Install/Update
   behavior, confirmed source identity/version, baselines and attachments.
2. Changed Project revision, old record, local content, physical attachments,
   occupied name or canonical path rejects at the existing checkpoint without
   overwriting the conflicting state. Preserve error precedence when several
   conditions fail at once.
3. A change during candidate preparation is caught before the old canonical
   directory is moved. Keep coverage for a record change after publication
   starts as a distinct case; an earlier rejection must not silently replace
   that race scenario.
4. Source staging/content failures keep their original errors and ordering.
   New-Install Agent detection and Update's retained attachments stay distinct.
5. Record-save failure prevents later baseline and attachment work. Baseline
   failure remains a warning; a later attachment failure retains that warning
   and the existing published state, task outcome and required backups.
6. Expert pack missing members, reused members and reused workflows retain
   their separate behavior, member-readiness requirements, cancellation,
   partial success and fatal-stop policy.
7. Single Update and Update all retain pinned versions and source-runtime
   isolation; Marketplace Batch Update remains unavailable.

Move duplicated common assertions only after equivalent behavior coverage
exists. Retain source-specific verification tests and caller tests that prove
task ownership or result aggregation.

Run focused publication, marketplace, update, manager and expert-pack tests,
then the Rust suite, formatting checks for affected Rust files and
`git diff --check`. Real CLI/network tests keep their existing opt-in policy.
Document any failures and distinguish pre-existing failures from regressions.

## Domain and existing decisions

The existing `Skill publication` glossary definition remains accurate; no new
domain term is needed. ADRs 0003, 0005, 0016, 0028, 0030, 0031 and 0035 continue
to govern ownership, attachment checks, backup retention, source identity and
Expert pack behavior. This design does not reopen those decisions.

Q3 — The user confirmed the complete scope and acceptance plan on 2026-11-20
and authorized implementation.

## Implementation and verification

Completed on 2026-11-20:

- `tasks/publication.rs` owns the shared `Plan` and `check`. Both source plans
  hold that plan; the source interface now exposes only the common plan,
  source staging check and installation record construction (three methods,
  previously seven).
- Migrated Install, Update and Expert pack download constructors and fixtures.
  Existing callers retain their checks at preview, owned start, preflight,
  `before_write` and both publication checkpoints.
- Common filesystem tests cover occupied names, stale/changed records,
  duplicate coordinates, changed content/attachments/physical paths, Relocate
  including returning to the original root, error precedence and Install versus
  Update Agent detection. Source tests retain staging error precedence.
- The preparation-race test now changes real content after the candidate copy
  and proves the old installation is not replaced. The record-race test injects
  the competing record after replacement and verifies that baseline and
  attachment work does not follow the failed record check.
- Replaced the duplicate SkillHub-only name/record/coordinate assertions with
  the common tests in both Global and Project scopes. Retained lifecycle,
  source content verification, baseline/attachment failure and Expert pack
  partial-success/cancellation coverage.

Validation:

- Before refactoring: `cargo test --locked --lib tasks::` — 286 passed,
  2 ignored.
- Final `cargo test --locked` — 887 passed, 0 failed, 31 ignored across the
  library, integration and documentation test targets. This includes all
  publication, marketplace, update, manager and Expert pack tests.
- `rustfmt --check --edition 2024 --config skip_children=true` on changed Rust
  files and `git diff --check` passed.

The 31 ignored tests retain the repository's opt-in policy for real CLI,
native Agent and network integration. No frontend or IPC changes were made;
frontend tests were not rerun during implementation.
