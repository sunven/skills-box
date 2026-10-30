import { useRef } from "react";
import { HardDrive, ShieldAlert, Unlink, X } from "lucide-react";
import { Button } from "./ui/button";
import { MutationProgress } from "./mutation/mutation-progress";
import { useModalFocus } from "../hooks/use-modal-focus";
import { ATTACHMENT_LABELS } from "../lib/inventory-view";
import type { AgentAttachment, AppError } from "../lib/types";

interface DetachDialogProps {
  open: boolean;
  skillName: string;
  canonicalPath: string | null;
  target: AgentAttachment | null;
  otherAttachments: AgentAttachment[];
  pending: boolean;
  progressMessage?: string;
  error: AppError | null;
  /** Disabled for unavailable/error scopes. */
  disabled?: boolean;
  onClose: () => void;
  onConfirm: () => void;
}

/**
 * Confirmation for removing one agent's attachment. It never mutates by
 * itself; the parent owns the invoke, invalidation and success notice.
 */
export function DetachDialog({
  open,
  skillName,
  canonicalPath,
  target,
  otherAttachments,
  pending,
  progressMessage,
  error,
  disabled = false,
  onClose,
  onConfirm,
}: DetachDialogProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const pendingRef = useRef(pending);
  pendingRef.current = pending;

  useModalFocus({
    open,
    panelRef,
    initialFocusRef: closeRef,
    onClose,
    canClose: () => !pendingRef.current,
  });

  if (!open || !target) return null;

  const others = otherAttachments
    .map((attachment) => attachment.agentName)
    .join("、");

  return (
    <div
      className="modal-overlay"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !pending) {
          onClose();
        }
      }}
    >
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="detach-dialog-title"
        data-testid="detach-dialog"
        data-agent-id={target.agentId}
        className="modal-panel max-w-lg"
      >
        <div className="modal-head">
          <div>
            <h2 id="detach-dialog-title" className="modal-title">
              <Unlink className="size-4 text-muted-foreground" aria-hidden="true" />
              断开 agent 挂载 · {skillName}
            </h2>
            <p className="modal-sub">
              断开只移除该 agent 的挂载；canonical 与 lock 将保留，其他 agent
              不受影响。
            </p>
          </div>
          <Button
            ref={closeRef}
            variant="ghost"
            size="icon"
            aria-label="关闭断开确认"
            disabled={pending}
            onClick={onClose}
            className="size-7 shrink-0"
          >
            <X className="size-4" aria-hidden="true" />
          </Button>
        </div>

        <div className="modal-body">
          <dl className="panel-well">
            <div className="kv-row">
              <dt>agent</dt>
              <dd data-testid="detach-agent">{target.agentName}</dd>
            </div>
            <div className="kv-row">
              <dt>挂载模式</dt>
              <dd data-testid="detach-mode">{ATTACHMENT_LABELS[target.mode]}</dd>
            </div>
            <div className="kv-row">
              <dt>挂载路径</dt>
              <dd className="mono">{target.path}</dd>
            </div>
            <div className="kv-row">
              <dt>canonical（保留）</dt>
              <dd className="mono">{canonicalPath ?? "—"}</dd>
            </div>
            <div className="kv-row">
              <dt>lock 条目</dt>
              <dd>保留，不修改</dd>
            </div>
            <div className="kv-row">
              <dt>其他受影响的 agent</dt>
              <dd>{others || "无（断开后显示「未挂载」）"}</dd>
            </div>
          </dl>

          {target.mode === "copy" ? (
            <div
              data-testid="detach-copy-warning"
              className="notice mt-3"
              data-tone="warning"
            >
              <ShieldAlert className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
              <div>
                <p className="font-medium">copy 挂载会先备份再删除</p>
                <p className="mt-0.5">
                  copy 挂载断开前会创建备份（copy 目录 + lock
                  快照）。删除前会检查其他 agent 的符号链接：若仍指向该目录，断开会被阻止，状态不变，且不会自动改指。
                </p>
              </div>
            </div>
          ) : (
            <div className="notice mt-3">
              <HardDrive className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
              符号链接只做 unlink；不会触碰 canonical、lock 或其他 agent
              的挂载。
            </div>
          )}

          {pending ? (
            <MutationProgress
              phase={null}
              label="执行中"
              message={progressMessage ?? "正在断开挂载…"}
              completed={0}
              total={0}
              indeterminate
              cancelling={false}
            />
          ) : null}

          {error ? (
            <div
              role="alert"
              data-testid="detach-error"
              className="notice mt-3"
              data-tone="negative"
            >
              <div>
                <p className="font-medium">{error.message}</p>
                {error.detail ? (
                  <p className="mono mt-0.5 break-all">{error.detail}</p>
                ) : null}
                {error.hint ? <p className="mt-0.5">{error.hint}</p> : null}
              </div>
            </div>
          ) : null}
        </div>

        <div className="modal-foot">
          <div className="modal-foot-note">
            断开不调用 skills CLI；不提供断链修复。
          </div>
          <div className="flex items-center gap-2">
            <Button variant="ghost" size="sm" disabled={pending} onClick={onClose}>
              取消
            </Button>
            <Button
              size="sm"
              aria-busy={pending}
              disabled={pending || disabled}
              onClick={onConfirm}
            >
              {pending ? "正在断开…" : "确认断开"}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
