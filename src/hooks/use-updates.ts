import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { checkUpdates } from "../lib/api";
import type { UpdateCheckReport } from "../lib/types";
import { mergeUpdateReports } from "../lib/update-view";

export const updateReportQueryKey = ["updateReport"] as const;

/**
 * Manual-only report slot. `enabled: false` guarantees no startup or focus
 * check; a user action runs the mutation and stores the report here.
 */
export function useUpdateReportQuery() {
  return useQuery({
    queryKey: updateReportQueryKey,
    queryFn: () => checkUpdates(),
    enabled: false,
  });
}

export function useCheckUpdates() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (skillIds?: string[]) => checkUpdates(skillIds),
    onSuccess: (report, skillIds) => {
      queryClient.setQueryData<UpdateCheckReport>(
        updateReportQueryKey,
        (previous) =>
          skillIds && skillIds.length > 0
            ? mergeUpdateReports(previous, report)
            : report,
      );
    },
  });
}
