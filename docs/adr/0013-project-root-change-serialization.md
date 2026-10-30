# Project root changes share the mutation permit

Status: accepted

Mutations resolved their `ScopeLayout` before acquiring the single-writer permit, and `ensure_project_registry_idle` refused Relocate/Forget only while a task was registered in the task map. Native attach/detach hold the permit without registering, and install planned before taking it, so a Project root could change under a layout that was already pinned to it. We make the mutation permit the one serialization seam for Project root changes: Relocate and Forget acquire it through `TaskManager::begin_registry_change` and hold the guard across the whole registry call, and every mutation (install, update, uninstall, attach, detach) acquires the permit before resolving its `ScopeLayout`. Add, active-scope switching, reads and project update checks stay outside the slot. The lock order is always permit, then the Project registry mutex. Refusals keep `task.busy`; the detail names the running task when one is registered and falls back to the generic message for native actions.

## Considered options

- **A registry-level root pin held by `ScopeLayout`**: rejected because layouts are cloned into blocking tasks, so it would need owned guards or generation checks, and every read path would pay for a guarantee only writers need.
- **Registering native actions in the task map**: rejected because it conflates task events with native actions and still leaves the pre-permit layout window.
- **Guarding Add and active-scope switching**: rejected because Add's duplicate check and insert already run atomically under the registry mutex, and switching the active scope changes no root; both would queue behind long mutations for no safety gain.
- **Holding the slot across project update checks**: rejected because a GitHub-bound check would block every mutation for its duration. Checks revalidate the Project check revision before publishing through the lifecycle module described in [ADR 0015](0015-project-update-check-lifecycle.md).

## Consequences

- A Project root cannot move while any mutation holds a layout pinned to it, including native attach/detach and the install planning window.
- An invalid install request submitted during another mutation now reports `task.busy` before its own validation error.
- Reads and previews may still observe a root disappearing; they fail as a Scope error on the next request.
- The rule "layout after permit" has no observable regression test; it is held by comments on `try_acquire_mutation` and `scope_layout` plus review.
