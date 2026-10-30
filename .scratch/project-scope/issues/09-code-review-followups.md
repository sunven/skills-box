# 09: Code review follow-up refactors

**Parent:** [spec.md](../spec.md)

**What to build:** Behavior-preserving consolidation of the duplication identified in the post-implementation review. These are not spec gaps; the project-scope feature is complete without them, but each one makes the next scope-related change safer.

**Blocked by:** None

**Status:** needs-triage

- [ ] Centralize project scope-layout resolution (`ScopeLayout::resolve`) so `ReadModel`, `commands::check_updates` and `update/project.rs` stop re-deriving root/lock paths.
- [ ] Unify `inventory::CatalogLock` and `tasks/install.rs::ScopeLock` into one lock view.
- [ ] Consolidate the three containment implementations (`attachment_target`, `scope_layout`, inventory scans) behind one path guard.
- [ ] Remove `InstallPlan`/`UpdateItem` cwd/global duplication; derive both from the scope layout so `cwd` and `global` cannot disagree.
- [ ] Reuse one agent-validation path between the global and project target sets.
- [ ] Note: non-ASCII relative paths are intentionally reported unsupported by the content-hash checker rather than risking a false update candidate; see ADR 0012.
