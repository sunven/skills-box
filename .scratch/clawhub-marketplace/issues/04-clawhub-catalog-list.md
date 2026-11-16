# 04: ClawHub catalog list end-to-end

**What to build:** ClawHub appears as the third source in the skill-market switcher, and the user can browse its catalog: three documented sorts, cursor-based "load more", owner-qualified cards, ClawHub metrics and official badge. Source choice and per-source session position persist. Failures keep the last successful list with a stale label, error detail, retry and official link, and never switch sources silently. No fake totals or page counts are shown. The list's composition with respect to skills.sh aliases is verified and handled as specified.

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] Selecting ClawHub and restarting restores that source; list position and sort restore within the session; other sources keep their own state.
- [ ] The three documented sorts return upstream order; "load more" appends the next cursor page without duplicates or reordering.
- [ ] Cards show name, `@owner/slug`, description when present, ClawHub metrics and the official flag; missing fields are omitted or marked as not provided.
- [ ] List failure retains the last successful result with its old sort/search context, shows the error, retry and canonical ClawHub link, and does not switch sources.
- [ ] 429 responses show a rate-limit state with retry/backoff rather than a generic empty result.
- [ ] The implementation records whether the list endpoint includes skills.sh aliases; when they cannot be identified there, the detail-time redirect rule applies.
- [ ] Malformed or oversized responses and unexpected fields are rejected without crashing; tests cover parsing, pagination, empty and failure states.
- [ ] The fake backend and UI tests cover switching, persistence and failure retention.
