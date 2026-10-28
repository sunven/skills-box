import { Boxes, Download, RefreshCw, RotateCcw } from "lucide-react";
import { Button } from "./ui/button";
import { EnvironmentBadge } from "./environment-badge";
import type { EnvReport } from "../lib/types";

interface AppHeaderProps {
  report?: EnvReport;
  isLoading: boolean;
  isError: boolean;
  mutationsEnabled: boolean;
  rechecking: boolean;
  onRecheck: () => void;
  onInstall: () => void;
}

export function AppHeader({
  report,
  isLoading,
  isError,
  mutationsEnabled,
  rechecking,
  onRecheck,
  onInstall,
}: AppHeaderProps) {
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
          disabled
          title="更新检查尚未接入"
        >
          <RefreshCw className="size-3.5" aria-hidden="true" />
          检查更新
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
