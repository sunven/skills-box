import type { AppError, ScopeId } from "./types";

export interface DiscoveryId { source: string; name: string }
export interface DiscoveryProgress { runId: number; running: boolean; total: number; completed: number }
export type InstallationStatus = "installed" | "notInstalled" | "conflict" | "stale" | "unknown";
export interface ScopeInstallation { scope: ScopeId; label: string; status: InstallationStatus }
export interface DiscoveredSkill {
  id: DiscoveryId; name: string; description: string; group: string | null;
  isNew: boolean; unread: boolean; ignored: boolean; firstSeenAt: number;
  installIssue: string | null; installations: ScopeInstallation[];
}
export interface DiscoverySource {
  source: string; lastAttemptAt: number | null; lastSuccessAt: number | null;
  error: string | null; scanning: boolean; needsRevalidation: boolean; pausedUntil: number | null;
}
export interface DiscoveryState {
  skills: DiscoveredSkill[]; sources: DiscoverySource[]; unreadCount: number;
  progress: DiscoveryProgress; warnings: string[]; error: AppError | null;
}
export interface DiscoveryInstallSeed { source: string; name: string }
export type DiscoveryFilter = "uninstalled" | "all" | "new" | "ignored";
export const discoveryQueryKey = ["discovery"] as const;
export const installationLabels: Record<InstallationStatus, string> = {
  installed: "已安装", notInstalled: "未安装", conflict: "同名冲突", stale: "安装记录异常", unknown: "安装状态未知",
};
export function installationFor(skill: DiscoveredSkill, scope: ScopeId): InstallationStatus {
  return skill.installations.find((s) => s.scope === scope)?.status ?? "unknown";
}
export function discoveryKey(id: DiscoveryId): string { return JSON.stringify([id.source, id.name]); }
export function filterDiscoveries(skills: DiscoveredSkill[], scope: ScopeId, filter: DiscoveryFilter, search: string, source: string): DiscoveredSkill[] {
  const query = search.trim().toLocaleLowerCase();
  return skills.filter((s) => {
    if (source !== "all" && s.id.source !== source) return false;
    if (filter === "ignored" ? !s.ignored : s.ignored) return false;
    if (filter === "new" && !s.unread) return false;
    if (filter === "uninstalled" && installationFor(s, scope) === "installed") return false;
    return !query || `${s.name}\n${s.description}`.toLocaleLowerCase().includes(query);
  });
}
