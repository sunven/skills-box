import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, Loader2, PackageOpen, X } from "lucide-react";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { useModalFocus } from "../hooks/use-modal-focus";
import {
  asAppError,
  cancelTask,
  startInstall,
  startPreview,
} from "../lib/api";
import type {
  DetectedAgent,
  InventorySkill,
  PreviewSkill,
  PreviewSummary,
  TaskEvent,
  TaskPhase,
  TaskResult,
} from "../lib/types";

interface InstallWizardProps {
  open: boolean;
  inventorySkills: InventorySkill[];
  agents: DetectedAgent[];
  onClose: () => void;
}

interface WizardError {
  code?: string;
  message: string;
  detail?: string;
  hint?: string;
}

const FINAL_PHASES: ReadonlySet<TaskPhase> = new Set([
  "completed",
  "failed",
  "cancelled",
]);

const PHASE_LABELS: Record<TaskPhase, string> = {
  preparing: "准备中",
  cloning: "正在克隆仓库",
  backup: "正在备份",
  running_cli: "正在执行 CLI",
  verifying: "正在校验结果",
  completed: "已完成",
  failed: "失败",
  cancelled: "已取消",
};

type Step = 1 | 2 | 3 | 4 | 5;

interface AgentCount {
  managed: number;
  unmanaged: number;
}

function isFinalPhase(phase: TaskPhase | null): boolean {
  return phase !== null && FINAL_PHASES.has(phase);
}

function parseManualNames(input: string): string[] {
  const names = input
    .split(/[\s,，、;；]+/)
    .map((name) => name.trim())
    .filter(Boolean);
  return Array.from(new Set(names));
}

function managementLabel(management: InventorySkill["management"]): string {
  switch (management) {
    case "managed":
      return "已安装";
    case "copy_only":
      return "仅副本";
    case "unmanaged":
      return "未受管";
  }
}

function conflictLabel(skill: PreviewSkill): string {
  if (!skill.conflict) return "";
  return skill.conflict.management === "unmanaged" ? "未受管" : "已安装";
}

function countAgentSkills(skills: InventorySkill[]): Map<string, AgentCount> {
  const counts = new Map<string, AgentCount>();
  for (const skill of skills) {
    for (const attachment of skill.attachments) {
      const current = counts.get(attachment.agentId) ?? {
        managed: 0,
        unmanaged: 0,
      };
      if (skill.management === "unmanaged") current.unmanaged += 1;
      else current.managed += 1;
      counts.set(attachment.agentId, current);
    }
  }
  return counts;
}

function defaultAgentSelection(
  agents: DetectedAgent[],
  skills: InventorySkill[],
): Set<string> {
  const withManaged = new Set<string>();
  for (const skill of skills) {
    if (skill.management === "unmanaged") continue;
    for (const attachment of skill.attachments) {
      withManaged.add(attachment.agentId);
    }
  }
  return new Set(
    agents
      .filter((agent) => !agent.universal && withManaged.has(agent.id))
      .map((agent) => agent.id),
  );
}

function skillStatusLabel(status: TaskResult["skills"][number]["status"]): string {
  switch (status) {
    case "installed":
      return "已安装";
    case "updated":
      return "已更新";
    case "removed":
      return "已卸载";
    case "skipped":
      return "未找到";
    case "failed":
      return "失败";
  }
}

function overallHeading(overall: TaskResult["overall"]): string {
  switch (overall) {
    case "success":
      return "安装完成";
    case "partial":
      return "安装部分失败";
    case "failed":
      return "安装失败";
    case "cancelled":
      return "安装已取消";
  }
}

function StepBars({ step }: { step: Step }) {
  return (
    <div className="mt-3 flex gap-1.5" aria-hidden="true">
      {[1, 2, 3, 4, 5].map((index) => (
        <span
          key={index}
          className={
            index < step
              ? "h-1 flex-1 rounded-full bg-primary/45"
              : index === step
                ? "h-1 flex-1 rounded-full bg-primary"
                : "h-1 flex-1 rounded-full bg-muted"
          }
        />
      ))}
    </div>
  );
}

export function TaskProgress({
  phase,
  message,
  completed,
  total,
  cancelling,
  canCancel,
  cancelLabel,
  onCancel,
}: {
  phase: TaskPhase | null;
  message: string;
  completed: number;
  total: number;
  cancelling: boolean;
  canCancel: boolean;
  cancelLabel: string;
  onCancel: () => void;
}) {
  const safeTotal = total > 0 ? total : 1;
  const percent = Math.min(100, Math.round((completed / safeTotal) * 100));
  return (
    <div
      className="mt-3 rounded-lg border border-border bg-muted/40 p-3"
      data-testid="task-progress"
    >
      <div className="flex items-center gap-2 text-xs">
        <Loader2 className="size-3.5 animate-spin text-primary" aria-hidden="true" />
        <span className="font-medium">
          {phase ? PHASE_LABELS[phase] : PHASE_LABELS.preparing}
        </span>
        <span className="font-mono text-[11px] text-muted-foreground">
          {completed}/{safeTotal}
        </span>
        {canCancel ? (
          <Button
            variant="ghost"
            size="sm"
            className="ml-auto h-7"
            onClick={onCancel}
            disabled={cancelling}
          >
            {cancelLabel}
          </Button>
        ) : null}
      </div>
      <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-border">
        <div
          className="h-full rounded-full bg-primary transition-[width]"
          style={{ width: `${percent}%` }}
        />
      </div>
      {message ? (
        <p className="mt-1.5 text-[11px] text-muted-foreground">{message}</p>
      ) : null}
    </div>
  );
}

export function InstallWizard({
  open,
  inventorySkills,
  agents,
  onClose,
}: InstallWizardProps) {
  const queryClient = useQueryClient();
  const panelRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const onCloseRef = useRef(onClose);
  const taskIdRef = useRef<string | null>(null);
  const phaseRef = useRef<TaskPhase | null>(null);
  const installRunningRef = useRef(false);
  const agentsTouchedRef = useRef(false);
  const lastInstallRef = useRef<{ skills: string[]; agents: string[] } | null>(
    null,
  );
  const runSeq = useRef(0);

  const [step, setStep] = useState<Step>(1);
  const [sourceInput, setSourceInput] = useState("");
  const [taskId, setTaskId] = useState<string | null>(null);
  const [phase, setPhase] = useState<TaskPhase | null>(null);
  const [completed, setCompleted] = useState(0);
  const [total, setTotal] = useState(1);
  const [lastMessage, setLastMessage] = useState("");
  const [previewRunning, setPreviewRunning] = useState(false);
  const [installRunning, setInstallRunning] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [startError, setStartError] = useState<WizardError | null>(null);
  const [taskError, setTaskError] = useState<WizardError | null>(null);
  const [cancelledNotice, setCancelledNotice] = useState(false);
  const [installCancelled, setInstallCancelled] = useState(false);
  const [preview, setPreview] = useState<PreviewSummary | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [manualInput, setManualInput] = useState("");
  const [selectedAgents, setSelectedAgents] = useState<Set<string>>(new Set());
  const [installResult, setInstallResult] = useState<TaskResult | null>(null);

  useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);
  useEffect(() => {
    taskIdRef.current = taskId;
  }, [taskId]);
  useEffect(() => {
    phaseRef.current = phase;
  }, [phase]);
  useEffect(() => {
    installRunningRef.current = installRunning;
  }, [installRunning]);

  // Compute agent defaults when the step is first shown. Recompute if the
  // inventory arrives late, but never overwrite an explicit user choice.
  useEffect(() => {
    if (!open || step !== 3 || agentsTouchedRef.current) return;
    setSelectedAgents(defaultAgentSelection(agents, inventorySkills));
  }, [open, step, agents, inventorySkills]);

  const invalidateMutationQueries = useCallback(() => {
    void queryClient.invalidateQueries({ queryKey: ["inventory"] });
    void queryClient.invalidateQueries({ queryKey: ["skill"] });
  }, [queryClient]);

  const closeWizard = useCallback(() => {
    const activeTask = taskIdRef.current;
    const activePhase = phaseRef.current;
    if (installRunningRef.current) {
      // Before the CLI spawns the task can still be cancelled. Once writing
      // starts the modal stays open and the backend refuses cancellation.
      if (activePhase !== "preparing") return;
      if (activeTask) {
        void cancelTask(activeTask).catch(() => {
          // Best effort; the backend rejects cancellation after the seal.
        });
      }
    } else if (activeTask && activePhase && !isFinalPhase(activePhase)) {
      void cancelTask(activeTask).catch(() => {
        // Closing is best-effort cancellation; the read-only task times out.
      });
    }
    runSeq.current += 1;
    onCloseRef.current();
  }, []);

  useEffect(() => {
    if (!open) return;
    runSeq.current += 1;
    setStep(1);
    setSourceInput("");
    setTaskId(null);
    setPhase(null);
    setCompleted(0);
    setTotal(1);
    setLastMessage("");
    setPreviewRunning(false);
    setInstallRunning(false);
    setCancelling(false);
    setStartError(null);
    setTaskError(null);
    setCancelledNotice(false);
    setInstallCancelled(false);
    setPreview(null);
    setSelected(new Set());
    setManualInput("");
    setSelectedAgents(new Set());
    setInstallResult(null);
    lastInstallRef.current = null;
    agentsTouchedRef.current = false;
  }, [open]);

  useModalFocus({
    open,
    panelRef,
    initialFocusRef: closeRef,
    onClose: closeWizard,
  });

  const manualNames = useMemo(() => parseManualNames(manualInput), [manualInput]);
  const inventoryByName = useMemo(() => {
    const map = new Map<string, InventorySkill>();
    for (const skill of inventorySkills) map.set(skill.name, skill);
    return map;
  }, [inventorySkills]);
  const manualConflicts = useMemo(
    () =>
      manualNames
        .map((name) => inventoryByName.get(name))
        .filter((skill): skill is InventorySkill => Boolean(skill)),
    [manualNames, inventoryByName],
  );

  const normalSkills = preview?.skills ?? [];
  const normalSkillsAllConflicted =
    normalSkills.length > 0 && normalSkills.every((skill) => Boolean(skill.conflict));
  const selectedNames = useMemo(() => Array.from(selected), [selected]);
  const effectiveNames = preview?.parseFailed ? manualNames : selectedNames;
  const nextDisabled =
    !preview ||
    preview.noSkills ||
    (preview.parseFailed
      ? manualNames.length === 0 || manualConflicts.length > 0
      : selected.size === 0 || normalSkillsAllConflicted);

  const editableAgents = useMemo(
    () => agents.filter((agent) => !agent.universal),
    [agents],
  );
  const sharedAgents = useMemo(
    () => agents.filter((agent) => agent.universal),
    [agents],
  );
  const agentCounts = useMemo(
    () => countAgentSkills(inventorySkills),
    [inventorySkills],
  );
  const selectedAgentObjects = useMemo(
    () => editableAgents.filter((agent) => selectedAgents.has(agent.id)),
    [editableAgents, selectedAgents],
  );

  const writingLocked = installRunning && phase !== "preparing";
  const canCancelTask =
    Boolean(taskId) && !cancelling && (previewRunning || phase === "preparing");

  const handleTaskEvent = (event: TaskEvent) => {
    setPhase(event.phase);
    if (event.completed !== undefined) setCompleted(event.completed);
    if (event.total !== undefined) setTotal(event.total);
    if (event.message) setLastMessage(event.message);

    if (event.phase === "completed") {
      setPreviewRunning(false);
      setCancelling(false);
      const resultPreview = event.result?.preview ?? null;
      setPreview(resultPreview);
      if (resultPreview) {
        const installable = resultPreview.skills.filter(
          (skill) => !skill.conflict,
        );
        setSelected(new Set(installable.map((skill) => skill.name)));
        if (
          !resultPreview.parseFailed &&
          resultPreview.skills.length === 1 &&
          installable.length === 1
        ) {
          setStep(3);
        } else {
          setStep(2);
        }
      } else {
        setStep(2);
      }
      return;
    }
    if (event.phase === "failed") {
      setPreviewRunning(false);
      setCancelling(false);
      // Backend errors always carry a code; do not invent one on the client.
      setTaskError(event.error ?? { message: "预览失败，请重试。" });
      setStep(1);
      return;
    }
    if (event.phase === "cancelled") {
      setPreviewRunning(false);
      setCancelling(false);
      setCancelledNotice(true);
      setStep(1);
    }
  };

  const handlePreview = async () => {
    if (previewRunning || installRunning) return;
    const source = sourceInput.trim();
    if (!source) {
      setStartError({
        message: "请输入公开 GitHub 仓库，例如 owner/repo。",
      });
      return;
    }
    const seq = ++runSeq.current;
    setStartError(null);
    setTaskError(null);
    setCancelledNotice(false);
    setPreview(null);
    setSelected(new Set());
    setSelectedAgents(new Set());
    setInstallResult(null);
    lastInstallRef.current = null;
    agentsTouchedRef.current = false;
    setManualInput("");
    setStep(1);
    setPreviewRunning(true);
    setPhase("preparing");
    setCompleted(0);
    setTotal(1);
    setLastMessage("正在准备预览…");
    setTaskId(null);

    try {
      const id = await startPreview(source, (event) => {
        if (runSeq.current !== seq) return;
        handleTaskEvent(event);
      });
      if (runSeq.current !== seq) {
        void cancelTask(id).catch(() => {
          // The wizard was closed while the invoke was in flight.
        });
        return;
      }
      setTaskId(id);
    } catch (error) {
      if (runSeq.current !== seq) return;
      setStartError(
        asAppError(error) ?? {
          message: "无法开始预览，请稍后重试。",
        },
      );
      setPhase(null);
      setPreviewRunning(false);
    }
  };

  const handleInstallEvent = (event: TaskEvent) => {
    setPhase(event.phase);
    if (event.completed !== undefined) setCompleted(event.completed);
    if (event.total !== undefined) setTotal(event.total);
    if (event.message) setLastMessage(event.message);

    if (event.phase === "completed") {
      setInstallRunning(false);
      setCancelling(false);
      setInstallResult(event.result ?? null);
      invalidateMutationQueries();
      return;
    }
    if (event.phase === "failed") {
      setInstallRunning(false);
      setCancelling(false);
      setTaskError(event.error ?? { message: "安装失败，请重试。" });
      invalidateMutationQueries();
      return;
    }
    if (event.phase === "cancelled") {
      setInstallRunning(false);
      setCancelling(false);
      setInstallCancelled(true);
    }
  };

  const handleStartInstall = async (names: string[], retry = false) => {
    if (!preview || installRunning || previewRunning || names.length === 0) {
      return;
    }
    const requestAgents = Array.from(selectedAgents);
    lastInstallRef.current = { skills: names, agents: requestAgents };
    const seq = ++runSeq.current;
    setStep(5);
    setInstallRunning(true);
    setTaskError(null);
    setStartError(null);
    setInstallCancelled(false);
    setInstallResult(null);
    setTaskId(null);
    setPhase("preparing");
    setCompleted(0);
    setTotal(names.length);
    setLastMessage("正在准备安装…");

    try {
      // `retry` is only set by the explicit retry buttons. A plain install
      // still rejects every same-name managed/stale/copy-only row.
      const id = await startInstall(
        { source: preview.source, skills: names, agents: requestAgents, retry },
        (event) => {
          if (runSeq.current !== seq) return;
          handleInstallEvent(event);
        },
      );
      if (runSeq.current !== seq) {
        void cancelTask(id).catch(() => {
          // The wizard was closed while the invoke was in flight.
        });
        return;
      }
      setTaskId(id);
    } catch (error) {
      if (runSeq.current !== seq) return;
      setTaskError(
        asAppError(error) ?? {
          message: "无法开始安装，请稍后重试。",
        },
      );
      setPhase(null);
      setInstallRunning(false);
    }
  };

  const handleCancel = async () => {
    if (!taskId || cancelling) return;
    setCancelling(true);
    try {
      const ack = await cancelTask(taskId);
      if (!ack.accepted) {
        setTaskError({
          message: ack.message ?? "任务已结束，无法取消。",
        });
      }
    } catch (error) {
      setTaskError(
        asAppError(error) ?? {
          message: "无法取消任务。",
        },
      );
    } finally {
      setCancelling(false);
    }
  };

  const toggleSkill = (name: string) => {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  };

  const toggleAgent = (id: string) => {
    agentsTouchedRef.current = true;
    setSelectedAgents((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const handleNext = () => {
    if (nextDisabled) return;
    // Agent defaults were computed when the preview completed; going back to
    // this step must not discard the user's changes.
    setStep(3);
  };

  const retryNames =
    installResult?.skills
      .filter((skill) => skill.status !== "installed")
      .map((skill) => skill.name) ?? [];
  const lastInstall = lastInstallRef.current;
  const agentSummary = [
    ...selectedAgentObjects.map((agent) => agent.displayName),
    ...sharedAgents.map((agent) => agent.displayName),
    "Universal（共享 canonical）",
  ].join("、");
  const targetPaths = [
    { label: "共享 canonical", path: "~/.agents/skills" },
    ...selectedAgentObjects.map((agent) => ({
      label: agent.displayName,
      path: agent.globalSkillsDir ?? "—",
    })),
  ];

  if (!open) return null;

  const stepTitle =
    step === 1
      ? "选择来源"
      : step === 2
        ? "选择 skill"
        : step === 3
          ? "选择 agent"
          : step === 4
            ? "确认"
            : "执行安装";

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/30 p-6"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) closeWizard();
      }}
    >
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="install-wizard-title"
        data-testid="install-wizard"
        className="flex max-h-[86vh] w-full max-w-2xl flex-col rounded-xl border border-border bg-card shadow-xl"
      >
        <div className="flex items-start justify-between gap-3 px-5 pt-4">
          <div className="min-w-0">
            <h2 id="install-wizard-title" className="text-sm font-semibold">
              安装 skill · {stepTitle}
            </h2>
            <p className="mt-1 text-[11px] text-muted-foreground">
              v1 只接受公开 GitHub 根仓库，只安装默认分支。
            </p>
          </div>
          <Button
            ref={closeRef}
            variant="ghost"
            size="sm"
            aria-label="关闭安装向导"
            onClick={closeWizard}
            disabled={writingLocked}
            title={writingLocked ? "写入阶段不可取消" : undefined}
            className="size-7 shrink-0 p-0"
          >
            <X className="size-4" aria-hidden="true" />
          </Button>
        </div>
        <div className="px-5">
          <StepBars step={step} />
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
          {step === 1 ? (
            <>
              <div className="flex items-start gap-2">
                <Input
                  aria-label="仓库来源"
                  placeholder="owner/repo 或 https://github.com/owner/repo"
                  value={sourceInput}
                  onChange={(event) => setSourceInput(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") void handlePreview();
                  }}
                  disabled={previewRunning || installRunning}
                />
                <Button
                  onClick={() => void handlePreview()}
                  disabled={previewRunning || installRunning}
                  className="shrink-0"
                >
                  预览仓库
                </Button>
              </div>
              <p className="mt-2 rounded-md border border-border bg-muted/40 px-2.5 py-1.5 text-[11px] text-muted-foreground">
                示例：mattpocock/skills · 将读取仓库内的 SKILL.md 列表。不支持子目录、#ref、GitLab、SSH、本地路径或 Notion。
              </p>

              {startError ? (
                <div
                  role="alert"
                  className="mt-3 rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive"
                >
                  <p className="font-medium">{startError.message}</p>
                  {startError.code ? (
                    <p className="mt-0.5 font-mono text-[10.5px] opacity-70">
                      {startError.code}
                    </p>
                  ) : null}
                </div>
              ) : null}

              {taskError ? (
                <div
                  role="alert"
                  className="mt-3 rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive"
                >
                  <p className="font-medium">{taskError.message}</p>
                  {taskError.detail ? (
                    <p className="mt-0.5 line-clamp-3 text-[10.5px] opacity-70">
                      {taskError.detail}
                    </p>
                  ) : null}
                </div>
              ) : null}

              {previewRunning ? (
                <TaskProgress
                  phase={phase}
                  message={lastMessage}
                  completed={completed}
                  total={total}
                  cancelling={cancelling}
                  canCancel={canCancelTask}
                  cancelLabel="取消预览"
                  onCancel={() => void handleCancel()}
                />
              ) : null}

              {cancelledNotice ? (
                <p
                  role="status"
                  className="mt-3 rounded-md border border-border bg-muted/50 px-2.5 py-1.5 text-[11px] text-muted-foreground"
                >
                  预览已取消，可重新开始。
                </p>
              ) : null}
            </>
          ) : null}

          {step === 2 && preview ? (
            <>
              <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1 text-xs">
                <span className="font-medium">{preview.source}</span>
                <span className="text-muted-foreground">
                  · {preview.skills.length} 个 skill
                </span>
                <span
                  role="status"
                  className="ml-auto font-mono text-[10.5px] text-muted-foreground"
                >
                  {lastMessage} · {completed}/{total}
                </span>
              </div>

              {preview.noSkills ? (
                <div
                  data-testid="preview-empty"
                  className="mt-4 flex flex-col items-center justify-center rounded-lg border border-dashed border-border bg-muted/30 px-6 py-10 text-center"
                >
                  <PackageOpen
                    className="size-7 text-muted-foreground/60"
                    aria-hidden="true"
                  />
                  <p className="mt-2 text-sm font-medium">没有可安装的 skill</p>
                  <p className="mt-1 max-w-md text-[11px] leading-relaxed text-muted-foreground">
                    仓库中没有找到 SKILL.md（或缺少 name/description），因此无法继续安装。
                  </p>
                </div>
              ) : null}

              {preview.parseFailed ? (
                <>
                  <div
                    role="alert"
                    className="mt-3 flex items-start gap-2 rounded-lg border border-warning/30 bg-warning-soft px-3 py-2 text-[11px] text-warning"
                  >
                    <AlertTriangle
                      className="mt-0.5 size-3.5 shrink-0"
                      aria-hidden="true"
                    />
                    <div>
                      <p className="font-medium">预览输出无法解析</p>
                      <p className="mt-0.5 opacity-90">
                        已保留原始文本。你可以手动输入 skill 名继续。
                      </p>
                    </div>
                  </div>
                  <pre
                    aria-label="预览原始输出"
                    className="mt-2 max-h-52 overflow-auto rounded-lg border border-border bg-muted/40 p-2.5 font-mono text-[10.5px] leading-relaxed text-muted-foreground"
                  >
                    {preview.rawOutput || "（CLI 没有返回可展示的输出）"}
                  </pre>
                  <div className="mt-3">
                    <label
                      htmlFor="manual-skill-names"
                      className="text-[11px] font-medium"
                    >
                      手动输入 skill 名
                    </label>
                    <Input
                      id="manual-skill-names"
                      aria-label="手动输入 skill 名"
                      className="mt-1"
                      placeholder="多个名称用逗号、空格或换行分隔"
                      value={manualInput}
                      onChange={(event) => setManualInput(event.target.value)}
                    />
                    {manualNames.length > 0 ? (
                      <p className="mt-1 text-[10.5px] text-muted-foreground">
                        将安装：{manualNames.join("、")}
                      </p>
                    ) : null}
                    {manualConflicts.map((skill) => (
                      <p
                        key={skill.name}
                        className="mt-1 text-[10.5px] text-destructive"
                      >
                        同名冲突：{skill.name}（{managementLabel(skill.management)}
                        ）· 现有来源：{skill.source ?? "未知"}
                      </p>
                    ))}
                  </div>
                </>
              ) : null}

              {!preview.noSkills && !preview.parseFailed ? (
                <ul className="mt-3 space-y-1.5">
                  {preview.skills.map((skill, index) => {
                    const previousGroup =
                      index > 0 ? preview.skills[index - 1]!.group : undefined;
                    const showGroup =
                      Boolean(skill.group) && skill.group !== previousGroup;
                    const conflict = skill.conflict;
                    return (
                      <li key={skill.name}>
                        {showGroup ? (
                          <p className="mb-1 mt-2 text-[10.5px] font-semibold uppercase tracking-wide text-muted-foreground">
                            {skill.group}
                          </p>
                        ) : null}
                        <label
                          data-testid="preview-skill"
                          data-skill-name={skill.name}
                          className={
                            conflict
                              ? "flex cursor-not-allowed items-start gap-2.5 rounded-lg border border-border bg-muted/40 px-3 py-2 opacity-70"
                              : "flex cursor-pointer items-start gap-2.5 rounded-lg border border-border px-3 py-2 transition-colors hover:border-primary/40 aria-[checked=true]:border-primary/50 aria-[checked=true]:bg-primary/5"
                          }
                          aria-checked={selected.has(skill.name)}
                        >
                          <input
                            type="checkbox"
                            className="mt-0.5"
                            checked={selected.has(skill.name)}
                            disabled={Boolean(conflict)}
                            onChange={() => toggleSkill(skill.name)}
                            aria-label={`选择 ${skill.name}`}
                          />
                          <span className="min-w-0 flex-1">
                            <span className="flex flex-wrap items-center gap-1.5">
                              <span className="text-xs font-medium">
                                {skill.name}
                              </span>
                              {conflict ? (
                                <span className="rounded border border-border-strong bg-card px-1.5 py-0.5 text-[10px] text-muted-foreground">
                                  {conflictLabel(skill)}
                                </span>
                              ) : null}
                            </span>
                            <span className="mt-0.5 block text-[11px] leading-relaxed text-muted-foreground">
                              {skill.description || "（无描述）"}
                            </span>
                            {conflict ? (
                              <span className="mt-0.5 block text-[10.5px] text-muted-foreground">
                                现有来源：{conflict.source ?? "未知"}
                              </span>
                            ) : null}
                          </span>
                        </label>
                      </li>
                    );
                  })}
                </ul>
              ) : null}

              {normalSkillsAllConflicted ? (
                <p className="mt-2 rounded-md border border-warning/30 bg-warning-soft px-2.5 py-1.5 text-[11px] text-warning">
                  仓库中的 skill 全部与现有 skill 同名，无法安装。
                </p>
              ) : null}
            </>
          ) : null}

          {step === 3 ? (
            <>
              <p className="text-[11px] leading-relaxed text-muted-foreground">
                只显示本机检测到的 agent。默认勾选已含受管 skill 的 agent；仅有未受管目录的 agent 默认不勾选。
              </p>
              <ul className="mt-3 space-y-1.5">
                {editableAgents.map((agent) => {
                  const counts = agentCounts.get(agent.id);
                  const managed = counts?.managed ?? 0;
                  const unmanaged = counts?.unmanaged ?? 0;
                  const onlyUnmanaged = managed === 0 && unmanaged > 0;
                  return (
                    <li key={agent.id}>
                      <label
                        data-testid="agent-option"
                        data-agent-id={agent.id}
                        data-only-unmanaged={onlyUnmanaged ? "true" : "false"}
                        className="flex cursor-pointer items-start gap-2.5 rounded-lg border border-border px-3 py-2 transition-colors hover:border-primary/40 aria-[checked=true]:border-primary/50 aria-[checked=true]:bg-primary/5"
                        aria-checked={selectedAgents.has(agent.id)}
                      >
                        <input
                          type="checkbox"
                          className="mt-0.5"
                          checked={selectedAgents.has(agent.id)}
                          onChange={() => toggleAgent(agent.id)}
                          aria-label={`选择 ${agent.displayName}`}
                        />
                        <span className="min-w-0 flex-1">
                          <span className="flex flex-wrap items-center gap-1.5">
                            <span className="text-xs font-medium">
                              {agent.displayName}
                            </span>
                            {onlyUnmanaged ? (
                              <span className="rounded border border-warning/40 bg-warning-soft px-1.5 py-0.5 text-[10px] text-warning">
                                仅含未受管 skill
                              </span>
                            ) : managed > 0 ? (
                              <span className="rounded border border-border-strong bg-card px-1.5 py-0.5 text-[10px] text-muted-foreground">
                                已含受管 skill
                              </span>
                            ) : null}
                          </span>
                          <span className="mt-0.5 block font-mono text-[10.5px] text-muted-foreground">
                            {agent.globalSkillsDir ?? "—"}
                          </span>
                          {onlyUnmanaged ? (
                            <span className="mt-0.5 block text-[10.5px] text-muted-foreground">
                              目录中只有未受管 skill，默认不勾选
                            </span>
                          ) : null}
                        </span>
                      </label>
                    </li>
                  );
                })}
              </ul>

              <div
                data-testid="shared-agents"
                className="mt-3 rounded-lg border border-border bg-muted/40 px-3 py-2 text-[11px]"
              >
                <p className="font-medium">共享 canonical</p>
                <p className="mt-1 leading-relaxed text-muted-foreground">
                  Universal · ~/.agents/skills（共享目录）
                </p>
                {sharedAgents.length > 0 ? (
                  <p className="mt-0.5 leading-relaxed text-muted-foreground">
                    {sharedAgents.map((agent) => agent.displayName).join("、")}
                    {" · 直接读取共享目录，无需单独挂载"}
                  </p>
                ) : null}
              </div>

              {agents.length === 0 ? (
                <p className="mt-3 rounded-md border border-border bg-muted/50 px-2.5 py-1.5 text-[11px] text-muted-foreground">
                  未检测到 agent，安装将只写入共享 canonical。
                </p>
              ) : null}

              <p className="mt-2 rounded-md border border-border bg-muted/50 px-2.5 py-1.5 text-[11px] leading-relaxed text-muted-foreground">
                后台始终追加 <span className="font-mono">-a universal</span>
                ，确保写入共享 canonical，避免单 agent 默认 copy。
              </p>
            </>
          ) : null}

          {step === 4 && preview ? (
            <div data-testid="install-confirm">
              <div className="rounded-lg border border-border bg-muted/40 p-3 text-xs">
                <div className="flex justify-between gap-4 py-0.5">
                  <span className="text-muted-foreground">来源</span>
                  <strong className="font-mono">{preview.source}</strong>
                </div>
                <div className="flex justify-between gap-4 py-0.5">
                  <span className="text-muted-foreground">Skill</span>
                  <strong>
                    {effectiveNames.length} 个 · {effectiveNames.join("、") || "—"}
                  </strong>
                </div>
                <div className="flex justify-between gap-4 py-0.5">
                  <span className="text-muted-foreground">Agents</span>
                  <strong>{agentSummary}</strong>
                </div>
              </div>

              <div className="mt-3 rounded-lg border border-border p-3 text-xs">
                <p className="font-medium">目标路径</p>
                <ul className="mt-1.5 space-y-1">
                  {targetPaths.map((target) => (
                    <li
                      key={`${target.label}-${target.path}`}
                      className="flex justify-between gap-4"
                    >
                      <span className="text-muted-foreground">{target.label}</span>
                      <span className="font-mono text-[10.5px]">{target.path}</span>
                    </li>
                  ))}
                </ul>
                {sharedAgents.length > 0 ? (
                  <p className="mt-1.5 text-[10.5px] text-muted-foreground">
                    共享 canonical 的 agent（
                    {sharedAgents.map((agent) => agent.displayName).join("、")}
                    ）直接读取共享目录。
                  </p>
                ) : null}
              </div>

              <p
                data-testid="writing-warning"
                className="mt-3 rounded-md border border-warning/30 bg-warning-soft px-2.5 py-1.5 text-[11px] leading-relaxed text-warning"
              >
                写入阶段不可取消。安装前会快照 lock；每个来源只执行一次 CLI；完成后逐项校验 canonical、agent 挂载与 lock。
              </p>
            </div>
          ) : null}

          {step === 5 ? (
            <>
              {installRunning ? (
                <>
                  <TaskProgress
                    phase={phase}
                    message={lastMessage}
                    completed={completed}
                    total={total}
                    cancelling={cancelling}
                    canCancel={
                      Boolean(taskId) && phase === "preparing" && !cancelling
                    }
                    cancelLabel="取消安装"
                    onCancel={() => void handleCancel()}
                  />
                  <p
                    data-testid="install-lock-note"
                    className="mt-2 rounded-md border border-border bg-muted/50 px-2.5 py-1.5 text-[11px] text-muted-foreground"
                  >
                    {phase === "preparing"
                      ? "正在写入前准备；取消后不会调用 CLI。"
                      : "写入阶段不可取消。"}
                  </p>
                </>
              ) : null}

              {installCancelled ? (
                <p
                  role="status"
                  data-testid="install-cancelled"
                  className="mt-3 rounded-md border border-border bg-muted/50 px-2.5 py-1.5 text-[11px] text-muted-foreground"
                >
                  安装已取消，未写入任何文件。
                </p>
              ) : null}

              {taskError ? (
                <div
                  role="alert"
                  data-testid="install-error"
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

              {installResult ? (
                <div data-testid="install-result" data-overall={installResult.overall}>
                  <div className="flex items-baseline gap-2">
                    <h3 className="text-sm font-semibold">
                      {overallHeading(installResult.overall)}
                    </h3>
                    <span className="text-[11px] text-muted-foreground">
                      {lastMessage}
                    </span>
                  </div>
                  <ul className="mt-2 space-y-1.5">
                    {installResult.skills.map((skill) => (
                      <li
                        key={skill.name}
                        data-testid="install-skill-result"
                        data-skill-name={skill.name}
                        data-skill-status={skill.status}
                        className="rounded-lg border border-border px-3 py-2"
                      >
                        <div className="flex items-center gap-2">
                          <span className="text-xs font-medium">{skill.name}</span>
                          <span
                            className={
                              skill.status === "installed"
                                ? "rounded border border-success/30 bg-success-soft px-1.5 py-0.5 text-[10px] text-success"
                                : "rounded border border-warning/30 bg-warning-soft px-1.5 py-0.5 text-[10px] text-warning"
                            }
                          >
                            {skillStatusLabel(skill.status)}
                          </span>
                          {skill.status !== "installed" ? (
                            <Button
                              variant="ghost"
                              size="sm"
                              className="ml-auto h-7"
                              disabled={installRunning}
                              onClick={() =>
                                void handleStartInstall([skill.name], true)
                              }
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
                      </li>
                    ))}
                  </ul>

                  {installResult.warnings.length > 0 ? (
                    <ul
                      data-testid="install-warnings"
                      className="mt-2 space-y-1 rounded-lg border border-warning/30 bg-warning-soft px-3 py-2 text-[10.5px] text-warning"
                    >
                      {installResult.warnings.map((warning) => (
                        <li key={`${warning.code}-${warning.message}`}>
                          {warning.message}
                          {warning.detail ? ` · ${warning.detail}` : ""}
                        </li>
                      ))}
                    </ul>
                  ) : null}

                  {installResult.backupPath ? (
                    <p className="mt-2 font-mono text-[10px] text-muted-foreground">
                      lock 快照：{installResult.backupPath}
                    </p>
                  ) : null}
                </div>
              ) : null}
            </>
          ) : null}
        </div>

        <div className="flex items-center justify-between gap-2 border-t border-border px-5 py-3">
          <div className="text-[10.5px] text-muted-foreground">
            {step === 5 && writingLocked
              ? "写入阶段不可取消"
              : "Esc 关闭 · 公开 GitHub · 仅默认分支"}
          </div>
          <div className="flex items-center gap-2">
            {step === 2 || step === 3 ? (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setStep((step - 1) as Step)}
              >
                上一步
              </Button>
            ) : null}
            {step === 2 ? (
              <Button size="sm" onClick={handleNext} disabled={nextDisabled}>
                下一步
              </Button>
            ) : null}
            {step === 3 ? (
              <Button
                size="sm"
                onClick={() => setStep(4)}
                disabled={preview === null}
              >
                下一步
              </Button>
            ) : null}
            {step === 4 ? (
              <>
                <Button variant="ghost" size="sm" onClick={() => setStep(3)}>
                  上一步
                </Button>
                <Button
                  size="sm"
                  onClick={() => void handleStartInstall(effectiveNames)}
                  disabled={installRunning || effectiveNames.length === 0}
                >
                  开始安装
                </Button>
              </>
            ) : null}
            {step === 5 && !installRunning ? (
              <>
                {installResult && retryNames.length > 0 ? (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => void handleStartInstall(retryNames, true)}
                  >
                    重试失败项
                  </Button>
                ) : null}
                {taskError && lastInstall ? (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() =>
                      void handleStartInstall(lastInstall.skills, true)
                    }
                  >
                    重试
                  </Button>
                ) : null}
                {taskError || installCancelled ? (
                  <Button variant="ghost" size="sm" onClick={() => setStep(4)}>
                    返回确认
                  </Button>
                ) : null}
                <Button size="sm" onClick={closeWizard}>
                  {installResult?.overall === "success" ? "完成" : "关闭"}
                </Button>
              </>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}
