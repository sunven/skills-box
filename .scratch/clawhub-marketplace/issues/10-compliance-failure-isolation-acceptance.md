# 10: Compliance, failure isolation, and final acceptance

**What to build:** End-to-end verification that ClawHub integration meets its compliance and resilience boundaries and does not regress existing sources. This includes caching, rate-limit handling, canonical backlinks, no implied endorsement, stale-result behavior, an isolated real-CLI install/update for archive and GitHub-backed skills, and regressions for SkillHub, skills.sh and Git flows. Results and exceptions are recorded in the implementation notes.

**Blocked by:** 04, 05, 06, 07, 08, 09

**Status:** ready-for-agent

- [ ] Caching, 429/`Retry-After`, read/download limits, canonical backlinks and the no-endorsement rule are verified.
- [ ] List, search and detail preserve the last successful state with stale labeling and never leak another source's results or errors.
- [ ] Archive and GitHub-backed skills complete real-CLI install and update in an isolated HOME and registered Project; the real user environment is untouched.
- [ ] SkillHub browsing/version install/update, skills.sh Git browse/install/update and Git direct install/discovery regressions pass.
- [ ] Specification acceptance criteria 1–13 are verified or have a recorded exception.
- [ ] Implementation notes record the environment, versions, pass/fail evidence and any deviations.
