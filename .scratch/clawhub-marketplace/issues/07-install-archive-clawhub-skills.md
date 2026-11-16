# 07: Install archive-hosted ClawHub skills end-to-end

**What to build:** From ClawHub detail, the user installs one archive-hosted skill into the current Global or Project scope. The flow detects the optional CLI, shows a confirmation with `owner/slug`, resolved version, scope, agents, command and content caveat, stages the install in isolation with an explicit version and no force, independently verifies the staged content, then publishes the canonical folder, attachments, baseline and market record. Missing CLI, suspicious-skill blocks, conflicts and other-scope independence follow the confirmed rules.

**Blocked by:** 02, 03, 06

**Status:** wontfix

- [ ] Confirmation shows the true owner-qualified coordinate, resolved version, target scope, selected agents, actual command and the content-difference caveat.
- [ ] Installation pins the confirmed version, uses an isolated staging directory and never passes `--force`/`--force-install`.
- [ ] The app independently verifies the staged skill's folder and `SKILL.md` before publication instead of trusting CLI text output.
- [ ] Publication creates the canonical folder, Agent attachments, baseline and a ClawHub market record with the resolved reference.
- [ ] A missing CLI blocks only ClawHub install/update, with a recheck path; browsing and other sources are unaffected.
- [ ] A suspicious skill is blocked with the upstream error and canonical link; no partial overwrite occurs.
- [ ] Same-name conflicts, Stale lock entries and installations in other scopes follow the existing marketplace rules.
- [ ] Isolated Global and Project end-to-end tests pass; the fake backend and UI tests cover confirmation, failure and retry.
- [ ] No Git lock entry or Discovery source is created.

## Comments

- 2026-11-16: Closed without implementation by the ticket 02 hard gate. The official `clawhub` CLI installs an explicitly requested archive version correctly, but it cannot pin GitHub-backed entries to a caller-supplied commit (`--version <commit>` → `Version not found`). Per spec decision 20 and ADR-0034 the whole CLI installation channel is paused rather than silently narrowed to archive-only. No install code or CLI adapter was written. A native HTTP+ZIP adapter requires a new ADR and separate approval. Evidence: [02-clawhub-cli-feasibility.md](../research/02-clawhub-cli-feasibility.md).
