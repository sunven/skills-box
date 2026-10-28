import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  ArrowUpCircle,
  CheckCircle2,
  HardDrive,
  ShieldAlert,
  X,
} from "lucide-react";
import { Button } from "./ui/button";
import { TaskProgress } from "./install-wizard";
import { asAppError, cancelTask, revealAppData, startUpdate } from "../lib/api";
import type {
  InventoryAgent,
  TaskEvent,
  TaskPhase,
  TaskResult,
  UpdateCheckReport,
} from "../lib/types";
import type {
  UpdateSkipItem,
  UpdateSkipReason,
  UpdateViewSkill,
} from "../lib/update-view";
import {
  computeUpdateSummary,
  markSkillsCurrent,
} from "../lib/update-view";
import {
  LOCAL_MODIFICATION_LABELS,
  shortHash,
} from "../lib/inventory-view";
import { updateReportQueryKey } from "../hooks/use-updates";

type UpdateMode = "single" | "all";

interface UpdateDialogProps {
  open: boolean;
  mode: UpdateMode;
  skills: UpdateViewSkill[];
  agents?: InventoryAgent[];
  onClose: () => void;
}

interface DialogError {
  code?: string;
  message: string;
  detail?: string;
  hint?: string;
}

const REASON_ORDER: UpdateSkipReason[] = [
  "local_modified",
  "local_unknown",
  "unknown",
  "not_checked",
  "path_missing",
  "error",
  "copy_only",
];

function statusLabel(status: TaskResult["skills"][number]["status"]): string {
  switch (status) {
    case "updated":
      return "已更新";
    case "removed":
      return "已卸载";
    case "installed":
      return "已安装";
    case "skipped":
      return "跳过";
    case "failed":
      return "失败";
  }
}

function overallHeading(overall: TaskResult["overall"]): string {
  switch (overall) {
    case "success":
      return "更新完成";
    case "partial":
      return "更新部分失败";
    case "failed":
      return "更新失败";
    case "cancelled":
      return "更新已取消";
  }
}

function copyAgents(
  skill: UpdateViewSkill,
  universalIds: Set<string>,
): string[] {
  return skill.attachments
    .filter(
      (attachment) =>
        attachment.mode === "copy" && !universalIds.has(attachment.agentId),
    )
    .map((attachment) => attachment.agentName);
}

function affectedAgents(
  skill: UpdateViewSkill,
  universalIds: Set<string>,
): string[] {
  return skill.attachments
    .filter(
      (attachment) =>
        (attachment.mode === "link" || attachment.mode === "copy") &&
        !universalIds.has(attachment.agentId),
    )
    .map((attachment) => attachment.agentName);
}

function manageableSingle(skill: UpdateViewSkill | undefined): boolean {
  return Boolean(
    skill && skill.management === "managed" && skill.update === "update",
  );
}

export function UpdateDialog({
  open,
  mode,
  skills,
  agents = [],
  onClose,
}: UpdateDialogProps) {
  const queryClient = useQueryClient();
  const panelRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const runSeq = useRef(0);
  const runningRef = useRef(false);

  const [taskId, setTaskId] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [phase, setPhase] = useState<TaskPhase | null>(null);
  const [completed, setCompleted] = useState(0);
  const [total, setTotal] = useState(1);
  const [message, setMessage] = useState("");
  const [cancelling, setCancelling] = useState(false);
  const [confirmed, setConfirmed] = useState(false);
  const [result, setResult] = useState<TaskResult | null>(null);
  const [taskError, setTaskError] = useState<DialogError | null>(null);
  const [startError, setStartError] = useState<DialogError | null>(null);
  const [revealError, setRevealError] = useState<string | null>(null);

  useEffect(() => {
    runningRef.current = running;
  }, [running]);

  const singleSkill = mode === "single" ? skills[0] : undefined;
  const universalIds = useMemo(
    () =>
      new Set(
        agents
          .filter((agent) => agent.universal)
          .map((agent) => agent.id),
      ),
    [agents],
  );
  const summary = useMemo(() => computeUpdateSummary(skills), [skills]);
  const singleEligible = manageableSingle(singleSkill);
  const singleNeedsConfirmation = Boolean(
    singleSkill &&
      (singleSkill.localModification === "modified" ||
        singleSkill.localModification === "unknown"),
  );
  const singleCopyAgents = singleSkill
    ? copyAgents(singleSkill, universalIds)
    : [];
  const singleAffectedAgents = singleSkill
    ? affectedAgents(singleSkill, universalIds)
    : [];
  const initialNames =
    mode === "single"
      ? singleEligible && singleSkill
        ? [singleSkill.name]
        : []
      : summary.candidates.map((skill) => skill.name);
  const canStart =
    !running &&
    initialNames.length > 0 &&
    (!singleNeedsConfirmation || confirmed);

  const invalidateQueries = useCallback(() => {
    void queryClient.invalidateQueries({ queryKey: ["inventory"] });
    void queryClient.invalidateQueries({ queryKey: ["skill"] });
    void queryClient.invalidateQueries({ queryKey: ["updateReport"] });
  }, [queryClient]);

  const applyResultToReport = useCallback(
    (taskResult: TaskResult) => {
      const updatedNames = taskResult.skills
        .filter((skill) => skill.status === "updated")
        .map((skill) => skill.name);
      if (updatedNames.length > 0) {
        queryClient.setQueryData<UpdateCheckReport | undefined>(
          updateReportQueryKey,
          (previous) => markSkillsCurrent(previous, updatedNames),
        );
      }
    },
    [queryClient],
  );

  const handleTaskEvent = useCallback(
    (event: TaskEvent) => {
      setPhase(event.phase);
      if (event.completed !== undefined) setCompleted(event.completed);
      if (event.total !== undefined) setTotal(event.total);
      if (event.message) setMessage(event.message);

      if (event.phase === "completed" || event.phase === "cancelled") {
        setRunning(false);
        setCancelling(false);
        const taskResult = event.result ?? null;
        setResult(taskResult);
        if (taskResult) applyResultToReport(taskResult);
        invalidateQueries();
        return;
      }
      if (event.phase === "failed") {
        setRunning(false);
        setCancelling(false);
        setTaskError(
          event.error ?? {
            code: "cli.failed",
            message: "更新失败，请重试。",
          },
        );
        invalidateQueries();
      }
    },
    [applyResultToReport, invalidateQueries],
  );

  const launch = useCallback(
    async (names: string[]) => {
      if (names.length === 0 || runningRef.current) return;
      const seq = ++runSeq.current;
      setTaskError(null);
      setStartError(null);
      setRevealError(null);
      setResult(null);
      setRunning(true);
      setCancelling(false);
      setTaskId(null);
      setPhase("preparing");
      setCompleted(0);
      setTotal(names.length);
      setMessage("正在准备更新…");
      try {
        const id = await startUpdate(names, (event) => {
          if (runSeq.current !== seq) return;
          handleTaskEvent(event);
        });
        if (runSeq.current !== seq) {
          void cancelTask(id).catch(() => {
            // The dialog was closed while the invoke was in flight.
          });
          return;
        }
        setTaskId(id);
      } catch (error) {
        if (runSeq.current !== seq) return;
        setStartError(
          asAppError(error) ?? { message: "无法开始更新，请稍后重试。" },
        );
        setPhase(null);
        setRunning(false);
      }
    },
    [handleTaskEvent],
  );

  const retryNames =
    result?.skills
      .filter((skill) => skill.status === "failed")
      .map((skill) => skill.name) ?? [];

  useEffect(() => {
    if (!open) return;
    runSeq.current += 1;
    setTaskId(null);
    setRunning(false);
    setPhase(null);
    setCompleted(0);
    setTotal(1);
    setMessage("");
    setCancelling(false);
    setConfirmed(false);
    setResult(null);
    setTaskError(null);
    setStartError(null);
    setRevealError(null);
    const previousFocus = document.activeElement as HTMLElement | null;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        if (!runningRef.current) onCloseRef.current();
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
  }, [open, mode]);

  const handleCancelTask = async () => {
    if (!taskId || cancelling) return;
    setCancelling(true);
    try {
      const ack = await cancelTask(taskId);
      if (!ack.accepted) {
        setTaskError({
          message: ack.message ?? "当前 skill 正在写入，无法取消。",
        });
      }
    } catch (error) {
      setTaskError(
        asAppError(error) ?? { message: "无法取消更新任务。" },
      );
    } finally {
      setCancelling(false);
    }
  };

  const doReveal = async (kind: "backups" | "logs") => {
    setRevealError(null);
    try {
      await revealAppData(kind);
    } catch (error) {
      setRevealError(
        (asAppError(error) ?? { message: "无法打开目录。" }).message,
      );
    }
  };

  if (!open) return null;

  const title =
    mode === "single" && singleSkill
      ? `更新 skill · ${singleSkill.name}`
      : "更新全部";
  const grouped = new Map<UpdateSkipReason, UpdateSkipItem[]>();
  for (const item of summary.skipped) {
    const list = grouped.get(item.reason) ?? [];
    list.push(item);
    grouped.set(item.reason, list);
  }
  const showInitial = !running && !result;
  const closeDisabled = running;
  const copyCandidateCount = summary.candidates.filter(
    (skill) => copyAgents(skill, universalIds).length > 0,
  ).length;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/30 p-6"
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
        aria-labelledby="update-dialog-title"
        data-testid="update-dialog"
        data-mode={mode}
        className="flex max-h-[86vh] w-full max-w-2xl flex-col rounded-xl border border-border bg-card p-4 shadow-xl"
      >
        <div className="flex items-start justify-between gap-3">
          <div>
            <h2
              id="update-dialog-title"
              className="flex items-center gap-2 text-sm font-semibold"
            >
              <ArrowUpCircle className="size-4 text-primary" aria-hidden="true" />
              {title}
            </h2>
            <p className="mt-1 text-[11px] leading-relaxed text-muted-foreground">
              更新前强制备份；逐个 skill 调用 skills add，不调用 skills update /
              check。
            </p>
          </div>
          <Button
            ref={closeRef}
            variant="ghost"
            size="sm"
            aria-label="关闭更新"
            disabled={closeDisabled}
            title={closeDisabled ? "更新进行中，可在 skill 之间取消" : undefined}
            onClick={onClose}
            className="size-7 shrink-0 p-0"
          >
            <X className="size-4" aria-hidden="true" />
          </Button>
        </div>

        <div className="mt-3 min-h-0 flex-1 overflow-y-auto">
          {showInitial && mode === "single" ? (
            <div data-testid="update-single-confirm">
              {!singleSkill || !singleEligible ? (
                <p className="rounded-md border border-warning/30 bg-warning-soft px-2.5 py-2 text-[11px] text-warning">
                  当前 skill 没有可执行的更新候选；请先检查更新，或先处理本地改动 /
                  仅副本 / 来源异常。
                </p>
              ) : (
                <>
                  <div className="rounded-lg border border-border bg-muted/40 p-3 text-xs">
                    <div className="flex justify-between gap-4 py-0.5">
                      <span className="text-muted-foreground">来源</span>
                      <strong className="font-mono">
                        {singleSkill.source ?? "来源未知"}
                        {singleSkill.updateResult?.ref
                          ? `#${singleSkill.updateResult.ref}`
                          : ""}
                      </strong>
                    </div>
                    <div className="flex justify-between gap-4 py-0.5">
                      <span className="text-muted-foreground">当前版本</span>
                      <strong
                        className="font-mono"
                        title={singleSkill.updateResult?.lockHash ?? undefined}
                      >
                        {singleSkill.updateResult?.lockHash
                          ? shortHash(singleSkill.updateResult.lockHash, 10)
                          : "—"}
                      </strong>
                    </div>
                    <div className="flex justify-between gap-4 py-0.5">
                      <span className="text-muted-foreground">目标版本</span>
                      <strong
                        className="font-mono"
                        title={singleSkill.updateResult?.remoteHash ?? undefined}
                      >
                        {singleSkill.updateResult?.remoteHash
                          ? shortHash(singleSkill.updateResult.remoteHash, 10)
                          : "—"}
                      </strong>
                    </div>
                    <div className="flex justify-between gap-4 py-0.5">
                      <span className="text-muted-foreground">受影响 agent</span>
                      <strong>
                        {singleAffectedAgents.length > 0
                          ? singleAffectedAgents.join("、")
                          : "无（仅写入共享 canonical）"}
                      </strong>
                    </div>
                    <div className="flex justify-between gap-4 py-0.5">
                      <span className="text-muted-foreground">本地改动</span>
                      <strong>
                        {LOCAL_MODIFICATION_LABELS[singleSkill.localModification]}
                      </strong>
                    </div>
                  </div>

                  {singleSkill.localModification !== "clean" ? (
                    <div
                      data-testid="update-local-warning"
                      className="mt-3 flex items-start gap-2 rounded-lg border border-warning/30 bg-warning-soft px-3 py-2 text-[11px] text-warning"
                    >
                      <AlertTriangle
                        className="mt-0.5 size-3.5 shrink-0"
                        aria-hidden="true"
                      />
                      <div>
                        <p className="font-medium">
                          本地改动将被覆盖（需确认）
                        </p>
                        <p className="mt-0.5 opacity-90">
                          共享目录与基线不一致，或首次扫描无法确认。更新会覆盖这些修改；备份会保留更新前内容。
                        </p>
                      </div>
                    </div>
                  ) : null}

                  {singleCopyAgents.length > 0 ? (
                    <div
                      data-testid="update-copy-warning"
                      className="mt-3 flex items-start gap-2 rounded-lg border border-warning/30 bg-warning-soft px-3 py-2 text-[11px] text-warning"
                    >
                      <ShieldAlert
                        className="mt-0.5 size-3.5 shrink-0"
                        aria-hidden="true"
                      />
                      <div>
                        <p className="font-medium">
                          copy 挂载将转为 symlink
                        </p>
                        <p className="mt-0.5 opacity-90">
                          {singleCopyAgents.join("、")} 当前是副本目录；更新前会先备份这些副本，更新后改为指向共享 canonical 的符号链接。其他已挂载 agent
                          会按当前物理挂载重建。
                        </p>
                      </div>
                    </div>
                  ) : null}

                  {singleNeedsConfirmation ? (
                    <label className="mt-3 flex cursor-pointer items-start gap-2 rounded-lg border border-border px-3 py-2 text-[11px]">
                      <input
                        type="checkbox"
                        className="mt-0.5"
                        aria-label="确认本地改动"
                        checked={confirmed}
                        onChange={(event) => setConfirmed(event.target.checked)}
                      />
                      <span>
                        我已了解本地改动会被覆盖；本次更新会先创建备份，不自动回滚。
                      </span>
                    </label>
                  ) : null}

                  <p className="mt-3 flex items-start gap-2 rounded-md border border-border bg-muted/50 px-2.5 py-2 text-[10.5px] leading-relaxed text-muted-foreground">
                    <HardDrive
                      className="mt-0.5 size-3.5 shrink-0"
                      aria-hidden="true"
                    />
                    更新执行：rebuilt skills add（当前物理挂载的非 universal agents +
                    universal）。已断开的 agent 不会被重挂；无挂载时仅 -a universal。
                  </p>
                </>
              )}
            </div>
          ) : null}

          {showInitial && mode === "all" ? (
            <div className="grid min-h-0 flex-1 grid-cols-2 gap-2">
              <section
                data-testid="update-candidates"
                className="rounded-lg border border-border bg-muted/40 p-2.5"
              >
                <div className="flex items-baseline gap-2">
                  <strong className="text-lg leading-none text-success">
                    {summary.candidates.length}
                  </strong>
                  <span className="text-[11.5px] font-semibold">更新候选</span>
                </div>
                <p className="mt-1 text-[10.5px] leading-relaxed text-muted-foreground">
                  仅包含受管、本地干净且来源可更新的 skill；将串行执行。
                </p>
                {summary.candidates.length === 0 ? (
                  <p className="mt-2 text-[10.5px] text-muted-foreground/70">
                    没有可直接更新的 skill。
                  </p>
                ) : (
                  <ul className="mt-2 space-y-1 text-[10.5px]">
                    {summary.candidates.map((skill) => (
                      <li
                        key={skill.name}
                        data-testid="update-candidate"
                        data-skill-name={skill.name}
                        className="rounded-md bg-card px-2 py-1"
                      >
                        <span className="font-medium">{skill.name}</span>
                        <span className="ml-1 text-muted-foreground">
                          {skill.source ?? "来源未知"}
                        </span>
                        {skill.updateResult?.remoteHash ? (
                          <span
                            className="ml-1 font-mono text-[9.5px] text-muted-foreground/70"
                            title={skill.updateResult.remoteHash}
                          >
                            {shortHash(skill.updateResult.remoteHash, 8)}
                          </span>
                        ) : null}
                        {copyAgents(skill, universalIds).length > 0 ? (
                          <span className="ml-1 rounded border border-warning/40 bg-warning-soft px-1 py-0.5 text-[9.5px] text-warning">
                            copy → symlink
                          </span>
                        ) : null}
                      </li>
                    ))}
                  </ul>
                )}
              </section>

              <section
                data-testid="update-skipped"
                className="rounded-lg border border-border bg-muted/40 p-2.5"
              >
                <div className="flex items-baseline gap-2">
                  <strong className="text-lg leading-none text-warning">
                    {summary.skipped.length}
                  </strong>
                  <span className="text-[11.5px] font-semibold">跳过项</span>
                </div>
                <p className="mt-1 text-[10.5px] leading-relaxed text-muted-foreground">
                  本地改动、未知、源路径缺失、检查失败与仅副本不会进入更新执行。
                </p>
                {summary.skipped.length === 0 ? (
                  <p className="mt-2 text-[10.5px] text-muted-foreground/70">
                    没有被跳过的 skill。
                  </p>
                ) : (
                  <div className="mt-2 space-y-1.5">
                    {REASON_ORDER.filter((reason) => grouped.has(reason)).map(
                      (reason) => {
                        const items = grouped.get(reason)!;
                        return (
                          <div key={reason}>
                            <p className="text-[10px] font-semibold text-warning">
                              {items[0]!.label} · {items.length}
                            </p>
                            <ul className="mt-0.5 space-y-0.5 text-[10.5px] text-muted-foreground">
                              {items.map((item) => (
                                <li key={item.name} title={item.detail}>
                                  <span className="font-medium text-foreground">
                                    {item.name}
                                  </span>
                                  <span className="ml-1">{item.detail}</span>
                                </li>
                              ))}
                            </ul>
                          </div>
                        );
                      },
                    )}
                  </div>
                )}
              </section>

              {copyCandidateCount > 0 ? (
                <div
                  data-testid="update-copy-warning"
                  className="col-span-2 flex items-start gap-2 rounded-lg border border-warning/30 bg-warning-soft px-3 py-2 text-[11px] text-warning"
                >
                  <ShieldAlert
                    className="mt-0.5 size-3.5 shrink-0"
                    aria-hidden="true"
                  />
                  <div>
                    <p className="font-medium">copy 挂载将转为 symlink</p>
                    <p className="mt-0.5 opacity-90">
                      有 {copyCandidateCount} 个候选带 copy 挂载；更新前会先备份副本目录，更新后改为指向共享 canonical 的符号链接。
                    </p>
                  </div>
                </div>
              ) : null}
            </div>
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
                cancelLabel="取消更新"
                onCancel={() => void handleCancelTask()}
              />
              <p className="mt-2 rounded-md border border-border bg-muted/50 px-2.5 py-1.5 text-[11px] text-muted-foreground">
                每个 skill 写入前都会先备份；当前 skill 写入期间不可取消，skill 之间可取消。
              </p>
            </>
          ) : null}

          {startError ? (
            <div
              role="alert"
              data-testid="update-start-error"
              className="mt-3 rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive"
            >
              <p className="font-medium">{startError.message}</p>
              {startError.detail ? (
                <p className="mt-0.5 line-clamp-3 text-[10.5px] opacity-70">
                  {startError.detail}
                </p>
              ) : null}
            </div>
          ) : null}

          {taskError ? (
            <div
              role="alert"
              data-testid="update-error"
              className="mt-3 rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive"
            >
              <p className="font-medium">{taskError.message}</p>
              {taskError.detail ? (
                <p className="mt-0.5 line-clamp-4 text-[10.5px] opacity-70">
                  {taskError.detail}
                </p>
              ) : null}
              {taskError.hint ? (
                <p className="mt-0.5 text-[10.5px] opacity-80">
                  {taskError.hint}
                </p>
              ) : null}
            </div>
          ) : null}

          {result ? (
            <div data-testid="update-result" data-overall={result.overall}>
              <div className="flex items-baseline gap-2">
                <h3 className="flex items-center gap-1.5 text-sm font-semibold">
                  {result.overall === "success" ? (
                    <CheckCircle2
                      className="size-4 text-success"
                      aria-hidden="true"
                    />
                  ) : null}
                  {overallHeading(result.overall)}
                </h3>
                <span className="text-[11px] text-muted-foreground">
                  {message}
                </span>
              </div>
              <p className="mt-1 text-[10.5px] text-muted-foreground">
                不自动回滚；失败项可单独重试。备份与日志保留在应用数据目录。
              </p>

              <ul className="mt-2 space-y-1.5">
                {result.skills.map((skill) => (
                  <li
                    key={skill.name}
                    data-testid="update-skill-result"
                    data-skill-name={skill.name}
                    data-skill-status={skill.status}
                    className="rounded-lg border border-border px-3 py-2"
                  >
                    <div className="flex items-center gap-2">
                      <span className="text-xs font-medium">{skill.name}</span>
                      <span
                        className={
                          skill.status === "updated"
                            ? "rounded border border-success/30 bg-success-soft px-1.5 py-0.5 text-[10px] text-success"
                            : skill.status === "failed"
                              ? "rounded border border-destructive/30 bg-destructive/5 px-1.5 py-0.5 text-[10px] text-destructive"
                              : "rounded border border-warning/30 bg-warning-soft px-1.5 py-0.5 text-[10px] text-warning"
                        }
                      >
                        {statusLabel(skill.status)}
                      </span>
                      {skill.status === "failed" ? (
                        <Button
                          variant="ghost"
                          size="sm"
                          className="ml-auto h-7"
                          disabled={running}
                          onClick={() => void launch([skill.name])}
                        >
                          重试
                        </Button>
                      ) : null}
                    </div>
                    {skill.error ? (
                      <p className="mt-1 text-[10.5px] leading-relaxed text-muted-foreground">
                        {skill.error.message}
                        {skill.error.detail ? ` · ${skill.error.detail}` : ""}
                      </p>
                    ) : null}
                    {skill.backupPath ? (
                      <p className="mt-1 font-mono text-[9.5px] text-muted-foreground/80">
                        备份：{skill.backupPath}
                      </p>
                    ) : null}
                  </li>
                ))}
              </ul>

              {result.warnings.length > 0 ? (
                <ul
                  data-testid="update-warnings"
                  className="mt-2 space-y-1 rounded-lg border border-warning/30 bg-warning-soft px-3 py-2 text-[10.5px] text-warning"
                >
                  {result.warnings.map((warning) => (
                    <li key={`${warning.code}-${warning.message}`}>
                      {warning.message}
                      {warning.detail ? ` · ${warning.detail}` : ""}
                    </li>
                  ))}
                </ul>
              ) : null}

              <div className="mt-2 space-y-0.5">
                {result.backupPath ? (
                  <p
                    data-testid="update-backup-path"
                    className="break-all font-mono text-[10px] text-muted-foreground"
                  >
                    备份：{result.backupPath}
                  </p>
                ) : null}
                {result.logPath ? (
                  <p
                    data-testid="update-log-path"
                    className="break-all font-mono text-[10px] text-muted-foreground"
                  >
                    日志：{result.logPath}
                  </p>
                ) : null}
              </div>
              <div className="mt-2 flex flex-wrap gap-1.5">
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
                <p className="mt-1 text-[10.5px] text-destructive">{revealError}</p>
              ) : null}
            </div>
          ) : null}
        </div>

        <div className="mt-3 flex items-center justify-between gap-2 border-t border-border pt-3">
          <div className="text-[10.5px] text-muted-foreground">
            {running
              ? "更新进行中；当前 skill 写入期间不可取消"
              : "更新前自动备份 · 不调用 skills update / check"}
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
                  size="sm"
                  onClick={() => void launch(initialNames)}
                  disabled={!canStart}
                  title={
                    singleNeedsConfirmation && !confirmed
                      ? "请先勾选确认本地改动"
                      : undefined
                  }
                >
                  开始更新
                  {mode === "all" && initialNames.length > 0
                    ? ` (${initialNames.length})`
                    : ""}                </Button>
              </>
            ) : null}
            {running ? (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => void handleCancelTask()}
                disabled={!taskId || cancelling}
              >
                {cancelling ? "正在取消…" : "取消更新"}
              </Button>
            ) : null}
            {!running && result ? (
              <>
                {retryNames.length > 0 ? (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => void launch(retryNames)}
                  >
                    重试失败项
                  </Button>
                ) : null}
                <Button size="sm" onClick={onClose}>
                  {result.overall === "success" ? "完成" : "关闭"}
                </Button>
              </>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}
