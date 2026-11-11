import { SelectionCheckbox } from "./selection-checkbox";
import { BATCH_ACTION_LABELS } from "../lib/batch";
import type { BatchSelection } from "./batch-dialog";
import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import { AlertTriangle, Loader2, Search, X } from "lucide-react";
import { AnomalyBanner } from "./anomaly-panel";
import { ProjectHeader } from "./project-header";
import { SkillRow } from "./skill-row";
import {
  SkillRowMenu,
  type SkillMenuAction,
  type SkillMenuTarget,
} from "./skill-row-menu";
import { useSkillRowActions } from "../hooks/use-skill-row-actions";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import type {
  AnomalyView,
  AppError,
  InventoryAgent,
  ProjectEntry,
  ScopeId,
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
import type { ProjectHeaderView, SkillListSummary } from "../lib/workspace-view";

interface SkillListPaneProps {
  scope: ScopeId;
  checkRevision?: string;
  actionsDisabled: boolean;
  batchRunning: boolean;
  batchMutationsEnabled: boolean;
  batchSucceededIds: Set<string>;
  onBatch: (selection: BatchSelection) => void;
  checkingUpdates: boolean;
  checkError: AppError | null;
  installTargets: InventoryAgent[];
  onCheckUpdates: (names: string[]) => boolean;
  onOpenDetail: (name: string, destination: "content" | "attachments") => void;
  onUpdate: (name: string) => void;
  onUninstall: (name: string) => void;
  totalSkills: number;
  visibleSkills: UpdateViewSkill[];
  anomalies: AnomalyView;
  summary: SkillListSummary;
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
  skillCount,
  sourceCount,
  updateCount,
  anomalyCount,
}: SkillListSummary) {
  return (
    <p className="canvas-summary">
      <span className="n">{skillCount} 个</span> skill ·{" "}
      <span className="n">{sourceCount}</span> 个来源 ·{" "}
      <span className={updateCount > 0 ? "update" : "n"}>
        {updateCount}
      </span>{" "}
      个可更新 ·{" "}
      <span className={anomalyCount > 0 ? "anomaly" : "n"}>{anomalyCount}</span>{" "}
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
  scope,
  checkRevision,
  actionsDisabled,
  batchRunning,
  batchMutationsEnabled,
  batchSucceededIds,
  onBatch,
  checkingUpdates,
  checkError,
  installTargets,
  onCheckUpdates,
  onOpenDetail,
  onUpdate,
  onUninstall,
  totalSkills,
  visibleSkills,
  anomalies,
  summary,
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
  const [checkedIds, setCheckedIds] = useState<Set<string>>(() => new Set());
  const checkedSkills = useMemo(
    () => visibleSkills.filter((skill) => checkedIds.has(skill.id)),
    [visibleSkills, checkedIds],
  );
  useEffect(() => {
    if (batchRunning) return;
    const matching = new Set(visibleSkills.map((skill) => skill.id));
    setCheckedIds((current) => {
      const next = new Set(
        [...current].filter(
          (id) => matching.has(id) && !batchSucceededIds.has(id),
        ),
      );
      return next.size === current.size ? current : next;
    });
  }, [visibleSkills, batchRunning, batchSucceededIds]);
  const toggleSkill = useCallback((id: string) => {
    setCheckedIds((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);
  const toggleAll = () =>
    setCheckedIds(
      checkedSkills.length === visibleSkills.length
        ? new Set()
        : new Set(visibleSkills.map((skill) => skill.id)),
    );
  const [menuTarget, setMenuTarget] = useState<SkillMenuTarget | null>(null);
  const rowActions = useSkillRowActions({
    scope,
    checkRevision,
    checkingUpdates,
    checkError,
    skills: visibleSkills,
    onCheckUpdates,
  });
  const menuSkill = menuTarget
    ? visibleSkills.find((skill) => skill.id === menuTarget.skillId)
    : undefined;
  useEffect(() => {
    if (menuTarget && (!menuSkill || !menuTarget.anchor.isConnected))
      setMenuTarget(null);
  }, [menuTarget, menuSkill]);
  const openMenu = useCallback(
    (skill: UpdateViewSkill, anchor: HTMLButtonElement) => {
      setMenuTarget((current) =>
        current?.skillId === skill.id ? null : { skillId: skill.id, anchor },
      );
    },
    [],
  );
  const closeMenu = useCallback(
    (restoreFocus = false) => {
      if (restoreFocus && menuTarget?.anchor.isConnected)
        menuTarget.anchor.focus({ preventScroll: true });
      setMenuTarget(null);
    },
    [menuTarget],
  );
  const handleMenuAction = (
    skill: UpdateViewSkill,
    action: SkillMenuAction,
  ) => {
    if (action === "content" || action === "attachments")
      onOpenDetail(skill.name, action);
    else if (action === "check") rowActions.checkSkill(skill);
    else onUninstall(skill.name);
  };

  return (
    <section aria-label="Skill 列表" className="canvas">
      {projectHeader ? (
        <ProjectHeader
          view={projectHeader}
          refreshing={refreshingProject}
          onRefresh={onRefreshProject ?? (() => {})}
        />
      ) : null}
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
            disabled={batchRunning}
            placeholder="搜索 skill、来源或描述"
            className="h-9 pl-8 text-[13px]"
          />
        </div>
        <select
          aria-label="排序"
          disabled={batchRunning}
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
          disabled={batchRunning}
          value={status}
          onChange={(event) =>
            onStatusChange(event.target.value as StatusFilter)
          }
          className="toolbar-select h-9 rounded-md border border-border-strong bg-card px-2 text-[12.5px] text-foreground"
        >
          {STATUS_FILTERS.map((filter) => (
            <option key={filter.value} value={filter.value}>
              {filter.value === "all" ? "全部状态" : filter.label} (
              {statusCounts[filter.value]})
            </option>
          ))}
        </select>
        <select
          aria-label="Agent 筛选"
          disabled={batchRunning}
          value={agent}
          onChange={(event) => onAgentChange(event.target.value)}
          className="toolbar-select toolbar-agent-select h-9 rounded-md border border-border-strong bg-card px-2 text-[12.5px] text-foreground"
        >
          <option value="all">全部 Agent ({totalSkills})</option>
          {agents.map((option) => (
            <option key={option.id} value={option.id}>
              {option.displayName}
              {option.shared
                ? " · 共享"
                : !option.detected
                  ? " · 未检测"
                  : ""}{" "}
              ({option.count})
            </option>
          ))}
        </select>
        <div className="toolbar-summary">
          {availableDiscoveryCount > 0 ? (
            <Button
              variant="outline"
              size="sm"
              className="text-accent"
              title="查看当前来源在此作用域可安装的 skills"
              onClick={onOpenAvailableDiscoveries}
            >
              可安装 {availableDiscoveryCount}
            </Button>
          ) : null}
          <CanvasSummary {...summary} />
        </div>
      </div>

      {checkedSkills.length > 0 ? (
        <div className="batch-toolbar" role="toolbar" aria-label="批量操作">
          <strong aria-live="polite">已选 {checkedSkills.length} 个技能</strong>
          <div className="batch-toolbar-actions">
            {(["update", "uninstall", "attach", "detach"] as const).map(
              (action) => (
                <Button
                  key={action}
                  variant="outline"
                  size="sm"
                  className={
                    action === "uninstall" ? "text-destructive" : undefined
                  }
                  disabled={
                    actionsDisabled ||
                    batchRunning ||
                    ((action === "update" || action === "uninstall") &&
                      !batchMutationsEnabled && !(action === "uninstall" && checkedSkills.every((s) => ["skillhub", "skillhub-pack"].includes(s.sourceType ?? "") || s.management === "unmanaged")))
                  }
                  title={
                    actionsDisabled || batchRunning
                      ? "当前无法开始写入操作"
                      : (action === "update" || action === "uninstall") &&
                          !batchMutationsEnabled && !(action === "uninstall" && checkedSkills.every((s) => ["skillhub", "skillhub-pack"].includes(s.sourceType ?? "") || s.management === "unmanaged"))
                        ? "请先配置可用的执行环境"
                        : undefined
                  }
                  onClick={() => onBatch({ action, skills: checkedSkills })}
                >
                  批量{BATCH_ACTION_LABELS[action]}
                </Button>
              ),
            )}
            <Button
              variant="ghost"
              size="sm"
              disabled={batchRunning}
              onClick={() => setCheckedIds(new Set())}
            >
              清空选择
            </Button>
          </div>
        </div>
      ) : null}

      <AnomalyBanner anomalies={anomalies} onOpen={onOpenAnomalies} />

      {rowActions.notice ? (
        <div
          role="alert"
          className="skill-action-notice notice"
          data-tone="negative"
        >
          <div>
            <p>
              {rowActions.notice.name}：{rowActions.notice.error.message}
            </p>
            {rowActions.notice.error.detail ? (
              <p>{rowActions.notice.error.detail}</p>
            ) : null}
            {rowActions.notice.error.hint ? (
              <p>{rowActions.notice.error.hint}</p>
            ) : null}
          </div>
          <button
            type="button"
            aria-label="关闭操作提示"
            onClick={rowActions.dismissNotice}
          >
            <X aria-hidden="true" />
          </button>
        </div>
      ) : null}

      <div className="list-panel">
        <div className="list-head">
          <div className="skill-grid">
            <SelectionCheckbox
              checked={
                checkedSkills.length === visibleSkills.length &&
                visibleSkills.length > 0
              }
              mixed={
                checkedSkills.length > 0 &&
                checkedSkills.length < visibleSkills.length
              }
              disabled={
                batchRunning ||
                visibleSkills.length === 0 ||
                !!error ||
                !!unavailableProject
              }
              label="全选当前筛选结果"
              onChange={toggleAll}
            />
            <span>名称</span>
            <span className="skill-col-source">来源 / 更新于</span>
            <span className="skill-col-mounts">Agent 挂载</span>
            <span>状态</span>
            <span className="skill-actions-heading">操作</span>
          </div>
        </div>
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
                : "还没有全局 skill。安装后会显示在这里。"
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
            <div className="list-scroll" data-testid="skill-list">
              {visibleSkills.map((skill) => (
                <SkillRow
                  key={skill.name}
                  skill={skill}
                  selected={skill.name === selectedName}
                  checked={checkedIds.has(skill.id)}
                  selectionDisabled={batchRunning}
                  onToggle={toggleSkill}
                  onSelect={onSelect}
                  actionsDisabled={actionsDisabled}
                  openingFolder={rowActions.openingFolders.has(skill.id)}
                  checking={
                    rowActions.checkingName === skill.name && checkingUpdates
                  }
                  menuOpen={menuSkill?.id === skill.id}
                  onOpenFolder={rowActions.openFolder}
                  onOpenMenu={openMenu}
                  onUpdate={onUpdate}
                />
              ))}
            </div>
          </>
        )}
      </div>
      {menuSkill && menuTarget ? (
        <SkillRowMenu
          key={menuSkill.id}
          skill={menuSkill}
          target={menuTarget}
          agents={installTargets}
          actionsDisabled={actionsDisabled}
          checkingUpdates={checkingUpdates}
          onAction={handleMenuAction}
          onClose={closeMenu}
        />
      ) : null}
    </section>
  );
}
