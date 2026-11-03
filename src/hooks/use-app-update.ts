import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  APP_UPDATE_INTERVAL, appUpdateBusy, appUpdateCritical, appUpdateQueryKey,
  checkAppUpdate, getAppUpdateState, startAppUpdate, type AppUpdateSnapshot,
} from "../lib/app-update";
import { asAppError, fallbackError } from "../lib/api";

export function useAppUpdate() {
  const client = useQueryClient();
  const [dismissed, setDismissed] = useState<string | null>(null);
  const query = useQuery({
    queryKey: appUpdateQueryKey,
    queryFn: getAppUpdateState,
    refetchInterval: (query) => query.state.data && appUpdateBusy(query.state.data.phase) ? 400 : false,
    // An older read response must not overwrite a command's newer snapshot.
    structuralSharing: (previous, next) => {
      const old = previous as AppUpdateSnapshot | undefined;
      const incoming = next as AppUpdateSnapshot;
      return old && old.revision > incoming.revision ? old : incoming;
    },
  });
  const action = useMutation({
    mutationFn: (kind: "check" | "update") => kind === "check" ? checkAppUpdate() : startAppUpdate(),
    onSuccess: (state) => client.setQueryData(appUpdateQueryKey, state),
  });
  const state = query.data;
  const phase = state?.phase ?? "idle";
  const enabled = state?.enabled ?? false;
  const lastAttempt = state?.lastAttemptAt ?? null;
  const busy = appUpdateBusy(phase) || action.isPending;
  const protectedPhase = appUpdateCritical(phase) || phase === "ready" || Boolean(state?.hasVerifiedDownload);
  const run = action.mutate;

  useEffect(() => {
    if (!enabled || busy || protectedPhase) return;
    const delay = lastAttempt === null ? 1000 : Math.max(0, lastAttempt + APP_UPDATE_INTERVAL - Date.now());
    const timer = window.setTimeout(() => run("check"), delay);
    return () => window.clearTimeout(timer);
  }, [enabled, busy, protectedPhase, lastAttempt, run]);

  const candidateVersion = state?.candidate?.version;
  return {
    state, busy, critical: appUpdateCritical(phase), canCheck: enabled && !busy && !protectedPhase,
    error: state?.error ?? asAppError(action.error) ?? (query.isError ? fallbackError("无法读取应用更新状态。") : null),
    showReminder: Boolean(candidateVersion && candidateVersion !== dismissed && !busy && !protectedPhase),
    dismiss: () => setDismissed(candidateVersion ?? null),
    check: () => run("check"), update: () => run("update"),
  };
}
export type AppUpdateModel = ReturnType<typeof useAppUpdate>;
