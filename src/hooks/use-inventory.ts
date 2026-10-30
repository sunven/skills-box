import { useQuery } from "@tanstack/react-query";
import { getInventory } from "../lib/api";
import type { ScopeId } from "../lib/types";
import { GLOBAL_SCOPE } from "../lib/types";

export const inventoryQueryKey = (scope: ScopeId = GLOBAL_SCOPE) =>
  ["inventory", scope] as const;

export function useInventoryQuery(scope: ScopeId, enabled = true) {
  return useQuery({
    queryKey: inventoryQueryKey(scope),
    queryFn: () => getInventory(scope),
    enabled,
  });
}
