import { useQuery } from "@tanstack/react-query";
import { getSkillDetail } from "../lib/api";

export const skillQueryKey = (skillId: string | null) =>
  ["skill", skillId] as const;

/** Detail is only fetched for the selected row. */
export function useSkillDetailQuery(skillId: string | null) {
  return useQuery({
    queryKey: skillQueryKey(skillId),
    queryFn: () => getSkillDetail(skillId as string),
    enabled: skillId !== null,
  });
}
