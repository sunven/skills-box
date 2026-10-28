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
      className="modal-overlay"
      style={{ zIndex: 60 }}
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
        className="modal-panel max-w-lg"
      >
        <div className="modal-head">
          <div>
            <h2 id="exit-dialog-title" className="modal-title text-destructive">
              <LogOut className="size-4" aria-hidden="true" />
              退出 Skills Box？
            </h2>
            <p className="modal-sub">
              {title}。退出会立即终止 skills CLI 的整个进程组。
            </p>
          </div>
          <Button
            ref={closeRef}
            variant="ghost"
            size="icon"
            aria-label="关闭退出确认"
            disabled={pending}
            onClick={onCancel}
            className="size-7 shrink-0"
          >
            <X className="size-4" aria-hidden="true" />
          </Button>
        </div>

        <div className="modal-body">
          <div className="notice" data-tone="negative">
            <AlertTriangle className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
            <div>
              <p className="font-medium">写入可能只完成一部分</p>
              <p className="mt-0.5">
                应用不会自动回滚；已创建的 lock
                快照会保留。下次启动会显示中断警告并校验 lock
                与快照，请根据备份手动确认 skill 状态。
              </p>
            </div>
          </div>

          {info ? (
            <div
              data-testid="exit-active-task"
              className="panel-well mt-3 text-[12.5px]"
            >
              <p>
                <span className="text-muted-foreground">任务：</span>
                <strong>{KIND_LABELS[info.kind]}</strong>
              </p>
              <p className="mt-1 break-all">
                <span className="text-muted-foreground">Skill：</span>
                {info.skills.join("、") || "—"}
              </p>
              <p className="mono mt-1 break-all">
                快照：{info.snapshotPaths.join("、") || "尚未创建"}
              </p>
            </div>
          ) : null}

          {error ? (
            <p role="alert" className="mt-3 text-[12.5px] text-destructive">
              {error}
            </p>
          ) : null}
        </div>

        <div className="modal-foot justify-end">
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
