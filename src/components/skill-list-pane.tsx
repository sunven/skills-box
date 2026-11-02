import type { ReactNode } from "react";
import { AlertTriangle, Loader2, Search } from "lucide-react";
import { AnomalyBanner } from "./anomaly-panel";
import { ProjectHeader } from "./project-header";
import { SkillRow } from "./skill-row";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import type {
  AnomalyView,
  AppError,
  ProjectEntry,
  UpdateViewSkill,
} from "../lib/types";
import {
  SORT_OPTIONS,
  STATUS_FILTERS,
  type AgentOption,
  type SortKey,
  type StatusCounts,
  type StatusFilter,
} from "../lib/inventory-view";
import type { ProjectHeaderView } from "../lib/workspace-view";

interface SkillListPaneProps {
  totalSkills: number;
  visibleSkills: UpdateViewSkill[];
  anomalies: AnomalyView;
  sourceCount: number;
  candidateCount: number;
  availableDiscoveryCount: number;
  onOpenAvailableDiscoveries: () => void;
  query: string;
  sort: SortKey;
  status: StatusFilter;
  statusCounts: StatusCounts;
  agent: string;
  agents: AgentOption[];
  selectedName: string | null;
  isLoading: boolean;
  error: unknown;
  scopeError?: AppError | null;
  projectHeader?: ProjectHeaderView | null;
  unavailableProject?: ProjectEntry | null;
  refreshingProject?: boolean;
  onRefreshProject?: () => void;
  onInstall?: () => void;
  onRelocate?: (project: ProjectEntry) => void;
  onForget?: (project: ProjectEntry) => void;
  onQueryChange: (query: string) => void;
  onSortChange: (sort: SortKey) => void;
  onStatusChange: (status: StatusFilter) => void;
  onAgentChange: (agent: string) => void;
  onSelect: (name: string) => void;
  onClearFilters: () => void;
  onOpenAnomalies: () => void;
  onRetry: () => void;
}

function CanvasSummary({
  totalSkills,
  sourceCount,
  candidateCount,
  anomalyCount,
}: {
  totalSkills: number;
  sourceCount: number;
  candidateCount: number;
  anomalyCount: number;
}) {
  return (
    <p className="canvas-summary">
      <span className="n">{totalSkills}</span> 个 skill ·{" "}
      <span className="n">{sourceCount}</span> 个来源 ·{" "}
      <span className={candidateCount > 0 ? "update" : "n"}>
        {candidateCount}
      </span>{" "}
      个可更新 ·{" "}
      <span className={anomalyCount > 0 ? "anomaly" : "n"}>
        {anomalyCount}
      </span>{" "}
      个异常
    </p>
  );
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
      <div className="max-w-[19rem] text-center">
        <h2 className="text-sm font-medium text-foreground">{title}</h2>
        <p className="mt-1.5 text-[12.5px] leading-relaxed text-muted-foreground">
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
  sourceCount,
  candidateCount,
  availableDiscoveryCount,
  onOpenAvailableDiscoveries,
  query,
  sort,
  status,
  statusCounts,
  agent,
  agents,
  selectedName,
  isLoading,
  error,
  scopeError,
  projectHeader,
  unavailableProject,
  refreshingProject = false,
  onRefreshProject,
  onInstall,
  onRelocate,
  onForget,
  onQueryChange,
  onSortChange,
  onStatusChange,
  onAgentChange,
  onSelect,
  onClearFilters,
  onOpenAnomalies,
  onRetry,
}: SkillListPaneProps) {
  return (
    <section aria-label="Skill 列表" className="canvas">
      {projectHeader ? (
        <>
          <ProjectHeader
            view={projectHeader}
            refreshing={refreshingProject}
            onRefresh={onRefreshProject ?? (() => {})}
          />
          <CanvasSummary
            totalSkills={totalSkills}
            sourceCount={sourceCount}
            candidateCount={candidateCount}
            anomalyCount={anomalies.total}
          />
        </>
      ) : (
        <div className="canvas-head">
          <div className="min-w-0">
            <h1 className="canvas-title">全局 Skills</h1>
            <CanvasSummary
              totalSkills={totalSkills}
              sourceCount={sourceCount}
              candidateCount={candidateCount}
              anomalyCount={anomalies.total}
            />
          </div>
        </div>
      )}

      <div className="toolbar">
        <div className="toolbar-search">
          <Search
            className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground"
            aria-hidden="true"
          />
          <Input
            value={query}
            onChange={(event) => onQueryChange(event.target.value)}
            aria-label="搜索 skill、来源或描述"
            placeholder="搜索 skill、来源或描述"
            className="h-9 pl-8 text-[13px]"
          />
        </div>
        <select
          aria-label="排序"
          value={sort}
          onChange={(event) => onSortChange(event.target.value as SortKey)}
          className="toolbar-select h-9 rounded-md border border-border-strong bg-card px-2 text-[12.5px] text-foreground"
        >
          {SORT_OPTIONS.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
        <select
          aria-label="状态筛选"
          value={status}
          onChange={(event) => onStatusChange(event.target.value as StatusFilter)}
          className="toolbar-select h-9 rounded-md border border-border-strong bg-card px-2 text-[12.5px] text-foreground"
        >
          {STATUS_FILTERS.map((filter) => (
            <option key={filter.value} value={filter.value}>
              {filter.value === "all" ? "全部状态" : filter.label} ({statusCounts[filter.value]})
            </option>
          ))}
        </select>
        <select
          aria-label="Agent 筛选"
          value={agent}
          onChange={(event) => onAgentChange(event.target.value)}
          className="toolbar-select toolbar-agent-select h-9 rounded-md border border-border-strong bg-card px-2 text-[12.5px] text-foreground"
        >
          <option value="all">全部 Agent ({totalSkills})</option>
          {agents.map((option) => (
            <option key={option.id} value={option.id}>
              {option.displayName}{option.shared ? " · 共享" : !option.detected ? " · 未检测" : ""} ({option.count})
            </option>
          ))}
        </select>
        <div className="ml-auto flex items-center gap-3">
          {availableDiscoveryCount > 0 ? <Button variant="outline" size="sm" className="text-accent"
            title="查看当前来源在此作用域可安装的 skills" onClick={onOpenAvailableDiscoveries}>可安装 {availableDiscoveryCount}</Button> : null}
          <span className="toolbar-count">{visibleSkills.length} 个</span>
        </div>
      </div>

      <AnomalyBanner anomalies={anomalies} onOpen={onOpenAnomalies} />

      <div className="list-panel">
        {unavailableProject ? (
          <EmptyState
            title="项目路径不可用"
            description={
              unavailableProject.unavailableReason ??
              "项目根目录不存在、不是目录或不可读。条目会保留，可以重新定位或遗忘。"
            }
            action={
              <div className="flex flex-wrap items-center justify-center gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => onRelocate?.(unavailableProject)}
                >
                  重新定位
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => onForget?.(unavailableProject)}
                >
                  遗忘项目
                </Button>
              </div>
            }
          />
        ) : isLoading && totalSkills === 0 ? (
          <div
            role="status"
            className="flex flex-1 items-center justify-center gap-2 p-8 text-[12.5px] text-muted-foreground"
          >
            <Loader2 className="size-4 animate-spin" aria-hidden="true" />
            正在读取库存…
          </div>
        ) : error ? (
          <div className="flex flex-1 items-center justify-center p-8">
            <div className="max-w-[19rem] text-center">
              <AlertTriangle
                className="mx-auto size-5 text-destructive"
                aria-hidden="true"
              />
              <h2 className="mt-2 text-sm font-medium text-foreground">
                {projectHeader ? "项目读取失败" : "库存读取失败"}
              </h2>
              <p className="mt-1.5 text-[12.5px] leading-relaxed text-muted-foreground">
                {scopeError
                  ? `${scopeError.message}${scopeError.detail ? ` · ${scopeError.detail}` : ""}`
                  : "不会以空列表代替真实库存。请检查权限后重试。"}
              </p>
              <Button
                variant="outline"
                size="sm"
                className="mt-3"
                onClick={onRetry}
              >
                重试读取
              </Button>
            </div>
          </div>
        ) : totalSkills === 0 ? (
          <EmptyState
            title={projectHeader ? "项目暂无 skill" : "暂无 skill"}
            description={
              projectHeader
                ? "这个项目还没有 skill。安装后会写入项目的 .agents/skills，并记录在项目 lock 中。"
                : "还没有全局 skill。安装后会显示在这里；也可以先在终端运行 npx skills add … -g。"
            }
            action={
              projectHeader && onInstall ? (
                <Button
                  size="sm"
                  data-testid="project-empty-install"
                  onClick={onInstall}
                >
                  安装 skill
                </Button>
              ) : undefined
            }
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
          <>
            <div className="list-head">
              <div className="skill-grid">
                <span>名称</span>
                <span className="skill-col-source">来源 / 更新于</span>
                <span className="skill-col-mounts">Agent 挂载</span>
                <span>状态</span>
              </div>
            </div>
            <div className="list-scroll" data-testid="skill-list">
              {visibleSkills.map((skill) => (
                <SkillRow
                  key={skill.name}
                  skill={skill}
                  selected={skill.name === selectedName}
                  onSelect={onSelect}
                />
              ))}
            </div>
          </>
        )}
      </div>
    </section>
  );
}
