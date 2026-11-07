import { useCallback, useEffect, useRef, useState } from "react";
import {
  asAppError,
  fallbackError,
  previewBatch,
  refreshBatchPreview,
  startBatch,
} from "../lib/api";
import type {
  BatchAction,
  BatchPreview,
  BatchRequest,
  BatchRow,
} from "../lib/batch";
import {
  normalizeTaskResult,
  taskEventDriver,
  useMutation,
  type MutationResultView,
} from "../lib/mutation";
import type { AppError, InventoryAgent, ScopeId } from "../lib/types";

interface BatchOperationOptions {
  scope: ScopeId;
  action: BatchAction;
  skillIds: string[];
  selectedAgents: ReadonlySet<string>;
  sourceAgents: InventoryAgent[];
  projectRoot: string;
  disabled: boolean;
  onRunningChange: (running: boolean) => void;
  onCompleted: (ids: Set<string>) => void;
}

interface PreviewState {
  key: string;
  preview: BatchPreview | null;
  loading: boolean;
  error: AppError | null;
  changed: boolean;
  confirmedDelete: boolean;
  confirmedCopies: boolean;
}

function emptyPreview(key: string, loading: boolean): PreviewState {
  return {
    key,
    preview: null,
    loading,
    error: null,
    changed: false,
    confirmedDelete: false,
    confirmedCopies: false,
  };
}

const NO_AGENTS: InventoryAgent[] = [];
const NO_COUNTS = new Map<string, number>();

function completedSkillIds(rows: BatchRow[]): Set<string> {
  const grouped = new Map<string, BatchRow[]>();
  for (const row of rows) {
    if (!row.eligible) continue;
    const group = grouped.get(row.skillId);
    if (group) group.push(row);
    else grouped.set(row.skillId, [row]);
  }
  return new Set(
    [...grouped]
      .filter(([, group]) => group.every((row) => row.status === "success"))
      .map(([id]) => id),
  );
}

function mergeBatchRows(
  previous: BatchRow[],
  current: BatchRow[],
): BatchRow[] {
  const changed = new Map(current.map((row) => [row.key, row]));
  const result = previous.map((row) => {
    const next = changed.get(row.key);
    return next ? { ...next, eligible: row.eligible || next.eligible } : row;
  });
  const known = new Set(previous.map((row) => row.key));
  return result.concat(current.filter((row) => !known.has(row.key)));
}

/**
 * One open Batch operation owns preview authorization and retry history.
 * Task execution, cancellation and query refresh stay in Mutation surface.
 */
export function useBatchOperation(options: BatchOperationOptions) {
  const { scope, action, skillIds, selectedAgents, sourceAgents, projectRoot } = options;
  const isCopy = action === "copy_to_project";
  const isRestore = action === "restore";
  const isAgentRemoval = action === "uninstall_agents";
  const hasAgents =
    action === "attach" || action === "detach" || isCopy || isRestore || isAgentRemoval;
  const needsDelete = action === "uninstall" || isAgentRemoval;
  const destinationKey = JSON.stringify([scope, isCopy ? projectRoot : null]);
  const [projectAgents, setProjectAgents] = useState({
    key: destinationKey,
    agents: NO_AGENTS,
  });
  const agents = isCopy
    ? projectAgents.key === destinationKey
      ? projectAgents.agents
      : NO_AGENTS
    : sourceAgents;
  const agentIds = [
    ...agents.filter((agent) => selectedAgents.has(agent.id)).map((agent) => agent.id),
    // Removed Agents remain explicit targets in a new retry preview.
    ...[...selectedAgents].filter((id) => !agents.some((agent) => agent.id === id)),
  ];
  const [retryKeys, setRetryKeys] = useState<string[] | undefined>();
  const [history, setHistory] = useState<BatchRow[]>([]);
  const [viewingResult, setViewingResult] = useState(false);
  const request: BatchRequest = {
    action,
    skillIds,
    agentIds,
    targetKeys: retryKeys,
    projectRoot: isCopy ? projectRoot : undefined,
  };
  const requestKey = JSON.stringify([scope, request]);
  const ready = !isCopy || !!projectRoot;
  const countRequest: BatchRequest = {
    ...request,
    agentIds: agents
      .filter((agent) => agent.detected && !agent.shared && (isAgentRemoval || !agent.universal))
      .map((agent) => agent.id),
  };
  const countKey = hasAgents && !isRestore && ready
    ? JSON.stringify([scope, countRequest])
    : null;
  const [storedPreview, setStoredPreview] = useState(() => emptyPreview(requestKey, ready));
  const previewRef = useRef(storedPreview);
  const requestSeq = useRef(0);
  const countPreview = useRef<{ key: string; id: string | null } | null>(null);
  const [counts, setCounts] = useState({ key: destinationKey, values: NO_COUNTS });
  const mounted = useRef(true);
  const latest = useRef({ options, request, requestKey, countRequest, countKey, destinationKey, ready });
  latest.current = { options, request, requestKey, countRequest, countKey, destinationKey, ready };
  const mutation = useMutation({
    action,
    scope,
    fallbackFailure: "批量操作未完成。",
    fallbackStart: "无法开始批量操作，请重新预览。",
  });
  const { running, result } = mutation.state;

  const updatePreview = useCallback((update: (state: PreviewState) => PreviewState) => {
    const state = update(previewRef.current);
    previewRef.current = state;
    setStoredPreview(state);
  }, []);
  const applyPreview = useCallback((next: BatchPreview) => {
    updatePreview((state) => {
      const changed = !!state.preview && state.preview.revision !== next.revision;
      return {
        ...state,
        preview: next,
        error: null,
        changed: state.changed || changed,
        confirmedDelete: changed ? false : state.confirmedDelete,
        confirmedCopies: changed ? false : state.confirmedCopies,
      };
    });
    if (next.agents) {
      setProjectAgents({ key: latest.current.destinationKey, agents: next.agents });
    }
  }, [updatePreview]);
  const applyCounts = useCallback((next: BatchPreview) => {
    const values = new Map<string, number>();
    for (const target of next.targets) {
      if (target.eligible && target.agentId) {
        values.set(target.agentId, (values.get(target.agentId) ?? 0) + 1);
      }
    }
    setCounts({ key: latest.current.destinationKey, values });
  }, []);

  const reload = useCallback(async () => {
    if (!mounted.current || mutation.stateRef.current.running) return;
    const input = latest.current;
    const seq = ++requestSeq.current;
    updatePreview((state) => ({
      ...(input.ready && state.key === input.requestKey
        ? state
        : emptyPreview(input.requestKey, input.ready)),
      loading: input.ready,
      confirmedDelete: false,
      confirmedCopies: false,
    }));
    if (!input.ready) return;
    const isCurrent = () =>
      mounted.current && seq === requestSeq.current &&
      input.requestKey === latest.current.requestKey && !mutation.stateRef.current.running;
    try {
      const next = await previewBatch(input.options.scope, input.request);
      if (isCurrent()) applyPreview(next);
    } catch (cause) {
      if (isCurrent()) {
        updatePreview((state) => ({
          ...state,
          preview: null,
          error: asAppError(cause) ?? fallbackError("无法预览操作。"),
        }));
      }
    } finally {
      if (isCurrent()) updatePreview((state) => ({ ...state, loading: false }));
    }
  }, [applyPreview, mutation.stateRef, updatePreview]);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      requestSeq.current += 1;
    };
  }, []);

  useEffect(() => {
    setProjectAgents({ key: destinationKey, agents: NO_AGENTS });
    setCounts({ key: destinationKey, values: NO_COUNTS });
  }, [destinationKey]);

  useEffect(() => {
    countPreview.current = null;
    if (!countKey) return;
    let disposed = false;
    const pending = { key: countKey, id: null as string | null };
    countPreview.current = pending;
    const input = latest.current;
    void previewBatch(input.options.scope, input.countRequest)
      .then((next) => {
        if (disposed || countKey !== latest.current.countKey) return;
        pending.id = next.id;
        applyCounts(next);
      })
      .catch(() => {
        // Only the selected-target preview supplies an authoritative error.
      });
    return () => {
      disposed = true;
    };
  }, [countKey, applyCounts]);

  useEffect(() => {
    if (!viewingResult) void reload();
    return () => {
      requestSeq.current += 1;
    };
  }, [requestKey, viewingResult, reload]);

  // Hide obsolete authorization in the same render as a selection change.
  const state = storedPreview.key === requestKey
    ? storedPreview
    : emptyPreview(requestKey, ready);
  const preview = state.preview;
  useEffect(() => {
    if (!preview || viewingResult || running) return;
    let disposed = false;
    let refreshing = false;
    const seq = requestSeq.current;
    const isCurrent = () =>
      !disposed && seq === requestSeq.current &&
      requestKey === latest.current.requestKey && !mutation.stateRef.current.running;
    const refresh = async () => {
      if (refreshing || !isCurrent()) return;
      refreshing = true;
      const counted = countPreview.current;
      try {
        // Keep the existing joint wait: advisory counts can fail independently,
        // but a pending count refresh delays applying this whole refresh cycle.
        const [next, all] = await Promise.all([
          refreshBatchPreview(preview.id),
          counted?.id ? refreshBatchPreview(counted.id).catch(() => null) : null,
        ]);
        if (isCurrent()) {
          applyPreview(next);
          if (all && counted === countPreview.current && counted?.key === latest.current.countKey) {
            applyCounts(all);
          }
        }
      } catch (cause) {
        if (isCurrent()) {
          updatePreview((current) => ({
            ...current,
            error: asAppError(cause) ?? fallbackError("预览已失效，请重新预览。"),
            confirmedDelete: false,
            confirmedCopies: false,
          }));
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
    preview?.id, requestKey, viewingResult, running,
    applyPreview, applyCounts, mutation.stateRef, updatePreview,
  ]);

  const candidates = preview?.targets.filter((target) => target.eligible) ?? [];
  const skipped = preview?.targets.filter((target) => !target.eligible) ?? [];
  const needsCopies = candidates.some((target) => target.deletesCopy);
  const submitDisabled =
    options.disabled || running || viewingResult || state.loading || !!state.error ||
    !preview || candidates.length === 0 ||
    (needsDelete && !state.confirmedDelete) || (needsCopies && !state.confirmedCopies);

  const confirm = async () => {
    if (
      submitDisabled || latest.current.options.disabled || !preview || !mounted.current ||
      mutation.stateRef.current.running || requestKey !== latest.current.requestKey ||
      previewRef.current !== state
    ) return;
    requestSeq.current += 1;
    options.onRunningChange(true);
    updatePreview((current) => ({ ...current, changed: false }));
    const outcome = await mutation.start(
      taskEventDriver({
        total: candidates.length,
        message: "正在准备批量操作…",
        launch: (onEvent) => startBatch(scope, {
          previewId: preview.id,
          revision: preview.revision,
          confirmedDelete: state.confirmedDelete,
          confirmedCopies: state.confirmedCopies,
        }, onEvent),
      }),
    );
    if (!mounted.current) return;
    latest.current.options.onRunningChange(false);
    if (outcome?.batch) {
      const merged = mergeBatchRows(history, outcome.batch.rows);
      setHistory(merged);
      setViewingResult(true);
      latest.current.options.onCompleted(completedSkillIds(merged));
    } else {
      await reload();
    }
  };

  const retry = () => {
    if (options.disabled || mutation.stateRef.current.running || !viewingResult) return;
    const unfinished = history.filter(
      (row) => row.status === "failed" || row.status === "not_executed",
    );
    if (!unfinished.length) return;
    requestSeq.current += 1;
    setRetryKeys(unfinished.map((row) => row.key));
    setViewingResult(false);
    mutation.reset();
    updatePreview(() => emptyPreview("", true));
  };

  const combinedResult: MutationResultView | null = viewingResult && result
    ? {
        ...normalizeTaskResult({
          action: "batch",
          overall: result.outcome === "cancelled"
            ? "cancelled"
            : history.every((row) => row.status === "success")
              ? "success"
              : history.some((row) => row.status === "success") ? "partial" : "failed",
          skills: [],
          warnings: result.warnings,
          startedAt: 0,
          finishedAtEpochMs: 0,
          scope: result.scope ?? scope,
          batch: { action, rows: history },
        }),
        backupPath: result.backupPath,
        logPath: result.logPath,
      }
    : null;

  const confirmDeletion = (value: boolean) => {
    const current = previewRef.current;
    if (current.key === latest.current.requestKey && current.preview &&
      !current.loading && !mutation.stateRef.current.running) {
      updatePreview((state) => ({ ...state, confirmedDelete: value }));
    }
  };
  const confirmCopies = (value: boolean) => {
    const current = previewRef.current;
    if (current.key === latest.current.requestKey && current.preview &&
      !current.loading && !mutation.stateRef.current.running) {
      updatePreview((state) => ({ ...state, confirmedCopies: value }));
    }
  };
  const reportSelectionError = (error: AppError) => {
    updatePreview((current) => ({ ...current, error }));
  };

  return {
    preview,
    agents,
    agentIds,
    agentCounts: counts.key === destinationKey ? counts.values : NO_COUNTS,
    loading: state.loading,
    previewError: state.error,
    changed: state.changed,
    confirmedDelete: state.confirmedDelete,
    confirmedCopies: state.confirmedCopies,
    needsDelete,
    needsCopies,
    removesLastCopy: candidates.some((target) => target.removesLastCopy),
    candidates,
    skipped,
    submitDisabled,
    task: mutation.state,
    result: combinedResult,
    viewingResult,
    retrying: !!retryKeys,
    hasHistory: history.length > 0,
    successfulTargets: history.filter((row) => row.status === "success").length,
    canRetry: history.some((row) => row.status === "failed" || row.status === "not_executed"),
    confirmDeletion,
    confirmCopies,
    reportSelectionError,
    reload,
    confirm,
    retry,
    cancel: mutation.cancel,
    canClose: mutation.canClose,
  };
}
