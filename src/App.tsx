import { useEffect, useMemo, useState } from "react";
import { AnomalyDialog } from "./components/anomaly-panel";
import { AppHeader } from "./components/app-header";
import { DetailPane } from "./components/detail-pane";
import { EnvironmentBanner } from "./components/environment-banner";
import { EnvironmentMetrics } from "./components/environment-metrics";
import { FilterSidebar } from "./components/filter-sidebar";
import { InstallWizard } from "./components/install-wizard";
import { LockErrorScreen } from "./components/lock-error-screen";
import { SkillListPane } from "./components/skill-list-pane";
import { UpdateAllDialog } from "./components/update-all-dialog";
import { useEnvironmentQuery, useRecheckEnvironment } from "./hooks/use-environment";
import { useInventoryQuery } from "./hooks/use-inventory";
import { useCheckUpdates, useUpdateReportQuery } from "./hooks/use-updates";
import { asAppError } from "./lib/api";
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
} from "./lib/update-view";
import type { AnomalySummary } from "./lib/types";

const EMPTY_ANOMALIES: AnomalySummary = {
  total: 0,
  stale: { count: 0, examples: [] },
  unmanaged: { count: 0, examples: [] },
  broken: { count: 0, examples: [] },
  modified: { count: 0, examples: [] },
};

function App() {
  const environment = useEnvironmentQuery();
  const recheck = useRecheckEnvironment();
  const inventoryQuery = useInventoryQuery();
  const updateReportQuery = useUpdateReportQuery();
  const checkUpdates = useCheckUpdates();
  const report = environment.data;
  const snapshot = inventoryQuery.data;
  const updateData = updateReportQuery.data ?? null;

  const [query, setQuery] = useState("");
  const [source, setSource] = useState("all");
  const [agent, setAgent] = useState("all");
  const [status, setStatus] = useState<StatusFilter>("all");
  const [sort, setSort] = useState<SortKey>("recent");
  const [selectedName, setSelectedName] = useState<string | null>(null);
  const [anomaliesOpen, setAnomaliesOpen] = useState(false);
  const [installOpen, setInstallOpen] = useState(false);
  const [updateAllOpen, setUpdateAllOpen] = useState(false);

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

  useEffect(() => {
    if (selectedName === null) return;
    if (!visibleSkills.some((skill) => skill.name === selectedName)) {
      setSelectedName(visibleSkills[0]?.name ?? null);
    }
  }, [visibleSkills, selectedName]);

  const inventoryError = asAppError(inventoryQuery.error);
  const updateError = asAppError(checkUpdates.error);
  const lockError =
    inventoryError && inventoryError.code?.startsWith("lock.")
      ? inventoryError
      : null;
  const mutationsEnabled = (report?.mutationsEnabled ?? false) && !lockError;

  const handleRecheck = () => {
    recheck.mutate();
  };

  const handleCheckUpdates = (skillIds?: string[]) => {
    checkUpdates.mutate(skillIds);
  };

  const handleRetryInventory = () => {
    recheck.mutate();
    void inventoryQuery.refetch();
  };

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
        onUpdateAll={() => setUpdateAllOpen(true)}
        updateCandidateCount={updateSummary.candidates.length}
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
            />
          </main>
        </>
      )}
      <AnomalyDialog
        anomalies={anomalies}
        open={anomaliesOpen}
        onClose={() => setAnomaliesOpen(false)}
      />
      <UpdateAllDialog
        open={updateAllOpen}
        skills={skills}
        onClose={() => setUpdateAllOpen(false)}
      />
      <InstallWizard
        open={installOpen}
        inventorySkills={skills}
        agents={report?.agents ?? []}
        onClose={() => setInstallOpen(false)}
      />
    </div>
  );
}

export default App;
