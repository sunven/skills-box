import { useQuery } from "@tanstack/react-query";
import { listSkillDir, readSkillFile } from "../lib/api";
import type { ScopeId } from "../lib/types";

export const skillDirQueryKey = (
  scope: ScopeId,
  skillId: string,
  path: string,
) => ["skill-dir", scope, skillId, path] as const;

export const skillFileQueryKey = (
  scope: ScopeId,
  skillId: string,
  path: string,
) => ["skill-file", scope, skillId, path] as const;

/**
 * Content entries leave the cache shortly after the drawer closes; image
 * previews are data URLs, so keeping them for the whole session would grow
 * memory without bound.
 */
const CONTENT_GC_TIME_MS = 5 * 60_000;

/**
 * Content is read from disk on demand. `staleTime: 0` means reopening the
 * drawer always re-reads; the manual refresh button invalidates the prefix.
 */
export function useSkillDirQuery(
  scope: ScopeId,
  skillId: string,
  path: string,
  enabled = true,
) {
  return useQuery({
    queryKey: skillDirQueryKey(scope, skillId, path),
    queryFn: () => listSkillDir(skillId, path, scope),
    enabled,
    staleTime: 0,
    gcTime: CONTENT_GC_TIME_MS,
  });
}

export function useSkillFileQuery(
  scope: ScopeId,
  skillId: string,
  path: string | null,
) {
  return useQuery({
    queryKey: skillFileQueryKey(scope, skillId, path ?? ""),
    queryFn: () => readSkillFile(skillId, path as string, scope),
    enabled: path !== null,
    staleTime: 0,
    gcTime: CONTENT_GC_TIME_MS,
  });
}
