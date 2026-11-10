import { MarketplaceCliHelp } from "./marketplace-cli-help";
import { useCliCommandRecords, useCliCommandPreview } from "../hooks/use-cli-commands";
import type { CliCommandEntry, CliCommandRequest } from "../lib/cli-commands";
import type { ScopeId } from "../lib/types";
import { asAppError } from "../lib/api";
import { Button } from "./ui/button";
import { CommandHelp } from "./ui/command-help";

export function CliCommandDisplay({ command, label = "将执行的命令" }: { command: string; label?: string }) {
  return (
    <div className="cli-command-display">
      <p className="text-xs text-muted-foreground">{label}</p>
      <pre className="batch-command"><code>{command}</code></pre>
    </div>
  );
}
export function CliCommandPreviewList({ entries }: { entries: CliCommandEntry[] }) {
  if (entries.length === 0) return <p className="text-xs text-muted-foreground">没有可执行的 CLI 命令。</p>;
  return <div className="cli-command-list">{entries.map((entry, index) => entry.command ?
    <CliCommandDisplay key={`${entry.name}:${index}`} command={entry.command} label={entries.length > 1 ? `将执行的命令 · ${entry.name}` : undefined} /> :
      <p key={`${entry.name}:${index}`} className="text-xs text-muted-foreground">{entry.name}：{entry.reason ?? "由应用直接处理，不执行 CLI 命令。"}</p>)}</div>;
}
export function CliPreviewHelp({ preview, emptyMessage = "请先选择操作目标。", label = "查看将执行的命令" }: {
  preview: ReturnType<typeof useCliCommandPreview>;
  emptyMessage?: string;
  label?: string;
}) {
  return <CommandHelp label={label}>
    {preview.isFetching ? <p className="text-xs text-muted-foreground">正在准备执行命令…</p> :
      preview.error ? <p className="text-xs text-muted-foreground">{asAppError(preview.error)?.message ?? "无法读取执行命令。"}</p> :
      preview.data ? <CliCommandPreviewList entries={preview.data.entries} /> :
      <p className="text-xs text-muted-foreground">{emptyMessage}</p>}
  </CommandHelp>;
}
const labels = {
  running: "正在执行的命令",
  succeeded: "已执行的命令",
  failed: "执行失败的命令",
  cancelled: "已取消的命令",
} as const;
export function ActualCliCommands({ taskId, origin, source }: { taskId?: string | null; origin?: "environment" | "discovery"; source?: string }) {
  const records = useCliCommandRecords().data ?? [];
  const matching = records.filter((record) => taskId ? record.context.taskId === taskId :
    origin && record.context.origin === origin && (source === undefined || record.context.source === source));
  if (!taskId && !origin) return null;
  return (
    <div className="cli-command-records">
      <span>{origin === "environment" ? "CLI 调用记录" : origin === "discovery" ? "来源调用记录" : "调用记录"}</span>
      <CommandHelp label={source ? `查看 ${source} 的实际调用命令` : origin === "environment" ? "查看环境检测的实际调用命令" : "查看实际调用命令"}>
        {matching.length === 0 ? <p className="text-xs text-muted-foreground">尚无实际 CLI 调用。</p> : null}
        {matching.map((record) => (
          <CliCommandDisplay
            key={record.id}
            command={record.command}
            label={`${origin ? "最近实际调用 · " : ""}${new Date(record.startedAt).toLocaleString("zh-CN")} · ${labels[record.status]}`}
          />
        ))}
      </CommandHelp>
    </div>
  );
}

export function CliPreviewStatus({ preview }: { preview: ReturnType<typeof useCliCommandPreview> }) {
  if (preview.isFetching) return <p role="status" className="mt-2 text-xs text-muted-foreground">正在准备执行命令…</p>;
  if (asAppError(preview.error)?.code === "marketplace.cli_unavailable" || preview.data?.marketplaceCliRequired) return <MarketplaceCliHelp onReady={() => void preview.refetch()} />;
  if (preview.error) return <div className="mt-2 text-xs"><p role="alert">{asAppError(preview.error)?.message ?? "无法读取执行命令。"}</p><Button variant="ghost" size="sm" onClick={() => void preview.refetch()}>刷新命令</Button></div>;
  return null;
}
export function CliRetryControl({ scope, request, label, disabled, onExecute }: {
  scope: ScopeId;
  request: CliCommandRequest;
  label: string;
  disabled: boolean;
  onExecute: (previewId: string) => void;
}) {
  const preview = useCliCommandPreview(scope, disabled ? null : request);
  return (
    <div className="cli-command-list">
      <CliPreviewStatus preview={preview} />
      <div className="flex items-center gap-2">
        <Button
          variant="ghost"
          size="sm"
          disabled={disabled || !preview.data || preview.isFetching || preview.isError}
          onClick={() => { if (preview.data) onExecute(preview.data.id); }}
        >
          {label}
        </Button>
        <CliPreviewHelp preview={preview} />
      </div>
    </div>
  );
}
