import { useCliCommandRecords, useCliCommandPreview } from "../hooks/use-cli-commands";
import type { CliCommandEntry, CliCommandRequest } from "../lib/cli-commands";
import type { ScopeId } from "../lib/types";
import { asAppError } from "../lib/api";
import { Button } from "./ui/button";

export function CliCommandDisplay({ command, label = "将执行的命令" }: { command: string; label?: string }) {
  return (
    <div className="cli-command-display">
      <p className="text-xs text-muted-foreground">{label}</p>
      <pre className="batch-command"><code>{command}</code></pre>
    </div>
  );
}
export function CliCommandPreviewList({ entries }: { entries: CliCommandEntry[] }) {
  return <div className="cli-command-list">{entries.map((entry, index) => entry.command ?
    <CliCommandDisplay key={`${entry.name}:${index}`} command={entry.command} /> : entry.reason ?
      <p key={`${entry.name}:${index}`} className="text-xs text-muted-foreground">{entry.name}：{entry.reason}</p> : null)}</div>;
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
  if (matching.length === 0) return null;
  return (
    <div className="cli-command-list">
      {matching.map((record) => (
        <CliCommandDisplay
          key={record.id}
          command={record.command}
          label={origin
            ? `最近实际调用 · ${new Date(record.startedAt).toLocaleString("zh-CN")} · ${labels[record.status]}`
            : labels[record.status]}
        />
      ))}
    </div>
  );
}

export function CliPreviewStatus({ preview }: { preview: ReturnType<typeof useCliCommandPreview> }) {
  if (preview.isFetching) return <p role="status" className="mt-2 text-xs text-muted-foreground">正在准备执行命令…</p>;
  if (preview.error) return <div className="mt-2 text-xs"><p role="alert">{asAppError(preview.error)?.message ?? "无法读取执行命令。"}</p><Button variant="ghost" size="sm" onClick={() => void preview.refetch()}>刷新命令</Button></div>;
  return preview.data ? <CliCommandPreviewList entries={preview.data.entries} /> : null;
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
      <Button
        variant="ghost"
        size="sm"
        disabled={disabled || !preview.data || preview.isFetching || preview.isError}
        onClick={() => { if (preview.data) onExecute(preview.data.id); }}
      >
        {label}
      </Button>
    </div>
  );
}
