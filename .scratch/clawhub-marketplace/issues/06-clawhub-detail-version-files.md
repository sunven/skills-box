# 06: ClawHub detail with version anchoring and file preview

**What to build:** The user can open a ClawHub skill detail and inspect one version at a time: owner-qualified identity, canonical ClawHub link, description, ClawHub metrics, categories, license, version list and changelog, plus the version's `SKILL.md` and file tree. Version switching updates all content atomically. skills.sh alias entries redirect to the skills.sh source and are not installable here. No scan field is shown.

**Blocked by:** 04

**Status:** resolved

- [ ] Detail defaults to the latest version and can switch to historical versions; markdown and file tree always belong to the same selected version.
- [ ] Markdown and file preview follow the existing read-only rendering and size limits; oversized files show metadata only.
- [ ] Missing detail, missing version and missing files show explicit states with retry and canonical link; content is never presented as guaranteed-identical to installation.
- [ ] Alias entries are identified and redirect to skills.sh instead of offering ClawHub installation.
- [ ] The detail exposes the canonical ClawHub listing backlink.
- [ ] Scan information is not displayed.
- [ ] Tests cover version switching, mixed-version prevention, preview limits and alias redirect.

## Comments

- 2026-11-16: Resolved on `integration/clawhub-marketplace`. Detail defaults to latest with historical switching; license/changelog, markdown and file tree follow the selected version; read-only preview limits, missing/retry/canonical states, explicit no-SKILL.md/no-files states, skills.sh alias redirect, canonical backlink and no scan display are covered. GitHub-backed entries expose the resolver's `repo/path/commit` as a diagnostic reference while installation remains paused. Feature `c4855ae`, merge `e5b8bfc`; 10 detail UI tests; full suites green. Code-review fixes in `1d61bcf`, merge `b485f38`.
