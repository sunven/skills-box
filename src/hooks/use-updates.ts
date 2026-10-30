import { useCallback, useRef } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { checkUpdates, getUpdateCheckState } from "../lib/api";
import type { ScopeId, UpdateCheckReport } from "../lib/types";
import { GLOBAL_SCOPE } from "../lib/types";
import {
  mergeUpdateReports,
  updateReportQueryKey,
} from "../lib/update-view";

export const updateCheckStateQueryKey = (scope: ScopeId) =>
  ["updateCheckState", scope] as const;
export const updateCheckStatesQueryKey = ["updateCheckStates"] as const;
export const lastFullCheckQueryKey = (scope: ScopeId) =>
  ["lastFullCheck", scope] as const;
export const lastFullAttemptQueryKey = (scope: ScopeId) =>
  ["lastFullAttempt", scope] as const;

/**
 * Report slot shared by automatic and manual checks. `enabled: false` keeps
 * React Query from starting checks on its own; the auto-check hook and the
 * header button decide when to run one.
 */
export function useUpdateReportQuery(scope: ScopeId = GLOBAL_SCOPE) {
  return useQuery({
    queryKey: updateReportQueryKey(scope),
    queryFn: async () => (await checkUpdates(undefined, scope)).report,
    enabled: false,
  });
}

/** Persisted summary loaded at startup, before the first check finishes. */
export function useUpdateCheckStateQuery(scope: ScopeId = GLOBAL_SCOPE) {
  return useQuery({
    queryKey: updateCheckStateQueryKey(scope),
    queryFn: () => getUpdateCheckState(scope),
    staleTime: Infinity,
  });
}

/** Live full-check report; scoped per-skill checks never write this slot. */
export function useLastFullCheckQuery(scope: ScopeId = GLOBAL_SCOPE) {
  return useQuery<UpdateCheckReport | null>({
    queryKey: lastFullCheckQueryKey(scope),
    queryFn: () => Promise.resolve(null),
    enabled: false,
  });
}

/** Start time of the most recent full check attempt, including failures. */
export function useLastFullAttemptQuery(scope: ScopeId = GLOBAL_SCOPE) {
  return useQuery<number | null>({
    queryKey: lastFullAttemptQueryKey(scope),
    queryFn: () => Promise.resolve(null),
    enabled: false,
  });
}

export function useCheckUpdates(scope: ScopeId = GLOBAL_SCOPE) {
  const queryClient = useQueryClient();
  const inFlight = useRef(false);
  const mutation = useMutation({
    mutationFn: (skillIds?: string[]) => checkUpdates(skillIds, scope),
    onSuccess: (response, skillIds) => {
      const report = response.report;
      const full = skillIds === undefined;
      queryClient.setQueryData<UpdateCheckReport>(
        updateReportQueryKey(scope),
        (previous) => (full ? report : mergeUpdateReports(previous, report)),
      );
      if (full) {
        queryClient.setQueryData(lastFullCheckQueryKey(scope), report);
        queryClient.setQueryData(
          lastFullAttemptQueryKey(scope),
          report.checkedAtEpochMs,
        );
        if (response.state) {
          queryClient.setQueryData(
            updateCheckStateQueryKey(scope),
            response.state,
          );
        } else {
          void queryClient.invalidateQueries({
            queryKey: updateCheckStateQueryKey(scope),
          });
        }
        void queryClient.invalidateQueries({
          queryKey: updateCheckStatesQueryKey,
        });
      }
    },
    onError: (_error, skillIds) => {
      if (skillIds === undefined) {
        queryClient.setQueryData(lastFullAttemptQueryKey(scope), Date.now());
      }
    },
    onSettled: () => {
      inFlight.current = false;
    },
  });
  // Keep `mutate` stable so callers can memoize their own handlers; the
  // in-flight guard already prevents duplicate checks, so it does not need to
  // depend on the ever-changing mutation result object.
  const mutateRef = useRef(mutation.mutate);
  mutateRef.current = mutation.mutate;
  const mutate = useCallback((skillIds?: string[]) => {
    if (inFlight.current) return;
    inFlight.current = true;
    mutateRef.current(skillIds);
  }, []);
  return { ...mutation, mutate };
}
