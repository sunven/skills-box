# 09: ClawHub update checks and updates

**What to build:** Update checks route each marketplace record to its source and compare the reference recorded at installation: semver for archive entries, commit for GitHub-backed entries. A changed reference is an update candidate; the same reference never triggers an update even if content changed. Update execution reuses the same CLI adapter and publication pipeline. ClawHub outages produce error/unknown states without fake candidates, and local attach/detach/uninstall/clean-record operations continue to work.

**Blocked by:** 07, 08

**Status:** wontfix

- [ ] Archive version changes produce update candidates; the same version reports current.
- [ ] GitHub-backed commit changes produce update candidates; the same commit reports current.
- [ ] Same-reference content changes do not produce an update candidate.
- [ ] Update execution uses the ClawHub adapter, records the new reference and refreshes the baseline.
- [ ] ClawHub failure or rate limiting yields an explicit error/unknown state, not a fabricated update.
- [ ] Local management operations remain available without the ClawHub source.
- [ ] The update UI shows the ClawHub source and its recorded reference.
- [ ] Tests cover comparison, execution, failure and source dispatch.

## Comments

- 2026-11-16: Closed without implementation because update execution depends on the ticket 07/08 installation channel, which the ticket 02 hard gate stopped. Source-aware update-check dispatch exists in the identity model (ticket 01): a ClawHub record is never queried as SkillHub and reports an explicit unknown/uncheckable state, and local management remains available. There are no executable ClawHub updates until a native adapter is approved. Evidence: [02-clawhub-cli-feasibility.md](../research/02-clawhub-cli-feasibility.md).
