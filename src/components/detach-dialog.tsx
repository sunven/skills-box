import { useEffect, useRef } from "react";
import { HardDrive, ShieldAlert, Unlink, X } from "lucide-react";
import { Button } from "./ui/button";
import { ATTACHMENT_LABELS } from "../lib/inventory-view";
import type { AgentAttachment } from "../lib/types";

export interface DetachDialogError {
  code?: string;
  message: string;
  detail?: string;
  hint?: string;
}

interface DetachDialogProps {
  open: boolean;
  skillName: string;
  canonicalPath: string | null;
  target: AgentAttachment | null;
  otherAttachments: AgentAttachment[];
  pending: boolean;
  error: DetachDialogError | null;
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
  error,
  onClose,
  onConfirm,
}: DetachDialogProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const pendingRef = useRef(pending);
  pendingRef.current = pending;

  useEffect(() => {
    if (!open) return;
    const previousFocus = document.activeElement as HTMLElement | null;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        if (!pendingRef.current) onCloseRef.current();
        return;
      }
      if (event.key !== "Tab") return;
      const focusables = panelRef.current?.querySelectorAll<HTMLElement>(
        'button:not([disabled]), [href], input:not([disabled]), select, textarea, [tabindex]:not([tabindex="-1"])',
      );
      if (!focusables || focusables.length === 0) return;
      const first = focusables[0]!;
      const last = focusables[focusables.length - 1]!;
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", handleKeyDown);
    closeRef.current?.focus();
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
      previousFocus?.focus();
    };
  }, [open]);

  if (!open || !target) return null;

  const others = otherAttachments
    .map((attachment) => attachment.agentName)
    .join("、");

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/30 p-6"
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
        className="flex max-h-[86vh] w-full max-w-lg flex-col rounded-xl border border-border bg-card p-4 shadow-xl"
      >
        <div className="flex items-start justify-between gap-3">
          <div>
            <h2
              id="detach-dialog-title"
              className="flex items-center gap-2 text-sm font-semibold"
            >
              <Unlink className="size-4 text-primary" aria-hidden="true" />
              断开 agent 挂载 · {skillName}
            </h2>
            <p className="mt-1 text-[11px] leading-relaxed text-muted-foreground">
              断开只移除该 agent 的挂载；canonical 与 lock 将保留，其他 agent
              不受影响。
            </p>
          </div>
          <Button
            ref={closeRef}
            variant="ghost"
            size="sm"
            aria-label="关闭断开确认"
            disabled={pending}
            onClick={onClose}
            className="size-7 shrink-0 p-0"
          >
            <X className="size-4" aria-hidden="true" />
          </Button>
        </div>

        <div className="mt-3 min-h-0 flex-1 overflow-y-auto">
          <dl className="rounded-lg border border-border bg-muted/40 p-3 text-xs">
            <div className="flex justify-between gap-4 py-0.5">
              <dt className="text-muted-foreground">agent</dt>
              <dd className="font-medium" data-testid="detach-agent">
                {target.agentName}
              </dd>
            </div>
            <div className="flex justify-between gap-4 py-0.5">
              <dt className="text-muted-foreground">挂载模式</dt>
              <dd data-testid="detach-mode">{ATTACHMENT_LABELS[target.mode]}</dd>
            </div>
            <div className="flex justify-between gap-4 py-0.5">
              <dt className="shrink-0 text-muted-foreground">挂载路径</dt>
              <dd className="break-all text-right font-mono text-[10.5px]">
                {target.path}
              </dd>
            </div>
            <div className="flex justify-between gap-4 py-0.5">
              <dt className="shrink-0 text-muted-foreground">
                canonical（保留）
              </dt>
              <dd className="break-all text-right font-mono text-[10.5px]">
                {canonicalPath ?? "—"}
              </dd>
            </div>
            <div className="flex justify-between gap-4 py-0.5">
              <dt className="text-muted-foreground">lock 条目</dt>
              <dd>保留，不修改</dd>
            </div>
            <div className="flex justify-between gap-4 py-0.5">
              <dt className="shrink-0 text-muted-foreground">
                其他受影响的 agent
              </dt>
              <dd className="text-right">
                {others || "无（断开后显示「未挂载」）"}
              </dd>
            </div>
          </dl>

          {target.mode === "copy" ? (
            <div
              data-testid="detach-copy-warning"
              className="mt-3 flex items-start gap-2 rounded-lg border border-warning/30 bg-warning-soft px-3 py-2 text-[11px] text-warning"
            >
              <ShieldAlert
                className="mt-0.5 size-3.5 shrink-0"
                aria-hidden="true"
              />
              <div>
                <p className="font-medium">copy 挂载会先备份再删除</p>
                <p className="mt-0.5 opacity-90">
                  copy 挂载断开前会创建备份（copy 目录 + lock
                  快照）。删除前会检查其他 agent 的符号链接：若仍指向该目录，断开会被阻止，状态不变，且不会自动改指。
                </p>
              </div>
            </div>
          ) : (
            <div className="mt-3 flex items-start gap-2 rounded-md border border-border bg-muted/50 px-2.5 py-2 text-[10.5px] leading-relaxed text-muted-foreground">
              <HardDrive
                className="mt-0.5 size-3.5 shrink-0"
                aria-hidden="true"
              />
              符号链接只做 unlink；不会触碰 canonical、lock 或其他 agent
              的挂载。
            </div>
          )}

          {error ? (
            <div
              role="alert"
              data-testid="detach-error"
              className="mt-3 rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive"
            >
              <p className="font-medium">{error.message}</p>
              {error.detail ? (
                <p className="mt-0.5 break-all text-[10.5px] opacity-80">
                  {error.detail}
                </p>
              ) : null}
              {error.hint ? (
                <p className="mt-0.5 text-[10.5px] opacity-90">{error.hint}</p>
              ) : null}
            </div>
          ) : null}
        </div>

        <div className="mt-3 flex items-center justify-between gap-2 border-t border-border pt-3">
          <div className="text-[10.5px] text-muted-foreground">
            断开不调用 skills CLI；不提供断链修复。
          </div>
          <div className="flex items-center gap-2">
            <Button
              variant="ghost"
              size="sm"
              disabled={pending}
              onClick={onClose}
            >
              取消
            </Button>
            <Button
              size="sm"
              aria-busy={pending}
              disabled={pending}
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
