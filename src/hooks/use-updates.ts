import { useCallback, useRef } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { checkUpdates, getUpdateCheckState } from "../lib/api";
import type { UpdateCheckReport } from "../lib/types";
import { mergeUpdateReports } from "../lib/update-view";

export const updateReportQueryKey = ["updateReport"] as const;
export const updateCheckStateQueryKey = ["updateCheckState"] as const;
export const lastFullCheckQueryKey = ["lastFullCheck"] as const;
export const lastFullAttemptQueryKey = ["lastFullAttempt"] as const;

/**
 * Report slot shared by automatic and manual checks. `enabled: false` keeps
 * React Query from starting checks on its own; the auto-check hook and the
 * header button decide when to run one.
 */
export function useUpdateReportQuery() {
  return useQuery({
    queryKey: updateReportQueryKey,
    queryFn: () => checkUpdates(),
    enabled: false,
  });
}

/** Persisted summary loaded at startup, before the first check finishes. */
export function useUpdateCheckStateQuery() {
  return useQuery({
    queryKey: updateCheckStateQueryKey,
    queryFn: getUpdateCheckState,
    staleTime: Infinity,
  });
}

/** Live full-check report; scoped per-skill checks never write this slot. */
export function useLastFullCheckQuery() {
  return useQuery<UpdateCheckReport | null>({
    queryKey: lastFullCheckQueryKey,
    queryFn: () => Promise.resolve(null),
    enabled: false,
  });
}

/** Start time of the most recent full check attempt, including failures. */
export function useLastFullAttemptQuery() {
  return useQuery<number | null>({
    queryKey: lastFullAttemptQueryKey,
    queryFn: () => Promise.resolve(null),
    enabled: false,
  });
}

export function useCheckUpdates() {
  const queryClient = useQueryClient();
  const inFlight = useRef(false);
  const mutation = useMutation({
    mutationFn: (skillIds?: string[]) => checkUpdates(skillIds),
    onSuccess: (report, skillIds) => {
      const full = skillIds === undefined;
      queryClient.setQueryData<UpdateCheckReport>(
        updateReportQueryKey,
        (previous) => (full ? report : mergeUpdateReports(previous, report)),
      );
      if (full) {
        queryClient.setQueryData(lastFullCheckQueryKey, report);
        queryClient.setQueryData(
          lastFullAttemptQueryKey,
          report.checkedAtEpochMs,
        );
        void queryClient.invalidateQueries({
          queryKey: updateCheckStateQueryKey,
        });
      }
    },
    onError: (_error, skillIds) => {
      if (skillIds === undefined) {
        queryClient.setQueryData(lastFullAttemptQueryKey, Date.now());
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
