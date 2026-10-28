import type { ReactNode } from "react";
import { Bot, Filter } from "lucide-react";
import { cn } from "../lib/utils";
import type { DetectedAgent } from "../lib/types";

interface FilterSidebarProps {
  agents: DetectedAgent[];
}

interface FilterRowProps {
  label: string;
  count?: number | null;
  active?: boolean;
  shared?: boolean;
  title?: string;
}

function FilterRow({ label, count, active, shared, title }: FilterRowProps) {
  return (
    <button
      type="button"
      disabled
      title={title}
      className={cn(
        "flex w-full cursor-default items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs transition-colors",
        active
          ? "bg-primary/10 font-medium text-primary"
          : "text-muted-foreground hover:bg-muted",
      )}
    >
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {shared ? (
        <span className="text-[10px] text-muted-foreground/80">共享</span>
      ) : null}
      <span className="tabular-nums text-[11px] text-muted-foreground/80">
        {count === undefined ? "—" : (count ?? "—")}
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

export function FilterSidebar({ agents }: FilterSidebarProps) {
  return (
    <aside className="flex min-h-0 flex-col border-r border-border bg-card/40">
      <div className="flex items-center gap-2 border-b border-border px-4 py-3">
        <Filter className="size-3.5 text-muted-foreground" aria-hidden="true" />
        <h2 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
          筛选
        </h2>
      </div>

      <div className="flex-1 space-y-5 overflow-auto p-3">
        <section>
          <SectionTitle>来源</SectionTitle>
          <FilterRow label="全部来源" active />
          <p className="px-2 pt-2 text-[11px] text-muted-foreground/70">
            暂无来源
          </p>
        </section>

        <section>
          <SectionTitle>Agents</SectionTitle>
          <FilterRow label="全部 Agent" active count={agents.length || null} />
          {agents.length === 0 ? (
            <p className="px-2 pt-2 text-[11px] text-muted-foreground/70">
              未检测到 agent
            </p>
          ) : (
            <div className="mt-0.5 space-y-0.5">
              {agents.map((agent) => (
                <button
                  key={agent.id}
                  type="button"
                  disabled
                  title={agent.globalSkillsDir ?? undefined}
                  className="flex w-full cursor-default items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-muted-foreground hover:bg-muted"
                >
                  <Bot
                    className="size-3.5 shrink-0 text-muted-foreground/70"
                    aria-hidden="true"
                  />
                  <span className="min-w-0 flex-1 truncate">
                    {agent.displayName}
                  </span>
                  {agent.universal ? (
                    <span className="text-[10px] text-muted-foreground/80">
                      共享
                    </span>
                  ) : null}
                </button>
              ))}
            </div>
          )}
        </section>

        <section>
          <SectionTitle>状态</SectionTitle>
          <FilterRow label="全部" active />
          <FilterRow label="可更新" />
          <FilterRow label="已是最新" />
          <FilterRow label="未知" />
          <FilterRow label="异常" />
          <FilterRow label="未受管" />
        </section>

      </div>
    </aside>
  );
}
