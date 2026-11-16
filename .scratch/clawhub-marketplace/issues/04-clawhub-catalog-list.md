# 04: ClawHub catalog list end-to-end

**What to build:** ClawHub appears as the third source in the skill-market switcher, and the user can browse its catalog: three documented sorts, cursor-based "load more", owner-qualified cards, ClawHub metrics and official badge. Source choice and per-source session position persist. Failures keep the last successful list with a stale label, error detail, retry and official link, and never switch sources silently. No fake totals or page counts are shown. The list's composition with respect to skills.sh aliases is verified and handled as specified.

**Blocked by:** 01

**Status:** resolved

- [ ] Selecting ClawHub and restarting restores that source; list position and sort restore within the session; other sources keep their own state.
- [ ] The three documented sorts return upstream order; "load more" appends the next cursor page without duplicates or reordering.
- [ ] Cards show name, `@owner/slug`, description when present, ClawHub metrics and the official flag; missing fields are omitted or marked as not provided.
- [ ] List failure retains the last successful result with its old sort/search context, shows the error, retry and canonical ClawHub link, and does not switch sources.
- [ ] 429 responses show a rate-limit state with retry/backoff rather than a generic empty result.
- [ ] The implementation records whether the list endpoint includes skills.sh aliases; when they cannot be identified there, the detail-time redirect rule applies.
- [ ] Malformed or oversized responses and unexpected fields are rejected without crashing; tests cover parsing, pagination, empty and failure states.
- [ ] The fake backend and UI tests cover switching, persistence and failure retention.

## Comments

- 2026-11-16: Resolved on `integration/clawhub-marketplace`. ClawHub is the third source with the three documented sorts, cursor-only "load more", owner-qualified cards, ClawHub metrics/official flag with missing fields marked, persisted source choice and per-source session position, stale retention with old sort context, distinct 429/backoff and canonical official link; no fake totals. The list endpoint cannot identify skills.sh aliases (documented in the module; detail-time redirect covers it). Feature `d9c8ccd`, merge `2270690`; live list contract passed; full suites green.
