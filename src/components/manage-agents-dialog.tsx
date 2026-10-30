import { useMemo, useRef, useState, type FormEvent } from "react";
import { Plus, Settings2, Trash2, X } from "lucide-react";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { AgentLogo } from "./agent-logo";
import { MutationError } from "./mutation/mutation-error";
import { MutationProgress } from "./mutation/mutation-progress";
import { useModalFocus } from "../hooks/use-modal-focus";
import { addExtraAgent, fallbackError, removeExtraAgent } from "../lib/api";
import { promiseDriver, useMutation } from "../lib/mutation";
import type { AppError, InventoryAgent, InventorySkill } from "../lib/types";
import { GLOBAL_SCOPE } from "../lib/types";

interface ManageAgentsDialogProps {
  open: boolean;
  agents: InventoryAgent[];
  skills: InventorySkill[];
  warnings: string[];
  onClose: () => void;
  onChanged: () => void;
}

/**
 * Extra-agent management. Builtin extras are read-only; user extras can be
 * added and deleted (deletion detaches every attached skill first). The JSON
 * file stays the source of truth and can be edited by hand.
 */
export function ManageAgentsDialog({
  open,
  agents,
  skills,
  warnings,
  onClose,
  onChanged,
}: ManageAgentsDialogProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const [id, setId] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [skillsDir, setSkillsDir] = useState("");
  const [addError, setAddError] = useState<AppError | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<string | null>(null);
  const addMutation = useMutation({
    action: "manage_add",
    scope: GLOBAL_SCOPE,
    onEnvironmentRecheck: onChanged,
    fallbackFailure: "添加额外 agent 失败。",
    fallbackStart: "添加额外 agent 失败。",
  });
  const removeMutation = useMutation({
    action: "manage_remove",
    scope: GLOBAL_SCOPE,
    onEnvironmentRecheck: onChanged,
    fallbackFailure: "删除额外 agent 失败。",
    fallbackStart: "删除额外 agent 失败。",
  });
  const adding = addMutation.state.running;

  const extras = useMemo(
    () => agents.filter((agent) => agent.origin !== "catalog"),
    [agents],
  );
  const attachmentCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const skill of skills) {
      const seenAgents = new Set<string>();
      for (const attachment of skill.attachments) {
        if (seenAgents.has(attachment.agentId)) continue;
        seenAgents.add(attachment.agentId);
        counts.set(attachment.agentId, (counts.get(attachment.agentId) ?? 0) + 1);
      }
    }
    return counts;
  }, [skills]);

  const busy = adding || removeMutation.state.running;

  useModalFocus({
    open,
    panelRef,
    initialFocusRef: closeRef,
    onClose,
    canClose: () => !busy,
  });

  if (!open) return null;

  const resetAddForm = () => {
    setId("");
    setDisplayName("");
    setSkillsDir("");
    setAddError(null);
  };

  const handleAdd = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmedId = id.trim();
    const trimmedName = displayName.trim();
    const trimmedDir = skillsDir.trim();
    if (!/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(trimmedId)) {
      setAddError({
        message: "id 不合法。",
        hint: "只能包含字母、数字、`-`、`_`、`.`，且不能以 `-` 开头。",
        retryable: false,
      });
      return;
    }
    if (!trimmedName) {
      setAddError(fallbackError("请填写显示名。"));
      return;
    }
    if (!trimmedDir.startsWith("~/") && !trimmedDir.startsWith("/")) {
      setAddError({
        message: "skills 目录不合法。",
        hint: "必须以 `~/` 开头或是绝对路径，例如 ~/.workbuddy/skills。",
        retryable: false,
      });
      return;
    }
    setAddError(null);
    setNotice(null);
    await addMutation.start(
      promiseDriver({
        total: 0,
        message: "正在保存额外 agent…",
        indeterminate: true,
        run: async () => {
          const added = await addExtraAgent({
            id: trimmedId,
            displayName: trimmedName,
            skillsDir: trimmedDir,
          });
          setNotice(
            `已添加 ${added.displayName}（${added.skillsDir}）。检测到目录后即可作为安装目标。`,
          );
          resetAddForm();
          return {
            rows: [
              {
                name: added.displayName,
                status: "success" as const,
                statusLabel: "已添加",
              },
            ],
          };
        },
      }),
    );
  };

  const handleDelete = async (agentId: string) => {
    setDeleting(agentId);
    setNotice(null);
    await removeMutation.start(
      promiseDriver({
        total: 0,
        message: "正在删除额外 agent…",
        indeterminate: true,
        run: async () => {
          const result = await removeExtraAgent(agentId);
          setNotice(
            result.detached.length > 0
              ? `已删除 ${result.displayName}，并断开 ${result.detached.length} 个挂载。`
              : `已删除 ${result.displayName}。`,
          );
          setConfirming(null);
          return {
            rows: [
              {
                name: result.displayName,
                status: "success" as const,
                statusLabel: "已删除",
              },
            ],
          };
        },
      }),
    );
    setDeleting(null);
  };

  return (
    <div
      className="modal-overlay"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !busy) onClose();
      }}
    >
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="manage-agents-title"
        data-testid="manage-agents-dialog"
        className="modal-panel max-w-2xl"
      >
        <div className="modal-head">
          <div>
            <h2 id="manage-agents-title" className="modal-title">
              <Settings2
                className="size-4 text-muted-foreground"
                aria-hidden="true"
              />
              管理 agent
            </h2>
            <p className="modal-sub">
              额外 agent 不在 npx skills 的内置目录中，由 Skills Box
              直接创建挂载。内置定义随应用发布；自定义定义保存在应用数据目录的
              agents.json，可手动编辑。
            </p>
          </div>
          <Button
            ref={closeRef}
            variant="ghost"
            size="icon"
            aria-label="关闭管理 agent"
            disabled={busy}
            onClick={onClose}
            className="size-7 shrink-0"
          >
            <X className="size-4" aria-hidden="true" />
          </Button>
        </div>

        <div className="modal-body detail-scroll">
          {warnings.length > 0 ? (
            <div className="notice mb-3" data-tone="warning">
              <div>
                <p className="font-medium">配置警告</p>
                <ul className="mt-0.5 list-disc pl-4">
                  {warnings.map((warning) => (
                    <li key={warning}>{warning}</li>
                  ))}
                </ul>
              </div>
            </div>
          ) : null}

          {notice ? (
            <div
              role="status"
              data-testid="manage-agents-notice"
              className="notice mb-3"
              data-tone="positive"
            >
              {notice}
            </div>
          ) : null}

          {busy ? (
            <MutationProgress
              phase={null}
              label="执行中"
              message={
                adding
                  ? addMutation.state.message
                  : removeMutation.state.message
              }
              completed={0}
              total={0}
              indeterminate
              cancelling={false}
            />
          ) : null}

          <section className="detail-section">
            <h4 className="detail-section-title">
              额外 agent <span className="count">{extras.length} 个</span>
            </h4>
            {extras.length === 0 ? (
              <p className="notice mt-2.5">还没有额外 agent。</p>
            ) : (
              <ul className="mt-2.5 space-y-1.5">
                {extras.map((agent) => {
                  const count = attachmentCounts.get(agent.id) ?? 0;
                  const confirmingThis = confirming === agent.id;
                  return (
                    <li
                      key={agent.id}
                      data-testid="manage-agent-row"
                      data-agent-id={agent.id}
                      className="attach-row"
                    >
                      <span className="agent-token" aria-hidden="true">
                        <AgentLogo
                          agentId={agent.id}
                          agentName={agent.displayName}
                        />
                      </span>
                      <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
                          <span className="attach-name">
                            {agent.displayName}
                          </span>
                          <span className="attach-mode">
                            {agent.origin === "builtin" ? "内置" : "自定义"}
                          </span>
                          {!agent.detected ? (
                            <span className="attach-mode">未检测到</span>
                          ) : (
                            <span className="text-[11.5px] text-muted-foreground">
                              {count} 个挂载
                            </span>
                          )}
                        </div>
                        <p className="attach-path mono" title={agent.globalSkillsDir ?? ""}>
                          {agent.globalSkillsDir ?? "—"}
                        </p>
                        {confirmingThis ? (
                          <p className="mt-1 text-[12px] text-destructive">
                            删除会先断开 {count} 个挂载，且不可撤销。
                          </p>
                        ) : null}
                      </div>
                      {agent.origin === "user" ? (
                        <div className="ml-auto flex shrink-0 items-center gap-1">
                          {confirmingThis ? (
                            <>
                              <Button
                                variant="ghost"
                                size="sm"
                                className="h-7 px-2 text-[12px]"
                                disabled={deleting !== null}
                                onClick={() => {
                                  setConfirming(null);
                                  removeMutation.reset();
                                }}
                              >
                                取消
                              </Button>
                              <Button
                                variant="destructive"
                                size="sm"
                                className="h-7 px-2 text-[12px]"
                                disabled={deleting !== null}
                                onClick={() => void handleDelete(agent.id)}
                              >
                                {deleting === agent.id ? "正在删除…" : "确认删除"}
                              </Button>
                            </>
                          ) : (
                            <Button
                              variant="ghost"
                              size="sm"
                              className="h-7 px-2 text-[12px]"
                              aria-label={`删除 ${agent.displayName}`}
                              disabled={busy}
                              onClick={() => {
                                setConfirming(agent.id);
                                removeMutation.reset();
                              }}
                            >
                              <Trash2 aria-hidden="true" />
                              删除
                            </Button>
                          )}
                        </div>
                      ) : null}
                    </li>
                  );
                })}
              </ul>
            )}

            <MutationError
              error={
                removeMutation.state.startError ??
                removeMutation.state.error
              }
              testId="manage-agents-error"
              monoDetail
            />
          </section>

          <section className="detail-section mt-4">
            <h4 className="detail-section-title">添加额外 agent</h4>
            <form className="mt-2.5 space-y-2.5" onSubmit={handleAdd}>
              <div className="grid grid-cols-2 gap-2.5">
                <label>
                  <span className="field-label">id</span>
                  <Input
                    value={id}
                    disabled={busy}
                    placeholder="acme"
                    autoComplete="off"
                    spellCheck={false}
                    onChange={(event) => setId(event.target.value)}
                  />
                </label>
                <label>
                  <span className="field-label">显示名</span>
                  <Input
                    value={displayName}
                    disabled={busy}
                    placeholder="Acme Agent"
                    autoComplete="off"
                    onChange={(event) => setDisplayName(event.target.value)}
                  />
                </label>
              </div>
              <label>
                <span className="field-label">skills 目录</span>
                <Input
                  value={skillsDir}
                  disabled={busy}
                  placeholder="~/.acme/skills"
                  autoComplete="off"
                  spellCheck={false}
                  className="font-mono"
                  onChange={(event) => setSkillsDir(event.target.value)}
                />
              </label>
              <div className="flex items-center justify-between gap-3">
                <p className="text-[12px] text-muted-foreground">
                  检测规则：该目录的父目录存在即视为已安装。
                </p>
                <Button type="submit" size="sm" disabled={busy}>
                  <Plus aria-hidden="true" />
                  {adding ? "正在保存…" : "添加"}
                </Button>
              </div>
              <MutationError
                error={
                  addError ??
                  addMutation.state.startError ??
                  addMutation.state.error
                }
                testId="manage-agents-add-error"
                monoDetail
              />
            </form>
          </section>
        </div>

        <div className="modal-foot">
          <div className="modal-foot-note">
            额外 agent 的挂载由 Skills Box 原生创建；内置 agent 仍由 skills CLI
            挂载。
          </div>
          <Button variant="ghost" size="sm" disabled={busy} onClick={onClose}>
            关闭
          </Button>
        </div>
      </div>
    </div>
  );
}
