import { useCallback } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { getUpdateCheckState } from "../lib/api";
import type { ScopeId, UpdateCheckReport } from "../lib/types";
import { GLOBAL_SCOPE } from "../lib/types";
import { updateReportQueryKey } from "../lib/update-view";
import {
  checkRunQueryKey,
  IDLE_CHECK,
  updateCheckLifecycle,
  type CheckRunState,
  updateCheckStateQueryKey,
  lastFullCheckQueryKey,
  lastFullAttemptQueryKey,
} from "../lib/update-check-lifecycle";
export {
  updateCheckStateQueryKey,
  updateCheckStatesQueryKey,
  lastFullCheckQueryKey,
  lastFullAttemptQueryKey,
} from "../lib/update-check-lifecycle";

/**
 * Report slot shared by automatic and manual checks. `enabled: false` keeps
 * React Query from starting checks on its own; the auto-check hook and the
 * header button decide when to run one.
 */
export function useUpdateReportQuery(scope: ScopeId = GLOBAL_SCOPE) {
  return useQuery({
    queryKey: updateReportQueryKey(scope),
    queryFn: () => Promise.resolve<UpdateCheckReport | null>(null),
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

export function useCheckUpdates(
  scope: ScopeId = GLOBAL_SCOPE,
  checkRevision?: string,
) {
  const queryClient = useQueryClient();
  const revision =
    scope === GLOBAL_SCOPE ? undefined : (checkRevision ?? "initial");
  const run = useQuery<CheckRunState>({
    queryKey: checkRunQueryKey(scope, revision),
    queryFn: () => Promise.resolve(IDLE_CHECK),
    enabled: false,
  });
  const mutate = useCallback(
    (skillIds?: string[]) =>
      updateCheckLifecycle(queryClient).start(scope, revision, skillIds),
    [queryClient, scope, revision],
  );
  return { ...(run.data ?? IDLE_CHECK), mutate };
}
