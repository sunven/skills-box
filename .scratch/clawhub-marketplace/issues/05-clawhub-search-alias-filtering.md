# 05: ClawHub search with skills.sh alias filtering

**What to build:** Search inside the ClawHub source preserves upstream result order, shows the returned-results limit, and filters skills.sh alias entries where `install.kind` is available. Empty, error, stale and retry states stay isolated to ClawHub and never leak results or errors from another source.

**Blocked by:** 04

**Status:** resolved

- [ ] Searching preserves upstream order and applies no local re-ranking or fabricated total.
- [ ] The UI states how many results were returned and that the upstream limit applies; clearing the query returns to the list state.
- [ ] skills.sh aliases are filtered wherever the install kind is identifiable; no per-entry resolver calls are made.
- [ ] Empty, error, last-success and retry states are explicit and source-isolated.
- [ ] Search query and position restore within the session.
- [ ] Tests cover ordering, filtering, limits and failure isolation.

## Comments

- 2026-11-16: Resolved on `integration/clawhub-marketplace`. Search preserves upstream order, reports returned count and upstream limit, filters skills.sh aliases by `install.kind` (with counts; missing/invalid kinds no longer fail the whole search), and keeps empty/error/last-success/retry states isolated to ClawHub. Query and position restore within the session. Feature `7de133e`, merge `878c9d8`; live search/alias contract passed; full suites green.
