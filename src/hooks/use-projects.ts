import { useRef } from "react";
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
  const data = results.map((result) => result.data ?? null);
  // Keep the Map stable while the underlying query data is unchanged so
  // downstream workspace projections are not invalidated on every render.
  const cache = useRef<{
    scopes: ScopeId[];
    data: (UpdateCheckState | null)[];
    states: Map<ScopeId, UpdateCheckState | null>;
  } | null>(null);
  const cached = cache.current;
  if (
    cached &&
    cached.scopes === scopes &&
    cached.data.length === data.length &&
    data.every((value, index) => value === cached.data[index])
  ) {
    return cached.states;
  }
  const states = new Map<ScopeId, UpdateCheckState | null>();
  scopes.forEach((scope, index) => {
    states.set(scope, data[index] ?? null);
  });
  cache.current = { scopes, data, states };
  return states;
}
