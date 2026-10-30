import { useQuery } from "@tanstack/react-query";
import { getInventory } from "../lib/api";

export const inventoryQueryKey = ["inventory"] as const;

export function useInventoryQuery() {
  return useQuery({
    queryKey: inventoryQueryKey,
    queryFn: () => getInventory(),
  });
}
