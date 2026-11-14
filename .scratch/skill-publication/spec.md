# Skill publication

Status: implemented and verified; two pre-existing environment-dependent test failures recorded in implementation.md.
Date: 2026-11-14.
Implementation: complete; see [implementation record](implementation.md).

## Problem and intended result

Marketplace Install/Update and Pack workflow each own local publication rules:
Canonical store writes, source-specific Lock entries, Baseline fingerprints and
Agent attachments. A recent attachment correction changed both implementations.
Marketplace publication records its baseline after attachments, while a new
Pack workflow records it before attachments and treats a write warning as a
terminal failure. Marketplace Update also records the baseline a second time.

Concentrate these ordering rules in one internal Skill publication module.
Its interface should let callers request a local publication and receive its
actual result and warnings without assembling the publication steps themselves.

## Confirmed decisions

The user accepted both recommendations with “都同意” on 2026-11-14:

1. **Q1 — Baseline timing:** publish prepared Skill content, save its Source
   record, record the Baseline fingerprint, then create required Agent
   attachments. A later attachment failure retains the baseline for content
   already published.
2. **Q2 — Baseline failure:** a baseline write failure contributes a warning
   and does not stop attachment creation. This applies to Marketplace
   Install/Update and new Pack workflow publication.

Accepted consequences of Q2:

- Inventory may report Local modification as Unknown when the baseline is
  missing, or Modified when an Update leaves an older baseline. The read model
  currently does not use these source records' SHA-256 hashes as a fallback.
- A baseline warning alone does not make the row fail. If publication otherwise
  succeeds, existing Backup retention settings apply, including cleanup when
  retention is disabled. Actual failures retain their required backups.

## Confirmed ownership

| Owner | Responsibilities |
| --- | --- |
| Skill publication module | Same-volume preparation and Canonical store publication; source-specific record persistence and checks; one baseline attempt before attachments; replacement of prior Copy attachments where Update requires it; required attachments through the existing strict target gate; publication verification; accumulated warnings and terminal result |
| Marketplace execution | CLI command and downloaded-content validation; confirmed coordinate/version; existing plan checks, task progress, logs and result projection |
| Expert pack execution | Pack snapshot, preparation of downloads and workflow content, member order and reuse, readiness for workflow publication, continuation/stop decisions and overall Pack result |
| Mutation session and task ownership | Scope write ownership, backup creation/registration, cancellation, worker lifetime, interruption markers and task completion |

The shared implementation covers Marketplace Install, Marketplace Update,
downloaded Pack members and newly published Pack workflows. Existing Pack
members and workflows that only need attachments use the same attachment
implementation without replacing content, source records or baselines.

Git mutations and standalone Attach/Detach keep their existing protocols. The
Marketplace branch of Update stops recording the baseline a second time; Git
Update retains its existing baseline step.

## Interface contract

Place the internal module under `src-tauri/src/tasks/publication.rs`. Callers
supply the confirmed source-specific inputs and prepared content; the module
owns the local publication sequence. Keep the real differences between a
Marketplace installation record and a Pack workflow record explicit. Concrete
Rust type names are implementation choices, with no public IPC changes.

The outcome carries both `Result<(), AppError>` and collected warnings. Once
baseline failure becomes a warning before attachments, a later attachment or
verification error must not discard that warning. The terminal error still
determines the failed row and existing continuation policy. This is a direct
consequence of Q2's requirement to retain the warning while continuing work.

Validation runs at the existing execution points, including after backup and
immediately before writes. Preserve source-specific plan checks, Project
identity/revision checks, staged-content verification and strict attachment
eligibility. A successful outcome still requires the existing final checks.
Source record persistence failure remains terminal and stops later publication
steps; only baseline persistence failure receives warning treatment.

Publication uses the callers' existing blocking-worker lifetime and Scope
ownership. It neither starts an independent task nor creates or releases the
caller's Backup snapshot. Failures report actual retained content and earlier
attachments; no rollback or automatic retry is introduced.

## Seam, depth and verification

The dependencies are local filesystem operations, source record stores,
BaselineStore and the existing attachment gate. Real temporary directories
exercise them through the publication interface. Existing ProcessExecutor
adapters continue to cover download execution; a new generic filesystem or
source adapter is unnecessary for this change.

Depth comes from owning the complete local sequence. Locality means a change
to required attachments or baseline policy has one implementation. Leverage
comes from sharing that sequence across the existing callers and testing its
observable filesystem state and outcomes through one seam.

Acceptance checks for implementation:

1. Successful Marketplace Install/Update and new Pack workflow publication
   produce their correct source records, verified baselines and attachments
   in both Global and Project Scope.
2. Force attachment failure after content and record publication; verify the
   baseline already exists, the row fails, earlier successful attachments and
   published content remain, and the required backup is retained.
3. Force baseline persistence failure; attachments still complete and the
   successful outcome carries a warning. Exercise this through Marketplace and
   Pack workflow callers, including their final task results.
4. Combine baseline failure with a later attachment failure; retain both the
   warning and terminal error through the public task result. Verify existing
   failure/partial-result and Backup retention rules.
5. Update records its Marketplace baseline once, and Git Update still records
   its baseline. Verify no lost or duplicate warnings.
6. Pack reuse only adds missing attachments and preserves content, source
   records and baselines. Keep existing member-readiness, cancellation,
   retained-success, snapshot and fatal-stop coverage.
7. Retain staged-content, conflict, changed-plan, path/attachment and Scope
   isolation checks. Move redundant local publication assertions to the new
   interface only once equivalent coverage exists; retain caller integration
   tests that verify result aggregation or task ownership.

Run focused Rust publication/caller tests, the Rust suite and relevant
formatting checks, then `git diff --check`. Real CLI/network tests keep their
existing opt-in policy. Results are recorded in [implementation.md](implementation.md).

## Domain and existing decisions

`Skill publication` is defined in `GLOSSARY.md`. ADRs 0003, 0005, 0016,
0028 and 0030–0032 continue to constrain task ownership, attachment guards,
backup retention, source identity, Pack snapshots and partial outcomes. Q1–Q2
specify baseline behavior that those ADRs do not prescribe.

The user confirmed Q3, including the complete ownership and verification plan,
and authorized implementation with “确认” on 2026-11-14.
