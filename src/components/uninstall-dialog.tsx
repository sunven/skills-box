import { useCallback, useEffect, useRef, useState } from "react";
import { AlertTriangle, Trash2, X } from "lucide-react";
import { Button } from "./ui/button";
import { MutationError } from "./mutation/mutation-error";
import { MutationProgress } from "./mutation/mutation-progress";
import { MutationResult } from "./mutation/mutation-result";
import { useModalFocus } from "../hooks/use-modal-focus";
import { startUninstall } from "../lib/api";
import type { UpdateViewSkill } from "../lib/types";
import { ATTACHMENT_LABELS } from "../lib/inventory-view";
import { taskEventDriver, useMutation } from "../lib/mutation";

interface UninstallDialogProps {
  open: boolean;
  skill: UpdateViewSkill | null;
  onClose: () => void;
}

export function UninstallDialog({
  open,
  skill,
  onClose,
}: UninstallDialogProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  const [confirmed, setConfirmed] = useState(false);

  const mutation = useMutation({
    action: "uninstall",
    fallbackFailure: "卸载失败，请重试。",
    fallbackStart: "无法开始卸载，请稍后重试。",
  });
  const {
    taskId,
    running,
    phase,
    completed,
    total,
    message,
    cancelling,
    result,
    error,
    startError,
  } = mutation.state;

  const launch = useCallback(() => {
    if (!skill || mutation.stateRef.current.running) return;
    void mutation.start(
      taskEventDriver({
        total: 1,
        message: "正在准备卸载…",
        launch: (onEvent) => startUninstall(skill.name, onEvent),
      }),
    );
  }, [skill, mutation.start, mutation.stateRef]);

  useEffect(() => {
    if (!open) return;
    mutation.reset();
    setConfirmed(false);
  }, [open, skill?.name, mutation.reset]);

  useModalFocus({
    open,
    panelRef,
    initialFocusRef: closeRef,
    onClose,
    canClose: () => !mutation.stateRef.current.running,
  });

  const handleCancelTask = () =>
    mutation.cancel("当前操作正在写入，无法取消。");

  if (!open || !skill) return null;

  const attachments = skill.attachments;
  const canonicalLabel =
    skill.management === "copy_only"
      ? "无 canonical（仅副本）"
      : (skill.canonicalPath ?? "canonical 路径未知（可能已缺失）");
  const showInitial = !running && !result;
  const closeDisabled = running;

  return (
    <div
      className="modal-overlay"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !running) {
          onCloseRef.current();
        }
      }}
    >
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="uninstall-dialog-title"
        data-testid="uninstall-dialog"
        className="modal-panel max-w-xl"
      >
        <div className="modal-head">
          <div>
            <h2 id="uninstall-dialog-title" className="modal-title">
              <Trash2 className="size-4 text-destructive" aria-hidden="true" />
              彻底卸载 · {skill.name}
            </h2>
            <p className="modal-sub">
              先备份，再执行 skills remove -g -s；CLI 退出后会逐项校验。
            </p>
          </div>
          <Button
            ref={closeRef}
            variant="ghost"
            size="icon"
            aria-label="关闭卸载"
            disabled={closeDisabled}
            title={closeDisabled ? "卸载进行中，CLI 启动前可取消" : undefined}
            onClick={onClose}
            className="size-7 shrink-0"
          >
            <X className="size-4" aria-hidden="true" />
          </Button>
        </div>

        <div className="modal-body">
          {showInitial ? (
            <>
              <div
                data-testid="uninstall-scope"
                className="rounded-md border border-destructive/25 bg-destructive-soft p-3.5"
              >
                <p className="flex items-center gap-1.5 text-[13px] font-medium text-destructive">
                  <AlertTriangle className="size-3.5" aria-hidden="true" />
                  将永久删除以下内容
                </p>
                <dl className="mt-2.5 space-y-2 text-[12.5px]">
                  <div>
                    <dt className="text-[12px] text-muted-foreground">
                      canonical
                    </dt>
                    <dd className="mono break-all">{canonicalLabel}</dd>
                  </div>
                  <div>
                    <dt className="text-[12px] text-muted-foreground">
                      agent 挂载（{attachments.length} 个）
                    </dt>
                    {attachments.length === 0 ? (
                      <dd className="text-muted-foreground">
                        未挂载任何 agent。
                      </dd>
                    ) : (
                      <dd>
                        <ul className="mt-0.5 space-y-1">
                          {attachments.map((attachment) => (
                            <li
                              key={`${attachment.agentId}:${attachment.path}`}
                              className="break-all"
                            >
                              <span className="font-medium">
                                {attachment.agentName}
                              </span>
                              <span className="ml-1 text-muted-foreground">
                                {ATTACHMENT_LABELS[attachment.mode]}
                              </span>
                              <span className="mono ml-1 text-muted-foreground">
                                {attachment.path}
                              </span>
                            </li>
                          ))}
                        </ul>
                      </dd>
                    )}
                  </div>
                  <div>
                    <dt className="text-[12px] text-muted-foreground">
                      lock 条目
                    </dt>
                    <dd className="mono break-all">lock 条目：{skill.name}</dd>
                  </div>
                  <div>
                    <dt className="text-[12px] text-muted-foreground">
                      本地基线
                    </dt>
                    <dd>本地基线指纹（baseline）</dd>
                  </div>
                </dl>
              </div>

              <div
                data-testid="uninstall-danger"
                className="notice mt-3"
                data-tone="negative"
              >
                <AlertTriangle
                  className="mt-0.5 size-4 shrink-0"
                  aria-hidden="true"
                />
                <div>
                  <p className="font-medium">此操作不可撤销</p>
                  <p className="mt-0.5">
                    卸载前会创建 lock、canonical、copy 挂载与 manifest
                    的备份；但应用不会自动回滚，也不提供「仅卸载、保留目录」。
                  </p>
                </div>
              </div>

              <label className="mt-3 flex cursor-pointer items-start gap-2 rounded-md border border-border px-3 py-2.5 text-[12.5px] leading-relaxed">
                <input
                  type="checkbox"
                  className="mt-0.5"
                  aria-label="确认彻底卸载"
                  checked={confirmed}
                  onChange={(event) => setConfirmed(event.target.checked)}
                />
                <span>
                  我已了解以上 canonical、所有 agent
                  挂载、lock 条目与基线都会被删除，且不会自动回滚。
                </span>
              </label>
            </>
          ) : null}

          {running ? (
            <>
              <MutationProgress
                phase={phase}
                message={message}
                completed={completed}
                total={total}
                cancelling={cancelling}
                canCancel={Boolean(taskId) && !cancelling}
                cancelLabel="取消卸载"
                onCancel={() => void handleCancelTask()}
              />
              <p className="mt-2 text-[12px] text-muted-foreground">
                备份完成后、CLI 启动前可取消；CLI 启动后不可取消。
              </p>
            </>
          ) : null}

          <MutationError error={startError} testId="uninstall-start-error" />
          <MutationError error={error} testId="uninstall-error" />

          {result ? (
            <MutationResult
              action="uninstall"
              result={result}
              message={message}
              testIdPrefix="uninstall"
              copy="不自动回滚；备份与日志保留在应用数据目录。"
              showReveal
              warningsTestId="uninstall-warnings"
            />
          ) : null}
        </div>

        <div className="modal-foot">
          <div className="modal-foot-note">
            {running
              ? "卸载进行中；CLI 启动后不可取消"
              : "危险操作 · 自动备份 · 不自动回滚"}
          </div>
          <div className="flex items-center gap-2">
            {!running && !result ? (
              <>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={onClose}
                  disabled={closeDisabled}
                >
                  取消
                </Button>
                <Button
                  variant="destructive"
                  size="sm"
                  onClick={() => void launch()}
                  disabled={!confirmed}
                  title={!confirmed ? "请先勾选确认" : undefined}
                >
                  确认卸载
                </Button>
              </>
            ) : null}
            {running ? (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => void handleCancelTask()}
                disabled={!taskId || cancelling}
              >
                {cancelling ? "正在取消…" : "取消卸载"}
              </Button>
            ) : null}
            {!running && result ? (
              <Button size="sm" onClick={onClose}>
                {result.outcome === "success" ? "完成" : "关闭"}
              </Button>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}
