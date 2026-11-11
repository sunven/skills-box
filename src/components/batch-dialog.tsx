import { AffectedPacks } from "./expert-pack-context";
import { useMemo, useRef, useState } from "react";
import { AlertTriangle, FolderOpen, Loader2, X } from "lucide-react";
import { Button } from "./ui/button";
import { ActualCliCommands, CliCommandPreviewList } from "./cli-command-display";
import { CommandHelp } from "./ui/command-help";
import { AgentLogo } from "./agent-logo";
import { MutationProgress } from "./mutation/mutation-progress";
import { MutationResult } from "./mutation/mutation-result";
import { MutationError } from "./mutation/mutation-error";
import { useModalFocus } from "../hooks/use-modal-focus";
import { useBatchOperation } from "../hooks/use-batch-operation";
import { asAppError, fallbackError } from "../lib/api";
import {
  BATCH_ACTION_LABELS,
  batchTargetLabel,
  type BatchAction,
} from "../lib/batch";
import type {
  InventoryAgent,
  ScopeId,
  UpdateViewSkill,
  ProjectEntry,
} from "../lib/types";
import { pickProjectFolder } from "../lib/folder-picker";

export interface BatchSelection {
  action: BatchAction;
  skills: Pick<UpdateViewSkill, "id">[];
}
interface BatchDialogProps {
  selection: BatchSelection;
  scope: ScopeId;
  scopeLabel?: string;
  agents: InventoryAgent[];
  projects?: ProjectEntry[];
  onViewProject?: (scope: ScopeId) => void;
  disabled: boolean;
  onRunningChange: (running: boolean) => void;
  onCompleted: (ids: Set<string>) => void;
  onClose: () => void;
}

export function BatchDialog({
  selection,
  scope,
  scopeLabel,
  agents: sourceAgents,
  projects = [],
  onViewProject,
  disabled,
  onRunningChange,
  onCompleted,
  onClose,
}: BatchDialogProps) {
  const canChooseTarget = selection.action === "attach" && scope === "global";
  const canChooseRemoval = selection.action === "uninstall";
  const [uninstallMode, setUninstallMode] = useState<"agents" | "all">("all");
  const [targetScope, setTargetScope] = useState<"global" | "project">("global");
  const [projectRoot, setProjectRoot] = useState("");
  const [picking, setPicking] = useState(false);
  const action: BatchAction = canChooseRemoval && uninstallMode === "agents" ? "uninstall_agents"
    : canChooseTarget && targetScope === "project" ? "copy_to_project" : selection.action;
  const isAgentRemoval = action === "uninstall_agents";
  const actionLabel = action === "uninstall" ? "完全卸载" : BATCH_ACTION_LABELS[action];
  const isCopy = action === "copy_to_project";
  const isRestore = action === "restore";
  const isClean = action === "clean_records";
  const hasAgents = action === "attach" || action === "detach" || isCopy || isAgentRemoval || isRestore;
  const [selectedAgents, setSelectedAgents] = useState<Set<string>>(
    () => new Set(),
  );
  const panelRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const skillIds = useMemo(() => selection.skills.map((skill) => skill.id), [selection]);
  const operation = useBatchOperation({
    scope, action, skillIds, selectedAgents, sourceAgents, projectRoot,
    disabled: disabled || picking, onRunningChange, onCompleted,
  });
  const {
    preview, agents, agentIds, agentCounts, loading, previewError, changed,
    confirmedDelete, confirmedCopies, needsDelete, needsCopies, removesLastCopy,
    candidates, skipped, submitDisabled, viewingResult, retrying,
    hasHistory, successfulTargets, canRetry, result: combinedResult,
  } = operation;
  const { running, phase, message, completed, total, taskId, cancelling, result, startError, error } = operation.task;

  useModalFocus({
    open: true, panelRef, initialFocusRef: closeRef, onClose,
    canClose: operation.canClose,
  });

  const chooseDirectory = async () => {
    setPicking(true);
    try {
      const root = await pickProjectFolder();
      if (root && root !== projectRoot) {
        setProjectRoot(root);
        setSelectedAgents(new Set());
      }
    } catch (cause) {
      operation.reportSelectionError(asAppError(cause) ?? fallbackError("无法选择项目目录。"));
    } finally {
      setPicking(false);
    }
  };

  return (
    <div
      className="modal-overlay"
      onMouseDown={(event) => {
        if (
          event.target === event.currentTarget &&
          operation.canClose()
        )
          onClose();
      }}
    >
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="batch-dialog-title"
        data-testid="batch-dialog"
        className="modal-panel batch-panel max-w-2xl"
      >
        <div className="modal-head">
          <div>
            <h2 id="batch-dialog-title" className="modal-title">
              批量{canChooseRemoval ? "卸载" : BATCH_ACTION_LABELS[action]}
            </h2>
            <p className="modal-sub">
              {isCopy ? "全局 → 项目" : scopeLabel ?? (scope === "global" ? "全局" : "项目")} · 已选{" "}
              {selection.skills.length} {isClean ? "条记录" : "个技能"}
              {hasAgents ? ` · ${agentIds.length} 个 Agent` : ""}
            </p>
            {isCopy && projectRoot ? <p className="mono break-all mt-1 text-xs text-muted-foreground">{preview?.project?.displayName} · {preview?.project?.root ?? projectRoot}</p> : null}
          </div>
          <Button
            ref={closeRef}
            variant="ghost"
            size="icon"
            className="size-7 shrink-0"
            aria-label="关闭批量操作"
            disabled={running}
            onClick={onClose}
          >
            <X className="size-4" aria-hidden="true" />
          </Button>
        </div>
        <div className="modal-body" tabIndex={0}>
          {["uninstall", "uninstall_agents", "detach"].includes(action) && <AffectedPacks skillIds={skillIds} />}
          <ActualCliCommands taskId={taskId} />
          {!running && !viewingResult ? (
            <>
              {isClean ? <p className="notice mb-4">仅清理所列安装记录和对应基线，保留技能文件、挂载与发现历史。执行前备份，不自动回滚。</p> : null}
              {isRestore ? <p className="notice mb-4">按原来源的指定分支或标签恢复同名技能；未记录 ref 时使用默认分支。默认只恢复共享目录，可选择 Agent 追加挂载。执行前备份，不自动回滚。</p> : null}
              {canChooseRemoval ? (
                <fieldset className="mb-4" disabled={disabled || retrying}>
                  <legend className="field-label">卸载方式</legend>
                  <div className="flex flex-wrap gap-5 mt-2">
                    {(["all", "agents"] as const).map((value) => (
                      <label key={value} className="flex items-center gap-2 text-sm">
                        <input type="radio" name="batch-uninstall-mode" value={value} checked={uninstallMode === value}
                          onChange={() => {
                            setUninstallMode(value);
                          }} />
                        {value === "agents" ? "按 Agent 移除" : "完全卸载"}
                      </label>
                    ))}
                  </div>
                </fieldset>
              ) : null}
              {canChooseTarget ? (
                <fieldset className="mb-4" disabled={disabled || retrying || picking}>
                  <legend className="field-label">挂载位置</legend>
                  <div className="flex gap-5 mt-2">
                    {(["global", "project"] as const).map((value) => (
                      <label key={value} className="flex items-center gap-2 text-sm">
                        <input type="radio" name="batch-target-scope" checked={targetScope === value}
                          onChange={() => {
                            setTargetScope(value);
                            setSelectedAgents(new Set());
                          }} />
                        {value === "global" ? "全局" : "项目"}
                      </label>
                    ))}
                  </div>
                  {isCopy ? (
                    <div className="mt-3">
                      <div className="flex flex-wrap items-center gap-2">
                        <select className="batch-project-select flex-1 min-w-0" aria-label="选择目标项目"
                          value={projectRoot} onChange={(event) => {
                            setProjectRoot(event.target.value);
                            setSelectedAgents(new Set());
                          }}>
                          <option value="">选择已有项目</option>
                          {projects.map((project) => <option key={project.id} value={project.root} disabled={!project.available}>
                            {project.displayName}{!project.available ? "（不可用）" : ""}
                          </option>)}
                          {projectRoot && !projects.some((p) => p.root === projectRoot) ? <option value={projectRoot}>新项目 · {projectRoot.split("/").pop()}</option> : null}
                        </select>
                        <Button variant="outline" size="sm" onClick={() => void chooseDirectory()}>
                          <FolderOpen className="size-4" aria-hidden="true" />{picking ? "正在选择…" : "选择目录"}
                        </Button>
                      </div>
                      <p className="notice mt-3">复制当前内容，项目独立管理。Codex 等共享 Agent 直接读取项目共享目录，无需单独挂载；也可仅复制到项目。</p>
                    </div>
                  ) : null}
                </fieldset>
              ) : null}
              {hasAgents ? (
                <fieldset className="batch-agents">
                  <legend className="field-label">选择 Agent</legend>
                  {agents.map((agent) => {
                    const unavailable =
                      agent.shared || (!isAgentRemoval && agent.universal) || !agent.detected;
                    const count = agentCounts.get(agent.id) ?? 0;
                    return (
                      <label
                        key={agent.id}
                        className="batch-agent"
                        data-disabled={unavailable || undefined}
                      >
                        <input
                          type="checkbox"
                          checked={selectedAgents.has(agent.id)}
                          disabled={unavailable || disabled || retrying}
                          aria-label={`选择 Agent ${agent.displayName}`}
                          onChange={() => {
                            setSelectedAgents((current) => {
                              const next = new Set(current);
                              if (next.has(agent.id)) next.delete(agent.id);
                              else next.add(agent.id);
                              return next;
                            });
                          }}
                        />
                        <AgentLogo
                          agentId={agent.id}
                          agentName={agent.displayName}
                        />
                        <span>{agent.displayName}</span>
                        <span className="ml-auto text-muted-foreground text-xs">
                          {unavailable
                            ? agent.shared || (!isAgentRemoval && agent.universal)
                              ? isAgentRemoval ? "共享目录，无法单独移除" : "共享，无法独立操作"
                              : "未检测到"
                            : isRestore ? "追加挂载" : `${count} 个技能可操作`}
                        </span>
                      </label>
                    );
                  })}
                  {agents.length === 0 ? (
                    <p className="text-xs text-muted-foreground">
                      没有可用 Agent。
                    </p>
                  ) : null}
                </fieldset>
              ) : null}
              {changed ? (
                <p role="status" className="notice mt-3">
                  目标状态已变化，清单已更新，请重新确认。
                </p>
              ) : null}
              {loading ? (
                <p
                  role="status"
                  className="flex items-center gap-2 py-5 text-sm text-muted-foreground"
                >
                  <Loader2 className="size-4 animate-spin" aria-hidden="true" />
                  正在核对操作目标…
                </p>
              ) : (
                <>
                  <div className="batch-summary" aria-live="polite">
                    <strong>可执行 {candidates.length} 个目标</strong>
                    <span>跳过 {skipped.length} 个目标</span>
                  </div>
                  {isCopy && preview ? <p className="mt-1 mb-3 text-xs text-muted-foreground">
                    {candidates.filter((t) => t.operation === "copy").length} 个复制 · {candidates.filter((t) => t.operation === "attach").length} 个挂载
                  </p> : null}
                  {isCopy && !projectRoot ? <p className="notice">请先选择项目或项目目录。</p> : null}
                  {hasAgents && !isCopy && !isRestore && agentIds.length === 0 ? (
                    <p className="notice">请先选择要操作的 Agent。</p>
                  ) : null}
                  {preview && preview.targets.length > 0 ? (
                    <ul className="batch-preview-list">
                      {preview.targets.map((target) => (
                        <li
                          key={target.key}
                          data-testid="batch-target"
                          data-eligible={target.eligible}
                        >
                          <div className="flex items-center justify-between gap-3">
                            <strong>{batchTargetLabel(target)}</strong>
                            <span className="shrink-0 text-xs text-muted-foreground">
                              {target.eligible ? "可执行" : "跳过"}
                            </span>
                          </div>
                          {isRestore ? <p className="mt-1 text-muted-foreground break-all">{target.source} · {target.reference ?? "默认分支"}</p> : null}
                          {target.reason ? (
                            <p className="mt-1 text-muted-foreground">
                              {target.reason}
                            </p>
                          ) : null}
                          {isCopy && target.eligible && target.operation === "copy" && target.localModification !== "clean" ? (
                            <p className="mt-1 text-warning">{target.localModification === "modified" ? "将复制当前本地改动。" : "无法确认本地改动，将复制当前内容。"}项目之后可能提示与上游内容不同。</p>
                          ) : null}
                          {target.eligible &&
                          (action === "uninstall" || (isAgentRemoval && target.deletesCopy)) &&
                          target.localModification !== "clean" ? (
                            <p className="mt-1 text-warning">
                              {target.localModification === "modified"
                                ? "存在本地改动，卸载将删除这些改动。"
                                : "无法确认本地改动，卸载将删除现有内容。"}
                            </p>
                          ) : null}
                          {target.eligible && target.deletesCopy ? (
                            <p className="mt-1 text-warning">
                              {target.removesLastCopy ? "最后一份内容，会先备份再删除。" : "副本目录会先备份再删除。"}
                            </p>
                          ) : null}
                          {target.eligible &&
                          (needsDelete || action === "detach" || isCopy || isClean || isRestore) ? (
                            <ul className="mt-1 text-muted-foreground">
                              {target.paths.map((path) => (
                                <li key={path} className="mono break-all">
                                  {path}
                                </li>
                              ))}
                            </ul>
                          ) : null}
                        </li>
                      ))}
                    </ul>
                  ) : null}
                  {candidates.length === 0 &&
                  (!hasAgents || isCopy || isRestore || agentIds.length > 0) &&
                  !previewError ? (
                    <p className="notice">
                      没有可执行目标，请查看跳过原因或调整选择。
                    </p>
                  ) : null}
                </>
              )}
              {isAgentRemoval ? (
                <p className="notice mt-3">
                  移除所选 Agent 的挂载，保留共享技能本体及已有安装记录。符号链接只删除链接本身；删除前创建备份。
                </p>
              ) : null}
              {removesLastCopy ? (
                <p role="alert" className="notice mt-3" data-tone="negative">
                  本次将删除已标记技能的最后一份内容；备份会保留，已有安装记录不会自动清理。
                </p>
              ) : null}
              {action === "uninstall" ? (
                <div className="notice mt-3" data-tone="negative">
                  <AlertTriangle
                    className="mt-0.5 size-4 shrink-0"
                    aria-hidden="true"
                  />
                  <p>
                    将删除所列技能的目录、全部 Agent
                    挂载，以及已有安装记录与本地基线。未受管技能也可卸载；符号链接只删除链接本身。执行前创建备份，不自动回滚。
                  </p>
                </div>
              ) : null}
              {action === "update" ? (
                <p className="notice mt-3">
                  仅更新本地干净且有可用更新的技能；写入前再次核对，先备份再更新。已有副本挂载将转为共享目录的符号链接。
                </p>
              ) : null}
              {needsDelete ? (
                <label className="batch-confirm">
                  <input
                    type="checkbox"
                    aria-label={isAgentRemoval ? "确认按 Agent 移除" : "确认批量卸载"}
                    checked={confirmedDelete}
                    disabled={loading || candidates.length === 0}
                    onChange={(e) => operation.confirmDeletion(e.target.checked)}
                  />
                  <span>
                    {isAgentRemoval ? "我确认移除以上所选 Agent 的挂载，并了解不会自动回滚。"
                      : "我确认删除以上全部内容，含已提示的本地改动，并了解不会自动回滚。"}
                  </span>
                </label>
              ) : null}
              {needsCopies ? (
                <label className="batch-confirm">
                  <input
                    type="checkbox"
                    aria-label="确认删除副本挂载"
                    checked={confirmedCopies}
                    disabled={loading}
                    onChange={(e) => operation.confirmCopies(e.target.checked)}
                  />
                  <span>{removesLastCopy ? "我确认先备份再删除所列副本，包括已标记技能的最后一份内容。"
                    : "我确认先备份再删除所列副本挂载目录。"}</span>
                </label>
              ) : null}
              <MutationError
                error={previewError}
                testId="batch-preview-error"
              />
              {previewError ? (
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => void operation.reload()}
                  disabled={loading}
                >
                  重新预览
                </Button>
              ) : null}
            </>
          ) : null}
          {running ? (
            <>
              <MutationProgress
                phase={
                  action === "uninstall" && phase === "running_cli"
                    ? null
                    : phase
                }
                label={`正在${actionLabel}`}
                message={message}
                completed={completed}
                total={total}
                cancelling={cancelling}
                canCancel={!!taskId && !cancelling}
                cancelLabel="停止后续操作"
                onCancel={() => void operation.cancel()}
              />
              <p className="mt-2 text-xs text-muted-foreground">
                当前目标完成后停止后续操作，已完成结果保留。
              </p>
            </>
          ) : null}
          <MutationError error={startError} testId="batch-start-error" />
          <MutationError error={error} testId="batch-error" />
          {combinedResult ? (
            <MutationResult
              action={action}
              result={combinedResult}
              testIdPrefix="batch"
              showReveal
              copy="已完成的操作保留，不自动回滚。"
            />
          ) : null}
          {hasHistory && !viewingResult && !running ? (
            <p className="mt-3 text-xs text-muted-foreground">
              此前 {successfulTargets}{" "}
              个成功目标已保留，本次仅重试未完成目标。
            </p>
          ) : null}
        </div>
        <div className="modal-foot batch-foot">
          <span className="modal-foot-note">
            {running
              ? "按目标串行执行"
              : retrying
                ? "重新核对重试目标"
                : "先预览，再确认"}
          </span>
          <div className="flex flex-wrap items-center gap-2">
            {viewingResult ? (
              <>
                {isCopy && result?.scope && result.scope !== "global" && onViewProject ? (
                  <Button variant="outline" size="sm" onClick={() => onViewProject(result.scope!)}>查看项目</Button>
                ) : null}
                {canRetry ? (
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={disabled}
                    onClick={operation.retry}
                  >
                    重试失败及未执行项
                  </Button>
                ) : null}
                <Button size="sm" onClick={onClose}>
                  完成
                </Button>
              </>
            ) : running ? (
              <Button
                variant="outline"
                size="sm"
                disabled={!taskId || cancelling}
                onClick={() => void operation.cancel()}
              >
                {cancelling ? "等待当前目标完成…" : "停止后续操作"}
              </Button>
            ) : (
              <>
                <Button variant="ghost" size="sm" onClick={onClose}>
                  取消
                </Button>
                <Button
                  size="sm"
                  variant={needsDelete ? "destructive" : "default"}
                  disabled={submitDisabled}
                  onClick={() => void operation.confirm()}
                >{isClean ? `清理 ${candidates.length} 条记录` : isRestore ? `恢复 ${candidates.length} 个技能` : `确认${actionLabel} (${candidates.length})`}</Button>
                <CommandHelp label="查看将执行的命令">
                  {loading ? <p className="text-xs text-muted-foreground">正在核对操作目标…</p> :
                    previewError ? <p className="text-xs text-muted-foreground">{previewError.message}</p> :
                    !preview ? <p className="text-xs text-muted-foreground">请先选择操作目标。</p> :
                    <CliCommandPreviewList entries={candidates.map((target) => ({
                      name: batchTargetLabel(target), command: target.command ?? null,
                      reason: target.command ? null : "由应用直接处理，不执行 CLI 命令。",
                    }))} />}
                </CommandHelp>
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
