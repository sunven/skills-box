import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AlertTriangle, Loader2, PackageOpen, X } from "lucide-react";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { asAppError, cancelTask, startPreview } from "../lib/api";
import type {
  InventorySkill,
  PreviewSkill,
  PreviewSummary,
  TaskEvent,
  TaskPhase,
} from "../lib/types";

interface InstallWizardProps {
  open: boolean;
  inventorySkills: InventorySkill[];
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
  running_cli: "正在执行 CLI",
  verifying: "正在解析结果",
  completed: "已完成",
  failed: "失败",
  cancelled: "已取消",
};

type Step = 1 | 2 | 3;

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

function StepBars({ step }: { step: Step }) {
  return (
    <div className="mt-3 flex gap-1.5" aria-hidden="true">
      {[1, 2, 3, 4].map((index) => (
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

function TaskProgress({
  phase,
  message,
  completed,
  total,
  cancelling,
  canCancel,
  onCancel,
}: {
  phase: TaskPhase | null;
  message: string;
  completed: number;
  total: number;
  cancelling: boolean;
  canCancel: boolean;
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
            取消预览
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
  onClose,
}: InstallWizardProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const onCloseRef = useRef(onClose);
  const taskIdRef = useRef<string | null>(null);
  const phaseRef = useRef<TaskPhase | null>(null);
  const runSeq = useRef(0);

  const [step, setStep] = useState<Step>(1);
  const [sourceInput, setSourceInput] = useState("");
  const [taskId, setTaskId] = useState<string | null>(null);
  const [phase, setPhase] = useState<TaskPhase | null>(null);
  const [completed, setCompleted] = useState(0);
  const [total, setTotal] = useState(1);
  const [lastMessage, setLastMessage] = useState("");
  const [starting, setStarting] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [startError, setStartError] = useState<WizardError | null>(null);
  const [taskError, setTaskError] = useState<WizardError | null>(null);
  const [cancelledNotice, setCancelledNotice] = useState(false);
  const [preview, setPreview] = useState<PreviewSummary | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [manualInput, setManualInput] = useState("");

  useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);
  useEffect(() => {
    taskIdRef.current = taskId;
  }, [taskId]);
  useEffect(() => {
    phaseRef.current = phase;
  }, [phase]);

  const closeWizard = useCallback(() => {
    const activeTask = taskIdRef.current;
    const activePhase = phaseRef.current;
    if (activeTask && activePhase && !isFinalPhase(activePhase)) {
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
    setStarting(false);
    setCancelling(false);
    setStartError(null);
    setTaskError(null);
    setCancelledNotice(false);
    setPreview(null);
    setSelected(new Set());
    setManualInput("");
    const previousFocus = document.activeElement as HTMLElement | null;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        closeWizard();
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
  }, [open, closeWizard]);

  const running = starting || (phase !== null && !isFinalPhase(phase));
  const canCancel = running && Boolean(taskId) && !cancelling;

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

  const handleTaskEvent = (event: TaskEvent) => {
    setPhase(event.phase);
    if (event.completed !== undefined) setCompleted(event.completed);
    if (event.total !== undefined) setTotal(event.total);
    if (event.message) setLastMessage(event.message);

    if (event.phase === "completed") {
      setStarting(false);
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
      setStarting(false);
      setCancelling(false);
      setTaskError(
        event.error ?? {
          code: "source.preview_failed",
          message: "预览失败，请重试。",
        },
      );
      setStep(1);
      return;
    }
    if (event.phase === "cancelled") {
      setStarting(false);
      setCancelling(false);
      setCancelledNotice(true);
      setStep(1);
    }
  };

  const handlePreview = async () => {
    if (running) return;
    const source = sourceInput.trim();
    if (!source) {
      setStartError({
        code: "source.missing",
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
    setManualInput("");
    setStep(1);
    setStarting(true);
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
      setStarting(false);
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
          message: "无法取消预览。",
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

  const handleNext = () => {
    if (nextDisabled) return;
    setStep(3);
  };

  if (!open) return null;

  const stepTitle =
    step === 1
      ? "选择来源"
      : step === 2
        ? "选择 skill"
        : "确认（下一步）";

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
                  disabled={running}
                  autoFocus
                />
                <Button
                  onClick={() => void handlePreview()}
                  disabled={running}
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

              {running || phase === "preparing" ? (
                <TaskProgress
                  phase={phase}
                  message={lastMessage}
                  completed={completed}
                  total={total}
                  cancelling={cancelling}
                  canCancel={canCancel}
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

          {step === 3 && preview ? (
            <div data-testid="wizard-confirm-placeholder">
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
                  <span className="text-muted-foreground">目标</span>
                  <strong>~/.agents/skills + agent 链接</strong>
                </div>
              </div>
              <p className="mt-3 rounded-md border border-border bg-muted/50 px-2.5 py-1.5 text-[11px] leading-relaxed text-muted-foreground">
                下一步将选择 agent、确认目标并执行安装（后续版本）。写入阶段不可取消。
              </p>
            </div>
          ) : null}
        </div>

        <div className="flex items-center justify-between gap-2 border-t border-border px-5 py-3">
          <div className="text-[10.5px] text-muted-foreground">
            Esc 关闭 · 公开 GitHub · 仅默认分支
          </div>
          <div className="flex items-center gap-2">
            {step > 1 ? (
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
                disabled
                title="选择 agent、确认与执行将在后续版本提供"
              >
                继续（后续版本）
              </Button>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}
