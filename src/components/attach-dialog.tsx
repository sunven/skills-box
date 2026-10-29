import { useEffect, useMemo, useRef, useState } from "react";
import { Link2, X } from "lucide-react";
import { Button } from "./ui/button";
import { AgentLogo } from "./agent-logo";
import { useModalFocus } from "../hooks/use-modal-focus";
import { asAppError, attachAgent, fallbackError } from "../lib/api";
import type { AgentAttachment, AppError, InventoryAgent } from "../lib/types";

interface AttachDialogProps {
  open: boolean;
  skillName: string;
  agents: InventoryAgent[];
  attachments: AgentAttachment[];
  onClose: () => void;
  onChanged: () => void;
}

/**
 * Attach one managed skill to agents that do not have it yet. Multi-select,
 * executed sequentially through the native attach command; a partial failure
 * keeps the dialog open with the per-agent reasons.
 */
export function AttachDialog({
  open,
  skillName,
  agents,
  attachments,
  onClose,
  onChanged,
}: AttachDialogProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<AppError | null>(null);

  const attachedIds = useMemo(
    () => new Set(attachments.map((attachment) => attachment.agentId)),
    [attachments],
  );
  const eligible = useMemo(
    () =>
      agents.filter(
        (agent) =>
          agent.detected &&
          !agent.universal &&
          !agent.shared &&
          !attachedIds.has(agent.id),
      ),
    [agents, attachedIds],
  );

  useModalFocus({
    open,
    panelRef,
    initialFocusRef: closeRef,
    onClose,
    canClose: () => !pending,
  });

  useEffect(() => {
    if (!open) return;
    setSelected(new Set());
    setPending(false);
    setError(null);
  }, [open]);

  if (!open) return null;

  const toggle = (id: string) => {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const confirm = async () => {
    const targets = eligible.filter((agent) => selected.has(agent.id));
    if (targets.length === 0) {
      setError(fallbackError("请选择至少一个 agent。"));
      return;
    }
    setPending(true);
    setError(null);
    const failures: string[] = [];
    for (const agent of targets) {
      try {
        await attachAgent(skillName, agent.id);
      } catch (cause) {
        const mapped = asAppError(cause);
        failures.push(
          `${agent.displayName}：${mapped?.message ?? "挂载失败"}${
            mapped?.detail ? `（${mapped.detail}）` : ""
          }`,
        );
      }
    }
    setPending(false);
    onChanged();
    if (failures.length === 0) {
      onClose();
      return;
    }
    setError({
      message:
        failures.length === targets.length
          ? "挂载失败。"
          : "部分 agent 挂载失败。",
      detail: failures.join("\n"),
      retryable: false,
    });
    setSelected(new Set());
  };

  return (
    <div
      className="modal-overlay"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !pending) onClose();
      }}
    >
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="attach-dialog-title"
        data-testid="attach-dialog"
        className="modal-panel max-w-lg"
      >
        <div className="modal-head">
          <div>
            <h2 id="attach-dialog-title" className="modal-title">
              <Link2 className="size-4 text-muted-foreground" aria-hidden="true" />
              挂载到 agent · {skillName}
            </h2>
            <p className="modal-sub">
              为已安装的 skill 追加挂载；canonical 与 lock
              保持不变。只列出已检测到、未挂载且不共享 canonical 的 agent。
            </p>
          </div>
          <Button
            ref={closeRef}
            variant="ghost"
            size="icon"
            aria-label="关闭挂载对话框"
            disabled={pending}
            onClick={onClose}
            className="size-7 shrink-0"
          >
            <X className="size-4" aria-hidden="true" />
          </Button>
        </div>

        <div className="modal-body">
          {eligible.length === 0 ? (
            <p className="notice">
              没有可挂载的 agent。未检测到或已挂载的 agent 不会出现在这里。
            </p>
          ) : (
            <ul className="space-y-1.5" data-testid="attach-agent-list">
              {eligible.map((agent) => (
                <li key={agent.id} className="attach-row">
                  <label className="flex w-full cursor-pointer items-center gap-2.5">
                    <input
                      type="checkbox"
                      className="size-4 accent-[var(--primary)]"
                      checked={selected.has(agent.id)}
                      disabled={pending}
                      aria-label={`挂载到 ${agent.displayName}`}
                      onChange={() => toggle(agent.id)}
                    />
                    <span className="agent-token" aria-hidden="true">
                      <AgentLogo
                        agentId={agent.id}
                        agentName={agent.displayName}
                      />
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="attach-name block">
                        {agent.displayName}
                      </span>
                      <span
                        className="attach-path mono block"
                        title={agent.globalSkillsDir ?? ""}
                      >
                        {agent.globalSkillsDir ?? "—"}
                      </span>
                    </span>
                    {agent.origin !== "catalog" ? (
                      <span className="attach-mode shrink-0">
                        {agent.origin === "builtin" ? "内置" : "自定义"}
                      </span>
                    ) : null}
                  </label>
                </li>
              ))}
            </ul>
          )}

          {error ? (
            <div
              role="alert"
              data-testid="attach-error"
              className="notice mt-3"
              data-tone="negative"
            >
              <div>
                <p className="font-medium">{error.message}</p>
                {error.detail ? (
                  <p className="mono mt-0.5 whitespace-pre-wrap break-all">
                    {error.detail}
                  </p>
                ) : null}
              </div>
            </div>
          ) : null}
        </div>

        <div className="modal-foot">
          <div className="modal-foot-note">
            挂载由 Skills Box 原生创建（符号链接），不调用 skills CLI。
          </div>
          <div className="flex items-center gap-2">
            <Button variant="ghost" size="sm" disabled={pending} onClick={onClose}>
              取消
            </Button>
            <Button
              size="sm"
              aria-busy={pending}
              disabled={pending || eligible.length === 0 || selected.size === 0}
              onClick={() => void confirm()}
            >
              {pending ? "正在挂载…" : `挂载${selected.size > 0 ? `（${selected.size}）` : ""}`}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
