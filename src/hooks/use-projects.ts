import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { getUpdateCheckStates, listProjects } from "../lib/api";
import type { ScopeId, UpdateCheckState } from "../lib/types";
import { updateCheckStatesQueryKey } from "./use-updates";
import { projectsQueryKey } from "../lib/scope";

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
  const { data } = useQuery({
    queryKey: updateCheckStatesQueryKey,
    queryFn: getUpdateCheckStates,
    staleTime: Infinity,
  });
  return useMemo(() => {
    const states = new Map<ScopeId, UpdateCheckState | null>();
    for (const scope of scopes) {
      states.set(scope, data?.[scope] ?? null);
    }
    return states;
  }, [data, scopes]);
}
