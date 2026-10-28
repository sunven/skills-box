import type { ReactNode } from "react";
import { Bot, Filter } from "lucide-react";
import { cn } from "../lib/utils";
import {
  STATUS_FILTERS,
  type AgentOption,
  type SourceOption,
  type StatusCounts,
  type StatusFilter,
} from "../lib/inventory-view";

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
}

interface FilterRowProps {
  label: string;
  count: number;
  active: boolean;
  onClick: () => void;
  title?: string;
  trailing?: ReactNode;
}

function FilterRow({
  label,
  count,
  active,
  onClick,
  title,
  trailing,
}: FilterRowProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={active}
      title={title}
      className={cn(
        "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs transition-colors",
        active
          ? "bg-primary/10 font-medium text-primary"
          : "text-muted-foreground hover:bg-muted",
      )}
    >
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {trailing}
      <span className="tabular-nums text-[11px] text-muted-foreground/80">
        {count}
      </span>
    </button>
  );
}

function SectionTitle({ children }: { children: ReactNode }) {
  return (
    <h3 className="px-2 pb-1.5 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground/80">
      {children}
    </h3>
  );
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
}: FilterSidebarProps) {
  return (
    <aside
      aria-label="筛选"
      className="flex min-h-0 flex-col border-r border-border bg-card/40"
    >
      <div className="flex items-center gap-2 border-b border-border px-4 py-3">
        <Filter className="size-3.5 text-muted-foreground" aria-hidden="true" />
        <h2 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
          筛选
        </h2>
      </div>

      <div className="flex-1 space-y-5 overflow-auto p-3">
        <section>
          <SectionTitle>来源</SectionTitle>
          <FilterRow
            label="全部来源"
            count={statusCounts.all}
            active={activeSource === "all"}
            onClick={() => onSourceChange("all")}
          />
          {sources.length === 0 ? (
            <p className="px-2 pt-2 text-[11px] text-muted-foreground/70">
              暂无来源
            </p>
          ) : (
            <div className="mt-0.5 space-y-0.5">
              {sources.map((source) => (
                <FilterRow
                  key={source.value}
                  label={source.label}
                  count={source.count}
                  active={activeSource === source.value}
                  onClick={() => onSourceChange(source.value)}
                />
              ))}
            </div>
          )}
        </section>

        <section>
          <SectionTitle>Agents</SectionTitle>
          <FilterRow
            label="全部 Agent"
            count={statusCounts.all}
            active={activeAgent === "all"}
            onClick={() => onAgentChange("all")}
          />
          {agents.length === 0 ? (
            <p className="px-2 pt-2 text-[11px] text-muted-foreground/70">
              未检测到 agent
            </p>
          ) : (
            <div className="mt-0.5 space-y-0.5">
              {agents.map((agent) => (
                <FilterRow
                  key={agent.id}
                  label={agent.displayName}
                  count={agent.count}
                  active={activeAgent === agent.id}
                  onClick={() => onAgentChange(agent.id)}
                  trailing={
                    agent.shared ? (
                      <span className="text-[10px] text-muted-foreground/80">
                        共享
                      </span>
                    ) : null
                  }
                />
              ))}
            </div>
          )}
        </section>

        <section>
          <SectionTitle>状态</SectionTitle>
          <div className="space-y-0.5">
            {STATUS_FILTERS.map((filter) => (
              <FilterRow
                key={filter.value}
                label={filter.label}
                count={statusCounts[filter.value]}
                active={activeStatus === filter.value}
                onClick={() => onStatusChange(filter.value)}
              />
            ))}
          </div>
        </section>

        <section className="px-2 pt-1">
          <p className="flex items-start gap-1.5 text-[10.5px] leading-relaxed text-muted-foreground/70">
            <Bot className="mt-0.5 size-3 shrink-0" aria-hidden="true" />
            Agent 挂载来自物理目录与 lstat，不使用 CLI 的 agents 字段。
          </p>
        </section>
      </div>
    </aside>
  );
}
