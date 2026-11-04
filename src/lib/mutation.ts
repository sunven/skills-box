import {
  batchTargetLabel,
  BATCH_SUCCESS_LABELS,
  type BatchResult,
} from "./batch";
import { discoveryQueryKey } from "./discovery";
//! One mutation surface for task state, results and refresh.
//!
//! CLI actions stream `TaskEvent`s and native actions complete a promise, but
//! every dialog needs the same lifecycle: start → progress → outcome, the same
//! failure shapes, and the cache invalidation that follows. This module
//! normalizes both drivers into one state machine so dialogs only keep their
//! confirmation, selection and wizard state.

import { useCallback, useReducer, useRef } from "react";
import type { MutableRefObject } from "react";
import { useQueryClient } from "@tanstack/react-query";
import type { QueryClient, QueryKey } from "@tanstack/react-query";
import { asAppError, cancelTask, fallbackError } from "./api";
import { TASK_SKILL_STATUS_LABELS } from "./task-labels";
import { inventoryQueryKey } from "../hooks/use-inventory";
import { projectsQueryKey } from "./scope";
import { skillQueryKey, skillQueryPrefix } from "../hooks/use-skill-detail";
import {
  skillDirQueryPrefix,
  skillFileQueryPrefix,
} from "../hooks/use-skill-content";
import {
  lastFullAttemptQueryKey,
  lastFullCheckQueryKey,
  updateCheckStateQueryKey,
  updateCheckStatesQueryKey,
} from "../hooks/use-updates";
import type {
  AppError,
  MutationKind,
  ScopeId,
  TaskEvent,
  TaskPhase,
  TaskResult,
  TaskSkillStatus,
  UpdateCheckReport,
} from "./types";
import { updateCheckLifecycle } from "./update-check-lifecycle";
import { markSkillsCurrent, updateReportQueryKey } from "./update-view";

export const MUTATION_KIND_LABELS: Record<MutationKind, string> = {
  install: "安装",
  update: "更新",
  uninstall: "卸载",
  batch: "批量操作",
};

export type MutationAction =
  | "install"
  | "update"
  | "uninstall"
  | "attach"
  | "detach"
  | "copy_to_project"
  | "manage_add"
  | "manage_remove"
  | "project_add"
  | "project_relocate"
  | "project_forget"
  | "preview";

export type MutationOutcome = TaskResult["overall"];

export type MutationRowStatus = "success" | "skipped" | "failed";

export interface MutationRow {
  name: string;
  /** Normalized tone used for colors and retry eligibility. */
  status: MutationRowStatus;
  /** Backend vocabulary for the row ("已更新", "已卸载", …). */
  statusLabel: string;
  /** Original CLI status; native rows leave it unset. */
  taskStatus?: TaskSkillStatus;
  error?: AppError;
  reason?: string;
  backupPath?: string;
}

export interface MutationResultView {
  outcome: MutationOutcome;
  rows: MutationRow[];
  warnings: AppError[];
  backupPath?: string;
  logPath?: string;
  /** Scope the mutation affected; refresh policy prefers it over options. */
  scope?: ScopeId;
  checkRevision?: string;
  batch?: BatchResult;
}

const HEADING_LABELS: Record<
  MutationAction,
  Record<MutationOutcome, string>
> = {
  install: {
    success: "安装完成",
    partial: "安装部分失败",
    failed: "安装失败",
    cancelled: "安装已取消",
  },
  update: {
    success: "更新完成",
    partial: "更新部分失败",
    failed: "更新失败",
    cancelled: "更新已取消",
  },
  uninstall: {
    success: "卸载完成",
    partial: "卸载部分完成",
    failed: "卸载失败",
    cancelled: "卸载已取消",
  },
  attach: {
    success: "挂载完成",
    partial: "挂载部分失败",
    failed: "挂载失败",
    cancelled: "挂载已取消",
  },
  detach: {
    success: "断开完成",
    partial: "断开部分失败",
    failed: "断开失败",
    cancelled: "断开已取消",
  },
  copy_to_project: {
    success: "复制到项目完成",
    partial: "复制与挂载部分完成",
    failed: "复制到项目失败",
    cancelled: "复制到项目已停止",
  },
  manage_add: {
    success: "已添加 agent",
    partial: "添加部分失败",
    failed: "添加失败",
    cancelled: "添加已取消",
  },
  manage_remove: {
    success: "已删除 agent",
    partial: "删除部分失败",
    failed: "删除失败",
    cancelled: "删除已取消",
  },
  project_add: {
    success: "已添加项目",
    partial: "添加项目部分失败",
    failed: "添加项目失败",
    cancelled: "添加项目已取消",
  },
  project_relocate: {
    success: "已重新定位项目",
    partial: "重新定位部分失败",
    failed: "重新定位失败",
    cancelled: "重新定位已取消",
  },
  project_forget: {
    success: "已遗忘项目",
    partial: "遗忘项目部分失败",
    failed: "遗忘项目失败",
    cancelled: "遗忘项目已取消",
  },
  preview: {
    success: "预览完成",
    partial: "预览部分失败",
    failed: "预览失败",
    cancelled: "预览已取消",
  },
};

export function mutationHeading(
  action: MutationAction,
  outcome: MutationOutcome,
): string {
  return HEADING_LABELS[action][outcome];
}

function normalizedRowStatus(status: TaskSkillStatus): MutationRowStatus {
  switch (status) {
    case "failed":
      return "failed";
    case "skipped":
      return "skipped";
    default:
      return "success";
  }
}

export function normalizeTaskResult(result: TaskResult): MutationResultView {
  return {
    outcome: result.overall,
    batch: result.batch,
    rows: result.batch
      ? result.batch.rows.map((row) => ({
          name: batchTargetLabel(row),
          status:
            row.status === "not_executed" ? ("skipped" as const) : row.status,
          statusLabel:
            row.status === "success"
              ? row.operation === "attach" ? "已挂载" : BATCH_SUCCESS_LABELS[result.batch!.action]
              : row.status === "failed"
                ? "失败"
                : row.status === "skipped"
                  ? "跳过"
                  : "未执行",
          error: row.error ?? undefined,
          reason:
            row.status === "not_executed"
              ? (row.reason ?? "批量操作已停止，此目标尚未执行。")
              : (row.reason ?? undefined),
          backupPath: row.backupPath ?? undefined,
        }))
      : result.skills.map((skill) => ({
      name: skill.name,
      status: normalizedRowStatus(skill.status),
      statusLabel: TASK_SKILL_STATUS_LABELS[skill.status],
      taskStatus: skill.status,
      error: skill.error,
      backupPath: skill.backupPath,
    })),
    warnings: result.warnings,
    backupPath: result.backupPath,
    logPath: result.logPath,
    scope: result.scope,
  };
}

export interface PromiseMutationResult {
  rows: MutationRow[];
  warnings?: AppError[];
  backupPath?: string;
  logPath?: string;
  /** Scope the native action affected; refresh policy prefers it. */
  scope?: ScopeId;
  checkRevision?: string;
  /** Optional terminal message; the progress message is kept otherwise. */
  message?: string;
}

export function normalizePromiseResult(
  value: PromiseMutationResult,
): MutationResultView {
  const failed = value.rows.filter((row) => row.status === "failed").length;
  const succeeded = value.rows.some((row) => row.status === "success");
  return {
    outcome: failed === 0 ? "success" : succeeded ? "partial" : "failed",
    rows: value.rows,
    warnings: value.warnings ?? [],
    backupPath: value.backupPath,
    logPath: value.logPath,
    scope: value.scope,
    checkRevision: value.checkRevision,
  };
}

export interface TaskEventDriver {
  kind: "task";
  total: number;
  message: string;
  launch: (onEvent: (event: TaskEvent) => void) => Promise<string>;
}

export interface PromiseProgress {
  completed?: number;
  total?: number;
  message?: string;
}

export interface PromiseDriver {
  kind: "promise";
  total: number;
  message: string;
  indeterminate: boolean;
  run: (
    report: (progress: PromiseProgress) => void,
  ) => Promise<PromiseMutationResult>;
}

export type MutationDriver = TaskEventDriver | PromiseDriver;

export function taskEventDriver(
  options: Omit<TaskEventDriver, "kind">,
): TaskEventDriver {
  return { kind: "task", ...options };
}

export function promiseDriver(
  options: Omit<PromiseDriver, "kind">,
): PromiseDriver {
  return { kind: "promise", ...options };
}

export interface MutationState {
  running: boolean;
  phase: TaskPhase | null;
  completed: number;
  total: number;
  message: string;
  indeterminate: boolean;
  cancelling: boolean;
  taskId: string | null;
  result: MutationResultView | null;
  error: AppError | null;
  startError: AppError | null;
}

export function initialMutationState(): MutationState {
  return {
    running: false,
    phase: null,
    completed: 0,
    total: 1,
    message: "",
    indeterminate: false,
    cancelling: false,
    taskId: null,
    result: null,
    error: null,
    startError: null,
  };
}

export type MutationRunAction =
  | {
      type: "start";
      total: number;
      message: string;
      indeterminate: boolean;
    }
  | { type: "taskId"; taskId: string }
  | {
      type: "progress";
      phase: TaskPhase | null;
      completed?: number;
      total?: number;
      message?: string;
    }
  | {
      type: "terminal";
      phase: TaskPhase | null;
      message?: string;
      completed?: number;
      total?: number;
      result: MutationResultView | null;
    }
  | {
      type: "failed";
      phase: TaskPhase | null;
      message?: string;
      error: AppError;
    }
  | { type: "startFailed"; error: AppError }
  | { type: "startError"; error: AppError | null }
  | { type: "cancelling"; value: boolean }
  | { type: "cancelRejected"; message: string }
  | { type: "cancelFailed"; error: AppError }
  | { type: "reset" };

export function mutationRunReducer(
  state: MutationState,
  action: MutationRunAction,
): MutationState {
  switch (action.type) {
    case "start":
      return {
        ...initialMutationState(),
        running: true,
        total: action.total,
        message: action.message,
        indeterminate: action.indeterminate,
      };
    case "taskId":
      return { ...state, taskId: action.taskId };
    case "progress":
      return {
        ...state,
        phase: action.phase ?? state.phase,
        completed: action.completed ?? state.completed,
        total: action.total ?? state.total,
        message: action.message || state.message,
      };
    case "terminal":
      return {
        ...state,
        running: false,
        cancelling: false,
        phase: action.phase,
        completed: action.completed ?? state.completed,
        total: action.total ?? state.total,
        message: action.message || state.message,
        result: action.result,
        error: null,
      };
    case "failed":
      return {
        ...state,
        running: false,
        cancelling: false,
        phase: action.phase,
        message: action.message || state.message,
        error: action.error,
      };
    case "startFailed":
      return {
        ...state,
        running: false,
        phase: null,
        startError: action.error,
      };
    case "startError":
      return { ...state, startError: action.error };
    case "cancelling":
      return { ...state, cancelling: action.value };
    case "cancelRejected":
      return { ...state, error: fallbackError(action.message) };
    case "cancelFailed":
      return { ...state, error: action.error };
    case "reset":
      return initialMutationState();
  }
}

export interface MutationRefreshPlan {
  invalidate: QueryKey[];
  remove: QueryKey[];
}

/**
 * Cache policy after an action settles. Attach/detach stay scoped to the
 * mutated skill so an open drawer does not refetch every detail. Forgetting a
 * project removes its scope caches instead of refetching a scope that no
 * longer exists.
 */
export function mutationRefreshPlan(
  action: MutationAction,
  scope: ScopeId,
  skillId?: string,
): MutationRefreshPlan {
  const invalidate = (keys: QueryKey[]): MutationRefreshPlan => ({
    invalidate: keys,
    remove: [],
  });
  switch (action) {
    case "copy_to_project":
      return invalidate([
        projectsQueryKey,
        inventoryQueryKey(scope),
        skillQueryPrefix(scope),
        updateReportQueryKey(scope),
        updateCheckStatesQueryKey,
      ]);
    case "install":
      return invalidate([inventoryQueryKey(scope), skillQueryPrefix(scope)]);
    case "update":
      return invalidate([
        inventoryQueryKey(scope),
        skillQueryPrefix(scope),
        updateReportQueryKey(scope),
        skillDirQueryPrefix(scope, skillId),
        skillFileQueryPrefix(scope, skillId),
      ]);
    case "uninstall":
      return invalidate([
        inventoryQueryKey(scope),
        skillQueryPrefix(scope),
        updateReportQueryKey(scope),
      ]);
    case "attach":
    case "detach":
      return invalidate(
        skillId
          ? [
              inventoryQueryKey(scope),
              skillQueryKey(scope, skillId),
              skillDirQueryPrefix(scope, skillId),
              skillFileQueryPrefix(scope, skillId),
            ]
          : [
              inventoryQueryKey(scope),
              skillQueryPrefix(scope),
              skillDirQueryPrefix(scope),
              skillFileQueryPrefix(scope),
            ],
      );
    case "manage_add":
    case "manage_remove":
    case "preview":
      return invalidate([]);
    case "project_add":
    case "project_relocate":
      return invalidate([
        projectsQueryKey,
        inventoryQueryKey(scope),
        skillQueryPrefix(scope),
        updateReportQueryKey(scope),
        updateCheckStateQueryKey(scope),
        updateCheckStatesQueryKey,
        lastFullCheckQueryKey(scope),
        lastFullAttemptQueryKey(scope),
      ]);
    case "project_forget":
      return {
        invalidate: [projectsQueryKey, updateCheckStatesQueryKey],
        remove: [
          inventoryQueryKey(scope),
          skillQueryPrefix(scope),
          updateReportQueryKey(scope),
          updateCheckStateQueryKey(scope),
          lastFullCheckQueryKey(scope),
          lastFullAttemptQueryKey(scope),
        ],
      };
  }
}

export interface MutationRefreshOptions {
  action: MutationAction;
  /** Every cache entry is scoped; refresh never crosses scopes. */
  scope: ScopeId;
  skillId?: string;
  /** Manage actions must re-run the environment check, not just the caches. */
  onEnvironmentRecheck?: () => void;
}

export function refreshMutationQueries(
  queryClient: QueryClient,
  options: MutationRefreshOptions,
  result: MutationResultView | null,
): void {
  const scope = result?.scope ?? options.scope;
  if (result && options.action === "project_relocate" && result.checkRevision) {
    updateCheckLifecycle(queryClient).invalidate(scope, result.checkRevision);
  }
  if (result && options.action === "project_forget") {
    updateCheckLifecycle(queryClient).invalidate(scope, null);
  }
  if (options.action === "update" && result) {
    const updatedNames = result.rows
      .filter(
        (row) =>
          row.status === "success" &&
          (row.taskStatus === undefined || row.taskStatus === "updated"),
      )
      .map((row) => row.name);
    if (updatedNames.length > 0) {
      queryClient.setQueryData<UpdateCheckReport | undefined>(
        updateReportQueryKey(scope),
        (previous) => markSkillsCurrent(previous, updatedNames),
      );
    }
  }
  if (
    [
      "install",
      "uninstall",
      "project_add",
      "project_relocate",
      "project_forget",
    ].includes(options.action)
  ) {
    void queryClient.invalidateQueries({ queryKey: discoveryQueryKey });
  }
  const plan = mutationRefreshPlan(options.action, scope, options.skillId);
  for (const queryKey of plan.invalidate) {
    void queryClient.invalidateQueries({ queryKey });
  }
  for (const queryKey of plan.remove) {
    queryClient.removeQueries({ queryKey });
  }
  if (options.action === "manage_add" || options.action === "manage_remove") {
    options.onEnvironmentRecheck?.();
  }
}

export interface MutationOptions extends MutationRefreshOptions {
  /** Shown when a task fails or a native promise rejects without an error. */
  fallbackFailure: string;
  /** Shown when a task invoke rejects before a task exists. */
  fallbackStart: string;
  /** Read-only observers (preview wizard steps) for each task event. */
  onEvent?: (event: TaskEvent) => void;
}

export interface MutationHandle {
  state: MutationState;
  /** Latest state for one-time callbacks (focus traps, close handlers). */
  stateRef: MutableRefObject<MutationState>;
  /** Resolves with the normalized outcome, or `null` on failure/reset. */
  start: (driver: MutationDriver) => Promise<MutationResultView | null>;
  cancel: (fallbackMessage?: string) => Promise<void>;
  reset: () => void;
  canClose: () => boolean;
  /** Best-effort cancellation and reset; false leaves the run unchanged. */
  requestClose: (companions?: readonly MutationHandle[]) => boolean;
  /** Local validation error, shown before a task exists. */
  setStartError: (error: AppError | null) => void;
}

export function useMutation(options: MutationOptions): MutationHandle {
  const queryClient = useQueryClient();
  const [state, reducerDispatch] = useReducer(
    mutationRunReducer,
    undefined,
    initialMutationState,
  );
  const stateRef = useRef(state);
  const dispatch = useCallback((action: MutationRunAction) => {
    // Close and cancel can run before React renders a queued transition.
    stateRef.current = mutationRunReducer(stateRef.current, action);
    reducerDispatch(action);
  }, []);
  const runSeq = useRef(0);
  const resolverRef = useRef<
    ((result: MutationResultView | null) => void) | null
  >(null);
  const optionsRef = useRef(options);
  optionsRef.current = options;

  const settle = useCallback(
    (seq: number, result: MutationResultView | null) => {
      if (runSeq.current !== seq) return;
      const resolve = resolverRef.current;
      resolverRef.current = null;
      refreshMutationQueries(queryClient, optionsRef.current, result);
      resolve?.(result);
    },
    [queryClient],
  );

  const handleTaskEvent = useCallback(
    (seq: number, event: TaskEvent) => {
      if (runSeq.current !== seq) return;
      optionsRef.current.onEvent?.(event);
      const fallback = optionsRef.current.fallbackFailure;
      switch (event.phase) {
        case "completed":
        case "cancelled": {
          const result = event.result
            ? normalizeTaskResult(event.result)
            : null;
          dispatch({
            type: "terminal",
            phase: event.phase,
            message: event.message,
            completed: event.completed,
            total: event.total,
            result,
          });
          settle(seq, result);
          return;
        }
        case "failed":
          dispatch({
            type: "failed",
            phase: event.phase,
            message: event.message,
            error: event.error ?? fallbackError(fallback),
          });
          settle(seq, null);
          return;
        default:
          dispatch({
            type: "progress",
            phase: event.phase,
            completed: event.completed,
            total: event.total,
            message: event.message,
          });
      }
    },
    [dispatch, settle],
  );

  const start = useCallback(
    (driver: MutationDriver): Promise<MutationResultView | null> => {
      const seq = ++runSeq.current;
      const previous = resolverRef.current;
      resolverRef.current = null;
      previous?.(null);
      dispatch({
        type: "start",
        total: driver.total,
        message: driver.message,
        indeterminate: driver.kind === "promise" && driver.indeterminate,
      });

      return new Promise<MutationResultView | null>((resolve) => {
        resolverRef.current = resolve;

        if (driver.kind === "task") {
          void (async () => {
            try {
              const taskId = await driver.launch((event) =>
                handleTaskEvent(seq, event),
              );
              if (runSeq.current !== seq) {
                void cancelTask(taskId).catch(() => {
                  // The dialog moved on; the backend will finish or reject it.
                });
                return;
              }
              dispatch({ type: "taskId", taskId });
            } catch (error) {
              if (runSeq.current !== seq) return;
              dispatch({
                type: "startFailed",
                error:
                  asAppError(error) ??
                  fallbackError(optionsRef.current.fallbackStart),
              });
              resolverRef.current = null;
              resolve(null);
            }
          })();
          return;
        }

        void (async () => {
          try {
            const value = await driver.run((progress) => {
              if (runSeq.current !== seq) return;
              dispatch({
                type: "progress",
                phase: null,
                completed: progress.completed,
                total: progress.total,
                message: progress.message,
              });
            });
            if (runSeq.current !== seq) return;
            const result = normalizePromiseResult(value);
            dispatch({
              type: "terminal",
              phase: null,
              message: value.message,
              result,
            });
            settle(seq, result);
          } catch (error) {
            if (runSeq.current !== seq) return;
            dispatch({
              type: "failed",
              phase: null,
              error:
                asAppError(error) ??
                fallbackError(optionsRef.current.fallbackFailure),
            });
            settle(seq, null);
          }
        })();
      });
    },
    [dispatch, handleTaskEvent, settle],
  );

  const cancel = useCallback(
    async (fallbackMessage = "无法取消任务。") => {
      const current = stateRef.current;
      if (!current.running || !current.taskId || current.cancelling) return;
      const seq = runSeq.current;
      const isCurrent = () =>
        runSeq.current === seq &&
        stateRef.current.running &&
        stateRef.current.taskId === current.taskId;
      dispatch({ type: "cancelling", value: true });
      try {
        const ack = await cancelTask(current.taskId);
        if (!isCurrent()) return;
        if (!ack.accepted) {
          dispatch({
            type: "cancelRejected",
            message: ack.message ?? fallbackMessage,
          });
        }
      } catch (error) {
        if (!isCurrent()) return;
        dispatch({
          type: "cancelFailed",
          error: asAppError(error) ?? fallbackError(fallbackMessage),
        });
      } finally {
        if (isCurrent()) dispatch({ type: "cancelling", value: false });
      }
    },
    [dispatch],
  );

  const reset = useCallback(() => {
    runSeq.current += 1;
    const resolve = resolverRef.current;
    resolverRef.current = null;
    resolve?.(null);
    dispatch({ type: "reset" });
  }, [dispatch]);

  const canClose = useCallback(() => {
    const current = stateRef.current;
    if (!current.running) return true;
    if (optionsRef.current.action === "preview") return true;
    return (
      optionsRef.current.action === "install" && current.phase === "preparing"
    );
  }, []);

  const requestClose = useCallback(
    (companions: readonly MutationHandle[] = []) => {
      const participants = [{ stateRef, canClose, reset }, ...companions];
      if (participants.some((handle) => !handle.canClose())) return false;
      const active = participants.find(
        (handle) => handle.stateRef.current.running,
      );
      const taskId = active?.stateRef.current.taskId;
      if (taskId) {
        void cancelTask(taskId).catch(() => {
          // Closure does not wait for the backend cancellation window.
        });
      }
      for (const handle of participants) handle.reset();
      return true;
    },
    [canClose, reset],
  );

  const setStartError = useCallback(
    (error: AppError | null) => {
    dispatch({ type: "startError", error });
  },
    [dispatch],
  );

  return {
    state,
    stateRef,
    start,
    cancel,
    reset,
    canClose,
    requestClose,
    setStartError,
  };
}
