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
import { SkillListPane } from "./components/skill-list-pane";
import { UninstallDialog } from "./components/uninstall-dialog";
import { UpdateDialog } from "./components/update-dialog";
import { useAutoUpdateCheck } from "./hooks/use-auto-update-check";
import { environmentQueryKey, useEnvironmentQuery, useRecheckEnvironment } from "./hooks/use-environment";
import { useInventoryQuery } from "./hooks/use-inventory";
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
} from "./lib/api";
import { listenForExitRequest } from "./lib/lifecycle";
import { defaultFilters, type SortKey, type StatusFilter } from "./lib/inventory-view";
import { buildWorkspaceView } from "./lib/workspace-view";
import type {
  ActiveMutationInfo,
  DetectedAgent,
  EnvReport,
  InventoryAgent,
  UpdateViewSkill,
} from "./lib/types";

const EMPTY_INVENTORY_AGENTS: InventoryAgent[] = [];
const EMPTY_DETECTED_AGENTS: DetectedAgent[] = [];
const EMPTY_WARNINGS: string[] = [];

function App() {
  const queryClient = useQueryClient();
  const environment = useEnvironmentQuery();
  const recheck = useRecheckEnvironment();
  const inventoryQuery = useInventoryQuery();
  const updateReportQuery = useUpdateReportQuery();
  const updateCheckStateQuery = useUpdateCheckStateQuery();
  const lastFullCheckQuery = useLastFullCheckQuery();
  const lastFullAttemptQuery = useLastFullAttemptQuery();
  const checkUpdates = useCheckUpdates();
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
  const view = useMemo(
    () =>
      buildWorkspaceView({
        snapshot,
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

  const inventoryError = asAppError(inventoryQuery.error);
  const updateError = asAppError(checkUpdates.error);
  const lockError =
    isLockError(inventoryError)
      ? inventoryError
      : null;
  const mutationsEnabled = (report?.mutationsEnabled ?? false) && !lockError;

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
    enabled: inventoryQuery.isSuccess && !lockError && view.hasManagedSkills,
    checking: checkUpdates.isPending,
    lastFullAttemptAt: lastFullAttemptQuery.data,
    onCheck: runAutoCheck,
  });

  // A manual environment refresh is also a full read-model refresh: the
  // inventory may have changed under us, and cached detail/update results are
  // no longer trustworthy.
  const handleRecheck = () => {
    recheck.mutate();
    void inventoryQuery.refetch();
    void queryClient.invalidateQueries({ queryKey: ["skill"] });
    void queryClient.invalidateQueries({ queryKey: ["updateReport"] });
  };

  const handleCheckUpdates = useCallback(
    (skillIds?: string[]) => {
      mutateCheckUpdates(skillIds);
    },
    [mutateCheckUpdates],
  );

  const handleRetryInventory = handleRecheck;

  const handleClearFilters = useCallback(() => {
    const defaults = defaultFilters();
    setQuery(defaults.query);
    setSource(defaults.source);
    setAgent(defaults.agent);
    setStatus(defaults.status);
  }, []);

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
        updatesEnabled={!lockError}
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
              onQueryChange={setQuery}
              onSortChange={setSort}
              onSelect={handleSelectSkill}
              onClearFilters={handleClearFilters}
              onOpenAnomalies={() => setAnomaliesOpen(true)}
              onRetry={handleRetryInventory}
            />
            {detailOpen && view.selectedSkill ? (
              <DetailPane
                skill={view.selectedSkill}
                agents={snapshot?.agents ?? EMPTY_INVENTORY_AGENTS}
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
          view={view.updateDialog}
            onClose={() => {
            setUpdateMode(null);
            setUpdateSkillName(null);
          }}
        />
      ) : null}
      <UninstallDialog
        open={uninstallSkill !== null}
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
        inventorySkills={view.skills}
        agents={report?.agents ?? EMPTY_DETECTED_AGENTS}
        onClose={() => setInstallOpen(false)}
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
