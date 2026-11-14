import { describe, expect, it } from "vitest";
import {
  initialMutationState,
  mutationHeading,
  mutationRunReducer,
  normalizePromiseResult,
  normalizeTaskResult,
} from "./mutation";
import type { TaskResult } from "./types";

function taskResult(
  overall: TaskResult["overall"],
  skills: TaskResult["skills"] = [],
): TaskResult {
  return {
    action: "update",
    overall,
    skills,
    warnings: [],
    startedAt: 1,
    finishedAtEpochMs: 2,
    backupPath: "/tmp/backups/task",
    logPath: "/tmp/logs/operations.jsonl",
  };
}

describe("mutation normalization", () => {
  it("projects CLI task results without losing status vocabulary", () => {
    const result = normalizeTaskResult(
      taskResult("partial", [
        {
          name: "alpha",
          status: "updated",
          backupPath: "/tmp/backups/alpha",
        },
        {
          name: "beta",
          status: "failed",
          error: { message: "boom", retryable: true },
        },
        { name: "gamma", status: "skipped" },
      ]),
    );

    expect(result).toMatchObject({
      outcome: "partial",
      backupPath: "/tmp/backups/task",
      logPath: "/tmp/logs/operations.jsonl",
    });
    expect(result.rows).toEqual([
      {
        name: "alpha",
        status: "success",
        statusLabel: "已更新",
        taskStatus: "updated",
        backupPath: "/tmp/backups/alpha",
        error: undefined,
      },
      {
        name: "beta",
        status: "failed",
        statusLabel: "失败",
        taskStatus: "failed",
        error: { message: "boom", retryable: true },
        backupPath: undefined,
      },
      {
        name: "gamma",
        status: "skipped",
        statusLabel: "跳过",
        taskStatus: "skipped",
        error: undefined,
        backupPath: undefined,
      },
    ]);
  });

  it("derives a native outcome from its rows", () => {
    expect(
      normalizePromiseResult({
        rows: [{ name: "a", status: "success", statusLabel: "已挂载" }],
      }).outcome,
    ).toBe("success");
    expect(
      normalizePromiseResult({
        rows: [
          { name: "a", status: "success", statusLabel: "已挂载" },
          { name: "b", status: "failed", statusLabel: "挂载失败" },
        ],
      }).outcome,
    ).toBe("partial");
    expect(
      normalizePromiseResult({
        rows: [{ name: "b", status: "failed", statusLabel: "挂载失败" }],
      }).outcome,
    ).toBe("failed");
  });

  it("labels every action and outcome without reusing CLI wording", () => {
    expect(mutationHeading("install", "success")).toBe("安装完成");
    expect(mutationHeading("update", "partial")).toBe("更新部分失败");
    expect(mutationHeading("uninstall", "failed")).toBe("卸载失败");
    expect(mutationHeading("attach", "failed")).toBe("挂载失败");
    expect(mutationHeading("detach", "success")).toBe("断开完成");
    expect(mutationHeading("manage_add", "success")).toBe("已添加 agent");
    expect(mutationHeading("manage_remove", "failed")).toBe("删除失败");
  });
});

describe("mutation lifecycle", () => {
  it("tracks task progress and settles on a terminal event", () => {
    let state = mutationRunReducer(initialMutationState(), {
      type: "start",
      total: 2,
      message: "正在准备更新…",
      indeterminate: false,
    });
    expect(state).toMatchObject({
      running: true,
      total: 2,
      message: "正在准备更新…",
      error: null,
      startError: null,
    });

    state = mutationRunReducer(state, {
      type: "progress",
      phase: "backup",
      completed: 1,
      message: "备份中",
    });
    expect(state).toMatchObject({
      running: true,
      phase: "backup",
      completed: 1,
      message: "备份中",
    });

    const result = normalizeTaskResult(taskResult("success"));
    state = mutationRunReducer(state, {
      type: "terminal",
      phase: "completed",
      message: "更新完成",
      result,
    });
    expect(state).toMatchObject({
      running: false,
      phase: "completed",
      message: "更新完成",
      result,
    });
  });

  it("keeps backend errors and client fallbacks distinct", () => {
    let state = mutationRunReducer(initialMutationState(), {
      type: "start",
      total: 1,
      message: "正在准备更新…",
      indeterminate: false,
    });
    state = mutationRunReducer(state, {
      type: "failed",
      phase: "failed",
      error: { code: "cli.timeout", message: "超时", retryable: true },
    });
    expect(state.error).toEqual({
      code: "cli.timeout",
      message: "超时",
      retryable: true,
    });

    state = mutationRunReducer(state, {
      type: "startFailed",
      error: { message: "无法开始更新。", retryable: false },
    });
    expect(state).toMatchObject({
      running: false,
      phase: null,
      startError: { message: "无法开始更新。", retryable: false },
    });
  });

  it("sets and clears a local start error", () => {
    let state = mutationRunReducer(initialMutationState(), {
      type: "startError",
      error: { message: "请输入公开 GitHub 仓库。", retryable: false },
    });
    expect(state.startError).toEqual({
      message: "请输入公开 GitHub 仓库。",
      retryable: false,
    });

    state = mutationRunReducer(state, { type: "startError", error: null });
    expect(state.startError).toBeNull();
  });
});
