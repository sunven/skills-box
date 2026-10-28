import { AppHeader } from "./components/app-header";
import { DetailPane } from "./components/detail-pane";
import { EnvironmentBanner } from "./components/environment-banner";
import { EnvironmentMetrics } from "./components/environment-metrics";
import { FilterSidebar } from "./components/filter-sidebar";
import { SkillListPane } from "./components/skill-list-pane";
import { useEnvironmentQuery, useRecheckEnvironment } from "./hooks/use-environment";

function App() {
  const environment = useEnvironmentQuery();
  const recheck = useRecheckEnvironment();
  const report = environment.data;
  const mutationsEnabled = report?.mutationsEnabled ?? false;

  const handleRecheck = () => {
    recheck.mutate();
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
      />
      <EnvironmentMetrics report={report} />
      <EnvironmentBanner
        report={report}
        isLoading={environment.isPending}
        error={environment.error}
        rechecking={recheck.isPending}
        onRecheck={handleRecheck}
      />
      <main className="grid min-h-0 flex-1 grid-cols-[220px_minmax(0,1fr)_340px]">
        <FilterSidebar agents={report?.agents ?? []} />
        <SkillListPane />
        <DetailPane />
      </main>
    </div>
  );
}

export default App;
