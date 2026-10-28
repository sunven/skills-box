import type { ReactNode } from "react";
import { AlertTriangle, Loader2, PackageOpen, Search } from "lucide-react";
import { AnomalyBanner } from "./anomaly-panel";
import { SkillRow } from "./skill-row";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import type { AnomalyView, InventorySkill } from "../lib/types";
import { SORT_OPTIONS, type SortKey } from "../lib/inventory-view";

interface SkillListPaneProps {
  totalSkills: number;
  visibleSkills: InventorySkill[];
  anomalies: AnomalyView;
  query: string;
  sort: SortKey;
  selectedName: string | null;
  isLoading: boolean;
  error: unknown;
  onQueryChange: (query: string) => void;
  onSortChange: (sort: SortKey) => void;
  onSelect: (name: string) => void;
  onClearFilters: () => void;
  onOpenAnomalies: () => void;
  onRetry: () => void;
}

function EmptyState({
  title,
  description,
  action,
}: {
  title: string;
  description: string;
  action?: ReactNode;
}) {
  return (
    <div className="flex flex-1 items-center justify-center p-8">
      <div className="max-w-xs text-center">
        <div className="mx-auto flex size-12 items-center justify-center rounded-xl border border-border bg-card text-muted-foreground/70 shadow-sm">
          <PackageOpen className="size-5" aria-hidden="true" />
        </div>
        <h2 className="mt-3 text-sm font-medium text-foreground">{title}</h2>
        <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
          {description}
        </p>
        {action ? <div className="mt-3">{action}</div> : null}
      </div>
    </div>
  );
}

export function SkillListPane({
  totalSkills,
  visibleSkills,
  anomalies,
  query,
  sort,
  selectedName,
  isLoading,
  error,
  onQueryChange,
  onSortChange,
  onSelect,
  onClearFilters,
  onOpenAnomalies,
  onRetry,
}: SkillListPaneProps) {
  return (
    <section
      aria-label="Skill 列表"
      className="flex min-h-0 min-w-0 flex-col bg-background"
    >
      <div className="flex items-center gap-2 border-b border-border bg-card px-4 py-2.5">
        <div className="relative min-w-0 flex-1">
          <Search
            className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground"
            aria-hidden="true"
          />
          <Input
            value={query}
            onChange={(event) => onQueryChange(event.target.value)}
            aria-label="搜索 skill、来源或描述"
            placeholder="搜索 skill、来源或描述"
            className="h-8 pl-8 text-xs"
          />
        </div>
        <select
          aria-label="排序"
          value={sort}
          onChange={(event) => onSortChange(event.target.value as SortKey)}
          className="h-8 rounded-md border border-border-strong bg-card px-2 text-xs text-foreground shadow-sm"
        >
          {SORT_OPTIONS.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
        <span className="w-12 shrink-0 text-right text-[11px] tabular-nums text-muted-foreground">
          {visibleSkills.length} 个
        </span>
      </div>

      <AnomalyBanner anomalies={anomalies} onOpen={onOpenAnomalies} />

      {isLoading && totalSkills === 0 ? (
        <div
          role="status"
          className="flex flex-1 items-center justify-center gap-2 p-8 text-xs text-muted-foreground"
        >
          <Loader2 className="size-4 animate-spin" aria-hidden="true" />
          正在读取库存…
        </div>
      ) : error ? (
        <div className="flex flex-1 items-center justify-center p-8">
          <div className="max-w-xs text-center">
            <div className="mx-auto flex size-12 items-center justify-center rounded-xl border border-destructive/20 bg-red-50 text-destructive shadow-sm">
              <AlertTriangle className="size-5" aria-hidden="true" />
            </div>
            <h2 className="mt-3 text-sm font-medium text-foreground">
              库存读取失败
            </h2>
            <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
              不会以空列表代替真实库存。请检查权限后重试。
            </p>
            <Button variant="outline" size="sm" className="mt-3" onClick={onRetry}>
              重试读取
            </Button>
          </div>
        </div>
      ) : totalSkills === 0 ? (
        <EmptyState
          title="暂无 skill"
          description="还没有全局 skill。安装后会显示在这里；也可以先在终端运行 npx skills add … -g。"
        />
      ) : visibleSkills.length === 0 ? (
        <EmptyState
          title="没有匹配的 skill"
          description="调整搜索或筛选条件，或者清除筛选后重试。"
          action={
            <Button variant="outline" size="sm" onClick={onClearFilters}>
              清除筛选
            </Button>
          }
        />
      ) : (
        <div className="min-h-0 flex-1 overflow-auto" data-testid="skill-list">
          {visibleSkills.map((skill) => (
            <SkillRow
              key={skill.name}
              skill={skill}
              selected={skill.name === selectedName}
              onSelect={onSelect}
            />
          ))}
        </div>
      )}
    </section>
  );
}
