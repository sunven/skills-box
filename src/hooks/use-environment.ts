import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { getEnvironment } from "../lib/api";

export const environmentQueryKey = ["environment"] as const;

export function useEnvironmentQuery() {
  return useQuery({
    queryKey: environmentQueryKey,
    queryFn: () => getEnvironment(false),
  });
}

export function useRecheckEnvironment() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => getEnvironment(true),
    onSuccess: (report) => {
      queryClient.setQueryData(environmentQueryKey, report);
    },
  });
}
