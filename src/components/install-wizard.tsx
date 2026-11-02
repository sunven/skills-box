import type { DiscoveryInstallSeed } from "../lib/discovery";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AlertTriangle, PackageOpen, X } from "lucide-react";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { MutationError } from "./mutation/mutation-error";
import { MutationProgress } from "./mutation/mutation-progress";
import { MutationResult } from "./mutation/mutation-result";
import { useModalFocus } from "../hooks/use-modal-focus";
import {
  fallbackError,
  invalidateDiscoverySource,
  startInstall,
  startPreview,
} from "../lib/api";
import type {
  DetectedAgent,
  InventoryAgent,
  InventorySkill,
  PreviewSkill,
  PreviewSummary,
  ProjectEntry,
  ScopeId,
  TaskEvent,
} from "../lib/types";
import { GLOBAL_SCOPE } from "../lib/types";
import { projectCanonicalPath, projectScope } from "../lib/scope";
import { taskEventDriver, useMutation } from "../lib/mutation";

interface InstallWizardProps {
  open: boolean;
  seed?: DiscoveryInstallSeed | null;
  scope: ScopeId;
  /** Active project for project scope; null for global. */
  project?: ProjectEntry | null;
  /** Registered projects for the compact scope shortcut. */
  projects?: ProjectEntry[];
  inventorySkills: InventorySkill[];
  /** Globally detected catalog agents plus extras (global target set). */
  agents: DetectedAgent[];
  /** Explicit scope target set (project scope); missing dirs stay listed. */
  installTargets?: InventoryAgent[];
  /** Switch the app's active scope; the wizard resets to step 1. */
  onSelectScope?: (scope: ScopeId) => void;
  onClose: () => void;
}

type Step = 1 | 2 | 3 | 4 | 5;

/** Minimal agent row shared by the global and project target sets. */
interface WizardAgent {
  id: string;
  displayName: string;
  universal: boolean;
  shared: boolean;
  globalSkillsDir: string | null;
}

interface AgentCount {
  managed: number;
  unmanaged: number;
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
  agents: WizardAgent[],
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

function StepBars({ step }: { step: Step }) {
  return (
    <div className="step-track" aria-hidden="true">
      {[1, 2, 3, 4, 5].map((index) => (
        <span key={index} data-done={index <= step ? "true" : "false"} />
      ))}
    </div>
  );
}

export function InstallWizard({
  open,
  seed = null,
  scope,
  project = null,
  projects = [],
  inventorySkills,
  agents,
  installTargets = [],
  onSelectScope,
  onClose,
}: InstallWizardProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const onCloseRef = useRef(onClose);
  const agentsTouchedRef = useRef(false);
  const lastInstallRef = useRef<string[] | null>(null);

  const seedRef = useRef(seed);
  seedRef.current = seed;
  const [seedIssue, setSeedIssue] = useState<string | null>(null);
  const [step, setStep] = useState<Step>(1);
  const [sourceInput, setSourceInput] = useState("");
  const [preview, setPreview] = useState<PreviewSummary | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [manualInput, setManualInput] = useState("");
  const [selectedAgents, setSelectedAgents] = useState<Set<string>>(new Set());

  useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);

  // Preview is read-only; only its completed summary drives the wizard steps.
  const handlePreviewEvent = useCallback((event: TaskEvent) => {
    if (event.phase === "completed") {
      const resultPreview = event.result?.preview ?? null;
      setPreview(resultPreview);
      setSeedIssue(null);
      const intent = seedRef.current;
      if (intent && resultPreview) {
        const matches = resultPreview.skills.filter((s) => s.name.toLowerCase() === intent.name.toLowerCase());
        if (resultPreview.parseFailed || matches.length !== 1 || resultPreview.noSkills) {
          setSelected(new Set());
          setSeedIssue("此发现项在实际预览中已不存在或无法按名称区分。请关闭向导并重新检查上游。");
          setStep(2);
          void invalidateDiscoverySource(intent.source).catch(() => {
            setSeedIssue("此发现项无法安装，且目录失效标记保存失败。请修复发现记录后重新检查上游。");
          });
          return;
        }
        setSelected(new Set(matches[0].conflict ? [] : [matches[0].name]));
        setStep(resultPreview.skills.length === 1 && !matches[0].conflict ? 3 : 2);
        return;
      }
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
      setStep(1);
      return;
    }
    if (event.phase === "cancelled") {
      setStep(1);
    }
  }, []);

  const previewMutation = useMutation({
    action: "preview",
    scope,
    fallbackFailure: "预览失败，请重试。",
    fallbackStart: "无法开始预览，请稍后重试。",
    onEvent: handlePreviewEvent,
  });
  const installMutation = useMutation({
    action: "install",
    scope,
    fallbackFailure: "安装失败，请重试。",
    fallbackStart: "无法开始安装，请稍后重试。",
  });

  const previewState = previewMutation.state;
  const installState = installMutation.state;
  const previewRunning = previewState.running;
  const installRunning = installState.running;
  const previewPhase = previewState.phase;
  const installPhase = installState.phase;
  const previewCancelling = previewState.cancelling;
  const installCancelling = installState.cancelling;
  const installResult = installState.result;
  const installCancelled = installPhase === "cancelled";
  const cancelledNotice = previewPhase === "cancelled";

  const projectMode = scope !== GLOBAL_SCOPE;
  const canonicalPath = projectMode
    ? project
      ? projectCanonicalPath(project.root)
      : "<项目>/.agents/skills"
    : "~/.agents/skills";
  // The project target set comes from the scoped snapshot and includes
  // configured extras whose project directory does not exist yet.
  const targetAgents = useMemo<WizardAgent[]>(
    () =>
      projectMode
        ? installTargets.map((agent) => ({
            id: agent.id,
            displayName: agent.displayName,
            universal: agent.universal,
            shared: agent.shared,
            globalSkillsDir: agent.globalSkillsDir,
          }))
        : agents.map((agent) => ({
            id: agent.id,
            displayName: agent.displayName,
            universal: agent.universal,
            shared: false,
            globalSkillsDir: agent.globalSkillsDir,
          })),
    [agents, installTargets, projectMode],
  );
  const targetScopeLabel = projectMode
    ? `项目「${project?.displayName ?? "未命名项目"}」`
    : "全局";

  // Compute agent defaults when the step is first shown. Recompute if the
  // inventory arrives late, but never overwrite an explicit user choice.
  // Project installs default to no agent: canonical is always written and
  // every project mount is an explicit choice.
  useEffect(() => {
    if (!open || step !== 3 || agentsTouchedRef.current) return;
    setSelectedAgents(
      projectMode ? new Set() : defaultAgentSelection(targetAgents, inventorySkills),
    );
  }, [open, step, projectMode, targetAgents, inventorySkills]);

  const closeWizard = useCallback(() => {
    if (installMutation.requestClose([previewMutation])) onCloseRef.current();
  }, [installMutation.requestClose, previewMutation]);

  const resetPreviewSelection = useCallback(() => {
    installMutation.reset();
    setPreview(null);
    setSelected(new Set());
    setSelectedAgents(new Set());
    lastInstallRef.current = null;
    agentsTouchedRef.current = false;
    setManualInput("");
    setSeedIssue(null);
    setStep(1);
  }, [installMutation.reset]);

  useEffect(() => {
    if (!open) return;
    previewMutation.reset();
    resetPreviewSelection();
    setSourceInput(seed?.source ?? "");
    if (seed) {
      void previewMutation.start(taskEventDriver({ total: 1, message: "正在验证发现项…",
        launch: (onEvent) => startPreview(seed.source, onEvent, scope) }));
    }
  }, [open, scope, seed?.source, seed?.name, resetPreviewSelection, previewMutation.reset, previewMutation.start]);

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
    Boolean(seedIssue) ||
    !preview ||
    preview.noSkills ||
    (preview.parseFailed
      ? manualNames.length === 0 || manualConflicts.length > 0
      : selected.size === 0 || normalSkillsAllConflicted);

  const editableAgents = useMemo(
    () => targetAgents.filter((agent) => !agent.universal),
    [targetAgents],
  );
  const sharedAgents = useMemo(
    () => targetAgents.filter((agent) => agent.universal),
    [targetAgents],
  );
  const agentCounts = useMemo(
    () => countAgentSkills(inventorySkills),
    [inventorySkills],
  );
  const selectedAgentObjects = useMemo(
    () => editableAgents.filter((agent) => selectedAgents.has(agent.id)),
    [editableAgents, selectedAgents],
  );

  const writingLocked = !installMutation.canClose();

  const handlePreview = () => {
    if (previewRunning || installRunning) return;
    const source = sourceInput.trim();
    if (!source) {
      previewMutation.setStartError(
        fallbackError("请输入公开 HTTPS Git 仓库，或 GitHub owner/repo。"),
      );
      return;
    }
    resetPreviewSelection();
    void previewMutation.start(
      taskEventDriver({
        total: 1,
        message: "正在准备预览…",
        launch: (onEvent) => startPreview(source, onEvent, scope),
      }),
    );
  };

  const handleStartInstall = (names: string[], retry = false) => {
    if (seedIssue || !preview || installRunning || previewRunning || names.length === 0) {
      return;
    }
    const requestAgents = Array.from(selectedAgents);
    lastInstallRef.current = names;
    setStep(5);
    // `retry` is only set by the explicit retry buttons. A plain install
    // still rejects every same-name managed/stale/copy-only row.
    void installMutation.start(
      taskEventDriver({
        total: names.length,
        message: "正在准备安装…",
        launch: (onEvent) =>
          startInstall(
            {
              source: preview.source,
              skills: names,
              agents: requestAgents,
              retry,
            },
            onEvent,
            scope,
          ),
      }),
    );
  };

  const handleCancel = () => {
    if (installRunning) {
      void installMutation.cancel("任务已结束，无法取消。");
    } else {
      void previewMutation.cancel("任务已结束，无法取消。");
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

  const lastInstall = lastInstallRef.current;
  const universalLabel = projectMode
    ? "Universal（项目 canonical）"
    : "Universal（共享 canonical）";
  const agentSummary = [
    ...selectedAgentObjects.map((agent) => agent.displayName),
    ...sharedAgents.map((agent) => agent.displayName),
    universalLabel,
  ].join("、");
  const targetPaths = [
    {
      label: projectMode ? "项目 canonical" : "共享 canonical",
      path: canonicalPath,
    },
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
      className="modal-overlay"
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
        className="modal-panel max-w-2xl"
      >
        {seedIssue ? <p role="alert" className="notice m-4" data-tone="error">{seedIssue}</p> : null}
        <div className="modal-head">
          <div className="min-w-0">
            <h2 id="install-wizard-title" className="modal-title">
              安装 skill · {stepTitle}
            </h2>
            <p className="modal-sub">
              支持公开 HTTPS Git 根仓库，只安装默认分支。
            </p>
            <p
              data-testid="install-target-scope"
              className="mt-1 text-[12px] text-muted-foreground"
            >
              目标作用域：{targetScopeLabel} · canonical{" "}
              <span className="mono">{canonicalPath}</span>
            </p>
            <label className="mt-1 flex items-center gap-1.5 text-[12px] text-muted-foreground">
              切换目标作用域
              <select
                aria-label="目标作用域"
                data-testid="install-scope-select"
                value={scope}
                disabled={writingLocked || !onSelectScope}
                onChange={(event) => {
                  const next = event.target.value as ScopeId;
                  if (next !== scope) onSelectScope?.(next);
                }}
                className="h-7 rounded-md border border-border-strong bg-card px-1.5 text-[12px] text-foreground"
              >
                <option value={GLOBAL_SCOPE}>全局</option>
                {projects.map((entry) => (
                  <option
                    key={entry.id}
                    value={projectScope(entry.id)}
                    disabled={!entry.available}
                  >
                    {entry.displayName}
                    {entry.available ? "" : "（不可用）"}
                  </option>
                ))}
              </select>
            </label>
          </div>
          <Button
            ref={closeRef}
            variant="ghost"
            size="icon"
            aria-label="关闭安装向导"
            onClick={closeWizard}
            disabled={writingLocked}
            title={writingLocked ? "写入阶段不可取消" : undefined}
            className="size-7 shrink-0"
          >
            <X className="size-4" aria-hidden="true" />
          </Button>
        </div>
        <div className="px-5">
          <StepBars step={step} />
        </div>

        <div className="modal-body">
          {step === 1 ? (
            <>
              <div className="flex items-start gap-2">
                <Input
                  aria-label="仓库来源"
                  placeholder="GitHub owner/repo 或 HTTPS Git 仓库地址"
                  value={sourceInput}
                  readOnly={Boolean(seed)}
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
              <p className="mt-2 text-[12px] text-muted-foreground">
                示例：mattpocock/skills · 将读取仓库内的 SKILL.md 列表。不支持子目录、#ref、SSH、本地路径或 Notion。
              </p>

              <MutationError error={previewState.startError} showCode />
              <MutationError error={previewState.error} />

              {previewRunning ? (
                <MutationProgress
                  phase={previewState.phase}
                  message={previewState.message}
                  completed={previewState.completed}
                  total={previewState.total}
                  cancelling={previewCancelling}
                  canCancel={Boolean(previewState.taskId) && !previewCancelling}
                  cancelLabel="取消预览"
                  onCancel={handleCancel}
                />
              ) : null}

              {cancelledNotice ? (
                <p
                  role="status"
                  className="mt-3 text-[12px] text-muted-foreground"
                >
                  预览已取消，可重新开始。
                </p>
              ) : null}
            </>
          ) : null}

          {step === 2 && preview ? (
            <>
              <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1 text-[12.5px]">
                <span className="font-medium">{preview.source}</span>
                <span className="text-muted-foreground">
                  · {preview.skills.length} 个 skill
                </span>
                <span
                  role="status"
                  className="ml-auto num text-[12px] text-muted-foreground"
                >
                  {previewState.message} · {previewState.completed}/{previewState.total}
                </span>
              </div>

              {preview.noSkills ? (
                <div
                  data-testid="preview-empty"
                  className="mt-4 flex flex-col items-center justify-center rounded-md border border-dashed border-border bg-raised px-6 py-10 text-center"
                >
                  <PackageOpen
                    className="size-6 text-muted-foreground"
                    aria-hidden="true"
                  />
                  <p className="mt-2 text-[13px] font-medium">没有可安装的 skill</p>
                  <p className="mt-1 max-w-md text-[12px] leading-relaxed text-muted-foreground">
                    仓库中没有找到 SKILL.md（或缺少 name/description），因此无法继续安装。
                  </p>
                </div>
              ) : null}

              {preview.parseFailed && !seed ? (
                <>
                  <div
                    role="alert"
                    className="notice mt-3" data-tone="warning"
                  >
                    <AlertTriangle
                      className="mt-0.5 size-3.5 shrink-0"
                      aria-hidden="true"
                    />
                    <div>
                      <p className="font-medium">预览输出无法解析</p>
                      <p className="mt-0.5">
                        已保留原始文本。你可以手动输入 skill 名继续。
                      </p>
                    </div>
                  </div>
                  <pre
                    aria-label="预览原始输出"
                    className="md-raw mt-2 max-h-52"
                  >
                    {preview.rawOutput || "（CLI 没有返回可展示的输出）"}
                  </pre>
                  <div className="mt-3">
                    <label
                      htmlFor="manual-skill-names"
                      className="field-label"
                    >
                      手动输入 skill 名
                    </label>
                    <Input
                      id="manual-skill-names"
                      aria-label="手动输入 skill 名"
                      className="mt-1.5"
                      placeholder="多个名称用逗号、空格或换行分隔"
                      value={manualInput}
                      onChange={(event) => setManualInput(event.target.value)}
                    />
                    {manualNames.length > 0 ? (
                      <p className="mt-1 text-[12px] text-muted-foreground">
                        将安装：{manualNames.join("、")}
                      </p>
                    ) : null}
                    {manualConflicts.map((skill) => (
                      <p
                        key={skill.name}
                        className="mt-1 text-[12px] text-destructive"
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
                          <p className="mb-1 mt-2 text-[12px] font-medium text-muted-foreground">
                            {skill.group}
                          </p>
                        ) : null}
                        <label
                          data-testid="preview-skill"
                          data-skill-name={skill.name}
                          className={
                            conflict
                              ? "flex cursor-not-allowed items-start gap-2.5 rounded-md border border-border bg-raised px-3 py-2.5 opacity-70"
                              : "flex cursor-pointer items-start gap-2.5 rounded-md border border-border px-3 py-2.5 transition-colors hover:border-border-strong aria-[checked=true]:border-border-strong aria-[checked=true]:bg-active"
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
                              <span className="text-[13px] font-medium">
                                {skill.name}
                              </span>
                              {conflict ? (
                                <span className="rounded-sm bg-muted px-1.5 py-0.5 text-[11.5px] text-muted-foreground">
                                  {conflictLabel(skill)}
                                </span>
                              ) : null}
                            </span>
                            <span className="mt-0.5 block text-[12px] leading-relaxed text-muted-foreground">
                              {skill.description || "（无描述）"}
                            </span>
                            {conflict ? (
                              <span className="mt-0.5 block text-[12px] text-muted-foreground">
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
                <p className="notice mt-2" data-tone="warning">
                  仓库中的 skill 全部与现有 skill 同名，无法安装。
                </p>
              ) : null}
            </>
          ) : null}

          {step === 3 ? (
            <>
              <p className="text-[12.5px] leading-relaxed text-muted-foreground">
                {projectMode
                  ? "项目作用域：只列出全局已检测到的 catalog agent 与已配置项目目录的 extra agent。默认不勾选；目录不存在时会在安装时创建。"
                  : "只显示本机检测到的 agent。默认勾选已含受管 skill 的 agent；仅有未受管目录的 agent 默认不勾选。"}
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
                        className="flex cursor-pointer items-start gap-2.5 rounded-md border border-border px-3 py-2.5 transition-colors hover:border-border-strong aria-[checked=true]:border-border-strong aria-[checked=true]:bg-active"
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
                            <span className="text-[13px] font-medium">
                              {agent.displayName}
                            </span>
                            {onlyUnmanaged ? (
                              <span className="ml-auto rounded-sm bg-warning-soft px-1.5 py-0.5 text-[11.5px] font-medium text-warning">
                                仅含未受管 skill
                              </span>
                            ) : managed > 0 ? (
                              <span className="rounded-sm bg-muted px-1.5 py-0.5 text-[11.5px] text-muted-foreground">
                                已含受管 skill
                              </span>
                            ) : null}
                          </span>
                          <span className="mt-0.5 block font-mono text-[11px] text-muted-foreground">
                            {agent.globalSkillsDir ?? "—"}
                          </span>
                          {onlyUnmanaged ? (
                            <span className="mt-0.5 block text-[12px] text-muted-foreground">
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
                className="panel-well mt-3 text-[12.5px]"
              >
                <p className="font-medium">
                  {projectMode ? "项目 canonical" : "共享 canonical"}
                </p>
                <p className="mt-1 leading-relaxed text-muted-foreground">
                  Universal · {canonicalPath}
                  {projectMode ? "（项目共享目录）" : "（共享目录）"}
                </p>
                {sharedAgents.length > 0 ? (
                  <p className="mt-0.5 leading-relaxed text-muted-foreground">
                    {sharedAgents.map((agent) => agent.displayName).join("、")}
                    {" · 直接读取共享目录，无需单独挂载"}
                  </p>
                ) : null}
              </div>

              {targetAgents.length === 0 ? (
                <p className="mt-3 text-[12px] text-muted-foreground">
                  {projectMode
                    ? "没有可用的目标 agent，安装将只写入项目 canonical。"
                    : "未检测到 agent，安装将只写入共享 canonical。"}
                </p>
              ) : null}

              <p className="mt-2 text-[12px] leading-relaxed text-muted-foreground">
                后台始终追加 <span className="font-mono">-a universal</span>
                {projectMode
                  ? "，写入项目 canonical；勾选的 catalog agent 由 CLI 挂载，extra agent 由应用创建项目内链接。"
                  : "，确保写入共享 canonical，避免单 agent 默认 copy。"}
              </p>
            </>
          ) : null}

          {step === 4 && preview ? (
            <div data-testid="install-confirm">
              <dl className="panel-well summary-list">
                <dt>作用域</dt>
                <dd>
                  {targetScopeLabel} · <span className="mono">{canonicalPath}</span>
                </dd>
                <dt>来源</dt>
                <dd className="font-mono">{preview.source}</dd>
                <dt>Skill</dt>
                <dd>
                  {effectiveNames.length} 个 · {effectiveNames.join("、") || "—"}
                </dd>
                <dt>Agents</dt>
                <dd>{agentSummary}</dd>
              </dl>

              <div className="panel-well mt-3 text-[12.5px]">
                <p className="text-[12.5px] font-medium">目标路径</p>
                <ul className="mt-1.5 space-y-1">
                  {targetPaths.map((target) => (
                    <li
                      key={`${target.label}-${target.path}`}
                      className="flex justify-between gap-4"
                    >
                      <span className="text-muted-foreground">{target.label}</span>
                      <span className="mono">{target.path}</span>
                    </li>
                  ))}
                </ul>
                {sharedAgents.length > 0 ? (
                  <p className="mt-1.5 text-[12px] text-muted-foreground">
                    共享 canonical 的 agent（
                    {sharedAgents.map((agent) => agent.displayName).join("、")}
                    ）直接读取共享目录。
                  </p>
                ) : projectMode ? (
                  <p className="mt-1.5 text-[12px] text-muted-foreground">
                    未勾选 agent 时只写入项目 canonical；extra agent
                    的项目链接由应用创建。
                  </p>
                ) : null}
              </div>

              <p
                data-testid="writing-warning"
                className="notice mt-3" data-tone="warning"
              >
                写入阶段不可取消。安装前会快照 lock；每个来源只执行一次 CLI；完成后逐项校验 canonical、agent 挂载与 lock。
              </p>
            </div>
          ) : null}

          {step === 5 ? (
            <>
              {installRunning ? (
                <>
                  <MutationProgress
                    phase={installState.phase}
                    message={installState.message}
                    completed={installState.completed}
                    total={installState.total}
                    cancelling={installCancelling}
                    canCancel={
                      Boolean(installState.taskId) &&
                      installPhase === "preparing" &&
                      !installCancelling
                    }
                    cancelLabel="取消安装"
                    onCancel={handleCancel}
                  />
                  <p
                    data-testid="install-lock-note"
                    className="mt-2 text-[12px] text-muted-foreground"
                  >
                    {installPhase === "preparing"
                      ? "正在写入前准备；取消后不会调用 CLI。"
                      : "写入阶段不可取消。"}
                  </p>
                </>
              ) : null}

              {installCancelled ? (
                <p
                  role="status"
                  data-testid="install-cancelled"
                  className="mt-3 text-[12px] text-muted-foreground"
                >
                  安装已取消，未写入任何文件。
                </p>
              ) : null}

              <MutationError
                error={installState.startError}
                testId="install-start-error"
              />
              <MutationError error={installState.error} testId="install-error" />

              {installResult ? (
                <MutationResult
                  action="install"
                  result={installResult}
                  message={installState.message}
                  testIdPrefix="install"
                  backupLabel="lock 快照"
                  showLog={false}
                  retryMode="incomplete"
                  onRetry={(names) => handleStartInstall(names, true)}
                  onRetryRow={(name) => handleStartInstall([name], true)}
                  warningsTestId="install-warnings"
                  disabled={installRunning}
                />
              ) : null}
            </>
          ) : null}
        </div>

        <div className="modal-foot">
          <div className="modal-foot-note">
            {step === 5 && writingLocked
              ? "写入阶段不可取消"
              : "Esc 关闭 · 公开 HTTPS Git · 仅默认分支"}
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
                {installState.error && lastInstall ? (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() =>
                      void handleStartInstall(lastInstall, true)
                    }
                  >
                    重试
                  </Button>
                ) : null}
                {installState.error || installCancelled ? (
                  <Button variant="ghost" size="sm" onClick={() => setStep(4)}>
                    返回确认
                  </Button>
                ) : null}
                <Button size="sm" onClick={closeWizard}>
                  {installResult?.outcome === "success" ? "完成" : "关闭"}
                </Button>
              </>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}
