import type { MutableRefObject } from "react";
import { asAppError, cancelTask } from "./api";
import type {
  AppError,
  TaskAction,
  TaskEvent,
  TaskPhase,
  TaskResult,
  TaskSkillStatus,
} from "./types";

/**
 * Shared mutation-task state machine for the install, update and uninstall
 * dialogs. The backend streams {@link TaskEvent}s; every dialog needs exactly
 * the same progress/cancel/result transitions, so they live here instead of
 * being copied three times.
 */
export interface TaskRunnerError {
  code?: string;
  message: string;
  detail?: string;
  hint?: string;
}

export interface TaskRunState {
  taskId: string | null;
  running: boolean;
  phase: TaskPhase | null;
  completed: number;
  total: number;
  message: string;
  cancelling: boolean;
  result: TaskResult | null;
  taskError: TaskRunnerError | null;
  startError: TaskRunnerError | null;
  revealError: string | null;
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
    revealError: null,
  };
}

export type TaskRunAction =
  | { type: "start"; total: number; message: string }
  | { type: "taskId"; taskId: string }
  | { type: "event"; event: TaskEvent; fallbackFailure: string }
  | { type: "cancelling"; value: boolean }
  | { type: "cancelRejected"; message: string }
  | { type: "cancelFailed"; error: TaskRunnerError }
  | { type: "startFailed"; error: TaskRunnerError }
  | { type: "revealError"; message: string | null }
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
        revealError: null,
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
          taskError: event.error ?? { message: action.fallbackFailure },
        };
      }
      return next;
    }
    case "cancelling":
      return { ...state, cancelling: action.value };
    case "cancelRejected":
      return { ...state, taskError: { message: action.message } };
    case "cancelFailed":
      return { ...state, taskError: action.error };
    case "startFailed":
      return {
        ...state,
        running: false,
        phase: null,
        startError: action.error,
      };
    case "revealError":
      return { ...state, revealError: action.message };
    case "reset":
      return initialTaskRunState();
  }
}

/** The backend is the only source of error codes; no client-invented codes. */
export function eventError(event: TaskEvent, fallbackMessage: string): TaskRunnerError {
  return event.error ?? { message: fallbackMessage };
}

/**
 * Launch one task and keep exactly one event stream alive. A stale launch
 * (dialog closed or restarted) cancels its task id when the invoke resolves
 * late and drops every event from the old {@link MutableRefObject} generation.
 */
export async function launchTask(options: {
  runSeq: MutableRefObject<number>;
  launch: (onEvent: (event: TaskEvent) => void) => Promise<string>;
  onEvent: (event: TaskEvent) => void;
  onTaskId: (taskId: string) => void;
  onStartError: (error: TaskRunnerError) => void;
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
      asAppError(error) ?? { message: options.fallbackMessage },
    );
  }
}

export const TASK_SKILL_STATUS_LABELS: Record<TaskSkillStatus, string> = {
  installed: "已安装",
  updated: "已更新",
  removed: "已卸载",
  skipped: "跳过",
  failed: "失败",
};

/** Result heading shared by the three mutation dialogs. */
export function taskOverallHeading(
  action: TaskAction,
  overall: TaskResult["overall"],
): string {
  const labels: Record<TaskAction, Record<TaskResult["overall"], string>> = {
    preview: {
      success: "预览完成",
      partial: "预览部分完成",
      failed: "预览失败",
      cancelled: "预览已取消",
    },
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
  };
  return labels[action][overall];
}

export type TaskEventError = AppError;
