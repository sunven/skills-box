import { RotateCcw, X } from "lucide-react";
import { Button } from "./ui/button";
import { EnvironmentBadge } from "./environment-badge";
import { EnvironmentMetrics } from "./environment-metrics";
import { cn } from "../lib/utils";
import type { SourceOption } from "../lib/inventory-view";
import type { EnvReport } from "../lib/types";

interface FilterSidebarProps {
  sources: SourceOption[];
  totalSkills: number;
  activeSource: string;
  onSourceChange: (value: string) => void;
  report?: EnvReport;
  envLoading: boolean;
  envError: boolean;
  rechecking: boolean;
  onRecheck: () => void;
  onClose?: () => void;
}

interface FilterRowProps {
  label: string;
  count: number;
  active: boolean;
  onClick: () => void;
  title?: string;
}

function FilterRow({
  label,
  count,
  active,
  onClick,
  title,
}: FilterRowProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={active}
      title={title}
      className="filter-row"
    >
      <span className="filter-name">{label}</span>
      <span className="filter-count">{count}</span>
    </button>
  );
}

export function FilterSidebar({
  sources,
  totalSkills,
  activeSource,
  onSourceChange,
  report,
  envLoading,
  envError,
  rechecking,
  onRecheck,
  onClose,
}: FilterSidebarProps) {
  return (
    <aside aria-label="筛选" className="rail">
      <div className="rail-head">
        <h2 className="rail-title">筛选</h2>
        {onClose ? (
          <Button
            variant="ghost"
            size="icon"
            className="rail-close size-7"
            aria-label="关闭筛选"
            onClick={onClose}
          >
            <X className="size-4" aria-hidden="true" />
          </Button>
        ) : null}
      </div>

      <div className="rail-scroll">
        <section aria-label="来源" className="filter-group">
          <h3 className="filter-label">来源</h3>
          <FilterRow
            label="全部来源"
            count={totalSkills}
            active={activeSource === "all"}
            onClick={() => onSourceChange("all")}
          />
          {sources.length === 0 ? (
            <p className="filter-note">暂无来源</p>
          ) : (
            sources.map((source) => (
              <FilterRow
                key={source.value}
                label={source.label}
                count={source.count}
                active={activeSource === source.value}
                onClick={() => onSourceChange(source.value)}
                title={source.label}
              />
            ))
          )}
        </section>
      </div>

      <div className="rail-foot">
        <div className="rail-foot-head">
          <span className="rail-foot-title">运行环境</span>
          <div className="flex items-center gap-1">
            <EnvironmentBadge
              report={report}
              isLoading={envLoading}
              isError={envError}
            />
            <Button
              variant="ghost"
              size="icon"
              className="size-6"
              onClick={onRecheck}
              disabled={rechecking}
              aria-label="重新检测环境"
              title="重新检测环境"
            >
              <RotateCcw
                className={cn("size-3.5", rechecking && "animate-spin")}
                aria-hidden="true"
              />
            </Button>
          </div>
        </div>
        <EnvironmentMetrics report={report} />
      </div>
    </aside>
  );
}
