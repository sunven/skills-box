import { ArrowLeft, ArrowUpCircle, Download, RefreshCw, Settings, SlidersHorizontal } from "lucide-react";
import { Button } from "./ui/button";
import { environmentSummary } from "../lib/environment-display";
import type { AppError, EnvReport, UpdateCheckReport } from "../lib/types";
import { formatEpochDateTime, type LastCheckSummary } from "../lib/update-view";

interface AppHeaderProps {
  settingsOpen: boolean;
  discoveryOpen: boolean;
  discoveryCount: number;
  onOpenDiscovery: () => void;
  onOpenSettings: () => void;
  onReturnToSkills: () => void;
  report?: EnvReport;
  isLoading: boolean;
  isError: boolean;
  mutationsEnabled: boolean;
  onInstall: () => void;
  updateReport?: UpdateCheckReport | null;
  lastCheck?: LastCheckSummary | null;
  updateError?: AppError | null;
  checkingUpdates: boolean;
  updatesEnabled: boolean;
  onCheckUpdates: () => void;
  onUpdateAll: () => void;
  updateCandidateCount: number;
  filtersOpen: boolean;
  onToggleFilters: () => void;
}

function BrandMark() {
  return (
    <svg
      viewBox="0 0 20 20"
      className="size-[18px]"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.4"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M2.8 6.4 10 2.8l7.2 3.6v7.2L10 17.2l-7.2-3.6V6.4Z" />
      <path d="M2.8 6.4 10 10l7.2-3.6M10 10v7.2" />
    </svg>
  );
}

export function AppHeader({
  settingsOpen,
  discoveryOpen,
  discoveryCount,
  onOpenDiscovery,
  onOpenSettings,
  onReturnToSkills,
  report,
  isLoading,
  isError,
  mutationsEnabled,
  onInstall,
  updateReport,
  lastCheck,
  updateError,
  checkingUpdates,
  updatesEnabled,
  onCheckUpdates,
  onUpdateAll,
  updateCandidateCount,
  filtersOpen,
  onToggleFilters,
}: AppHeaderProps) {
  const failedSources = lastCheck?.failedSourceCount ?? 0;
  const outcomeSuffix =
    lastCheck?.outcome === "failed"
      ? "（失败）"
      : lastCheck?.outcome === "partial"
        ? failedSources > 0
          ? "（部分失败）"
          : "（有来源未检查）"
        : "";
  const lastCheckLabel = lastCheck
    ? `上次检查：${formatEpochDateTime(lastCheck.lastAttemptAt)}${outcomeSuffix}`
    : "尚未检查更新";
  const lastCheckTooltip = [
    lastCheck?.lastSuccessAt != null
      ? `上次成功检查：${formatEpochDateTime(lastCheck.lastSuccessAt)}`
      : "尚无成功检查记录",
    failedSources > 0 ? `${failedSources} 个来源本次检查失败` : null,
    lastCheck?.cachedSourceCount
      ? `${lastCheck.cachedSourceCount} 个来源来自 6 小时缓存`
      : null,
    lastCheck?.skippedSourceCount
      ? `${lastCheck.skippedSourceCount} 个来源因上限未检查`
      : null,
    ...(lastCheck?.warnings ?? []),
    updateError?.message ?? null,
    "启动和每 6 小时自动检查，也可手动触发",
  ]
    .filter(Boolean)
    .join("\n");
  const env = isLoading
    ? { label: "环境检测中", tone: "muted" as const }
    : isError || !report
      ? { label: "环境异常", tone: "destructive" as const }
      : environmentSummary(report);

  return (
    <header className="topbar">
      <div className="flex min-w-0 items-center gap-3">
        {settingsOpen || discoveryOpen ? null : <Button
          variant="ghost"
          size="icon"
          className="rail-toggle"
          aria-label="打开筛选"
          aria-expanded={filtersOpen}
          onClick={onToggleFilters}
        >
          <SlidersHorizontal />
        </Button>}
        <div className="brand">
          <span className="brand-mark">
            <BrandMark />
          </span>
          <span className="brand-name">Skills Box</span>
          <span
            role="img"
            className="env-dot"
            data-tone={env.tone}
            title={env.label}
            aria-label={env.label}
          />
          <span className="brand-scope">
            {report?.lock.path ?? "~/.agents/.skill-lock.json"} ·{" "}
            {report?.agents.length ?? 0} 个 agent
          </span>
        </div>
      </div>

      <div className="topbar-actions">
        <Button variant={discoveryOpen ? "outline" : "ghost"} size="sm" onClick={onOpenDiscovery} aria-label={`发现 (${discoveryCount})`} aria-pressed={discoveryOpen}>发现{discoveryCount > 0 ? <span className="discovery-count" data-testid="discovery-badge">{discoveryCount}</span> : null}</Button>
        {settingsOpen || discoveryOpen ? <><Button variant="ghost" size="sm" onClick={onReturnToSkills}><ArrowLeft aria-hidden="true" />返回 skills</Button>{discoveryOpen ? <Button variant="ghost" size="icon" aria-label="设置" onClick={onOpenSettings}><Settings aria-hidden="true" /></Button> : null}</> : <>
        <span
          data-testid="last-update-check"
          data-error={!checkingUpdates && updateError ? "true" : undefined}
          title={lastCheckTooltip}
          className="topbar-stamp"
        >
          {checkingUpdates
            ? "正在检查…"
            : updateError
              ? `更新检查失败：${updateError.message}`
              : lastCheckLabel}
        </span>
        <Button
          variant="ghost"
          size="sm"
          disabled={!updatesEnabled || checkingUpdates}
          aria-busy={checkingUpdates}
          onClick={onCheckUpdates}
          aria-label="检查更新"
          title="只读检查更新；不会运行 skills check / skills update"
        >
          <RefreshCw
            className={checkingUpdates ? "size-3.5 animate-spin" : "size-3.5"}
            aria-hidden="true"
          />
          <span className="btn-label">检查更新</span>
        </Button>
        <Button
          variant="outline"
          size="sm"
          disabled={!updateReport}
          onClick={onUpdateAll}
          aria-label={`更新全部 (${updateCandidateCount})`}
          title={
            updateReport
              ? "查看更新候选与跳过项，备份后逐个执行更新"
              : "请先检查更新"
          }
        >
          <ArrowUpCircle className="size-3.5" aria-hidden="true" />
          <span className="btn-label">
            更新全部
            {updateCandidateCount > 0 ? ` (${updateCandidateCount})` : ""}
          </span>
        </Button>
        <Button
          size="sm"
          disabled={!mutationsEnabled}
          onClick={onInstall}
          title={
            mutationsEnabled
              ? "安装 skill"
              : "环境未就绪，安装等变更操作已禁用"
          }
        >
          <Download className="size-3.5" aria-hidden="true" />
          安装 skill
        </Button>
        <Button variant="ghost" size="icon" aria-label="设置" title="设置" onClick={onOpenSettings}><Settings aria-hidden="true" /></Button>
        </>}
      </div>
    </header>
  );
}
