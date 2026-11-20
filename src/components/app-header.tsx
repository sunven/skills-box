import { ArrowLeft, ArrowUpCircle, Download, RefreshCw, SlidersHorizontal } from "lucide-react";
import type { ReactNode } from "react";
import { Button } from "./ui/button";
import type { AppError, UpdateCheckReport } from "../lib/types";
import { formatEpochDateTime, type LastCheckSummary } from "../lib/update-view";

interface AppHeaderProps {
  inert?: boolean;
  settingsOpen: boolean;
  discoveryOpen: boolean;
  marketplaceOpen?: boolean;
  pluginsOpen?: boolean;
  onOpenPlugins?: () => void;
  onOpenMarketplace?: () => void;
  discoveryCount: number;
  onOpenDiscovery: () => void;
  onReturnToSkills: () => void;
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
  scopeSwitcher?: ReactNode;
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
  inert = false,
  settingsOpen,
  discoveryOpen,
  marketplaceOpen = false,
  pluginsOpen = false,
  onOpenPlugins,
  onOpenMarketplace,
  discoveryCount,
  onOpenDiscovery,
  onReturnToSkills,
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
  scopeSwitcher,
}: AppHeaderProps) {
  const skillsOpen = !settingsOpen && !discoveryOpen && !marketplaceOpen && !pluginsOpen;
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

  return (
    <header className="topbar" inert={inert}>
      <div className="topbar-start">
        {settingsOpen || discoveryOpen || marketplaceOpen || pluginsOpen ? null : <Button
          variant="ghost"
          size="icon"
          className="rail-toggle"
          aria-label="打开筛选"
          aria-expanded={filtersOpen}
          onClick={onToggleFilters}
        >
          <SlidersHorizontal />
        </Button>}
        <span className="brand-mark">
          <BrandMark />
        </span>
        {scopeSwitcher}
      </div>

      <nav className="topbar-nav" aria-label="功能导航">
        <button type="button" className="topbar-tab" aria-pressed={skillsOpen} onClick={onReturnToSkills}>技能</button>
        <button type="button" className="topbar-tab" aria-pressed={pluginsOpen} onClick={onOpenPlugins}>插件</button>
        <button type="button" className="topbar-tab" aria-pressed={marketplaceOpen} onClick={onOpenMarketplace}>技能市场</button>
        <button type="button" className="topbar-tab" onClick={onOpenDiscovery} aria-label={`发现 (${discoveryCount})`} aria-pressed={discoveryOpen}>发现{discoveryCount > 0 ? <span className="discovery-count" data-testid="discovery-badge">{discoveryCount}</span> : null}</button>
      </nav>

      <div className="topbar-actions">
        {settingsOpen || discoveryOpen || marketplaceOpen || pluginsOpen ? <Button variant="ghost" size="sm" onClick={onReturnToSkills}><ArrowLeft aria-hidden="true" />返回 skills</Button> : <>
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
        </>}
      </div>
    </header>
  );
}
