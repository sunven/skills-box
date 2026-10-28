import { useRef } from "react";
import { AlertTriangle, LogOut, X } from "lucide-react";
import { Button } from "./ui/button";
import { useModalFocus } from "../hooks/use-modal-focus";
import type { ActiveMutationInfo, MutationKind } from "../lib/types";

const KIND_LABELS: Record<MutationKind, string> = {
  install: "安装",
  update: "更新",
  uninstall: "卸载",
};

interface ExitDialogProps {
  open: boolean;
  info: ActiveMutationInfo | null;
  pending: boolean;
  error: string | null;
  onCancel: () => void;
  onConfirm: () => void;
}

/**
 * Confirmation shown when the window is closed (or Cmd+Q) while a mutation
 * runs. Rust vetoed the close; confirming calls `quit_app`, which kills the
 * CLI process group, keeps the lock snapshot and leaves the interruption
 * marker for the next startup.
 */
export function ExitDialog({
  open,
  info,
  pending,
  error,
  onCancel,
  onConfirm,
}: ExitDialogProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);

  useModalFocus({
    open,
    panelRef,
    initialFocusRef: cancelRef,
    onClose: onCancel,
    canClose: () => !pending,
  });

  if (!open) return null;

  const title = info
    ? `${KIND_LABELS[info.kind]}任务正在运行`
    : "变更任务正在运行";

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/40 p-6"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !pending) onCancel();
      }}
    >
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="exit-dialog-title"
        data-testid="exit-dialog"
        className="w-full max-w-lg rounded-xl border border-destructive/30 bg-card p-4 shadow-xl"
      >
        <div className="flex items-start justify-between gap-3">
          <div>
            <h2
              id="exit-dialog-title"
              className="flex items-center gap-2 text-sm font-semibold text-destructive"
            >
              <LogOut className="size-4" aria-hidden="true" />
              退出 Skills Box？
            </h2>
            <p className="mt-1 text-[11px] leading-relaxed text-muted-foreground">
              {title}。退出会立即终止 skills CLI 的整个进程组。
            </p>
          </div>
          <Button
            ref={closeRef}
            variant="ghost"
            size="sm"
            aria-label="关闭退出确认"
            disabled={pending}
            onClick={onCancel}
            className="size-7 shrink-0 p-0"
          >
            <X className="size-4" aria-hidden="true" />
          </Button>
        </div>

        <div className="mt-3 flex items-start gap-2 rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2 text-[11px] text-destructive">
          <AlertTriangle
            className="mt-0.5 size-3.5 shrink-0"
            aria-hidden="true"
          />
          <div>
            <p className="font-medium">写入可能只完成一部分</p>
            <p className="mt-0.5 opacity-90">
              应用不会自动回滚；已创建的 lock
              快照会保留。下次启动会显示中断警告并校验 lock
              与快照，请根据备份手动确认 skill 状态。
            </p>
          </div>
        </div>

        {info ? (
          <div
            data-testid="exit-active-task"
            className="mt-3 rounded-lg border border-border bg-muted/40 px-3 py-2 text-[11px]"
          >
            <p>
              <span className="text-muted-foreground">任务：</span>
              <strong>{KIND_LABELS[info.kind]}</strong>
            </p>
            <p className="mt-0.5 break-all">
              <span className="text-muted-foreground">Skill：</span>
              {info.skills.join("、") || "—"}
            </p>
            <p className="mt-0.5 break-all font-mono text-[10px] text-muted-foreground">
              快照：{info.snapshotPaths.join("、") || "尚未创建"}
            </p>
          </div>
        ) : null}

        {error ? (
          <p role="alert" className="mt-3 text-[11px] text-destructive">
            {error}
          </p>
        ) : null}

        <div className="mt-3 flex items-center justify-end gap-2 border-t border-border pt-3">
          <Button
            ref={cancelRef}
            variant="ghost"
            size="sm"
            disabled={pending}
            onClick={onCancel}
          >
            继续等待
          </Button>
          <Button
            variant="destructive"
            size="sm"
            aria-busy={pending}
            disabled={pending}
            onClick={onConfirm}
          >
            {pending ? "正在退出…" : "确认退出"}
          </Button>
        </div>
      </div>
    </div>
  );
}
