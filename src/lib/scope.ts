//! Scope identity helpers shared by App, the workspace projection and tests.
//! Project ids are opaque; the only structure is the `project:` wire prefix.

import type { ProjectEntry, ProjectList, ScopeId } from "./types";
import { GLOBAL_SCOPE } from "./types";

export const projectsQueryKey = ["projects"] as const;

export function projectScope(projectId: string): ScopeId {
  return `project:${projectId}`;
}

export function isProjectScope(
  scope: ScopeId,
): scope is `project:${string}` {
  return scope !== GLOBAL_SCOPE;
}

export function projectIdFromScope(scope: ScopeId): string | null {
  return isProjectScope(scope) ? scope.slice("project:".length) : null;
}

export function projectForScope(
  list: ProjectList | undefined,
  scope: ScopeId,
): ProjectEntry | null {
  const id = projectIdFromScope(scope);
  if (id === null) return null;
  return list?.projects.find((entry) => entry.id === id) ?? null;
}

/** Restore the persisted scope; a missing or unavailable entry falls back to global. */
export function resolveInitialScope(
  list: ProjectList | undefined,
): ScopeId {
  if (!list) return GLOBAL_SCOPE;
  const stored = list.lastActiveScope;
  if (stored === GLOBAL_SCOPE) return GLOBAL_SCOPE;
  const entry = projectForScope(list, stored);
  return entry?.available ? stored : GLOBAL_SCOPE;
}

/** The CLI writes the canonical store under `.agents/skills`. */
export function projectCanonicalPath(root: string): string {
  return `${root.replace(/\/+$/, "")}/.agents/skills`;
}
