import { useEffect, useMemo, useRef, useState } from "react";
import { Link2, X } from "lucide-react";
import { Button } from "./ui/button";
import { AgentLogo } from "./agent-logo";
import { MutationError } from "./mutation/mutation-error";
import { MutationProgress } from "./mutation/mutation-progress";
import { MutationResult } from "./mutation/mutation-result";
import { useModalFocus } from "../hooks/use-modal-focus";
import { asAppError, attachAgent, fallbackError } from "../lib/api";
import { promiseDriver, useMutation } from "../lib/mutation";
import type { MutationRow } from "../lib/mutation";
import type {
  AgentAttachment,
  InventoryAgent,
  ScopeId,
} from "../lib/types";

interface AttachDialogProps {
  open: boolean;
  scope: ScopeId;
  skillId: string;
  skillName: string;
  agents: InventoryAgent[];
  attachments: AgentAttachment[];
  /** Disabled for unavailable/error scopes. */
  disabled?: boolean;
  onClose: () => void;
}

/**
 * Attach one managed skill to agents that do not have it yet. Multi-select,
 * executed sequentially through the native attach command; a partial failure
 * keeps the dialog open with the per-agent reasons.
 */
export function AttachDialog({
  open,
  scope,
  skillId,
  skillName,
  agents,
  attachments,
  disabled = false,
  onClose,
}: AttachDialogProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const mutation = useMutation({
    action: "attach",
    scope,
    skillId,
    fallbackFailure: "挂载失败。",
    fallbackStart: "挂载失败。",
  });
  const {
    running,
    message,
    completed,
    total,
    result,
    error,
    startError,
  } = mutation.state;

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
    canClose: () => !mutation.stateRef.current.running,
  });

  useEffect(() => {
    if (!open) return;
    setSelected(new Set());
    mutation.reset();
  }, [open, mutation.reset]);

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
    if (targets.length === 0 || running || disabled) return;
    const outcome = await mutation.start(
      promiseDriver({
        total: targets.length,
        message: "正在挂载…",
        indeterminate: false,
        run: async (report) => {
          const rows: MutationRow[] = [];
          for (let index = 0; index < targets.length; index += 1) {
            const agent = targets[index]!;
            report({
              completed: index,
              total: targets.length,
              message: `正在挂载到 ${agent.displayName}…`,
            });
            try {
              await attachAgent(skillId, agent.id, scope);
              rows.push({
                name: agent.displayName,
                status: "success",
                statusLabel: "已挂载",
              });
            } catch (cause) {
              rows.push({
                name: agent.displayName,
                status: "failed",
                statusLabel: "挂载失败",
                error: asAppError(cause) ?? fallbackError("挂载失败。"),
              });
            }
            report({ completed: index + 1, total: targets.length });
          }
          return { rows };
        },
      }),
    );
    if (outcome?.outcome === "success") {
      onClose();
      return;
    }
    setSelected(new Set());
  };

  return (
    <div
      className="modal-overlay"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !running) onClose();
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
            disabled={running}
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
                      disabled={running || disabled}
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

          {running ? (
            <MutationProgress
              phase={null}
              label="执行中"
              message={message}
              completed={completed}
              total={total}
              cancelling={false}
            />
          ) : null}

          {result && result.outcome !== "success" ? (
            <MutationResult
              action="attach"
              result={result}
              testIdPrefix="attach"
              showLog={false}
            />
          ) : null}
          <MutationError
            error={startError ?? error}
            testId="attach-error"
            monoDetail
          />
        </div>

        <div className="modal-foot">
          <div className="modal-foot-note">
            挂载由 Skills Box 原生创建（符号链接），不调用 skills CLI。
          </div>
          <div className="flex items-center gap-2">
            <Button variant="ghost" size="sm" disabled={running} onClick={onClose}>
              取消
            </Button>
            <Button
              size="sm"
              aria-busy={running}
              disabled={running || disabled || eligible.length === 0 || selected.size === 0}
              onClick={() => void confirm()}
            >
              {running ? "正在挂载…" : `挂载${selected.size > 0 ? `（${selected.size}）` : ""}`}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
