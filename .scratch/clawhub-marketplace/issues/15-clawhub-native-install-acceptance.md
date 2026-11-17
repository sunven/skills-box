# 15: Native ClawHub install acceptance, compliance and regression

**What to build:** End-to-end acceptance for the native ClawHub adapter in isolated environments, covering install, update, moderation, rate limits, conflicts, compliance and regressions. This ticket produces the implementation record and the final EXCEPTION list; it does not add new product behavior beyond fixes required to pass.

**Blocked by:** 12, 14 (13 when GitHub-backed installation ships)

**Status:** resolved

- [ ] A real archive install and update run in an isolated HOME with a temporary Global scope and a temporary registered Project; verify the canonical folder, Agent attachments, Market record, baseline and operation log, and confirm no Git lock entry or Discovery source is created.
- [ ] GitHub-backed end-to-end runs only when ticket 13 passes its gate; otherwise the acceptance record states archive-only support and the blocked class.
- [ ] Compliance: anonymous GET only, no account, token, cookie or authorization flow, direct connection with host proxies disabled, catalog cache in process memory only, package bytes kept only in temporary staging, canonical backlinks, and app-side backoff for 429/`Retry-After`.
- [ ] Security: suspicious/malware entries are blocked with the canonical link, ZIP hardening cases from ticket 11 fail safely, oversized or truncated downloads leave no partial publication, alias entries never offer installation, and a failed staging leaves the previous installation untouched.
- [ ] Same-name conflicts, Unmanaged and Copy-only skills, Stale lock entries and other-scope independence follow the existing Marketplace rules.
- [ ] Regression suites pass for SkillHub install/update and expert packs, skills.sh, Git installation and Source discovery, plus the full Rust and frontend suites, `cargo fmt --check` and `tsc --noEmit`.
- [ ] Update the ClawHub specification, implementation record and ADR-0035 status; record every EXCEPTION with its evidence instead of claiming unverified behavior.
- [ ] No real user environment is modified; live tests remain ignored by default and are run only deliberately against anonymous read endpoints.

## Comments

- 2026-11-17: Drafted with ADR-0035. This replaces acceptance criteria 6–9 and 11 of the paused implementation record; those criteria remain EXCEPTION until this ticket passes.
- 2026-11-17: Resolved with the record in [implementation.md](../implementation.md). Isolated live install+update passed for `@steipete/nano-banana-pro` 1.0.0→1.0.1 in both Global and temporary Project scopes; live archive and GitHub-backed staging/hash tests passed (gate evidence in [03-github-content-hash-gate.md](../research/03-github-content-hash-gate.md)). Full Rust: 802 passed, 0 failed; frontend: 518 passed; `tsc` passed. EXCEPTIONs: full live GitHub-backed publication was not run because the sampled entry is moderation-blocked (download/hash/staging verified live; publication covered deterministically); real 429/`Retry-After` was not triggered; no single automated live Manager/UI run (core plans, native execution and UI flows are covered by unit, isolated-publication and fake-backend tests).
