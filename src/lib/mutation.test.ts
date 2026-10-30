import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import {
  initialMutationState,
  mutationHeading,
  mutationRefreshKeys,
  mutationRunReducer,
  normalizePromiseResult,
  normalizeTaskResult,
  refreshMutationQueries,
} from "./mutation";
import type { MutationResultView } from "./mutation";
import type { TaskResult, UpdateCheckReport } from "./types";
import { GLOBAL_SCOPE } from "./types";
import { updateReportQueryKey } from "./update-view";

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

function updateReport(): UpdateCheckReport {
  return {
    skills: [
      {
        name: "alpha",
        source: "acme/skills",
        ref: null,
        lockHash: "old",
        remoteHash: "new",
        state: "update",
        cached: false,
        lastSuccessAtEpochMs: 1,
        previousState: null,
        error: null,
        note: null,
      },
    ],
    sources: [],
    checkedAtEpochMs: 1,
    lastSuccessAtEpochMs: 1,
    skippedSourceCount: 0,
    cachedSourceCount: 0,
    warnings: [],
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

  it("reports cancel rejections as mutation errors and resets cleanly", () => {
    let state = mutationRunReducer(initialMutationState(), {
      type: "start",
      total: 1,
      message: "正在准备更新…",
      indeterminate: false,
    });
    state = mutationRunReducer(state, { type: "cancelling", value: true });
    state = mutationRunReducer(state, {
      type: "cancelRejected",
      message: "当前 skill 正在写入，无法取消。",
    });
    expect(state.cancelling).toBe(true);
    expect(state.error).toEqual({
      message: "当前 skill 正在写入，无法取消。",
      retryable: false,
    });

    state = mutationRunReducer(state, { type: "reset" });
    expect(state).toEqual(initialMutationState());
  });
});

describe("mutation refresh", () => {
  it("scopes refresh keys to both the scope and the mutated skill", () => {
    expect(mutationRefreshKeys("attach", GLOBAL_SCOPE, "alpha")).toEqual([
      ["inventory", GLOBAL_SCOPE],
      ["skill", GLOBAL_SCOPE, "alpha"],
      ["skill-dir", GLOBAL_SCOPE, "alpha"],
      ["skill-file", GLOBAL_SCOPE, "alpha"],
    ]);
    expect(mutationRefreshKeys("update", GLOBAL_SCOPE, "alpha")).toEqual([
      ["inventory", GLOBAL_SCOPE],
      ["skill", GLOBAL_SCOPE],
      ["updateReport", GLOBAL_SCOPE],
    ]);
    expect(mutationRefreshKeys("manage_add", GLOBAL_SCOPE)).toEqual([]);
  });

  it("patches updated skills to current before invalidating the report", () => {
    const client = new QueryClient();
    client.setQueryData(updateReportQueryKey(GLOBAL_SCOPE), updateReport());
    const invalidate = vi
      .spyOn(client, "invalidateQueries")
      .mockResolvedValue(undefined);
    const result: MutationResultView = {
      outcome: "success",
      warnings: [],
      rows: [
        {
          name: "alpha",
          status: "success",
          statusLabel: "已更新",
          taskStatus: "updated",
        },
      ],
    };

    refreshMutationQueries(
      client,
      { action: "update", scope: GLOBAL_SCOPE },
      result,
    );

    const report = client.getQueryData<UpdateCheckReport>(
      updateReportQueryKey(GLOBAL_SCOPE),
    );
    expect(report?.skills[0]).toMatchObject({
      state: "current",
      lockHash: "new",
    });
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["inventory", GLOBAL_SCOPE],
    });
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["updateReport", GLOBAL_SCOPE],
    });
  });

  it("routes manage actions through the environment recheck", () => {
    const client = new QueryClient();
    const invalidate = vi
      .spyOn(client, "invalidateQueries")
      .mockResolvedValue(undefined);
    const onEnvironmentRecheck = vi.fn();

    refreshMutationQueries(
      client,
      { action: "manage_remove", scope: GLOBAL_SCOPE, onEnvironmentRecheck },
      null,
    );

    expect(onEnvironmentRecheck).toHaveBeenCalledTimes(1);
    expect(invalidate).not.toHaveBeenCalled();
  });

  it("does not patch the update report for non-update actions", () => {
    const client = new QueryClient();
    client.setQueryData(updateReportQueryKey(GLOBAL_SCOPE), updateReport());
    vi.spyOn(client, "invalidateQueries").mockResolvedValue(undefined);

    refreshMutationQueries(
      client,
      { action: "uninstall", scope: GLOBAL_SCOPE },
      {
        outcome: "success",
        warnings: [],
        rows: [
          {
            name: "alpha",
            status: "success",
            statusLabel: "已卸载",
            taskStatus: "removed",
          },
        ],
      },
    );

    const report = client.getQueryData<UpdateCheckReport>(
      updateReportQueryKey(GLOBAL_SCOPE),
    );
    expect(report?.skills[0]?.state).toBe("update");
  });
});
