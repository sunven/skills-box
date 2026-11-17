# 12: Archive ClawHub install preview, moderation block and native publication

**What to build:** From a native archive ClawHub detail, the user installs the resolved archive release into the current Global or Project Scope and attaches it to selected Agents. Preview resolves the authoritative latest archive version, blocks skills.sh aliases and suspicious skills, validates the scope, Agents and conflicts, and shows the coordinate, reference, scope, Agents, canonical link and content caveat. Start re-resolves the same reference, stages through ticket 11, revalidates, and publishes natively through the existing source-neutral pipeline. No `clawhub` CLI is detected, required or invoked.

**Blocked by:** 11

**Status:** resolved

- [ ] A native archive ClawHub detail offers installation; skills.sh alias entries keep the redirect and offer none; GitHub-backed entries state that installation is not yet supported until ticket 13 ships.
- [ ] Preview verifies the coordinate is ClawHub-native (not an alias), resolves the latest archive version from `latestVersion` with the newest `/versions` item as fallback, requires `SKILL.md` in that version's file list, and reads moderation data only for installation, never for browsing or detail display.
- [ ] An explicit skill-level `moderation.isSuspicious` or `isMalwareBlocked`, or a version-level `security.status` of suspicious/malware, blocks installation with the upstream status and canonical link; a moderation read failure surfaces an error with retry; missing fields are treated as not flagged; there is no force path.
- [ ] Preview applies the existing scope, selected-Agent, same-name conflict, Stale lock entry and other-scope rules, and shows `@owner/slug`, the resolved version reference, scope, Agents, the content caveat and the canonical link.
- [ ] Start re-resolves the archive reference and requires exact equality with the preview; any change requires a new preview. It re-checks moderation and the target/Agent set immediately before writing.
- [ ] Staging failure leaves the existing installation untouched. Publication keeps the existing backups, canonical replacement, record-write race checks, baseline, Agent attachments and post-publication verification.
- [ ] The Market record stores the semver reference with no diagnostics; no Git lock entry or Discovery source is created.
- [ ] The confirmation and result UI reuse the marketplace mutation surface, show no CLI dependency for ClawHub, and cover success, upstream error, moderation block, conflict and retry with fake-backend and App tests.
- [ ] Isolated Global and Project end-to-end tests cover installation, attachments to shared and separate Agent directories, conflicts, other-scope independence and failure recovery.

## Comments

- 2026-11-17: Drafted with ADR-0035. Installation uses the native adapter from ticket 11 and the existing source-neutral publication contract; the CLI status surface does not apply to ClawHub.
- 2026-11-17: Resolved. Added the ClawHub archive installation plan and native action in `tasks/marketplace/clawhub.rs`, manager prepare/start in `tasks/manager/clawhub.rs`, the `prepare_clawhub_install`/`start_clawhub_install` commands, and the install-time moderation gate in `marketplace/clawhub/moderation.rs`. Preview resolves the latest archive release, requires `SKILL.md`, reads the original name, blocks aliases/GitHub-backed entries, checks moderation, and validates scope/Agents/conflicts. Start re-resolves the exact version, re-checks moderation, stages and verifies through ticket 11, requires the same original name, then publishes with backups, records, baselines and Agent attachments. The detail page now offers install for archive entries and a ClawHub install dialog reuses the shared mutation surface with no CLI dependency; alias and GitHub-backed entries keep explicit no-install states. Tests cover native publication into Global and Project scopes, attachments, scope independence, unmanaged name conflicts, record shape, moderation blocking, and the full UI install flow; fake-backend snapshot updates use `sourceType: "clawhub"`. Full Rust suite: 799 passed, 0 failed; frontend: 518 passed; `tsc --noEmit` passed.
