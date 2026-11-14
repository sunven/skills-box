import type { QueryClient, QueryKey } from "@tanstack/react-query";
import { inventoryQueryKey } from "../hooks/use-inventory";
import { skillQueryKey, skillQueryPrefix } from "../hooks/use-skill-detail";
import {
  skillDirQueryPrefix,
  skillFileQueryPrefix,
} from "../hooks/use-skill-content";
import { staleRecordsQueryKey } from "./batch";
import { discoveryQueryKey } from "./discovery";
import type { MutationAction, MutationResultView } from "./mutation";
import { projectForScope, projectsQueryKey } from "./scope";
import type { ProjectList, ScopeId, UpdateCheckReport } from "./types";
import {
  lastFullAttemptQueryKey,
  lastFullCheckQueryKey,
  updateCheckLifecycle,
  updateCheckStateQueryKey,
  updateCheckStatesQueryKey,
} from "./update-check-lifecycle";
import { markSkillsCurrent, updateReportQueryKey } from "./update-view";

export interface MutationRefreshOptions {
  action: MutationAction;
  /** Submitted Scope; a returned Scope takes precedence. */
  scope: ScopeId;
  skillId?: string;
  /** Manage actions must re-run the environment check, not just the caches. */
  onEnvironmentRecheck?: () => void;
}

/** Apply the complete terminal refresh policy without waiting for refetches. */
export function refreshMutationQueries(
  queryClient: QueryClient,
  options: MutationRefreshOptions,
  result: MutationResultView | null,
): void {
  if (options.action === "preview") return;

  const scope = result?.scope ?? options.scope;
  const { skillId } = options;
  const packs = ["expertPacks", "local", scope] as const;
  const invalidate = (...keys: QueryKey[]) => {
    for (const queryKey of keys) {
      void queryClient.invalidateQueries({ queryKey });
    }
  };

  if (options.action === "project_forget" && result?.outcome === "success") {
    updateCheckLifecycle(queryClient).invalidate(scope, null);
    // Remove observations without asking active observers to reread a forgotten Project.
    for (const queryKey of [
      inventoryQueryKey(scope),
      skillQueryPrefix(scope),
      skillDirQueryPrefix(scope),
      skillFileQueryPrefix(scope),
      updateReportQueryKey(scope),
      updateCheckStateQueryKey(scope),
      lastFullCheckQueryKey(scope),
      lastFullAttemptQueryKey(scope),
      staleRecordsQueryKey(scope),
      packs,
    ]) {
      queryClient.removeQueries({ queryKey });
    }
    invalidate(projectsQueryKey, updateCheckStatesQueryKey, discoveryQueryKey);
    return;
  }

  switch (options.action) {
    case "clean_records":
    case "restore": {
      const rows =
        result?.batch?.rows.filter((row) => row.status === "success") ?? [];
      if (rows.length > 0) {
        const names = new Set(rows.map((row) => row.name));
        queryClient.setQueryData<UpdateCheckReport>(
          updateReportQueryKey(scope),
          (previous) =>
            previous
              ? {
                  ...previous,
                  skills: previous.skills.filter((skill) => !names.has(skill.name)),
                }
              : previous,
        );
        if (options.action === "restore") {
          const project = projectForScope(
            queryClient.getQueryData<ProjectList>(projectsQueryKey), scope,
          );
          updateCheckLifecycle(queryClient).start(
            scope,
            scope === "global" ? undefined : project?.checkRevision ?? "initial",
            rows.map((row) => row.skillId),
          );
        }
      }
      invalidate(
        inventoryQueryKey(scope),
        skillQueryPrefix(scope),
        staleRecordsQueryKey(scope),
        discoveryQueryKey,
      );
      break;
    }
    case "copy_to_project":
      invalidate(
        projectsQueryKey,
        inventoryQueryKey(scope),
        skillQueryPrefix(scope),
        updateReportQueryKey(scope),
        updateCheckStatesQueryKey,
      );
      break;
    case "install":
      invalidate(inventoryQueryKey(scope), skillQueryPrefix(scope), discoveryQueryKey);
      break;
    case "update": {
      const names = result?.rows
        .filter((row) =>
          row.status === "success" &&
          (row.taskStatus === undefined || row.taskStatus === "updated"),
        )
        .map((row) => row.name) ?? [];
      if (names.length > 0) {
        queryClient.setQueryData<UpdateCheckReport | undefined>(
          updateReportQueryKey(scope),
          (previous) => markSkillsCurrent(previous, names),
        );
      }
      invalidate(
        inventoryQueryKey(scope),
        skillQueryPrefix(scope),
        updateReportQueryKey(scope),
        skillDirQueryPrefix(scope, skillId),
        skillFileQueryPrefix(scope, skillId),
      );
      break;
    }
    case "uninstall":
      invalidate(
        inventoryQueryKey(scope),
        skillQueryPrefix(scope),
        updateReportQueryKey(scope),
        discoveryQueryKey,
      );
      break;
    case "uninstall_agents":
      invalidate(
        inventoryQueryKey(scope),
        skillQueryPrefix(scope),
        updateReportQueryKey(scope),
        skillDirQueryPrefix(scope, skillId),
        skillFileQueryPrefix(scope, skillId),
      );
      break;
    case "attach":
    case "detach":
      invalidate(
        inventoryQueryKey(scope),
        skillId ? skillQueryKey(scope, skillId) : skillQueryPrefix(scope),
        skillDirQueryPrefix(scope, skillId),
        skillFileQueryPrefix(scope, skillId),
      );
      break;
    case "project_add":
    case "project_relocate":
    case "project_forget":
      if (options.action === "project_relocate" && result?.checkRevision) {
        updateCheckLifecycle(queryClient).invalidate(scope, result.checkRevision);
      }
      if (options.action === "project_forget") {
        // A failed Forget leaves the Project and its check revision valid.
        invalidate(
          skillDirQueryPrefix(scope),
          skillFileQueryPrefix(scope),
          staleRecordsQueryKey(scope),
        );
      }
      invalidate(
        projectsQueryKey,
        inventoryQueryKey(scope),
        skillQueryPrefix(scope),
        updateReportQueryKey(scope),
        updateCheckStateQueryKey(scope),
        updateCheckStatesQueryKey,
        lastFullCheckQueryKey(scope),
        lastFullAttemptQueryKey(scope),
        discoveryQueryKey,
      );
      break;
    case "manage_add":
    case "manage_remove":
      break;
  }
  invalidate(packs);
  if (options.action === "manage_add" || options.action === "manage_remove") {
    options.onEnvironmentRecheck?.();
  }
}
