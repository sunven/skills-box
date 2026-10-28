import {
  ArrowUpCircle,
  Boxes,
  Download,
  RefreshCw,
  RotateCcw,
} from "lucide-react";
import { Button } from "./ui/button";
import { EnvironmentBadge } from "./environment-badge";
import type { EnvReport, UpdateCheckReport } from "../lib/types";
import { formatEpochDateTime } from "../lib/update-view";

interface AppHeaderProps {
  report?: EnvReport;
  isLoading: boolean;
  isError: boolean;
  mutationsEnabled: boolean;
  rechecking: boolean;
  onRecheck: () => void;
  onInstall: () => void;
  updateReport?: UpdateCheckReport | null;
  updateError?: { message: string; detail?: string } | null;
  checkingUpdates: boolean;
  updatesEnabled: boolean;
  onCheckUpdates: () => void;
  onUpdateAll: () => void;
  updateCandidateCount: number;
}

export function AppHeader({
  report,
  isLoading,
  isError,
  mutationsEnabled,
  rechecking,
  onRecheck,
  onInstall,
  updateReport,
  updateError,
  checkingUpdates,
  updatesEnabled,
  onCheckUpdates,
  onUpdateAll,
  updateCandidateCount,
}: AppHeaderProps) {
  const lastCheck = updateReport?.lastSuccessAtEpochMs ?? null;
  const failedSources =
    updateReport?.sources.filter((source) => source.status === "error").length ??
    0;
  const lastCheckLabel = lastCheck
    ? `上次检查：${formatEpochDateTime(lastCheck)}`
    : failedSources > 0
      ? "检查失败（尚无成功记录）"
      : "尚未检查更新";
  const lastCheckTooltip = [
    lastCheck
      ? `上次成功检查：${formatEpochDateTime(lastCheck)}`
      : "尚未成功检查更新",
    failedSources > 0 ? `${failedSources} 个来源本次检查失败` : null,
    updateReport?.cachedSourceCount
      ? `${updateReport.cachedSourceCount} 个来源来自 6 小时缓存`
      : null,
    updateReport?.skippedSourceCount
      ? `${updateReport.skippedSourceCount} 个来源因上限未检查`
      : null,
    ...(updateReport?.warnings ?? []),
    updateError?.message ?? null,
  ]
    .filter(Boolean)
    .join("\n");

  return (
    <header className="flex items-center justify-between gap-4 border-b border-border bg-card px-5 py-3">
      <div className="flex min-w-0 items-center gap-3">
        <div className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
          <Boxes className="size-5" aria-hidden="true" />
        </div>
        <div className="min-w-0">
          <div className="flex items-baseline gap-2">
            <h1 className="text-lg font-semibold tracking-tight">Skills Box</h1>
            <span className="text-xs text-muted-foreground">全局 Skills 工作台</span>
          </div>
          <p className="truncate text-xs text-muted-foreground">
            管理跨 agent 共享的全局 skill
          </p>
        </div>
      </div>

      <div className="flex shrink-0 items-center gap-2">
        <EnvironmentBadge
          report={report}
          isLoading={isLoading}
          isError={isError}
        />
        <span
          data-testid="last-update-check"
          title={lastCheckTooltip}
          className={
            updateError
              ? "max-w-[13rem] truncate text-[10.5px] text-destructive"
              : "max-w-[13rem] truncate text-[10.5px] text-muted-foreground"
          }
        >
          {updateError ? `更新检查失败：${updateError.message}` : lastCheckLabel}
        </span>
        <Button
          variant="ghost"
          size="sm"
          onClick={onRecheck}
          disabled={rechecking}
          aria-label="重新检测环境"
          title="重新检测环境"
        >
          <RotateCcw
            className={rechecking ? "size-3.5 animate-spin" : "size-3.5"}
            aria-hidden="true"
          />
          重新检测
        </Button>
        <Button
          variant="outline"
          size="sm"
          disabled={!updatesEnabled || checkingUpdates}
          aria-busy={checkingUpdates}
          onClick={onCheckUpdates}
          title="只读检查更新；不会运行 skills check / skills update"
        >
          <RefreshCw
            className={checkingUpdates ? "size-3.5 animate-spin" : "size-3.5"}
            aria-hidden="true"
          />
          检查更新
        </Button>
        <Button
          variant="outline"
          size="sm"
          disabled={!updateReport}
          onClick={onUpdateAll}
          title={
            updateReport
              ? "查看更新候选与跳过项；执行更新将在后续接入"
              : "请先检查更新"
          }
        >
          <ArrowUpCircle className="size-3.5" aria-hidden="true" />
          更新全部
          {updateCandidateCount > 0 ? ` (${updateCandidateCount})` : ""}
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
      </div>
    </header>
  );
}
