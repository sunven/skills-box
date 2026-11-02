import type { DiscoveryState, DiscoveredSkill, InstallationStatus } from "../lib/discovery";
import type { ProjectEntry, ScopeId } from "../lib/types";
export const discoveryProject: ProjectEntry = {
  id: "5a7b6c1d-2e3f-4a5b-8c9d-0e1f2a3b4c5d", root: "/tmp/discovery-project", displayName: "示例项目",
  addedAt: 1, lastOpenedAt: 1, checkRevision: "initial", available: true,
};
export const discoveryProjectScope: ScopeId = `project:${discoveryProject.id}`;
export function discovered(name: string, status: InstallationStatus, options: Partial<DiscoveredSkill> = {}): DiscoveredSkill {
  return { id: { source: "acme/skills", name: name.toLowerCase() }, name, description: `${name} description`, group: "General",
    isNew: false, unread: false, ignored: false, firstSeenAt: 1_793_542_800_000, installIssue: null,
    installations: [{ scope: "global", label: "Global", status }, { scope: discoveryProjectScope, label: "示例项目", status: "notInstalled" }],
    ...options };
}
export function discoveryFixture(options: Partial<DiscoveryState> = {}): DiscoveryState {
  return { sources: [{ source: "acme/skills", lastAttemptAt: 1_793_542_800_000, lastSuccessAt: 1_793_542_800_000,
    error: null, scanning: false, needsRevalidation: false, pausedUntil: null }],
    skills: [discovered("alpha", "installed"), discovered("beta", "installed", { isNew: true, unread: true }),
      discovered("gamma", "notInstalled", { isNew: true, unread: true, installations: [{ scope: "global", label: "Global", status: "notInstalled" },
        { scope: discoveryProjectScope, label: "示例项目", status: "installed" }] })],
    unreadCount: 2, progress: { runId: 1, running: false, total: 1, completed: 1 }, warnings: [], error: null, ...options };
}
