# 10: Compliance, failure isolation, and final acceptance

**What to build:** End-to-end verification that ClawHub integration meets its compliance and resilience boundaries and does not regress existing sources. This includes caching, rate-limit handling, canonical backlinks, no implied endorsement, stale-result behavior, an isolated real-CLI install/update for archive and GitHub-backed skills, and regressions for SkillHub, skills.sh and Git flows. Results and exceptions are recorded in the implementation notes.

**Blocked by:** 04, 05, 06, 07, 08, 09

**Status:** resolved

- [ ] Caching, 429/`Retry-After`, read/download limits, canonical backlinks and the no-endorsement rule are verified.
- [ ] List, search and detail preserve the last successful state with stale labeling and never leak another source's results or errors.
- [ ] Archive and GitHub-backed skills complete real-CLI install and update in an isolated HOME and registered Project; the real user environment is untouched.
- [ ] SkillHub browsing/version install/update, skills.sh Git browse/install/update and Git direct install/discovery regressions pass.
- [ ] Specification acceptance criteria 1–13 are verified or have a recorded exception.
- [ ] Implementation notes record the environment, versions, pass/fail evidence and any deviations.

## Comments

- 2026-11-16: Resolved on `integration/clawhub-marketplace`. Compliance/acceptance recorded in [implementation.md](../implementation.md): anonymous read-only access, session-only cache obeying `Cache-Control`, no catalog/content in App data, 429/`Retry-After` backoff, canonical backlinks, no endorsement, stale-result isolation, no scan display, and direct requests (`.no_proxy()`). Acceptance 1–5, 10, 12 PASS; 6–9 and 11 are EXCEPTIONs caused by the ticket 02 NO-GO (installation/update execution paused); 13 is an EXCEPTION because the isolated CLI gate did not run against a registered Project. Live ClawHub list/search/detail passed 3/3 during acceptance; the later review recheck hit upstream slowness and is recorded as such. Docs `f23a72d`/`901931a`, merge `a569555`; review fixes `b485f38`.
