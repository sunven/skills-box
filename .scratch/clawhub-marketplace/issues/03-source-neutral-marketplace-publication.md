# 03: Source-neutral marketplace publication

**What to build:** The shared local publication pipeline accepts a source-neutral marketplace installation plan, so ClawHub can reuse backups, canonical replacement, market-record writes, baselines, Agent attachments and post-publication verification without duplicating them. SkillHub installs and updates remain behaviorally identical, including failure, rollback and attachment semantics. This is a prefactor with no user-visible change.

**Blocked by:** 01

**Status:** resolved

- [ ] SkillHub install and update flows pass their existing tests with unchanged user-visible messages and outcomes.
- [ ] The publication contract no longer depends on SkillHub-specific plan fields; a second marketplace source can publish through the same contract.
- [ ] Backup creation, record-write race checks, baselines, attachments and post-publication verification remain enforced.
- [ ] Failure and rollback behavior (prepared candidate, previous-install rename, no-overwrite rename) is unchanged.

## Comments

- 2026-11-16: Resolved on `integration/clawhub-marketplace`. Publication now consumes a source-neutral `MarketplaceInstallation` plan contract (scope, name, targets, previous record/paths, revalidation, record construction) implemented by the existing SkillHub plan. Backups, canonical replacement, record-write race checks, baselines, attachments and post-publication verification are unchanged; SkillHub messages/outcomes and rollback semantics are covered by existing tests plus new fake-plan, race and rollback tests. Feature `c897336`, merge `ca2eeda`; full Rust suite passed.
