import {
  useCallback,
  useDeferredValue,
  useEffect,
  useMemo,
  useState,
} from "react";
import { useQueryClient } from "@tanstack/react-query";
import { AnomalyDialog } from "./components/anomaly-panel";
import { AppHeader } from "./components/app-header";
import { DetailPane } from "./components/detail-pane";
import { EnvironmentBanner } from "./components/environment-banner";
import { ExitDialog } from "./components/exit-dialog";
import { FilterSidebar } from "./components/filter-sidebar";
import { InstallWizard } from "./components/install-wizard";
import { InterruptedBanner } from "./components/interrupted-banner";
import { LockErrorScreen } from "./components/lock-error-screen";
import { ManageAgentsDialog } from "./components/manage-agents-dialog";
import { ProjectDialogs, type ProjectDialogState } from "./components/project-dialogs";
import { ScopeSwitcher } from "./components/scope-switcher";
import { SkillListPane } from "./components/skill-list-pane";
import { UninstallDialog } from "./components/uninstall-dialog";
import { UpdateDialog } from "./components/update-dialog";
import { useAutoUpdateCheck } from "./hooks/use-auto-update-check";
import { environmentQueryKey, useEnvironmentQuery, useRecheckEnvironment } from "./hooks/use-environment";
import { useInventoryQuery } from "./hooks/use-inventory";
import { projectsQueryKey, useProjectsQuery, useScopeCheckStates } from "./hooks/use-projects";
import {
  lastFullAttemptQueryKey,
  lastFullCheckQueryKey,
  updateCheckStateQueryKey,
  useCheckUpdates,
  useLastFullAttemptQuery,
  useLastFullCheckQuery,
  useUpdateCheckStateQuery,
  useUpdateReportQuery,
} from "./hooks/use-updates";
import {
  asAppError,
  dismissInterruptedMutation,
  fallbackError,
  isLockError,
  getActiveMutation,
  quitApp,
  revealAppData,
  setActiveScope as persistActiveScope,
} from "./lib/api";
import { listenForExitRequest } from "./lib/lifecycle";
import { defaultFilters, type SortKey, type StatusFilter } from "./lib/inventory-view";
import {
  projectForScope,
  projectScope,
  resolveInitialScope,
} from "./lib/scope";
import { updateReportQueryKey } from "./lib/update-view";
import { buildWorkspaceView } from "./lib/workspace-view";
import { GLOBAL_SCOPE } from "./lib/types";
import type {
  ActiveMutationInfo,
  DetectedAgent,
  EnvReport,
  InventoryAgent,
  ProjectEntry,
  ScopeId,
  UpdateViewSkill,
} from "./lib/types";

const EMPTY_INVENTORY_AGENTS: InventoryAgent[] = [];
const EMPTY_DETECTED_AGENTS: DetectedAgent[] = [];
const EMPTY_WARNINGS: string[] = [];

function App() {
  const queryClient = useQueryClient();
  const environment = useEnvironmentQuery();
  const recheck = useRecheckEnvironment();
  const projectsQuery = useProjectsQuery();
  const [activeScope, setActiveScope] = useState<ScopeId | null>(null);
  const [projectDialog, setProjectDialog] = useState<ProjectDialogState>(null);
  const [scopeNotice, setScopeNotice] = useState<string | null>(null);
  const projects = projectsQuery.data;
  const scope = activeScope ?? GLOBAL_SCOPE;
  const scopeReady = activeScope !== null;
  const activeProject = projectForScope(projects, scope);
  const scopeUnavailable = Boolean(activeProject && !activeProject.available);

  useEffect(() => {
    if (activeScope !== null) return;
    if (projects) {
      setActiveScope(resolveInitialScope(projects));
      return;
    }
    if (projectsQuery.isError) setActiveScope(GLOBAL_SCOPE);
  }, [activeScope, projects, projectsQuery.isError]);

  const scopeIds = useMemo(
    () => [
      GLOBAL_SCOPE,
      ...(projects?.projects.map((entry) => projectScope(entry.id)) ?? []),
    ],
    [projects],
  );
  const scopeCheckStates = useScopeCheckStates(scopeIds);
  const inventoryQuery = useInventoryQuery(scope, scopeReady && !scopeUnavailable);
  const updateReportQuery = useUpdateReportQuery(scope);
  const updateCheckStateQuery = useUpdateCheckStateQuery(scope);
  const lastFullCheckQuery = useLastFullCheckQuery(scope);
  const lastFullAttemptQuery = useLastFullAttemptQuery(scope);
  const checkUpdates = useCheckUpdates(scope);
  const report = environment.data;
  const snapshot = inventoryQuery.data;
  const updateData = updateReportQuery.data ?? null;
  const lastFullCheck = lastFullCheckQuery.data ?? null;

  const [exitRequest, setExitRequest] = useState<ActiveMutationInfo | null>(
    null,
  );
  const [quitting, setQuitting] = useState(false);
  const [exitError, setExitError] = useState<string | null>(null);

  const [query, setQuery] = useState("");
  const [source, setSource] = useState("all");
  const [agent, setAgent] = useState("all");
  const [status, setStatus] = useState<StatusFilter>("all");
  const [sort, setSort] = useState<SortKey>("recent");
  const [selectedName, setSelectedName] = useState<string | null>(null);
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [detailOpen, setDetailOpen] = useState(false);
  const [anomaliesOpen, setAnomaliesOpen] = useState(false);
  const [installOpen, setInstallOpen] = useState(false);
  const [manageAgentsOpen, setManageAgentsOpen] = useState(false);
  const [updateMode, setUpdateMode] = useState<"single" | "all" | null>(null);
  const [updateSkillName, setUpdateSkillName] = useState<string | null>(null);
  // Keep the uninstall target stable across the inventory refresh that follows
  // a successful removal; the row itself may already be gone from `skills`.
  const [uninstallSkill, setUninstallSkill] = useState<UpdateViewSkill | null>(
    null,
  );

  // Filtering runs on a deferred query so typing stays urgent while the list
  // and the detail drawer re-render at a lower priority.
  const deferredQuery = useDeferredValue(query);
  const scopeError = asAppError(inventoryQuery.error);
  const view = useMemo(
    () =>
      buildWorkspaceView({
        snapshot,
        projects,
        activeScope: scope,
        scopeError,
        scopeCheckStates,
        inventoryLoading: inventoryQuery.isPending && !scopeUnavailable,
        updateReport: updateData,
        lastFullCheck,
        updateCheckState: updateCheckStateQuery.data ?? null,
        filters: { query: deferredQuery, source, agent, status },
        sort,
        selectedName,
        updateMode,
        updateSkillName,
      }),
    [
      snapshot,
      projects,
      scope,
      scopeError,
      scopeCheckStates,
      inventoryQuery.isPending,
      scopeUnavailable,
      updateData,
      lastFullCheck,
      updateCheckStateQuery.data,
      deferredQuery,
      source,
      agent,
      status,
      sort,
      selectedName,
      updateMode,
      updateSkillName,
    ],
  );

  useEffect(() => {
    if (selectedName === null) return;
    if (!view.visibleSkills.some((skill) => skill.name === selectedName)) {
      const fallback = view.visibleSkills[0]?.name ?? null;
      setSelectedName(fallback);
      if (fallback === null) setDetailOpen(false);
    }
  }, [view.visibleSkills, selectedName]);

  // Rust vetoes window close / Cmd+Q while a mutation is running and emits
  // this event with the active task so the renderer can explain the risk.
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listenForExitRequest((info) => {
      if (disposed) return;
      setExitError(null);
      setExitRequest(info);
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const handleConfirmExit = async () => {
    setQuitting(true);
    setExitError(null);
    try {
      await quitApp();
    } catch (error) {
      setQuitting(false);
      setExitError(
        (asAppError(error) ?? fallbackError("无法退出应用。")).message,
      );
    }
  };

  const handleDismissInterrupted = () => {
    queryClient.setQueryData<EnvReport | undefined>(
      environmentQueryKey,
      (previous) =>
        previous ? { ...previous, interrupted: null } : previous,
    );
    void dismissInterruptedMutation().catch(() => {
      // The marker may already be gone; the next launch re-verifies.
    });
  };

  const handleRevealBackups = () => {
    void revealAppData("backups").catch(() => {
      // Best effort; the banner still shows the snapshot paths.
    });
  };

  const inventoryError = scopeError;
  const updateError = asAppError(checkUpdates.error);
  const scopeLockError =
    isLockError(inventoryError) ? inventoryError : null;
  // Lock recovery is a global-scope surface; project lock errors stay inside
  // the project view and never take over the whole app.
  const lockError = scope === GLOBAL_SCOPE ? scopeLockError : null;
  const scopeBlocked =
    scopeUnavailable || scopeLockError !== null || view.scopeError !== null;
  const mutationsEnabled =
    (report?.mutationsEnabled ?? false) && !scopeBlocked;

  const mutateCheckUpdates = checkUpdates.mutate;
  const runAutoCheck = useCallback(async () => {
    
    try {
      const active = await getActiveMutation();
      if (active) return false;
    } catch {
      return false;
    }
    mutateCheckUpdates();
    return true;
  }, [mutateCheckUpdates]);

  useAutoUpdateCheck({
    scope,
    enabled:
      inventoryQuery.isSuccess &&
      updateCheckStateQuery.isSuccess &&
      !scopeBlocked &&
      view.hasManagedSkills,
    checking: checkUpdates.isPending,
    // The persisted attempt lets a scope switched into the foreground be
    // checked only when its own six-hour window expired; the session slot
    // keeps a manual check's fresh anchor for this scope.
    lastFullAttemptAt:
      Math.max(
        lastFullAttemptQuery.data ?? 0,
        updateCheckStateQuery.data?.lastAttemptAt ?? 0,
      ) || null,
    onCheck: runAutoCheck,
  });

  // A manual environment refresh is also a full read-model refresh: the
  // inventory may have changed under us, and cached detail/update results are
  // no longer trustworthy. Invalidation never crosses into other scopes.
  const handleRecheck = () => {
    recheck.mutate();
    void inventoryQuery.refetch();
    void queryClient.invalidateQueries({ queryKey: ["skill", scope] });
    void queryClient.invalidateQueries({ queryKey: ["updateReport", scope] });
  };

  const handleCheckUpdates = useCallback(
    (skillIds?: string[]) => {
      mutateCheckUpdates(skillIds);
    },
    [mutateCheckUpdates],
  );

  const handleRetryInventory = () => {
    if (view.scopeError) {
      void inventoryQuery.refetch();
      return;
    }
    handleRecheck();
  };

  const handleRefreshProject = () => {
    void inventoryQuery.refetch();
    mutateCheckUpdates();
  };

  const handleClearFilters = useCallback(() => {
    const defaults = defaultFilters();
    setQuery(defaults.query);
    setSource(defaults.source);
    setAgent(defaults.agent);
    setStatus(defaults.status);
  }, []);

  const resetWorkspaceState = useCallback(() => {
    const defaults = defaultFilters();
    setQuery(defaults.query);
    setSource(defaults.source);
    setAgent(defaults.agent);
    setStatus(defaults.status);
    setSort("recent");
    setSelectedName(null);
    setDetailOpen(false);
    setFiltersOpen(false);
    setAnomaliesOpen(false);
    setUpdateMode(null);
    setUpdateSkillName(null);
    setUninstallSkill(null);
  }, []);

  const handleSelectScope = useCallback(
    (next: ScopeId) => {
      if (!scopeReady || next === scope) return;
      resetWorkspaceState();
      setScopeNotice(null);
      setActiveScope(next);
      void persistActiveScope(next)
        .then(() =>
          queryClient.invalidateQueries({ queryKey: projectsQueryKey }),
        )
        .catch((error: unknown) => {
          const typed = asAppError(error) ?? fallbackError("无法保存当前作用域。");
          setScopeNotice(typed.message);
        });
    },
    [queryClient, resetWorkspaceState, scope, scopeReady],
  );

  // Forgetting or relocating clears the backend's per-scope check state, so
  // drop the cached badges/report for that scope instead of showing stale data.
  const clearScopeCheckCache = useCallback(
    (target: ScopeId) => {
      void queryClient.removeQueries({
        queryKey: updateCheckStateQueryKey(target),
      });
      void queryClient.removeQueries({
        queryKey: lastFullCheckQueryKey(target),
      });
      void queryClient.removeQueries({
        queryKey: lastFullAttemptQueryKey(target),
      });
      void queryClient.removeQueries({
        queryKey: updateReportQueryKey(target),
      });
    },
    [queryClient],
  );

  const cacheProjectList = useCallback(
    (update: (previous: typeof projects) => typeof projects) => {
      queryClient.setQueryData(projectsQueryKey, update);
    },
    [queryClient],
  );

  const handleProjectAdded = useCallback(
    (entry: ProjectEntry) => {
      resetWorkspaceState();
      setProjectDialog(null);
      cacheProjectList((previous) => {
        const list = previous ?? {
          projects: [],
          lastActiveScope: GLOBAL_SCOPE,
          warnings: [],
        };
        return {
          ...list,
          projects: [
            entry,
            ...list.projects.filter((item) => item.id !== entry.id),
          ],
          lastActiveScope: projectScope(entry.id),
        };
      });
      setActiveScope(projectScope(entry.id));
      void queryClient.invalidateQueries({ queryKey: projectsQueryKey });
    },
    [cacheProjectList, queryClient, resetWorkspaceState],
  );

  const handleProjectRelocated = useCallback(
    (entry: ProjectEntry) => {
      resetWorkspaceState();
      setProjectDialog(null);
      cacheProjectList((previous) => {
        const list = previous ?? {
          projects: [],
          lastActiveScope: GLOBAL_SCOPE,
          warnings: [],
        };
        return {
          ...list,
          projects: [
            entry,
            ...list.projects.filter((item) => item.id !== entry.id),
          ],
          lastActiveScope: projectScope(entry.id),
        };
      });
      setActiveScope(projectScope(entry.id));
      clearScopeCheckCache(projectScope(entry.id));
      void queryClient.invalidateQueries({ queryKey: projectsQueryKey });
    },
    [cacheProjectList, clearScopeCheckCache, queryClient, resetWorkspaceState],
  );

  const handleProjectForgotten = useCallback(
    (projectId: string) => {
      setProjectDialog(null);
      const forgottenScope = projectScope(projectId);
      const wasActive = scope === forgottenScope;
      if (wasActive) {
        resetWorkspaceState();
        setActiveScope(GLOBAL_SCOPE);
      }
      cacheProjectList((previous) =>
        previous
          ? {
              ...previous,
              projects: previous.projects.filter(
                (entry) => entry.id !== projectId,
              ),
              lastActiveScope: wasActive
                ? GLOBAL_SCOPE
                : previous.lastActiveScope,
            }
          : previous,
      );
      void queryClient.removeQueries({
        queryKey: ["inventory", forgottenScope],
      });
      void queryClient.removeQueries({
        queryKey: ["skill", forgottenScope],
      });
      clearScopeCheckCache(forgottenScope);
      void queryClient.invalidateQueries({ queryKey: projectsQueryKey });
    },
    [cacheProjectList, clearScopeCheckCache, queryClient, resetWorkspaceState, scope],
  );

  const handleSelectSkill = useCallback((name: string) => {
    setSelectedName(name);
    setDetailOpen(true);
  }, []);

  const handleCloseDetail = useCallback(() => {
    setDetailOpen(false);
    setSelectedName(null);
  }, []);

  const handleUpdateSkill = useCallback((name: string) => {
    setUpdateSkillName(name);
    setUpdateMode("single");
  }, []);

  const handleUninstallSkill = useCallback(
    (name: string) => {
      const target = view.skills.find((entry) => entry.name === name);
      if (target) setUninstallSkill(target);
    },
    [view.skills],
  );

  const handleSourceChange = (value: string) => {
    setSource(value);
    setFiltersOpen(false);
  };

  const handleAgentChange = (value: string) => {
    setAgent(value);
    setFiltersOpen(false);
  };

  const handleStatusChange = (value: StatusFilter) => {
    setStatus(value);
    setFiltersOpen(false);
  };

  return (
    <div className="app-shell text-foreground">
      <AppHeader
        report={report}
        isLoading={environment.isPending}
        isError={environment.isError}
        mutationsEnabled={mutationsEnabled}
        onInstall={() => setInstallOpen(true)}
        updateReport={updateData}
        lastCheck={view.lastCheck}
        updateError={updateError}
        checkingUpdates={checkUpdates.isPending}
        updatesEnabled={!scopeBlocked}
        onCheckUpdates={() => handleCheckUpdates()}
        onUpdateAll={() => {
          setUpdateSkillName(null);
          setUpdateMode("all");
        }}
        updateCandidateCount={view.candidateCount}
        filtersOpen={filtersOpen}
        onToggleFilters={() => setFiltersOpen((open) => !open)}
      />
      <InterruptedBanner
        report={report?.interrupted ?? null}
        scope={scope}
        onDismiss={handleDismissInterrupted}
        onRevealBackups={handleRevealBackups}
      />
      {lockError ? (
        <LockErrorScreen
          error={lockError}
          onRetry={handleRetryInventory}
          retrying={recheck.isPending || inventoryQuery.isFetching}
        />
      ) : (
        <>
          <EnvironmentBanner
            report={report}
            isLoading={environment.isPending}
            error={environment.error}
            rechecking={recheck.isPending}
            onRecheck={handleRecheck}
          />
          <main className="app-body">
            <div
              className="rail-wrap"
              data-open={filtersOpen ? "true" : "false"}
            >
              <FilterSidebar
                sources={view.sourceOptions}
                agents={view.agentOptions}
                statusCounts={view.statusCounts}
                activeSource={source}
                activeAgent={agent}
                activeStatus={status}
                onSourceChange={handleSourceChange}
                onAgentChange={handleAgentChange}
                onStatusChange={handleStatusChange}
                report={report}
                envLoading={environment.isPending}
                envError={environment.isError}
                rechecking={recheck.isPending}
                onRecheck={handleRecheck}
                onClose={() => setFiltersOpen(false)}
                onManageAgents={() => setManageAgentsOpen(true)}
                scopeSwitcher={
                  <ScopeSwitcher
                    activeScope={scope}
                    projects={view.scopeProjects}
                    warnings={projects?.warnings ?? EMPTY_WARNINGS}
                    notice={scopeNotice}
                    onSelect={handleSelectScope}
                    onAddProject={() => setProjectDialog({ mode: "add" })}
                    onRelocate={(project) =>
                      setProjectDialog({ mode: "relocate", project })
                    }
                    onForget={(project) =>
                      setProjectDialog({ mode: "forget", project })
                    }
                  />
                }
              />
            </div>
            <SkillListPane
              totalSkills={view.totalSkills}
              visibleSkills={view.visibleSkills}
              anomalies={view.anomalies}
              sourceCount={view.sourceCount}
              candidateCount={view.candidateCount}
              query={query}
              sort={sort}
              selectedName={view.selectedSkill?.name ?? null}
              isLoading={inventoryQuery.isPending}
              error={inventoryQuery.error}
              scopeError={view.scopeError}
              projectHeader={view.projectHeader}
              unavailableProject={
                view.activeProjectUnavailable ? view.activeProject : null
              }
              refreshingProject={checkUpdates.isPending}
              onRefreshProject={handleRefreshProject}
              onInstall={() => setInstallOpen(true)}
              onRelocate={(project) =>
                setProjectDialog({ mode: "relocate", project })
              }
              onForget={(project) =>
                setProjectDialog({ mode: "forget", project })
              }
              onQueryChange={setQuery}
              onSortChange={setSort}
              onSelect={handleSelectSkill}
              onClearFilters={handleClearFilters}
              onOpenAnomalies={() => setAnomaliesOpen(true)}
              onRetry={handleRetryInventory}
            />
            {detailOpen && view.selectedSkill ? (
              <DetailPane
                scope={scope}
                skill={view.selectedSkill}
                agents={snapshot?.installTargets ?? EMPTY_INVENTORY_AGENTS}
                actionsDisabled={scopeBlocked}
                checkingUpdates={checkUpdates.isPending}
                onCheckUpdates={handleCheckUpdates}
                onClose={handleCloseDetail}
                onUpdate={handleUpdateSkill}
                onUninstall={handleUninstallSkill}
              />
            ) : null}
          </main>
          {filtersOpen ? (
            <div
              className="rail-scrim"
              onClick={() => setFiltersOpen(false)}
              aria-hidden="true"
            />
          ) : null}
        </>
      )}
      <AnomalyDialog
        anomalies={view.anomalies}
        open={anomaliesOpen}
        onClose={() => setAnomaliesOpen(false)}
      />
      {view.updateDialog ? (
        <UpdateDialog
          open
          scope={scope}
          view={view.updateDialog}
          onClose={() => {
            setUpdateMode(null);
            setUpdateSkillName(null);
          }}
        />
      ) : null}
      <UninstallDialog
        open={uninstallSkill !== null}
        scope={scope}
        skill={uninstallSkill}
        onClose={() => setUninstallSkill(null)}
      />
      <ManageAgentsDialog
        open={manageAgentsOpen}
        agents={snapshot?.agents ?? EMPTY_INVENTORY_AGENTS}
        skills={view.skills}
        warnings={snapshot?.warnings ?? EMPTY_WARNINGS}
        onClose={() => setManageAgentsOpen(false)}
        onChanged={handleRecheck}
      />
      <InstallWizard
        open={installOpen}
        scope={scope}
        project={activeProject}
        inventorySkills={view.skills}
        agents={report?.agents ?? EMPTY_DETECTED_AGENTS}
        installTargets={snapshot?.installTargets ?? EMPTY_INVENTORY_AGENTS}
        onClose={() => setInstallOpen(false)}
      />
      <ProjectDialogs
        state={projectDialog}
        onClose={() => setProjectDialog(null)}
        onAdded={handleProjectAdded}
        onRelocated={handleProjectRelocated}
        onForgotten={handleProjectForgotten}
      />
      <ExitDialog
        open={exitRequest !== null}
        info={exitRequest}
        pending={quitting}
        error={exitError}
        onCancel={() => {
          if (!quitting) {
            setExitRequest(null);
            setExitError(null);
          }
        }}
        onConfirm={() => void handleConfirmExit()}
      />
    </div>
  );
}

export default App;
