import { useCallback, useReducer, useRef } from "react";
import type { MutableRefObject } from "react";
import { asAppError, cancelTask, fallbackError } from "./api";
import type {
  AppError,
  TaskEvent,
  TaskPhase,
  TaskResult,
  TaskSkillStatus,
} from "./types";

/**
 * Low-level state machine for one backend task stream. The install preview
 * and the mutation pipeline both consume it; it owns nothing but task events
 * and cancellation, so result normalization and refresh stay above it.
 */
export interface TaskRunState {
  taskId: string | null;
  running: boolean;
  phase: TaskPhase | null;
  completed: number;
  total: number;
  message: string;
  cancelling: boolean;
  result: TaskResult | null;
  taskError: AppError | null;
  startError: AppError | null;
}

export function initialTaskRunState(): TaskRunState {
  return {
    taskId: null,
    running: false,
    phase: null,
    completed: 0,
    total: 1,
    message: "",
    cancelling: false,
    result: null,
    taskError: null,
    startError: null,
  };
}

export type TaskRunAction =
  | { type: "start"; total: number; message: string }
  | { type: "taskId"; taskId: string }
  | { type: "event"; event: TaskEvent; fallbackFailure: string }
  | { type: "cancelling"; value: boolean }
  | { type: "cancelRejected"; message: string }
  | { type: "cancelFailed"; error: AppError }
  | { type: "startFailed"; error: AppError }
  | { type: "startError"; error: AppError | null }
  | { type: "taskError"; error: AppError | null }
  | { type: "reset" };

export function taskRunReducer(
  state: TaskRunState,
  action: TaskRunAction,
): TaskRunState {
  switch (action.type) {
    case "start":
      return {
        ...state,
        taskId: null,
        running: true,
        cancelling: false,
        phase: "preparing",
        completed: 0,
        total: action.total,
        message: action.message,
        result: null,
        taskError: null,
        startError: null,
      };
    case "taskId":
      return { ...state, taskId: action.taskId };
    case "event": {
      const { event } = action;
      const next: TaskRunState = {
        ...state,
        phase: event.phase,
        completed: event.completed ?? state.completed,
        total: event.total ?? state.total,
        message: event.message || state.message,
      };
      if (event.phase === "completed" || event.phase === "cancelled") {
        return {
          ...next,
          running: false,
          cancelling: false,
          result: event.result ?? null,
        };
      }
      if (event.phase === "failed") {
        return {
          ...next,
          running: false,
          cancelling: false,
          taskError: event.error ?? fallbackError(action.fallbackFailure),
        };
      }
      return next;
    }
    case "cancelling":
      return { ...state, cancelling: action.value };
    case "cancelRejected":
      return { ...state, taskError: fallbackError(action.message) };
    case "cancelFailed":
      return { ...state, taskError: action.error };
    case "startFailed":
      return {
        ...state,
        running: false,
        phase: null,
        startError: action.error,
      };
    case "startError":
      return { ...state, startError: action.error };
    case "taskError":
      return { ...state, taskError: action.error };
    case "reset":
      return initialTaskRunState();
  }
}

/**
 * Run one task and guard its event stream by generation. A stale launch
 * (dialog closed or restarted) cancels its task id when the invoke resolves
 * late and drops every event from the previous generation.
 */
async function launchTask(options: {
  runSeq: MutableRefObject<number>;
  launch: (onEvent: (event: TaskEvent) => void) => Promise<string>;
  onEvent: (event: TaskEvent) => void;
  onTaskId: (taskId: string) => void;
  onStartError: (error: AppError) => void;
  fallbackMessage: string;
}): Promise<void> {
  const seq = ++options.runSeq.current;
  try {
    const taskId = await options.launch((event) => {
      if (options.runSeq.current === seq) options.onEvent(event);
    });
    if (options.runSeq.current !== seq) {
      void cancelTask(taskId).catch(() => {
        // The dialog moved on; the backend will finish or reject the cancel.
      });
      return;
    }
    options.onTaskId(taskId);
  } catch (error) {
    if (options.runSeq.current !== seq) return;
    options.onStartError(
      asAppError(error) ?? fallbackError(options.fallbackMessage),
    );
  }
}

export interface TaskEventStart {
  total: number;
  message: string;
  launch: (onEvent: (event: TaskEvent) => void) => Promise<string>;
}

export interface TaskEventDriverOptions {
  /** Shown when a terminal event arrives without a backend error. */
  fallbackFailure: string;
  /** Shown when the invoke itself rejects without a backend error. */
  fallbackStart: string;
  /** Caller-specific side effects for the current task generation. */
  onEvent?: (event: TaskEvent) => void;
}

export interface TaskEventHandle {
  state: TaskRunState;
  /** Latest state for one-time callbacks (focus traps, close handlers). */
  stateRef: MutableRefObject<TaskRunState>;
  start: (start: TaskEventStart) => void;
  cancel: (fallbackMessage?: string) => Promise<void>;
  reset: () => void;
  setStartError: (error: AppError | null) => void;
}

/**
 * Reducer + launch/cancel controller for one task stream. It owns the
 * event-generation guard so a late invoke or event can never leak into a
 * restarted or closed dialog.
 */
export function useTaskEvents(
  options: TaskEventDriverOptions,
): TaskEventHandle {
  const [state, dispatch] = useReducer(
    taskRunReducer,
    undefined,
    initialTaskRunState,
  );
  const stateRef = useRef(state);
  stateRef.current = state;
  const runSeq = useRef(0);
  const optionsRef = useRef(options);
  optionsRef.current = options;

  const handleEvent = useCallback((event: TaskEvent) => {
    const current = optionsRef.current;
    dispatch({
      type: "event",
      event,
      fallbackFailure: current.fallbackFailure,
    });
    current.onEvent?.(event);
  }, []);

  const start = useCallback(
    ({ total, message, launch }: TaskEventStart) => {
      dispatch({ type: "start", total, message });
      void launchTask({
        runSeq,
        launch,
        onEvent: handleEvent,
        onTaskId: (taskId) => dispatch({ type: "taskId", taskId }),
        onStartError: (error) => dispatch({ type: "startFailed", error }),
        fallbackMessage: optionsRef.current.fallbackStart,
      });
    },
    [handleEvent],
  );

  const cancel = useCallback(
    async (fallbackMessage = "无法取消任务。") => {
      const current = stateRef.current;
      if (!current.taskId || current.cancelling) return;
      dispatch({ type: "cancelling", value: true });
      try {
        const ack = await cancelTask(current.taskId);
        if (!ack.accepted) {
          dispatch({
            type: "cancelRejected",
            message: ack.message ?? fallbackMessage,
          });
        }
      } catch (error) {
        dispatch({
          type: "cancelFailed",
          error: asAppError(error) ?? fallbackError(fallbackMessage),
        });
      } finally {
        dispatch({ type: "cancelling", value: false });
      }
    },
    [],
  );

  const reset = useCallback(() => {
    // Invalidate any in-flight invoke/event from the previous dialog session.
    runSeq.current += 1;
    dispatch({ type: "reset" });
  }, []);

  const setStartError = useCallback((error: AppError | null) => {
    dispatch({ type: "startError", error });
  }, []);

  return {
    state,
    stateRef,
    start,
    cancel,
    reset,
    setStartError,
  };
}

export const TASK_PHASE_LABELS: Record<TaskPhase, string> = {
  preparing: "准备中",
  cloning: "正在克隆仓库",
  backup: "正在备份",
  running_cli: "正在执行 CLI",
  verifying: "正在校验结果",
  completed: "已完成",
  failed: "失败",
  cancelled: "已取消",
};

export const TASK_SKILL_STATUS_LABELS: Record<TaskSkillStatus, string> = {
  installed: "已安装",
  updated: "已更新",
  removed: "已卸载",
  skipped: "跳过",
  failed: "失败",
};
