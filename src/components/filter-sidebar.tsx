import type { ReactNode } from "react";
import { RotateCcw, X } from "lucide-react";
import { Button } from "./ui/button";
import { AgentLogo } from "./agent-logo";
import { EnvironmentBadge } from "./environment-badge";
import { EnvironmentMetrics } from "./environment-metrics";
import { cn } from "../lib/utils";
import {
  STATUS_FILTERS,
  type AgentOption,
  type SourceOption,
  type StatusCounts,
  type StatusFilter,
} from "../lib/inventory-view";
import type { EnvReport } from "../lib/types";

interface FilterSidebarProps {
  sources: SourceOption[];
  agents: AgentOption[];
  statusCounts: StatusCounts;
  activeSource: string;
  activeAgent: string;
  activeStatus: StatusFilter;
  onSourceChange: (value: string) => void;
  onAgentChange: (value: string) => void;
  onStatusChange: (value: StatusFilter) => void;
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
  countTone?: "accent";
  leading?: ReactNode;
  trailing?: ReactNode;
}

function FilterRow({
  label,
  count,
  active,
  onClick,
  title,
  countTone,
  leading,
  trailing,
}: FilterRowProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={active}
      title={title}
      className="filter-row"
    >
      {leading}
      <span className="filter-name">{label}</span>
      {trailing}
      <span
        className={cn(
          "filter-count",
          countTone === "accent" && count > 0 && "update-count",
        )}
      >
        {count}
      </span>
    </button>
  );
}

function SectionTitle({ children }: { children: ReactNode }) {
  return <h3 className="filter-label">{children}</h3>;
}

export function FilterSidebar({
  sources,
  agents,
  statusCounts,
  activeSource,
  activeAgent,
  activeStatus,
  onSourceChange,
  onAgentChange,
  onStatusChange,
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
          <SectionTitle>来源</SectionTitle>
          <FilterRow
            label="全部来源"
            count={statusCounts.all}
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

        <section aria-label="Agents" className="filter-group">
          <SectionTitle>Agent</SectionTitle>
          <FilterRow
            label="全部 Agent"
            count={statusCounts.all}
            active={activeAgent === "all"}
            onClick={() => onAgentChange("all")}
            leading={<span className="agent-logo-slot" aria-hidden="true" />}
          />
          {agents.length === 0 ? (
            <p className="filter-note">未检测到 agent</p>
          ) : (
            agents.map((agent) => (
              <FilterRow
                key={agent.id}
                label={agent.displayName}
                count={agent.count}
                active={activeAgent === agent.id}
                onClick={() => onAgentChange(agent.id)}
                leading={
                  <AgentLogo
                    agentId={agent.id}
                    agentName={agent.displayName}
                  />
                }
                trailing={
                  agent.shared ? (
                    <span className="text-[11.5px] text-muted-foreground">
                      共享
                    </span>
                  ) : null
                }
                title={
                  agent.shared
                    ? `${agent.displayName} · 共享 canonical store`
                    : agent.displayName
                }
              />
            ))
          )}
        </section>

        <section aria-label="状态" className="filter-group">
          <SectionTitle>状态</SectionTitle>
          {STATUS_FILTERS.map((filter) => (
            <FilterRow
              key={filter.value}
              label={filter.label}
              count={statusCounts[filter.value]}
              active={activeStatus === filter.value}
              onClick={() => onStatusChange(filter.value)}
              countTone={filter.value === "update" ? "accent" : undefined}
            />
          ))}
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
