import type { QueryClient } from "@tanstack/react-query";
import { asAppError, checkUpdates, fallbackError } from "./api";
import {
  GLOBAL_SCOPE,
  type AppError,
  type ProjectList,
  type ScopeId,
  type UpdateCheckReport,
} from "./types";
import { mergeUpdateReports, updateReportQueryKey } from "./update-view";

export const updateCheckStateQueryKey = (scope: ScopeId) =>
  ["updateCheckState", scope] as const;
export const updateCheckStatesQueryKey = ["updateCheckStates"] as const;
export const lastFullCheckQueryKey = (scope: ScopeId) =>
  ["lastFullCheck", scope] as const;
export const lastFullAttemptQueryKey = (scope: ScopeId) =>
  ["lastFullAttempt", scope] as const;
export const checkRunQueryKey = (scope: ScopeId, revision?: string) =>
  ["updateCheckRun", scope, revision ?? null] as const;

export interface CheckRunState {
  isPending: boolean;
  error: AppError | null;
}
export const IDLE_CHECK: CheckRunState = { isPending: false, error: null };

/** One lifecycle per QueryClient, shared by checks and Project changes. */
class UpdateCheckLifecycle {
  private runs = new Map<string, object>();
  private revisions = new Map<ScopeId, string | null>();

  constructor(private client: QueryClient) {}

  private current(scope: ScopeId, revision?: string): boolean {
    if (scope === GLOBAL_SCOPE) return true;
    const projects = this.client.getQueryData<ProjectList>(["projects"]);
    const project = projects?.projects.find(
      (entry) => `project:${entry.id}` === scope,
    );
    const known = this.revisions.has(scope)
      ? this.revisions.get(scope)
      : projects && !project
        ? null
        : (project?.checkRevision ?? "initial");
    return known === revision;
  }

  start(
    scope: ScopeId,
    revision: string | undefined,
    skillIds?: string[],
  ): boolean {
    if (!this.current(scope, revision)) return false;
    const key = JSON.stringify(checkRunQueryKey(scope, revision));
    if (this.runs.has(key)) return false;
    const attempt = {};
    this.runs.set(key, attempt);
    this.client.setQueryData(checkRunQueryKey(scope, revision), {
      isPending: true,
      error: null,
    });
    void this.run(scope, revision, skillIds, key, attempt);
    return true;
  }

  private async run(
    scope: ScopeId,
    revision: string | undefined,
    skillIds: string[] | undefined,
    key: string,
    attempt: object,
  ) {
    let error: AppError | null = null;
    try {
      const response = await checkUpdates(skillIds, scope, revision);
      if (
        !this.current(scope, revision) ||
        response.invalidated ||
        (response.scope !== undefined && response.scope !== scope) ||
        (response.checkRevision !== undefined &&
          response.checkRevision !== revision)
      )
        return;
      const report = response.report;
      this.client.setQueryData<UpdateCheckReport>(
        updateReportQueryKey(scope),
        (previous) =>
          skillIds === undefined
            ? report
            : mergeUpdateReports(previous, report),
      );
      if (skillIds === undefined) {
        this.client.setQueryData(lastFullCheckQueryKey(scope), report);
        this.client.setQueryData(
          lastFullAttemptQueryKey(scope),
          report.checkedAtEpochMs,
        );
        if (response.state)
          this.client.setQueryData(
            updateCheckStateQueryKey(scope),
            response.state,
          );
        else
          void this.client.invalidateQueries({
            queryKey: updateCheckStateQueryKey(scope),
          });
        void this.client.invalidateQueries({
          queryKey: updateCheckStatesQueryKey,
        });
      }
    } catch (cause) {
      if (this.current(scope, revision)) {
        error = asAppError(cause) ?? fallbackError("无法检查更新。");
        if (skillIds === undefined)
          this.client.setQueryData(lastFullAttemptQueryKey(scope), Date.now());
      }
    } finally {
      if (this.runs.get(key) === attempt) {
        this.runs.delete(key);
        if (this.current(scope, revision))
          this.client.setQueryData(checkRunQueryKey(scope, revision), {
            isPending: false,
            error,
          });
      }
    }
  }

  invalidate(scope: ScopeId, revision: string | null) {
    this.revisions.set(scope, revision);
    for (const queryKey of [
      updateCheckStateQueryKey(scope),
      updateCheckStatesQueryKey,
    ]) {
      void this.client.cancelQueries({ queryKey });
    }
    this.client.setQueryData<ProjectList>(["projects"], (previous) =>
      previous
        ? {
            ...previous,
            projects:
              revision === null
                ? previous.projects.filter(
                    (project) => `project:${project.id}` !== scope,
                  )
                : previous.projects.map((project) =>
                    `project:${project.id}` === scope
                      ? { ...project, checkRevision: revision }
                      : project,
                  ),
          }
        : previous,
    );
    // Clear values in place: removal leaves mounted disabled observers
    // displaying their previous result until another render reattaches them.
    for (const queryKey of [
      updateReportQueryKey(scope),
      lastFullCheckQueryKey(scope),
      lastFullAttemptQueryKey(scope),
    ]) {
      this.client.setQueryData(queryKey, null);
    }
    this.client.setQueriesData(
      { queryKey: ["updateCheckRun", scope] },
      IDLE_CHECK,
    );
    this.client.setQueryData(updateCheckStateQueryKey(scope), null);
    this.client.setQueryData<Record<string, unknown>>(
      updateCheckStatesQueryKey,
      (previous) => {
        if (!previous) return previous;
        const next = { ...previous };
        delete next[scope];
        return next;
      },
    );
  }
}

const lifecycles = new WeakMap<QueryClient, UpdateCheckLifecycle>();
export function updateCheckLifecycle(
  client: QueryClient,
): UpdateCheckLifecycle {
  let lifecycle = lifecycles.get(client);
  if (!lifecycle) {
    lifecycle = new UpdateCheckLifecycle(client);
    lifecycles.set(client, lifecycle);
  }
  return lifecycle;
}
