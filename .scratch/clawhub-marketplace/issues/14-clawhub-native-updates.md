# 14: ClawHub native update checks and update execution

**What to build:** ClawHub records leave the paused/uncheckable placeholder and get real update checks and updates by their recorded reference. Archive entries compare the recorded semver against the current latest archive version; GitHub-backed entries compare the recorded commit against a fresh resolver result when ticket 13 ships. Update execution reuses the native adapter and publication pipeline, stores the new reference and refreshes the baseline, while ClawHub outages and rate limits never fabricate candidates and never affect local management.

**Blocked by:** 12 (13 for the GitHub-backed rows)

**Status:** resolved

- [ ] Each ClawHub record dispatches to the ClawHub source by its recorded source and never queries the SkillHub provider.
- [ ] Archive entries: fetch the current latest archive version and compare it with the recorded reference; equal is current, different is an update candidate, failure is an explicit error, never a fabricated candidate.
- [ ] GitHub-backed entries (after ticket 13): resolve the commit again and compare it with the recorded commit; equal is current, different is an update candidate.
- [ ] ClawHub checks use their own source budget, concurrency, deadline and 429/`Retry-After` state and do not consume the SkillHub source budget.
- [ ] Update execution re-resolves the target reference, stages and verifies through the native adapter, publishes through the existing pipeline, updates the record and baseline, preserves `installedAt` and advances `updatedAt`; a failure leaves the existing installation and record intact.
- [ ] The update UI shows the ClawHub source and the recorded/new reference, distinguishes current, update, error, rate-limited and not-checked states, and keeps Attach, Detach, Uninstall and stale-record cleanup available without network.
- [ ] Tests cover reference comparison, update execution, failure retention, rate limiting, source isolation and the absence of fake candidates.
- [ ] Same-reference content changes do not create candidates, matching the confirmed update semantics.

## Comments

- 2026-11-17: Drafted with ADR-0035. Ticket 09 was closed because there was no installation channel; this ticket replaces its execution scope for the native adapter.
- 2026-11-17: Tickets 12 and 13 are resolved, so this ticket is unblocked. Archive records compare the latest archive version; GitHub-backed records compare a freshly resolved commit. Execution must stage before the update session and reuse the native publication path.
- 2026-11-17: Resolved. Update checks now dispatch by source: SkillHub reads its platform version, ClawHub resolves the current archive semver or GitHub commit through the anonymous resolver. Each source has its own budget index and rows keep an explicit error/not-checked state. Update preview builds a ClawHub plan per record (new reference, `SKILL.md` name check, moderation, previous record/paths/fingerprint), pinned updates revalidate the preview, stage the exact new reference before the session, and `UpdateAction` publishes it natively via `execute_native` with `uses_native` preventing any CLI fallback for mixed batches. Command previews show a no-CLI reason and the ClawHub reference label. Tests cover reference comparison (current/update/error), source budget isolation, update publication replacing the canonical folder and record while preserving the attachment and `installedAt`; full Rust suite: 802 passed, 0 failed; frontend: 518 passed; `tsc --noEmit` passed. Acceptance remains in ticket 15.
