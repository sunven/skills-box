import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useState } from "react";
import { acknowledgeDiscoveries, asAppError, fallbackError, getDiscoveryState, setDiscoveryIgnored, startDiscoveryScan } from "../lib/api";
import { discoveryQueryKey, type DiscoveryId } from "../lib/discovery";
import type { AppError } from "../lib/types";

/** One application-level subscription; backend owns freshness and single flight. */
export function useDiscovery(enabled: boolean) {
  const client = useQueryClient();
  const [error, setError] = useState<AppError | null>(null);
  const [pending, setPending] = useState(false);
  const query = useQuery({
    queryKey: discoveryQueryKey,
    queryFn: async () => {
      await startDiscoveryScan(false);
      return getDiscoveryState();
    },
    enabled,
    refetchInterval: (q) => q.state.data?.progress.running ? 2_000 : 30_000,
    refetchIntervalInBackground: true,
  });
  const run = useCallback(async (operation: () => Promise<unknown>) => {
    setPending(true); setError(null);
    try { await operation(); await client.invalidateQueries({ queryKey: discoveryQueryKey }); }
    catch (failure) { setError(asAppError(failure) ?? fallbackError("发现操作失败，请重试。")); }
    finally { setPending(false); }
  }, [client]);
  return {
    query, error, pending,
    scan: () => run(() => startDiscoveryScan(true)),
    acknowledge: (ids?: DiscoveryId[]) => run(() => acknowledgeDiscoveries(ids)),
    ignore: (id: DiscoveryId, ignored: boolean) => run(() => setDiscoveryIgnored(id, ignored)),
  };
}
