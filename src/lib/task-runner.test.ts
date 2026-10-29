import { describe, expect, it } from "vitest";
import type { TaskEvent, TaskResult, TaskSkillStatus } from "./types";
import {
  initialTaskRunState,
  TASK_SKILL_STATUS_LABELS,
  taskOverallHeading,
  taskRunReducer,
} from "./task-runner";

function result(overall: TaskResult["overall"]): TaskResult {
  return {
    action: "update",
    overall,
    skills: [],
    warnings: [],
    startedAt: 1,
    finishedAtEpochMs: 2,
  };
}

function event(
  phase: TaskEvent["phase"],
  overrides: Partial<TaskEvent> = {},
): TaskEvent {
  return { taskId: "task-1", phase, message: "", ...overrides };
}

describe("shared task runner", () => {
  it("tracks progress and terminal transitions without inventing errors", () => {
    let state = taskRunReducer(initialTaskRunState(), {
      type: "start",
      total: 2,
      message: "正在准备更新…",
    });
    expect(state).toMatchObject({
      running: true,
      phase: "preparing",
      completed: 0,
      total: 2,
      message: "正在准备更新…",
      startError: null,
      taskError: null,
    });

    state = taskRunReducer(state, {
      type: "event",
      event: event("backup", { completed: 0, total: 2, message: "备份中" }),
      fallbackFailure: "更新失败，请重试。",
    });
    expect(state).toMatchObject({
      running: true,
      phase: "backup",
      message: "备份中",
    });

    const completed = result("success");
    state = taskRunReducer(state, {
      type: "event",
      event: event("completed", {
        completed: 2,
        total: 2,
        result: completed,
        message: "更新完成",
      }),
      fallbackFailure: "更新失败，请重试。",
    });
    expect(state).toMatchObject({
      running: false,
      phase: "completed",
      result: completed,
      taskError: null,
    });

    state = taskRunReducer(state, {
      type: "event",
      event: event("failed", { message: "boom" }),
      fallbackFailure: "更新失败，请重试。",
    });
    expect(state.running).toBe(false);
    expect(state.taskError).toEqual({
      message: "更新失败，请重试。",
      retryable: false,
    });

    state = taskRunReducer(state, {
      type: "event",
      event: event("failed", {
        error: { code: "cli.timeout", message: "超时", retryable: true },
      }),
      fallbackFailure: "更新失败，请重试。",
    });
    expect(state.taskError).toEqual({
      code: "cli.timeout",
      message: "超时",
      retryable: true,
    });

    state = taskRunReducer(state, { type: "reset" });
    expect(state).toEqual(initialTaskRunState());
  });

  it("keeps cancelled results and reports cancel rejections as task errors", () => {
    let state = taskRunReducer(initialTaskRunState(), {
      type: "start",
      total: 1,
      message: "正在准备更新…",
    });
    const cancelled = result("cancelled");
    state = taskRunReducer(state, {
      type: "event",
      event: event("cancelled", { result: cancelled }),
      fallbackFailure: "更新失败，请重试。",
    });
    expect(state).toMatchObject({
      running: false,
      phase: "cancelled",
      result: cancelled,
    });

    state = taskRunReducer(state, { type: "cancelling", value: true });
    expect(state.cancelling).toBe(true);
    state = taskRunReducer(state, {
      type: "cancelRejected",
      message: "当前 skill 正在写入，无法取消。",
    });
    expect(state.cancelling).toBe(true);
    expect(state.taskError).toEqual({
      message: "当前 skill 正在写入，无法取消。",
      retryable: false,
    });

    state = taskRunReducer(state, {
      type: "startFailed",
      error: { message: "无法开始更新。", retryable: false },
    });
    expect(state).toMatchObject({
      running: false,
      phase: null,
      startError: { message: "无法开始更新。", retryable: false },
    });
  });

  it("shares the same status and overall labels for all three dialogs", () => {
    const statuses: TaskSkillStatus[] = [
      "installed",
      "updated",
      "removed",
      "skipped",
      "failed",
    ];
    for (const status of statuses) {
      expect(TASK_SKILL_STATUS_LABELS[status]).not.toBe("");
    }
    expect(TASK_SKILL_STATUS_LABELS.skipped).toBe("跳过");

    expect(taskOverallHeading("install", "success")).toBe("安装完成");
    expect(taskOverallHeading("update", "partial")).toBe("更新部分失败");
    expect(taskOverallHeading("uninstall", "failed")).toBe("卸载失败");
    expect(taskOverallHeading("preview", "cancelled")).toBe("预览已取消");
  });
});
