import { useQuery } from "@tanstack/react-query";
import { getSkillDetail } from "../lib/api";
import type { ScopeId } from "../lib/types";

export const skillQueryPrefix = (scope: ScopeId) => ["skill", scope] as const;

export const skillQueryKey = (scope: ScopeId, skillId: string | null) =>
  [...skillQueryPrefix(scope), skillId] as const;

/** Detail is only fetched for the selected row, always inside its scope. */
export function useSkillDetailQuery(
  scope: ScopeId,
  skillId: string | null,
  enabled = true,
) {
  return useQuery({
    queryKey: skillQueryKey(scope, skillId),
    queryFn: () => getSkillDetail(skillId as string, scope),
    enabled: enabled && skillId !== null,
  });
}
