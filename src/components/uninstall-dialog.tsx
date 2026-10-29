import { useCallback, useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, CheckCircle2, Trash2, X } from "lucide-react";
import { Button } from "./ui/button";
import { TaskProgress } from "./install-wizard";
import { useModalFocus } from "../hooks/use-modal-focus";
import {
  asAppError,
  fallbackError,
  revealAppData,
  startUninstall,
} from "../lib/api";
import type { TaskEvent, UpdateViewSkill } from "../lib/types";
import { ATTACHMENT_LABELS } from "../lib/inventory-view";
import {
  TASK_SKILL_STATUS_LABELS,
  taskOverallHeading,
  useTaskRunner,
} from "../lib/task-runner";

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
  const queryClient = useQueryClient();
  const panelRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  const [confirmed, setConfirmed] = useState(false);

  const invalidateQueries = useCallback(() => {
    // Inventory carries anomaly counts; detail is invalidated because the
    // selected row may disappear or lose attachments.
    void queryClient.invalidateQueries({ queryKey: ["inventory"] });
    void queryClient.invalidateQueries({ queryKey: ["skill"] });
    void queryClient.invalidateQueries({ queryKey: ["updateReport"] });
  }, [queryClient]);

  const handleTaskEvent = useCallback(
    (event: TaskEvent) => {
      if (
        event.phase === "completed" ||
        event.phase === "cancelled" ||
        event.phase === "failed"
      ) {
        invalidateQueries();
      }
    },
    [invalidateQueries],
  );

  const {
    state: {
      taskId,
      running,
      phase,
      completed,
      total,
      message,
      cancelling,
      result,
      taskError,
      startError,
      revealError,
    },
    stateRef,
    start,
    cancel,
    reset,
    setRevealError,
  } = useTaskRunner({
    fallbackFailure: "卸载失败，请重试。",
    fallbackStart: "无法开始卸载，请稍后重试。",
    onEvent: handleTaskEvent,
  });

  const launch = useCallback(() => {
    if (!skill || stateRef.current.running) return;
    start({
      total: 1,
      message: "正在准备卸载…",
      launch: (onEvent) => startUninstall(skill.name, onEvent),
    });
  }, [skill, start, stateRef]);

  useEffect(() => {
    if (!open) return;
    reset();
    setConfirmed(false);
  }, [open, skill?.name, reset]);

  useModalFocus({
    open,
    panelRef,
    initialFocusRef: closeRef,
    onClose,
    canClose: () => !stateRef.current.running,
  });

  const handleCancelTask = () => cancel("当前操作正在写入，无法取消。");

  const doReveal = async (kind: "backups" | "logs") => {
    setRevealError(null);
    try {
      await revealAppData(kind);
    } catch (error) {
      setRevealError(
        (asAppError(error) ?? fallbackError("无法打开目录。")).message,
      );
    }
  };

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
              <TaskProgress
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

          {startError ? (
            <div
              role="alert"
              data-testid="uninstall-start-error"
              className="notice mt-3"
              data-tone="negative"
            >
              <div>
                <p className="font-medium">{startError.message}</p>
                {startError.detail ? (
                  <p className="mt-0.5">{startError.detail}</p>
                ) : null}
                {startError.hint ? (
                  <p className="mt-0.5">{startError.hint}</p>
                ) : null}
              </div>
            </div>
          ) : null}

          {taskError ? (
            <div
              role="alert"
              data-testid="uninstall-error"
              className="notice mt-3"
              data-tone="negative"
            >
              <div>
                <p className="font-medium">{taskError.message}</p>
                {taskError.detail ? (
                  <p className="mt-0.5">{taskError.detail}</p>
                ) : null}
                {taskError.hint ? <p className="mt-0.5">{taskError.hint}</p> : null}
              </div>
            </div>
          ) : null}

          {result ? (
            <div data-testid="uninstall-result" data-overall={result.overall}>
              <div className="flex items-baseline gap-2">
                <h3 className="flex items-center gap-1.5 text-sm font-semibold">
                  {result.overall === "success" ? (
                    <CheckCircle2
                      className="size-4 text-success"
                      aria-hidden="true"
                    />
                  ) : null}
                  {taskOverallHeading("uninstall", result.overall)}
                </h3>
                <span className="text-[12px] text-muted-foreground">
                  {message}
                </span>
              </div>
              <p className="mt-1 text-[12px] text-muted-foreground">
                不自动回滚；备份与日志保留在应用数据目录。
              </p>

              <ul className="mt-2.5 space-y-2">
                {result.skills.map((skillResult) => (
                  <li
                    key={skillResult.name}
                    data-testid="uninstall-skill-result"
                    data-skill-name={skillResult.name}
                    data-skill-status={skillResult.status}
                    className="rounded-md border border-border px-3 py-2.5"
                  >
                    <div className="flex items-center gap-2">
                      <span className="text-[13px] font-medium">
                        {skillResult.name}
                      </span>
                      <span
                        className={
                          skillResult.status === "removed"
                            ? "ml-auto rounded-sm bg-success-soft px-1.5 py-0.5 text-[11.5px] font-medium text-success"
                            : skillResult.status === "failed"
                              ? "ml-auto rounded-sm bg-destructive-soft px-1.5 py-0.5 text-[11.5px] font-medium text-destructive"
                              : "ml-auto rounded-sm bg-warning-soft px-1.5 py-0.5 text-[11.5px] font-medium text-warning"
                        }
                      >
                        {TASK_SKILL_STATUS_LABELS[skillResult.status]}
                      </span>
                    </div>
                    {skillResult.error ? (
                      <p className="mt-1 text-[12px] leading-relaxed text-muted-foreground">
                        {skillResult.error.message}
                        {skillResult.error.detail
                          ? ` · ${skillResult.error.detail}`
                          : ""}
                      </p>
                    ) : null}
                    {skillResult.backupPath ? (
                      <p className="mono mt-1 text-muted-foreground">
                        备份：{skillResult.backupPath}
                      </p>
                    ) : null}
                  </li>
                ))}
              </ul>

              {result.warnings.length > 0 ? (
                <ul className="notice mt-2 list-disc pl-8" data-tone="warning">
                  {result.warnings.map((warning) => (
                    <li key={warning.code ?? warning.message}>
                      {warning.message}
                      {warning.detail ? ` · ${warning.detail}` : ""}
                    </li>
                  ))}
                </ul>
              ) : null}

              <div className="mt-2.5 space-y-1">
                {result.backupPath ? (
                  <p
                    data-testid="uninstall-backup-path"
                    className="mono break-all text-muted-foreground"
                  >
                    备份：{result.backupPath}
                  </p>
                ) : null}
                {result.logPath ? (
                  <p
                    data-testid="uninstall-log-path"
                    className="mono break-all text-muted-foreground"
                  >
                    日志：{result.logPath}
                  </p>
                ) : null}
              </div>
              <div className="mt-2.5 flex flex-wrap gap-1.5">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => void doReveal("backups")}
                >
                  打开备份目录
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => void doReveal("logs")}
                >
                  打开日志目录
                </Button>
              </div>
              {revealError ? (
                <p className="mt-1.5 text-[12px] text-destructive">
                  {revealError}
                </p>
              ) : null}
            </div>
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
                {result.overall === "success" ? "完成" : "关闭"}
              </Button>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}
