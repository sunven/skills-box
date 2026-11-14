import { useEffect, useState, type Dispatch, type SetStateAction } from "react";
import { useQueryClient } from "@tanstack/react-query";

/** Browsing state lives for this app session, separately from upstream query results. */
export function useMarketplaceSession<T>(source: string, initial: T): [T, Dispatch<SetStateAction<T>>] {
  const client = useQueryClient();
  const [state, setState] = useState(() => client.getQueryData<T>(["marketplace-session", source]) ?? initial);
  useEffect(() => { client.setQueryData(["marketplace-session", source], state); }, [client, source, state]);
  return [state, setState];
}
