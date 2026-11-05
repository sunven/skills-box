import { useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { getCliCommands, previewCliCommands } from "../lib/api";
import { cliCommandsQueryKey, mergeCommandRecord, type CliCommandRecord, type CliCommandRequest } from "../lib/cli-commands";
import type { ScopeId } from "../lib/types";

export function useCliCommandRecords() {
  const client = useQueryClient();
  return useQuery({ queryKey: cliCommandsQueryKey, queryFn: async () => {
    const snapshot = await getCliCommands();
    return (client.getQueryData<CliCommandRecord[]>(cliCommandsQueryKey) ?? []).reduce(mergeCommandRecord, snapshot);
  }, staleTime: Infinity });
}
/** One subscription in App; all displays read the same query cache. */
export function useCliCommandSubscription() {
  const client = useQueryClient();
  useCliCommandRecords();
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const receive = (record: CliCommandRecord) => {
      client.setQueryData<CliCommandRecord[]>(cliCommandsQueryKey, (current = []) => mergeCommandRecord(current, record));
    };
    const local = (event: Event) => receive((event as CustomEvent<CliCommandRecord>).detail);
    window.addEventListener("cli-command", local);
    void listen<CliCommandRecord>("cli-command", (event) => receive(event.payload)).then((stop) => {
      if (disposed) stop(); else {
        unlisten = stop;
        // Close the gap between the initial snapshot and listener registration.
        void client.invalidateQueries({ queryKey: cliCommandsQueryKey });
      }
    }).catch(() => {});
    return () => { disposed = true; unlisten?.(); window.removeEventListener("cli-command", local); };
  }, [client]);
}
export function useCliCommandPreview(scope: ScopeId, request: CliCommandRequest | null) {
  return useQuery({
    queryKey: ["cli-command-preview", scope, request],
    queryFn: () => previewCliCommands(scope, request!),
    enabled: request !== null,
    staleTime: 0,
    retry: false,
    refetchOnWindowFocus: false,
  });
}
