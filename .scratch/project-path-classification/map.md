# Project root physical path classification

## Notes

Architecture candidate 05 from the whole-codebase review. Candidates 01-03
are complete and committed as `030c320`, `33bb4be` and `27cca08`. The user
requested the next candidate.

Candidate 04's main lifecycle ownership was absorbed by candidate 01:
`update-check-lifecycle.ts` owns Scope/revision identity, shared pending state,
deduplication, publication and invalidation. The remaining App scheduling
policy has one production caller and cross-module tests; moving it to another
hook currently offers insufficient leverage. Continue with candidate 05.

## Facts before implementation

- `attachment_target.rs:42` normalizes root/target, checks lexical containment,
  walks upward to the nearest existing ancestor and canonicalizes it before
  accepting a potentially missing attachment target. It maps failures to
  existing Path errors and retains a historical root-resolution fallback.
- `scope_layout.rs:266` validates relative Install target directories;
  `resolves_inside_root` at line 292 independently implements the same
  nearest-ancestor traversal. Resolution failure excludes the target.
- `scope_layout.rs:312` classifies already-existing Agent directories for
  scanning. It canonicalizes a directory only when the final path component
  is itself a symlink; missing directories do not join scanning.
- `inventory/mod.rs:511` resolves existing paths for anomaly classification.
  Its Canonical store and Agent directory guards likewise distinguish final
  symlinks from normal directories, then scan individual entries.
- `read_model.rs:491` rejects an out-of-root Skill path before reading its
  description. `build_view` at line 418 independently passes the primary path
  to BaselineStore.assess; the assessment fingerprints content and may record
  a baseline. Content command root resolution retains its independent strict
  allowlist check.
- ADR 0005 preserves Install linking's relaxed profile and the historical
  error order. ADR 0011 keeps scopes independent. ADRs 0013/0016 keep mutation
  permit and pinned-layout ownership in TaskManager. ADR 0004 retains normal
  baseline write-in-read behavior as a separate decision.

## Recommended scope

- Concentrate normalized/physical path facts and nearest-existing-ancestor
  traversal in one module used by attachment targets, Scope Install targets
  and Project catalog scanning.
- Keep the actual policies in callers: potentially missing mutation targets,
  already-existing scan directories, attachment eligibility, relaxed Install
  linking, error priority and warning/anomaly presentation.
- Avoid a single boolean that erases missing/unreadable/escaping distinctions.
  Use the same fact interface for production callers and tests.
- Use real temporary filesystem roots. The filesystem does not need a new
  trait or a hypothetical adapter; existing process/IPC seams are unaffected.
- Correct two reproduced containment gaps: reject out-of-root directories even
  when the symlink is in a parent, and prevent an out-of-root primary Skill
  path from participating in BaselineStore.assess. Preserve the existing
  content command safeguards and valid-path baseline behavior.

## Investigation evidence

- A temporary harness linked the currently compiled crate and exercised
  ScopeLayout and ReadModel directly. No repository source was changed for
  reproduction; temporary filesystem fixtures were cleaned up.
- `.claude -> outside` with an ordinary `outside/skills` directory: the layout
  accepts `claude-code` for scanning, Install targets exclude it, and snapshot
  displays the external Skill with an escape anomaly and no description.
  Content root resolution returns `path.outside_allowed_root`.
- Adding a Project Lock entry makes the snapshot create a Project baseline
  from the external Skill's files (two files, 87 bytes in the fixture).
  A parent `.agents` symlink and a Canonical Skill leaf symlink also cause an
  out-of-root baseline assessment.
- In contrast, a final Agent directory symlink `.claude/skills -> outside/skills`
  is excluded by the layout, appears as no Skill and creates no baseline.
- Harness executables are `/tmp/skills-box-project-path-facts-20261030` and
  `/tmp/skills-box-project-baseline-facts-20261030`. These reproduce directory
  scanning and baseline reads, not a bypass of the independent content opener
  safeguards.

## Test surface

- `attachment_target.rs::project_target_context_guards_lexical_and_symlink_escapes`
  covers missing targets, lexical escape, root-internal links and root-external
  links through the target context.
- ScopeLayout tests cover eligible missing Install targets, escaping Extra
  agent targets and excluded/warned existing Agent directories.
- `tests/project_read_model.rs` covers Project Canonical store and Agent
  attachments, escaping leaf symlinks, rejected descriptions/content roots,
  unavailable/forgotten Projects and Scope isolation.
- Add cross-caller scenarios for the same physical paths, including a
  symlinked parent with an ordinary final directory, missing descendants,
  broken links and resolution failures. Observe caller results, not duplicated
  helper internals.

## Decisions-so-far

- Implemented `src-tauri/src/project_path.rs` as the shared path-fact module.
  `lexical` returns the normalized candidate; `resolve` checks physical
  containment with Existing/AllowMissing modes; `existing` combines both for
  read paths. Typed failures preserve missing, inspection, resolution and
  lexical/physical escape facts. Only NotFound permits ancestor traversal.
- Root-resolution policy stays with callers: attachment retains its normalized
  root fallback, Install targets require canonicalization, and scan/read paths
  use the already-resolved Project root. Attachment physically checks its
  normalized target; layout/catalog preserve original filesystem semantics.
- ScopeLayout and the catalog independently reject directories whose parent
  symlink escapes. Individual escaping Skill leaves remain anomaly rows.
- Read model primary validation now gates description, baseline assessment and
  directory metadata together. Invalid Project paths report unknown Local
  modification without creating, replacing or erasing a Baseline fingerprint.
  Valid paths retain baseline initialization and assessment; strict content
  allowed-root checks remain independent.
- The cross-caller temporary-filesystem matrix covers ten cases through Attach,
  Detach, Install linking, Install target selection, ScopeLayout and Read model:
  missing directories, existing/missing internal and external linked parents,
  broken parents, non-directory parents and internal/external/broken leaf links.
  Separate regressions cover Canonical store parent escape, directly injected
  Agent paths, Managed/Copy-only escaping leaves, absent/seeded baselines, valid
  controls and independent Global/other-Project baseline records.
- Recorded [ADR 0018](../../docs/adr/0018-project-physical-path-facts.md) and
  resolved the containment item in the
  [Project scope follow-ups](../project-scope/issues/09-code-review-followups.md).

## Validation

- `cargo test --quiet --test project_read_model`: 11 passed.
- `cargo test --quiet`: 514 passed; 3 existing real CLI/network tests ignored.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- No frontend/IPC changes; no frontend checks required for this backend scope.

## Fog

The user accepted the recommended scope with “按推荐”. Implementation and
validation are complete; no design question remains. The user requested a
commit; this record accompanies the candidate 05 implementation commit.
