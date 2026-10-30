import { useQueries, useQuery } from "@tanstack/react-query";
import { getUpdateCheckState, listProjects } from "../lib/api";
import type { ScopeId, UpdateCheckState } from "../lib/types";
import { updateCheckStateQueryKey } from "./use-updates";

export const projectsQueryKey = ["projects"] as const;

/** App-registered scopes plus the persisted active scope. */
export function useProjectsQuery() {
  return useQuery({
    queryKey: projectsQueryKey,
    queryFn: () => listProjects(),
  });
}

/**
 * Persisted check state for every scope in the switcher. Badge counts come
 * from the backend state, which records per-scope candidate counts.
 */
export function useScopeCheckStates(scopes: ScopeId[]) {
  const results = useQueries({
    queries: scopes.map((scope) => ({
      queryKey: updateCheckStateQueryKey(scope),
      queryFn: () => getUpdateCheckState(scope),
      staleTime: Infinity,
    })),
  });
  const states = new Map<ScopeId, UpdateCheckState | null>();
  scopes.forEach((scope, index) => {
    states.set(scope, results[index]?.data ?? null);
  });
  return states;
}
