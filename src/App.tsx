import { useEffect, useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { AnomalyDialog } from "./components/anomaly-panel";
import { AppHeader } from "./components/app-header";
import { DetailPane } from "./components/detail-pane";
import { EnvironmentBanner } from "./components/environment-banner";
import { EnvironmentMetrics } from "./components/environment-metrics";
import { ExitDialog } from "./components/exit-dialog";
import { FilterSidebar } from "./components/filter-sidebar";
import { InstallWizard } from "./components/install-wizard";
import { InterruptedBanner } from "./components/interrupted-banner";
import { LockErrorScreen } from "./components/lock-error-screen";
import { SkillListPane } from "./components/skill-list-pane";
import { UninstallDialog } from "./components/uninstall-dialog";
import { UpdateDialog } from "./components/update-dialog";
import { environmentQueryKey, useEnvironmentQuery, useRecheckEnvironment } from "./hooks/use-environment";
import { useInventoryQuery } from "./hooks/use-inventory";
import { useCheckUpdates, useUpdateReportQuery } from "./hooks/use-updates";
import {
  asAppError,
  dismissInterruptedMutation,
  quitApp,
  revealAppData,
} from "./lib/api";
import { listenForExitRequest } from "./lib/lifecycle";
import {
  computeAgentOptions,
  computeSourceOptions,
  computeStatusCounts,
  defaultFilters,
  selectVisibleSkills,
  type InventoryFilters,
  type SortKey,
  type StatusFilter,
} from "./lib/inventory-view";
import {
  applyUpdateReport,
  computeUpdateSummary,
  withUpdateAnomalies,
  type UpdateViewSkill,
} from "./lib/update-view";
import type { ActiveMutationInfo, AnomalySummary, EnvReport } from "./lib/types";

const EMPTY_ANOMALIES: AnomalySummary = {
  total: 0,
  stale: { count: 0, examples: [] },
  unmanaged: { count: 0, examples: [] },
  broken: { count: 0, examples: [] },
  modified: { count: 0, examples: [] },
};

function App() {
  const queryClient = useQueryClient();
  const environment = useEnvironmentQuery();
  const recheck = useRecheckEnvironment();
  const inventoryQuery = useInventoryQuery();
  const updateReportQuery = useUpdateReportQuery();
  const checkUpdates = useCheckUpdates();
  const report = environment.data;
  const snapshot = inventoryQuery.data;
  const updateData = updateReportQuery.data ?? null;

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
  const [anomaliesOpen, setAnomaliesOpen] = useState(false);
  const [installOpen, setInstallOpen] = useState(false);
  const [updateMode, setUpdateMode] = useState<"single" | "all" | null>(null);
  const [updateSkillName, setUpdateSkillName] = useState<string | null>(null);
  // Keep the uninstall target stable across the inventory refresh that follows
  // a successful removal; the row itself may already be gone from `skills`.
  const [uninstallSkill, setUninstallSkill] = useState<UpdateViewSkill | null>(
    null,
  );

  const skills = useMemo(
    () => applyUpdateReport(snapshot?.skills ?? [], updateData),
    [snapshot, updateData],
  );
  const anomalies = useMemo(
    () => withUpdateAnomalies(snapshot?.anomalies ?? EMPTY_ANOMALIES, skills),
    [snapshot, skills],
  );
  const updateSummary = useMemo(() => computeUpdateSummary(skills), [skills]);
  const filters = useMemo<InventoryFilters>(
    () => ({ query, source, agent, status }),
    [query, source, agent, status],
  );
  const visibleSkills = useMemo(
    () => selectVisibleSkills(skills, filters, sort),
    [skills, filters, sort],
  );
  const statusCounts = useMemo(() => computeStatusCounts(skills), [skills]);
  const sourceOptions = useMemo(() => computeSourceOptions(skills), [skills]);
  const agentOptions = useMemo(
    () => computeAgentOptions(skills, snapshot?.agents ?? []),
    [skills, snapshot],
  );
  const selectedSkill = useMemo(
    () =>
      visibleSkills.find((skill) => skill.name === selectedName) ?? null,
    [visibleSkills, selectedName],
  );
  const updateDialogSkills = useMemo(() => {
    if (updateMode === "single") {
      const skill = skills.find((entry) => entry.name === updateSkillName);
      return skill ? [skill] : [];
    }
    return skills;
  }, [skills, updateMode, updateSkillName]);

  useEffect(() => {
    if (selectedName === null) return;
    if (!visibleSkills.some((skill) => skill.name === selectedName)) {
      setSelectedName(visibleSkills[0]?.name ?? null);
    }
  }, [visibleSkills, selectedName]);

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
        (asAppError(error) ?? { message: "无法退出应用。" }).message,
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
    inventoryError && inventoryError.code?.startsWith("lock.")
      ? inventoryError
      : null;
  const mutationsEnabled = (report?.mutationsEnabled ?? false) && !lockError;

  // A manual environment refresh is also a full read-model refresh: the
  // inventory may have changed under us, and cached detail/update results are
  // no longer trustworthy.
  const handleRecheck = () => {
    recheck.mutate();
    void inventoryQuery.refetch();
    void queryClient.invalidateQueries({ queryKey: ["skill"] });
    void queryClient.invalidateQueries({ queryKey: ["updateReport"] });
  };

  const handleCheckUpdates = (skillIds?: string[]) => {
    checkUpdates.mutate(skillIds);
  };

  const handleRetryInventory = handleRecheck;

  const handleClearFilters = () => {
    const defaults = defaultFilters();
    setQuery(defaults.query);
    setSource(defaults.source);
    setAgent(defaults.agent);
    setStatus(defaults.status);
  };

  return (
    <div className="flex h-screen min-w-[1080px] flex-col overflow-hidden bg-background text-foreground">
      <AppHeader
        report={report}
        isLoading={environment.isPending}
        isError={environment.isError}
        mutationsEnabled={mutationsEnabled}
        rechecking={recheck.isPending}
        onRecheck={handleRecheck}
        onInstall={() => setInstallOpen(true)}
        updateReport={updateData}
        updateError={updateError}
        checkingUpdates={checkUpdates.isPending}
        updatesEnabled={!lockError}
        onCheckUpdates={() => handleCheckUpdates()}
        onUpdateAll={() => {
          setUpdateSkillName(null);
          setUpdateMode("all");
        }}
        updateCandidateCount={updateSummary.candidates.length}
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
          <EnvironmentMetrics report={report} />
          <EnvironmentBanner
            report={report}
            isLoading={environment.isPending}
            error={environment.error}
            rechecking={recheck.isPending}
            onRecheck={handleRecheck}
          />
          <main className="grid min-h-0 flex-1 grid-cols-[220px_minmax(0,1fr)_340px]">
            <FilterSidebar
              sources={sourceOptions}
              agents={agentOptions}
              statusCounts={statusCounts}
              activeSource={source}
              activeAgent={agent}
              activeStatus={status}
              onSourceChange={setSource}
              onAgentChange={setAgent}
              onStatusChange={setStatus}
            />
            <SkillListPane
              totalSkills={skills.length}
              visibleSkills={visibleSkills}
              anomalies={anomalies}
              query={query}
              sort={sort}
              selectedName={selectedSkill?.name ?? null}
              isLoading={inventoryQuery.isPending}
              error={inventoryQuery.error}
              onQueryChange={setQuery}
              onSortChange={setSort}
              onSelect={setSelectedName}
              onClearFilters={handleClearFilters}
              onOpenAnomalies={() => setAnomaliesOpen(true)}
              onRetry={handleRetryInventory}
            />
            <DetailPane
              skill={selectedSkill}
              updateResult={selectedSkill?.updateResult}
              checkingUpdates={checkUpdates.isPending}
              onCheckUpdates={handleCheckUpdates}
              onUpdate={(name) => {
                setUpdateSkillName(name);
                setUpdateMode("single");
              }}
              onUninstall={(name) => {
                const target = skills.find((entry) => entry.name === name);
                if (target) setUninstallSkill(target);
              }}
            />
          </main>
        </>
      )}
      <AnomalyDialog
        anomalies={anomalies}
        open={anomaliesOpen}
        onClose={() => setAnomaliesOpen(false)}
      />
      <UpdateDialog
        open={updateMode !== null}
        mode={updateMode ?? "all"}
        skills={updateDialogSkills}
        agents={snapshot?.agents ?? []}
        onClose={() => {
          setUpdateMode(null);
          setUpdateSkillName(null);
        }}
      />
      <UninstallDialog
        open={uninstallSkill !== null}
        skill={uninstallSkill}
        onClose={() => setUninstallSkill(null)}
      />
      <InstallWizard
        open={installOpen}
        inventorySkills={skills}
        agents={report?.agents ?? []}
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
