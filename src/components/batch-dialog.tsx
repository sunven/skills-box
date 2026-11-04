import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AlertTriangle, Loader2, X } from "lucide-react";
import { Button } from "./ui/button";
import { AgentLogo } from "./agent-logo";
import { MutationProgress } from "./mutation/mutation-progress";
import { MutationResult } from "./mutation/mutation-result";
import { MutationError } from "./mutation/mutation-error";
import { useModalFocus } from "../hooks/use-modal-focus";
import {
  asAppError,
  fallbackError,
  previewBatch,
  refreshBatchPreview,
  startBatch,
} from "../lib/api";
import {
  taskEventDriver,
  useMutation,
  normalizeTaskResult,
  type MutationResultView,
} from "../lib/mutation";
import {
  BATCH_ACTION_LABELS,
  batchTargetLabel,
  completedSkillIds,
  mergeBatchRows,
  type BatchAction,
  type BatchPreview,
  type BatchRow,
} from "../lib/batch";
import type {
  AppError,
  InventoryAgent,
  ScopeId,
  UpdateViewSkill,
} from "../lib/types";

export interface BatchSelection {
  action: BatchAction;
  skills: UpdateViewSkill[];
}
interface BatchDialogProps {
  selection: BatchSelection;
  scope: ScopeId;
  agents: InventoryAgent[];
  disabled: boolean;
  onRunningChange: (running: boolean) => void;
  onCompleted: (ids: Set<string>) => void;
  onClose: () => void;
}

export function BatchDialog({
  selection,
  scope,
  agents,
  disabled,
  onRunningChange,
  onCompleted,
  onClose,
}: BatchDialogProps) {
  const action = selection.action;
  const hasAgents = action === "attach" || action === "detach";
  const [selectedAgents, setSelectedAgents] = useState<Set<string>>(
    () => new Set(),
  );
  const [preview, setPreview] = useState<BatchPreview | null>(null);
  const [agentCounts, setAgentCounts] = useState<Map<string, number>>(
    () => new Map(),
  );
  const [loading, setLoading] = useState(true);
  const [previewError, setPreviewError] = useState<AppError | null>(null);
  const [changed, setChanged] = useState(false);
  const [confirmedDelete, setConfirmedDelete] = useState(false);
  const [confirmedCopies, setConfirmedCopies] = useState(false);
  const [retryKeys, setRetryKeys] = useState<string[] | undefined>();
  const [history, setHistory] = useState<BatchRow[]>([]);
  const [viewingResult, setViewingResult] = useState(false);
  const panelRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const requestSeq = useRef(0);
  const previewRef = useRef(preview);
  const countsPreviewId = useRef<string | null>(null);
  previewRef.current = preview;
  const mutation = useMutation({
    action,
    scope,
    fallbackFailure: "批量操作未完成。",
    fallbackStart: "无法开始批量操作，请重新预览。",
  });
  const {
    running,
    phase,
    message,
    completed,
    total,
    taskId,
    cancelling,
    result,
    startError,
    error,
  } = mutation.state;
  const skillIds = useMemo(
    () => selection.skills.map((skill) => skill.id),
    [selection],
  );
  const agentIds = useMemo(
    () => [
      ...agents
        .filter((agent) => selectedAgents.has(agent.id))
        .map((agent) => agent.id),
      // Removed Agents remain explicit targets in a new retry preview.
      ...[...selectedAgents].filter(
        (id) => !agents.some((agent) => agent.id === id),
      ),
    ],
    [agents, selectedAgents],
  );
  const eligibleAgents = useMemo(
    () =>
      agents.filter(
        (agent) => agent.detected && !agent.shared && !agent.universal,
      ),
    [agents],
  );
  const selectionRef = useRef({ scope, action, skillIds, agentIds, retryKeys });
  selectionRef.current = { scope, action, skillIds, agentIds, retryKeys };

  const applyPreview = useCallback((next: BatchPreview) => {
    if (previewRef.current && previewRef.current.revision !== next.revision) {
      setConfirmedDelete(false);
      setConfirmedCopies(false);
      setChanged(true);
    }
    previewRef.current = next;
    setPreview(next);
    setPreviewError(null);
  }, []);

  const applyAgentCounts = useCallback((next: BatchPreview) => {
    const counts = new Map<string, number>();
    for (const target of next.targets) {
      if (target.eligible && target.agentId)
        counts.set(target.agentId, (counts.get(target.agentId) ?? 0) + 1);
    }
    setAgentCounts(counts);
  }, []);

  const loadPreview = useCallback(async () => {
    const seq = ++requestSeq.current;
    setLoading(true);
    setConfirmedDelete(false);
    setConfirmedCopies(false);
    const { scope, action, skillIds, agentIds, retryKeys } =
      selectionRef.current;
    try {
      const next = await previewBatch(scope, {
        action,
        skillIds,
        agentIds,
        targetKeys: retryKeys,
      });
      if (seq === requestSeq.current && !mutation.stateRef.current.running)
        applyPreview(next);
    } catch (cause) {
      if (seq === requestSeq.current) {
        setPreview(null);
        setPreviewError(asAppError(cause) ?? fallbackError("无法预览操作。"));
      }
    } finally {
      if (seq === requestSeq.current) setLoading(false);
    }
  }, [applyPreview, mutation.stateRef]);

  useEffect(() => {
    if (!hasAgents) return;
    let disposed = false;
    countsPreviewId.current = null;
    void previewBatch(scope, {
      action,
      skillIds,
      agentIds: eligibleAgents.map((agent) => agent.id),
      targetKeys: retryKeys,
    })
      .then((all) => {
        if (disposed) return;
        countsPreviewId.current = all.id;
        applyAgentCounts(all);
      })
      .catch(() => {
        /* Selected agents get an authoritative preview below. */
      });
    return () => {
      disposed = true;
    };
  }, [
    scope,
    action,
    hasAgents,
    skillIds,
    eligibleAgents,
    retryKeys,
    applyAgentCounts,
  ]);

  useEffect(() => {
    if (viewingResult || running) return;
    void loadPreview();
    return () => {
      requestSeq.current += 1;
    };
  }, [agentIds, retryKeys, viewingResult, loadPreview, running]);

  // Refresh an open preview without extending the user-selected target set.
  useEffect(() => {
    if (!preview || viewingResult || running) return;
    let disposed = false;
    let refreshing = false;
    const refresh = async () => {
      if (refreshing || mutation.stateRef.current.running) return;
      refreshing = true;
      try {
        const [next, counts] = await Promise.all([
          refreshBatchPreview(preview.id),
          countsPreviewId.current
            ? refreshBatchPreview(countsPreviewId.current).catch(() => null)
            : null,
        ]);
        if (!disposed && !mutation.stateRef.current.running) {
          applyPreview(next);
          if (counts) applyAgentCounts(counts);
        }
      } catch (cause) {
        if (!disposed && !mutation.stateRef.current.running) {
          setPreviewError(
            asAppError(cause) ?? fallbackError("预览已失效，请重新预览。"),
          );
          setConfirmedDelete(false);
          setConfirmedCopies(false);
        }
      } finally {
        refreshing = false;
      }
    };
    const timer = window.setInterval(() => void refresh(), 2000);
    return () => {
      disposed = true;
      window.clearInterval(timer);
    };
  }, [
    preview?.id,
    viewingResult,
    running,
    applyPreview,
    applyAgentCounts,
    mutation.stateRef,
  ]);

  useModalFocus({
    open: true,
    panelRef,
    initialFocusRef: closeRef,
    onClose,
    canClose: () => !mutation.stateRef.current.running,
  });
  useEffect(
    () => () => {
      requestSeq.current += 1;
    },
    [],
  );

  const candidates = preview?.targets.filter((target) => target.eligible) ?? [];
  const skipped = preview?.targets.filter((target) => !target.eligible) ?? [];
  const needsCopies = candidates.some((target) => target.deletesCopy);
  const submitDisabled =
    disabled ||
    loading ||
    !!previewError ||
    !preview ||
    candidates.length === 0 ||
    (action === "uninstall" && !confirmedDelete) ||
    (needsCopies && !confirmedCopies);

  const confirm = async () => {
    if (submitDisabled || !preview || mutation.stateRef.current.running) return;
    requestSeq.current += 1;
    onRunningChange(true);
    setChanged(false);
    const outcome = await mutation.start(
      taskEventDriver({
        total: candidates.length,
        message: "正在准备批量操作…",
        launch: (onEvent) =>
          startBatch(
            scope,
            {
              previewId: preview.id,
              revision: preview.revision,
              confirmedDelete,
              confirmedCopies,
            },
            onEvent,
          ),
      }),
    );
    onRunningChange(false);
    if (outcome?.batch) {
      const merged = mergeBatchRows(history, outcome.batch.rows);
      setHistory(merged);
      setViewingResult(true);
      onCompleted(completedSkillIds(merged));
    } else {
      setConfirmedDelete(false);
      setConfirmedCopies(false);
      await loadPreview();
    }
  };

  const retry = () => {
    const unfinished = history.filter(
      (row) => row.status === "failed" || row.status === "not_executed",
    );
    if (unfinished.length === 0) return;
    setRetryKeys(unfinished.map((row) => row.key));
    setViewingResult(false);
    mutation.reset();
    setPreview(null);
    previewRef.current = null;
    setChanged(false);
    setConfirmedDelete(false);
    setConfirmedCopies(false);
  };

  const combinedResult: MutationResultView | null =
    viewingResult && result
      ? {
          ...normalizeTaskResult({
            action: "batch",
            overall:
              result.outcome === "cancelled"
                ? "cancelled"
                : history.every((row) => row.status === "success")
                  ? "success"
                  : history.some((row) => row.status === "success")
                    ? "partial"
                    : "failed",
            skills: [],
            warnings: result.warnings,
            startedAt: 0,
            finishedAtEpochMs: 0,
            scope,
            batch: { action, rows: history },
          }),
          backupPath: result.backupPath,
          logPath: result.logPath,
        }
      : null;

  return (
    <div
      className="modal-overlay"
      onMouseDown={(event) => {
        if (
          event.target === event.currentTarget &&
          !mutation.stateRef.current.running
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
              批量{BATCH_ACTION_LABELS[action]}
            </h2>
            <p className="modal-sub">
              {scope === "global" ? "全局" : "项目"} · 已选{" "}
              {selection.skills.length} 个技能
              {hasAgents ? ` · ${agentIds.length} 个 Agent` : ""}
            </p>
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
        <div className="modal-body">
          {!running && !viewingResult ? (
            <>
              {hasAgents ? (
                <fieldset className="batch-agents">
                  <legend className="field-label">选择 Agent</legend>
                  {agents.map((agent) => {
                    const unavailable =
                      agent.shared || agent.universal || !agent.detected;
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
                          disabled={unavailable || disabled || !!retryKeys}
                          aria-label={`选择 Agent ${agent.displayName}`}
                          onChange={() =>
                            setSelectedAgents((current) => {
                              const next = new Set(current);
                              if (next.has(agent.id)) next.delete(agent.id);
                              else next.add(agent.id);
                              return next;
                            })
                          }
                        />
                        <AgentLogo
                          agentId={agent.id}
                          agentName={agent.displayName}
                        />
                        <span>{agent.displayName}</span>
                        <span className="ml-auto text-muted-foreground text-xs">
                          {unavailable
                            ? agent.shared || agent.universal
                              ? "共享，无法独立操作"
                              : "未检测到"
                            : `${count} 个技能可操作`}
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
                  {hasAgents && agentIds.length === 0 ? (
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
                          {target.reason ? (
                            <p className="mt-1 text-muted-foreground">
                              {target.reason}
                            </p>
                          ) : null}
                          {target.eligible &&
                          action === "uninstall" &&
                          target.localModification !== "clean" ? (
                            <p className="mt-1 text-warning">
                              {target.localModification === "modified"
                                ? "存在本地改动，卸载将删除这些改动。"
                                : "无法确认本地改动，卸载将删除现有内容。"}
                            </p>
                          ) : null}
                          {target.eligible && target.deletesCopy ? (
                            <p className="mt-1 text-warning">
                              副本目录会先备份再删除。
                            </p>
                          ) : null}
                          {target.eligible &&
                          (action === "uninstall" || action === "detach") ? (
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
                  (!hasAgents || agentIds.length > 0) &&
                  !previewError ? (
                    <p className="notice">
                      没有可执行目标，请查看跳过原因或调整选择。
                    </p>
                  ) : null}
                </>
              )}
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
              {action === "uninstall" ? (
                <label className="batch-confirm">
                  <input
                    type="checkbox"
                    aria-label="确认批量卸载"
                    checked={confirmedDelete}
                    disabled={loading || candidates.length === 0}
                    onChange={(e) => setConfirmedDelete(e.target.checked)}
                  />
                  <span>
                    我确认删除以上全部内容，含已提示的本地改动，并了解不会自动回滚。
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
                    onChange={(e) => setConfirmedCopies(e.target.checked)}
                  />
                  <span>我确认先备份再删除所列副本挂载目录。</span>
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
                  onClick={() => void loadPreview()}
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
                label="正在卸载"
                message={message}
                completed={completed}
                total={total}
                cancelling={cancelling}
                canCancel={!!taskId && !cancelling}
                cancelLabel="停止后续操作"
                onCancel={() => void mutation.cancel()}
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
          {history.length > 0 && !viewingResult && !running ? (
            <p className="mt-3 text-xs text-muted-foreground">
              此前 {history.filter((row) => row.status === "success").length}{" "}
              个成功目标已保留，本次仅重试未完成目标。
            </p>
          ) : null}
        </div>
        <div className="modal-foot batch-foot">
          <span className="modal-foot-note">
            {running
              ? "按目标串行执行"
              : retryKeys
                ? "重新核对重试目标"
                : "先预览，再确认"}
          </span>
          <div className="flex flex-wrap items-center gap-2">
            {viewingResult ? (
              <>
                {history.some(
                  (row) =>
                    row.status === "failed" || row.status === "not_executed",
                ) ? (
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={disabled}
                    onClick={retry}
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
                onClick={() => void mutation.cancel()}
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
                  variant={action === "uninstall" ? "destructive" : "default"}
                  disabled={submitDisabled}
                  onClick={() => void confirm()}
                >{`确认${BATCH_ACTION_LABELS[action]} (${candidates.length})`}</Button>
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
