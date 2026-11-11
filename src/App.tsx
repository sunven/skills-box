import { ExpertPackProvider } from "./components/expert-pack-context";
import { MarketplacePage } from "./components/marketplace-page";
import { BatchDialog, type BatchSelection } from "./components/batch-dialog";
import { useCliCommandSubscription } from "./hooks/use-cli-commands";
import { DiscoveryPage } from "./components/discovery-page";
import { useDiscovery } from "./hooks/use-discovery";
import type { DiscoveryFilter, DiscoveryInstallSeed } from "./lib/discovery";
import {
  useCallback,
  useDeferredValue,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useQueryClient } from "@tanstack/react-query";
import { StaleRecordsDialog } from "./components/stale-records-dialog";
import { AnomalyDialog } from "./components/anomaly-panel";
import { AppHeader } from "./components/app-header";
import { DetailPane, type DetailEntry } from "./components/detail-pane";
import { EnvironmentBanner } from "./components/environment-banner";
import { ExitDialog } from "./components/exit-dialog";
import { FilterSidebar } from "./components/filter-sidebar";
import { InstallWizard } from "./components/install-wizard";
import { InterruptedBanner } from "./components/interrupted-banner";
import { LockErrorScreen } from "./components/lock-error-screen";
import {
  ProjectDialogs,
  type ProjectDialogState,
} from "./components/project-dialogs";
import { ScopeSwitcher } from "./components/scope-switcher";
import { SettingsPage, type SettingsSection } from "./components/settings-page";
import { AppUpdateReminder } from "./components/app-update-settings";
import { useAppUpdate } from "./hooks/use-app-update";
import { SkillListPane } from "./components/skill-list-pane";
import { UninstallDialog } from "./components/uninstall-dialog";
import { UpdateDialog } from "./components/update-dialog";
import { useAutoUpdateCheck } from "./hooks/use-auto-update-check";
import {
  environmentQueryKey,
  useEnvironmentQuery,
  useRecheckEnvironment,
} from "./hooks/use-environment";
import { useInventoryQuery } from "./hooks/use-inventory";
import { skillQueryPrefix } from "./hooks/use-skill-detail";
import { useProjectsQuery, useScopeCheckStates } from "./hooks/use-projects";
import { projectsQueryKey } from "./lib/scope";
import {
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
import {
  defaultFilters,
  type SortKey,
  type StatusFilter,
} from "./lib/inventory-view";
import {
  projectForScope,
  projectScope,
  resolveInitialScope,
} from "./lib/scope";
import { updateReportQueryKey } from "./lib/update-view";
import { createWorkspaceViewSelector } from "./lib/workspace-view";
import { GLOBAL_SCOPE } from "./lib/types";
import type {
  ActiveMutationInfo,
  AppError,
  DetectedAgent,
  EnvReport,
  InventoryAgent,
  ProjectEntry,
  ScopeId,
  UpdateViewSkill,
} from "./lib/types";

const EMPTY_INVENTORY_AGENTS: InventoryAgent[] = [];
const EMPTY_DETECTED_AGENTS: DetectedAgent[] = [];
const EMPTY_PROJECTS: ProjectEntry[] = [];
const EMPTY_WARNINGS: string[] = [];

function App() {
  useCliCommandSubscription();
  const queryClient = useQueryClient();
  const environment = useEnvironmentQuery();
  const recheck = useRecheckEnvironment();
  const projectsQuery = useProjectsQuery();
  const [activeScope, setActiveScope] = useState<ScopeId | null>(null);
  const [projectDialog, setProjectDialog] = useState<ProjectDialogState>(null);
  const [batchSelection, setBatchSelection] = useState<
    (BatchSelection & { scope: ScopeId }) | null
  >(null);
  const [staleRecordsOpen, setStaleRecordsOpen] = useState(false);
  const [batchRunning, setBatchRunning] = useState(false);
  const batchRunningRef = useRef(false);
  const [batchSucceededIds, setBatchSucceededIds] = useState<Set<string>>(
    () => new Set(),
  );
  const handleBatchRunningChange = useCallback((running: boolean) => {
    batchRunningRef.current = running;
    setBatchRunning(running);
  }, []);
  const [scopeNotice, setScopeNotice] = useState<string | null>(null);
  const [projectWarnings, setProjectWarnings] = useState<AppError[]>([]);
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
  const inventoryQuery = useInventoryQuery(
    scope,
    scopeReady && !scopeUnavailable,
  );
  const updateReportQuery = useUpdateReportQuery(scope);
  const updateCheckStateQuery = useUpdateCheckStateQuery(scope);
  const lastFullCheckQuery = useLastFullCheckQuery(scope);
  const lastFullAttemptQuery = useLastFullAttemptQuery(scope);
  const checkUpdates = useCheckUpdates(scope, activeProject?.checkRevision);
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
  const [detailEntry, setDetailEntry] = useState<DetailEntry | null>(null);
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [anomaliesOpen, setAnomaliesOpen] = useState(false);
  const [installOpen, setInstallOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [packEntry, setPackEntry] = useState<{ slug: string; key: number } | undefined>();
  const [marketplaceOpen, setMarketplaceOpen] = useState(false);
  const [marketplaceInstallOpen, setMarketplaceInstallOpen] = useState(false);
  const [settingsSection, setSettingsSection] = useState<SettingsSection>("github");
  const appUpdate = useAppUpdate();
  const [discoveryEntry, setDiscoveryEntry] = useState<{
    filter: DiscoveryFilter;
    source: string;
  } | null>(null);
  const [installSeed, setInstallSeed] = useState<DiscoveryInstallSeed | null>(
    null,
  );
  const discovery = useDiscovery(scopeReady && !quitting);
  const [updateTarget, setUpdateTarget] = useState<
    { mode: "all" } | { mode: "single"; name: string } | null
  >(null);
  const updateMode = updateTarget?.mode ?? null;
  const updateSkillName =
    updateTarget?.mode === "single" ? updateTarget.name : null;
  // Keep the uninstall target stable across the inventory refresh that follows
  // a successful removal; the row itself may already be gone from `skills`.
  const [uninstallSkill, setUninstallSkill] = useState<UpdateViewSkill | null>(
    null,
  );

  // Filtering runs on a deferred query so typing stays urgent while the list
  // and the detail drawer re-render at a lower priority.
  const deferredQuery = useDeferredValue(query);
  const scopeError = asAppError(inventoryQuery.error);
  const [selectWorkspaceView] = useState(createWorkspaceViewSelector);
  const view = useMemo(
    () =>
      selectWorkspaceView({
        snapshot,
        discovery: discovery.query.data,
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
      selectWorkspaceView,
      snapshot,
      discovery.query.data,
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
    if (snapshot && !inventoryQuery.isError && agent !== "all"
      && !view.agentOptions.some((option) => option.id === agent)) {
      setAgent("all");
    }
  }, [snapshot, inventoryQuery.isError, view.agentOptions, agent]);

  useEffect(() => {
    if (selectedName === null) return;
    if (!view.visibleSkills.some((skill) => skill.name === selectedName)) {
      setSelectedName(view.visibleSkills[0]?.name ?? null);
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
      (previous) => (previous ? { ...previous, interrupted: null } : previous),
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
  const actionsDisabled = scopeBlocked || appUpdate.critical || batchRunning;
  const mutationsEnabled =
    (report?.mutationsEnabled ?? false) && !actionsDisabled;

  const mutateCheckUpdates = checkUpdates.mutate;
  const runAutoCheck = useCallback(async () => {
    try {
      const active = await getActiveMutation();
      if (active) return false;
    } catch {
      return false;
    }
    return mutateCheckUpdates();
  }, [mutateCheckUpdates]);

  useAutoUpdateCheck({
    scope,
    checkRevision: activeProject?.checkRevision,
    enabled:
      inventoryQuery.isSuccess && !inventoryQuery.isFetching &&
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
    void queryClient.invalidateQueries({ queryKey: skillQueryPrefix(scope) });
    void queryClient.invalidateQueries({
      queryKey: updateReportQueryKey(scope),
    });
  };

  const handleCheckUpdates = useCallback(
    (skillIds?: string[]) => {
      return mutateCheckUpdates(skillIds);
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

  const openInstall = useCallback((seed: DiscoveryInstallSeed | null = null) => {
    setInstallSeed(seed);
    setInstallOpen(true);
  }, []);

  const openRelocateProject = useCallback((project: ProjectEntry) => {
    setProjectDialog({ mode: "relocate", project });
  }, []);

  const openForgetProject = useCallback((project: ProjectEntry) => {
    setProjectDialog({ mode: "forget", project });
  }, []);

  const resetWorkspaceState = useCallback(() => {
    handleClearFilters();
    setSort("recent");
    setSelectedName(null);
    setDetailEntry(null);
    setFiltersOpen(false);
    setAnomaliesOpen(false);
    setStaleRecordsOpen(false);
    setUpdateTarget(null);
    setUninstallSkill(null);
    setBatchSelection(null);
    setBatchSucceededIds(new Set());
  }, [handleClearFilters]);

  const handleSelectScope = useCallback(
    (next: ScopeId) => {
      if (!scopeReady || next === scope || batchRunningRef.current || marketplaceInstallOpen) return;
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
    [queryClient, resetWorkspaceState, scope, scopeReady, marketplaceInstallOpen],
  );

  const handleProjectAdded = useCallback(
    (entry: ProjectEntry) => {
      resetWorkspaceState();
      setProjectDialog(null);
      setActiveScope(projectScope(entry.id));
    },
    [resetWorkspaceState],
  );

  const handleProjectRelocated = useCallback(
    (entry: ProjectEntry, warnings: AppError[]) => {
      resetWorkspaceState();
      setProjectDialog(null);
      setProjectWarnings(warnings);
      setActiveScope(projectScope(entry.id));
    },
    [resetWorkspaceState],
  );

  const handleProjectForgotten = useCallback(
    (projectId: string, warnings: AppError[]) => {
      setProjectDialog(null);
      setProjectWarnings(warnings);
      if (scope === projectScope(projectId)) {
        resetWorkspaceState();
        setActiveScope(GLOBAL_SCOPE);
      }
    },
    [resetWorkspaceState, scope],
  );

  const handleSelectSkill = useCallback((name: string) => {
    setSelectedName(name);
    setDetailEntry(null);
  }, []);

  const handleOpenSkillDetail = useCallback(
    (name: string, destination: "content" | "attachments") => {
    setSelectedName(name);
    setDetailEntry({ destination });
  },
    [],
  );

  const handleCloseDetail = useCallback(() => {
    setSelectedName(null);
    setDetailEntry(null);
  }, []);

  const handleUpdateSkill = useCallback((name: string) => {
    setUpdateTarget({ mode: "single", name });
  }, []);

  const handleUninstallSkill = useCallback(
    (name: string) => {
      const target = view.skills.find((entry) => entry.name === name);
      if (target) setUninstallSkill(target);
    },
    [view.skills],
  );

  const handleSourceChange = (value: string) => {
    if (batchRunningRef.current) return;
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
    <ExpertPackProvider scope={scope} onOpen={(slug) => { setPackEntry({ slug, key: Date.now() }); setMarketplaceOpen(true); setSettingsOpen(false); setDiscoveryEntry(null); setDetailEntry(null); }}>
    <div className="app-shell text-foreground">
      {appUpdate.critical ? (
        <p className="notice" role="status">正在安装应用更新并准备重启，暂时无法关闭应用或开始新的写入。</p>
      ) : null}
      <AppHeader
        inert={marketplaceInstallOpen}
        settingsOpen={settingsOpen}
        marketplaceOpen={marketplaceOpen}
        onOpenMarketplace={() => { setPackEntry(undefined); setMarketplaceOpen(true); setSettingsOpen(false); setDiscoveryEntry(null); }}
        discoveryOpen={discoveryEntry !== null}
        discoveryCount={discovery.query.data?.unreadCount ?? 0}
        onOpenDiscovery={() => { setMarketplaceOpen(false); setSettingsOpen(false); setDiscoveryEntry({ filter: "uninstalled", source: "all" }); }}
        onOpenSettings={() => { setMarketplaceOpen(false); setSettingsSection("github"); setSettingsOpen(true); setDiscoveryEntry(null); }}
        onReturnToSkills={() => { setMarketplaceOpen(false); setSettingsOpen(false); setDiscoveryEntry(null); }}
        report={report}
        isLoading={environment.isPending}
        isError={environment.isError}
        mutationsEnabled={mutationsEnabled}
        onInstall={() => openInstall()}
        updateReport={updateData}
        lastCheck={view.lastCheck}
        updateError={updateError}
        checkingUpdates={checkUpdates.isPending}
        updatesEnabled={!scopeBlocked}
        onCheckUpdates={() => handleCheckUpdates()}
        onUpdateAll={() => {
          if (!batchRunningRef.current) setUpdateTarget({ mode: "all" });
        }}
        updateCandidateCount={view.candidateCount}
        filtersOpen={filtersOpen}
        onToggleFilters={() => setFiltersOpen((open) => !open)}
        scopeSwitcher={
          settingsOpen || discoveryEntry ? null : (
            <ScopeSwitcher
              disabled={appUpdate.critical || batchRunning || marketplaceInstallOpen}
              activeScope={scope}
              projects={view.scopeProjects}
              warnings={projects?.warnings ?? EMPTY_WARNINGS}
              notice={scopeNotice}
              onSelect={handleSelectScope}
              onAddProject={() => setProjectDialog({ mode: "add" })}
              onRelocate={openRelocateProject}
              onForget={openForgetProject}
            />
          )
        }
      />
      {settingsOpen ? (
        <SettingsPage onAgentsChanged={handleRecheck} appUpdate={appUpdate} initialSection={settingsSection} />
      ) : marketplaceOpen ? (
        <MarketplacePage
          key={packEntry?.key ?? "marketplace"}
          initialPack={packEntry}
          scope={scope}
          scopeLabel={scope === GLOBAL_SCOPE ? "全局" : `项目 · ${activeProject?.displayName ?? "当前项目"}`}
          snapshot={snapshot}
          disabled={actionsDisabled || !snapshot}
          onInstallOpenChange={setMarketplaceInstallOpen}
          onView={(name) => { setMarketplaceOpen(false); handleClearFilters(); handleSelectSkill(name); }}
        />
      ) : discoveryEntry ? (
        <DiscoveryPage discovery={discovery} scope={scope}
        initialFilter={discoveryEntry.filter} initialSource={discoveryEntry.source}
        projects={projects?.projects ?? EMPTY_PROJECTS} mutationsEnabled={mutationsEnabled}
        writesDisabled={appUpdate.critical}
        onSelectScope={handleSelectScope} onInstall={openInstall} />
      ) : (
        <>
          {projectWarnings.length > 0 ? (
        <div
          className="notice mx-4 my-2 flex items-center justify-between gap-3"
          data-tone="warning"
          role="status"
          data-testid="project-change-warning"
        >
          <div>
            {projectWarnings.map((warning, index) => (
              <p key={index}>{warning.message} {warning.hint}</p>
            ))}
          </div>
          <button
            type="button"
            aria-label="关闭项目变更提示"
            onClick={() => setProjectWarnings([])}
          >
            关闭
          </button>
        </div>
      ) : null}
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
                totalSkills={view.totalSkills}
                activeSource={source}
                onSourceChange={handleSourceChange}
                report={report}
                envLoading={environment.isPending}
                envError={environment.isError}
                rechecking={recheck.isPending}
                onRecheck={handleRecheck}
                onClose={() => setFiltersOpen(false)}
              />
            </div>
                <SkillListPane
                  key={`${scope}:${activeProject?.checkRevision ?? "initial"}`}
                  scope={scope}
                  checkRevision={
                    scope === GLOBAL_SCOPE
                      ? undefined
                      : (activeProject?.checkRevision ?? "initial")
                  }
                  actionsDisabled={actionsDisabled}
                  batchRunning={batchRunning}
                  batchMutationsEnabled={mutationsEnabled}
                  batchSucceededIds={batchSucceededIds}
                  onBatch={(selection) => {
                    if (!batchRunningRef.current) {
                      setBatchSucceededIds(new Set());
                      setBatchSelection({ ...selection, scope });
                    }
                  }}
                  checkingUpdates={checkUpdates.isPending}
                  checkError={checkUpdates.error}
                  installTargets={
                    snapshot?.installTargets ?? EMPTY_INVENTORY_AGENTS
                  }
                  onCheckUpdates={handleCheckUpdates}
                  onOpenDetail={handleOpenSkillDetail}
                  onUpdate={handleUpdateSkill}
                  onUninstall={handleUninstallSkill}
                  totalSkills={view.totalSkills}
                  visibleSkills={view.visibleSkills}
                  anomalies={view.anomalies}
                  summary={view.listSummary}
                  availableDiscoveryCount={view.availableDiscoveryCount}
                  onOpenAvailableDiscoveries={() =>
                    setDiscoveryEntry({
                      filter: "available",
                      source: view.discoverySource,
                    })
                  }
                  query={query}
                  sort={sort}
                  status={status}
                  statusCounts={view.statusCounts}
                  agent={agent}
                  agents={view.agentOptions}
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
                  onInstall={() => openInstall()}
                  onRelocate={openRelocateProject}
                  onForget={openForgetProject}
                  onQueryChange={setQuery}
                  onSortChange={setSort}
                  onStatusChange={handleStatusChange}
                  onAgentChange={handleAgentChange}
                  onSelect={handleSelectSkill}
                  onClearFilters={handleClearFilters}
                  onOpenAnomalies={() => setAnomaliesOpen(true)}
                  onRetry={handleRetryInventory}
                />
                {view.selectedSkill ? (
                  <DetailPane
                    scope={scope}
                    skill={view.selectedSkill}
                    entry={detailEntry}
                    agents={snapshot?.installTargets ?? EMPTY_INVENTORY_AGENTS}
                    actionsDisabled={actionsDisabled}
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
        </>
      )}
      <AnomalyDialog
        anomalies={view.anomalies}
        open={anomaliesOpen}
        disabled={actionsDisabled}
        onProcessStale={() => { setAnomaliesOpen(false); setStaleRecordsOpen(true); }}
        onClose={() => setAnomaliesOpen(false)}
      />
      {staleRecordsOpen ? <StaleRecordsDialog scope={scope} scopeLabel={activeProject?.displayName ?? "全局"} disabled={actionsDisabled}
        onClose={() => setStaleRecordsOpen(false)}
        onSelect={(selection) => { setStaleRecordsOpen(false); setBatchSucceededIds(new Set()); setBatchSelection({ ...selection, scope }); }} /> : null}
      {!appUpdate.critical && view.updateDialog ? (
        <UpdateDialog
          open
          scope={scope}
          view={view.updateDialog}
          onClose={() => setUpdateTarget(null)}
        />
      ) : null}
      {batchSelection ? (
        <BatchDialog
          key={`${batchSelection.scope}:${batchSelection.action}`}
          selection={batchSelection}
          scope={batchSelection.scope}
          scopeLabel={activeProject?.displayName ?? "全局"}
          agents={
            (batchSelection.action === "detach" || batchSelection.action === "uninstall"
              ? snapshot?.agents
              : snapshot?.installTargets) ?? EMPTY_INVENTORY_AGENTS
          }
          disabled={scopeBlocked || appUpdate.critical}
          projects={projects?.projects ?? EMPTY_PROJECTS}
          onViewProject={(target) => handleSelectScope(target)}
          onRunningChange={handleBatchRunningChange}
          onCompleted={setBatchSucceededIds}
          onClose={() => {
            if (!batchRunningRef.current) {
              setBatchSelection(null);
              setBatchSucceededIds(new Set());
            }
          }}
        />
      ) : null}
      <UninstallDialog
        open={!appUpdate.critical && uninstallSkill !== null}
        scope={scope}
        skill={uninstallSkill}
        onClose={() => setUninstallSkill(null)}
      />
      <InstallWizard
        open={!appUpdate.critical && installOpen}
        seed={installSeed}
        scope={scope}
        project={activeProject}
        projects={projects?.projects ?? EMPTY_PROJECTS}
        inventorySkills={view.skills}
        agents={report?.agents ?? EMPTY_DETECTED_AGENTS}
        installTargets={snapshot?.installTargets ?? EMPTY_INVENTORY_AGENTS}
        onSelectScope={handleSelectScope}
        onClose={() => {
          setInstallOpen(false);
          setInstallSeed(null);
        }}
      />
      <ProjectDialogs
        state={appUpdate.critical ? null : projectDialog}
        onClose={() => setProjectDialog(null)}
        onAdded={handleProjectAdded}
        onRelocated={handleProjectRelocated}
        onForgotten={handleProjectForgotten}
      />
      <ExitDialog
        open={!appUpdate.critical && exitRequest !== null}
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
      <AppUpdateReminder
        update={appUpdate}
        onView={() => {
          setSettingsSection("updates");
          setSettingsOpen(true);
          setDiscoveryEntry(null);
          appUpdate.dismiss();
        }}
      />
    </div>
    </ExpertPackProvider>
  );
}

export default App;
