import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { Channel } from "@tauri-apps/api/core";
import { afterEach, describe, expect, it } from "vitest";
import App from "./App";
import { createQueryClient } from "./lib/query-client";
import type {
  InventorySnapshot,
  TaskEvent,
  TaskOverall,
  TaskPhase,
  TaskResult,
  TaskSkillResult,
  UpdateCheckReport,
  UpdateSkillResult,
  UpdateState,
} from "./lib/types";
import { healthyReport } from "./test/fixtures";
import { detailForSkill } from "./test/detail-fixtures";
import {
  attachment,
  makeAnomalies,
  makeSkill,
  makeSnapshot,
} from "./test/inventory-fixtures";

interface ExecState {
  channels: Channel<TaskEvent>[];
  indexes: Map<number, number>;
  updateCalls: string[][];
  revealCalls: string[];
  inventoryCalls: number;
  detailCalls: number;
  checkCalls: number;
}

const LAST_SUCCESS = 1793163988000;

function updateResult(
  name: string,
  state: UpdateState,
  overrides: Partial<UpdateSkillResult> = {},
): UpdateSkillResult {
  return {
    name,
    source: "acme/skills",
    ref: "main",
    lockHash: "a".repeat(40),
    remoteHash: "b".repeat(40),
    state,
    cached: false,
    lastSuccessAtEpochMs: LAST_SUCCESS,
    previousState: null,
    error: null,
    note: null,
    ...overrides,
  };
}

function baseSnapshot(): InventorySnapshot {
  return makeSnapshot({
    skills: [
      makeSkill({
        name: "alpha",
        description: "Alpha 描述",
        source: "acme/skills",
        // Universal agents can be physically mounted too; they are always
        // covered by `-a universal` and must not appear as affected agents.
        attachments: [
          attachment("claude-code"),
          attachment("gemini-cli"),
          attachment("codex", "native"),
        ],
      }),
      makeSkill({
        name: "modified-skill",
        description: "有本地改动",
        source: "acme/skills",
        localModification: "modified",
        attachments: [attachment("claude-code")],
      }),
      makeSkill({
        name: "copy-mount",
        description: "copy 挂载",
        source: "acme/skills",
        attachments: [
          attachment("claude-code", "copy"),
          attachment("gemini-cli", "copy"),
        ],
      }),
      makeSkill({
        name: "unknown-local",
        description: "无法确认本地改动",
        source: "acme/skills",
        localModification: "unknown",
        attachments: [attachment("claude-code")],
      }),
      makeSkill({
        name: "failed-skill",
        description: "检查失败",
        source: "acme/skills",
        attachments: [attachment("claude-code")],
      }),
      makeSkill({
        name: "path-missing",
        description: "源路径缺失",
        source: "acme/skills",
        attachments: [attachment("claude-code")],
      }),
      makeSkill({
        name: "unknown-skill",
        description: "未知",
        source: "acme/skills",
        attachments: [attachment("claude-code")],
      }),
      makeSkill({
        name: "copy-only",
        description: "仅副本",
        source: "other/repo",
        management: "copy_only",
        canonicalPath: null,
        attachments: [attachment("claude-code", "copy")],
      }),
    ],
    anomalies: makeAnomalies(),
  });
}

function baseReport(): UpdateCheckReport {
  return {
    skills: [
      updateResult("alpha", "update"),
      updateResult("modified-skill", "update"),
      updateResult("copy-mount", "update"),
      updateResult("unknown-local", "update"),
      updateResult("failed-skill", "error", {
        error: {
          code: "network.error",
          message: "检查更新失败。",
          retryable: true,
        },
      }),
      updateResult("path-missing", "path_missing", { remoteHash: null }),
      updateResult("unknown-skill", "unknown"),
      updateResult("copy-only", "update"),
    ],
    sources: [
      {
        source: "acme/skills",
        ref: "main",
        status: "error",
        skillCount: 6,
        cached: false,
        truncated: false,
        lastSuccessAtEpochMs: LAST_SUCCESS,
        error: {
          code: "network.error",
          message: "检查更新失败。",
          retryable: true,
        },
        note: null,
      },
    ],
    checkedAtEpochMs: LAST_SUCCESS,
    lastSuccessAtEpochMs: LAST_SUCCESS,
    skippedSourceCount: 0,
    cachedSourceCount: 0,
    warnings: [],
  };
}

function setup(): ExecState {
  const state: ExecState = {
    channels: [],
    indexes: new Map(),
    updateCalls: [],
    revealCalls: [],
    inventoryCalls: 0,
    detailCalls: 0,
    checkCalls: 0,
  };
  const snapshot = baseSnapshot();
  const report = baseReport();
  mockIPC((command, payload) => {
    if (command === "get_environment") return healthyReport();
    if (command === "get_inventory") {
      state.inventoryCalls += 1;
      return snapshot;
    }
    if (command === "get_skill_detail") {
      state.detailCalls += 1;
      const skillId = (payload as { skillId: string }).skillId;
      const skill = snapshot.skills.find((entry) => entry.name === skillId);
      return skill ? detailForSkill(skill) : null;
    }
    if (command === "check_updates") {
      state.checkCalls += 1;
      return report;
    }
    if (command === "start_update") {
      const args = payload as {
        skillIds: string[];
        onEvent: Channel<TaskEvent>;
      };
      state.updateCalls.push(args.skillIds);
      state.channels.push(args.onEvent);
      return `task-update-${state.channels.length}`;
    }
    if (command === "reveal_app_data") {
      state.revealCalls.push((payload as { kind: string }).kind);
      return null;
    }
    if (command === "cancel_task") {
      return { taskId: "task-update", accepted: true, message: "已请求取消" };
    }
    throw new Error(`unexpected command: ${command}`);
  });
  render(
    <QueryClientProvider client={createQueryClient()}>
      <App />
    </QueryClientProvider>,
  );
  return state;
}

function emitOn(
  state: ExecState,
  channel: Channel<TaskEvent> | undefined,
  event: TaskEvent,
) {
  if (!channel) throw new Error("channel is not available");
  const internals = (
    window as unknown as {
      __TAURI_INTERNALS__: {
        runCallback: (id: number, data: unknown) => void;
      };
    }
  ).__TAURI_INTERNALS__;
  const index = state.indexes.get(channel.id) ?? 0;
  state.indexes.set(channel.id, index + 1);
  act(() => {
    internals.runCallback(channel.id, { index, message: event });
  });
}

function taskEvent(
  phase: TaskPhase,
  overrides: Partial<TaskEvent> = {},
): TaskEvent {
  return { taskId: "task-update-1", phase, message: "", ...overrides };
}

function skillResult(
  name: string,
  status: TaskSkillResult["status"],
  overrides: Partial<TaskSkillResult> = {},
): TaskSkillResult {
  return { name, status, ...overrides };
}

function completed(result: TaskResult, message: string): TaskEvent {
  return taskEvent("completed", {
    message,
    completed: result.skills.length,
    total: Math.max(1, result.skills.length),
    result,
  });
}

function updateTaskResult(
  overall: TaskOverall,
  skills: TaskSkillResult[],
  overrides: Partial<TaskResult> = {},
): TaskResult {
  return {
    overall,
    skills,
    warnings: [],
    backupPath: "/tmp/backups/1-update-alpha",
    logPath: "/tmp/logs/operations.jsonl",
    finishedAtEpochMs: 2,
    ...overrides,
  };
}

async function checkUpdates() {
  await screen.findByText("alpha");
  fireEvent.click(screen.getByRole("button", { name: "检查更新" }));
  await waitFor(() => {
    expect(
      within(row("alpha")).getByTestId("update-badge"),
    ).toHaveTextContent("可更新");
  });
}

function rows(): HTMLElement[] {
  return screen.queryAllByTestId("skill-row");
}

function row(name: string): HTMLElement {
  const found = rows().find(
    (candidate) => candidate.getAttribute("data-skill-name") === name,
  );
  if (!found) throw new Error(`missing row ${name}`);
  return found;
}

function detail() {
  return within(screen.getByRole("complementary", { name: "详情" }));
}

async function openSingleUpdate(name: string) {
  fireEvent.click(row(name));
  await waitFor(() => {
    expect(
      detail().getByRole("button", { name: "更新" }),
    ).toBeInTheDocument();
  });
  fireEvent.click(detail().getByRole("button", { name: "更新" }));
  return screen.getByRole("dialog", { name: `更新 skill · ${name}` });
}

afterEach(() => {
  clearMocks();
});

describe("update execution", () => {
  it("confirms a single update, backs up, updates and reveals backup/log paths", async () => {
    const state = setup();
    await checkUpdates();
    const dialog = await openSingleUpdate("alpha");

    expect(within(dialog).getByTestId("update-single-confirm")).toHaveTextContent(
      "acme/skills",
    );
    expect(within(dialog).getByTestId("update-single-confirm")).toHaveTextContent(
      "aaaaaaaaaa",
    );
    expect(within(dialog).getByTestId("update-single-confirm")).toHaveTextContent(
      "bbbbbbbbbb",
    );
    expect(within(dialog).getByTestId("update-single-confirm")).toHaveTextContent(
      "Claude Code",
    );
    // Universal-catalog agents are covered by `-a universal`, not listed as
    // affected agents or as copy-conversion targets.
    expect(within(dialog).getByTestId("update-single-confirm")).not.toHaveTextContent(
      "Gemini CLI",
    );
    expect(within(dialog).getByTestId("update-single-confirm")).not.toHaveTextContent(
      "Codex",
    );
    expect(within(dialog).queryByTestId("update-copy-warning")).not.toBeInTheDocument();
    expect(within(dialog).getByTestId("update-single-confirm")).toHaveTextContent(
      "干净",
    );
    expect(within(dialog).queryByLabelText("确认本地改动")).not.toBeInTheDocument();

    fireEvent.click(within(dialog).getByRole("button", { name: "开始更新" }));
    await waitFor(() => expect(state.updateCalls).toEqual([["alpha"]]));
    expect(state.updateCalls[0]).toEqual(["alpha"]);

    emitOn(
      state,
      state.channels[0],
      taskEvent("backup", {
        message: "正在备份 alpha…",
        skill: "alpha",
        completed: 0,
        total: 1,
      }),
    );
    expect(within(dialog).getByText("正在备份")).toBeInTheDocument();
    emitOn(
      state,
      state.channels[0],
      completed(
        updateTaskResult("success", [
          skillResult("alpha", "updated", {
            backupPath: "/tmp/backups/1-update-alpha",
          }),
        ]),
        "更新完成：1 个 skill 已更新。",
      ),
    );

    const result = await within(dialog).findByTestId("update-result");
    expect(result).toHaveAttribute("data-overall", "success");
    expect(result).toHaveTextContent("更新完成");
    expect(result).toHaveTextContent("已更新");
    expect(within(dialog).getByTestId("update-skill-result")).toHaveAttribute(
      "data-skill-status",
      "updated",
    );
    expect(within(dialog).getByTestId("update-backup-path")).toHaveTextContent(
      "/tmp/backups/1-update-alpha",
    );
    expect(within(dialog).getByTestId("update-log-path")).toHaveTextContent(
      "/tmp/logs/operations.jsonl",
    );

    fireEvent.click(within(dialog).getByRole("button", { name: "打开备份目录" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "打开日志目录" }));
    await waitFor(() =>
      expect(state.revealCalls).toEqual(["backups", "logs"]),
    );

    // Successful updates are patched to current without an automatic check.
    await waitFor(() => {
      expect(
        within(row("alpha")).getByTestId("update-badge"),
      ).toHaveTextContent("已是最新");
    });
    expect(state.inventoryCalls).toBeGreaterThanOrEqual(2);
    await waitFor(() => expect(state.detailCalls).toBeGreaterThanOrEqual(2));
    // Update execution must never trigger an automatic check.
    expect(state.checkCalls).toBe(1);

    fireEvent.click(within(dialog).getByRole("button", { name: "完成" }));
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: "更新 skill · alpha" }),
      ).not.toBeInTheDocument(),
    );
  });

  it("requires an explicit checkbox for local modifications", async () => {
    const state = setup();
    await checkUpdates();
    const dialog = await openSingleUpdate("modified-skill");

    expect(within(dialog).getByTestId("update-local-warning")).toHaveTextContent(
      "本地改动将被覆盖",
    );
    const start = within(dialog).getByRole("button", { name: "开始更新" });
    expect(start).toBeDisabled();
    fireEvent.click(start);
    expect(state.updateCalls).toHaveLength(0);

    fireEvent.click(within(dialog).getByLabelText("确认本地改动"));
    expect(start).toBeEnabled();
    fireEvent.click(start);
    await waitFor(() =>
      expect(state.updateCalls).toEqual([["modified-skill"]]),
    );

    emitOn(
      state,
      state.channels[0],
      completed(
        updateTaskResult("success", [
          skillResult("modified-skill", "updated", {
            backupPath: "/tmp/backups/1-update-modified",
          }),
        ]),
        "更新完成：1 个 skill 已更新。",
      ),
    );
    expect(await within(dialog).findByTestId("update-result")).toHaveAttribute(
      "data-overall",
      "success",
    );
  });

  it("requires confirmation when local modification cannot be determined", async () => {
    const state = setup();
    await checkUpdates();
    const dialog = await openSingleUpdate("unknown-local");

    expect(within(dialog).getByTestId("update-local-warning")).toHaveTextContent(
      "本地改动将被覆盖",
    );
    const start = within(dialog).getByRole("button", { name: "开始更新" });
    expect(start).toBeDisabled();
    fireEvent.click(within(dialog).getByLabelText("确认本地改动"));
    expect(start).toBeEnabled();
    expect(state.updateCalls).toHaveLength(0);
    fireEvent.click(within(dialog).getByRole("button", { name: "取消" }));
  });

  it("warns that copy attachments become symlinks before a single update", async () => {
    const state = setup();
    await checkUpdates();
    const dialog = await openSingleUpdate("copy-mount");

    expect(within(dialog).getByTestId("update-copy-warning")).toHaveTextContent(
      "copy 挂载将转为 symlink",
    );
    expect(within(dialog).getByTestId("update-copy-warning")).toHaveTextContent(
      "Claude Code",
    );
    expect(within(dialog).getByTestId("update-copy-warning")).not.toHaveTextContent(
      "Gemini CLI",
    );
    expect(within(dialog).getByTestId("update-copy-warning")).toHaveTextContent(
      "备份",
    );
    const start = within(dialog).getByRole("button", { name: "开始更新" });
    expect(start).toBeEnabled();
    fireEvent.click(within(dialog).getByRole("button", { name: "取消" }));
    expect(state.updateCalls).toHaveLength(0);
  });

  it("keeps completed skill results when a between-skill cancel arrives", async () => {
    const state = setup();
    await checkUpdates();
    const dialog = await openSingleUpdate("alpha");

    fireEvent.click(within(dialog).getByRole("button", { name: "开始更新" }));
    await waitFor(() => expect(state.updateCalls).toHaveLength(1));

    emitOn(
      state,
      state.channels[0],
      taskEvent("cancelled", {
        message: "操作已取消。",
        completed: 1,
        total: 2,
        result: updateTaskResult("cancelled", [
          skillResult("alpha", "updated", {
            backupPath: "/tmp/backups/1-update-alpha",
          }),
        ]),
      }),
    );

    const result = await within(dialog).findByTestId("update-result");
    expect(result).toHaveAttribute("data-overall", "cancelled");
    expect(result).toHaveTextContent("更新已取消");
    expect(within(dialog).getByTestId("update-skill-result")).toHaveAttribute(
      "data-skill-status",
      "updated",
    );
    expect(within(dialog).queryByRole("button", { name: "重试失败项" })).not.toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "关闭" })).toBeInTheDocument();
    // A cancelled task still reconciles inventory/detail; the patched report
    // keeps the completed row current without an automatic check.
    await waitFor(() => expect(state.inventoryCalls).toBeGreaterThanOrEqual(2));
    expect(state.checkCalls).toBe(1);
    fireEvent.click(within(dialog).getByRole("button", { name: "关闭" }));
  });

  it("updates only update-available candidates, skips the rest and retries failures", async () => {
    const state = setup();
    await checkUpdates();

    fireEvent.click(
      screen.getByRole("button", { name: /更新全部 \(2\)/ }),
    );
    const dialog = screen.getByRole("dialog", { name: "更新全部" });
    const candidates = within(dialog).getByTestId("update-candidates");
    expect(candidates).toHaveTextContent("alpha");
    expect(candidates).toHaveTextContent("copy-mount");
    expect(candidates).not.toHaveTextContent("modified-skill");
    const skipped = within(dialog).getByTestId("update-skipped");
    for (const [name, label] of [
      ["modified-skill", "本地改动"],
      ["failed-skill", "检查失败"],
      ["path-missing", "源路径缺失"],
      ["unknown-skill", "未知"],
      ["copy-only", "仅副本"],
    ] as const) {
      expect(within(skipped).getAllByText(name).length).toBeGreaterThan(0);
      expect(
        within(skipped).getAllByText(new RegExp(label)).length,
      ).toBeGreaterThan(0);
    }
    expect(within(dialog).getByTestId("update-copy-warning")).toHaveTextContent(
      "copy 挂载将转为 symlink",
    );
    expect(within(dialog).queryByLabelText("确认本地改动")).not.toBeInTheDocument();

    fireEvent.click(within(dialog).getByRole("button", { name: /开始更新 \(2\)/ }));
    await waitFor(() =>
      expect(state.updateCalls).toEqual([["alpha", "copy-mount"]]),
    );

    emitOn(
      state,
      state.channels[0],
      completed(
        updateTaskResult(
          "partial",
          [
            skillResult("alpha", "updated", {
              backupPath: "/tmp/backups/1-update-alpha",
            }),
            skillResult("copy-mount", "failed", {
              backupPath: "/tmp/backups/2-update-copy-mount",
              error: {
                code: "skill.install_failed",
                message: "CLI 报告更新失败。",
                detail: "boom",
                retryable: true,
              },
            }),
          ],
          { backupPath: undefined },
        ),
        "部分更新完成：1 个成功，1 个未完成。",
      ),
    );

    const result = await within(dialog).findByTestId("update-result");
    expect(result).toHaveAttribute("data-overall", "partial");
    expect(result).toHaveTextContent("更新部分失败");
    const resultRows = within(dialog).getAllByTestId("update-skill-result");
    expect(resultRows[0]).toHaveAttribute("data-skill-status", "updated");
    expect(resultRows[1]).toHaveAttribute("data-skill-status", "failed");
    expect(resultRows[1]).toHaveTextContent("CLI 报告更新失败");
    expect(resultRows[1]).toHaveTextContent("/tmp/backups/2-update-copy-mount");
    expect(within(dialog).getByTestId("update-log-path")).toHaveTextContent(
      "/tmp/logs/operations.jsonl",
    );

    fireEvent.click(
      within(resultRows[1]!).getByRole("button", { name: "重试" }),
    );
    await waitFor(() =>
      expect(state.updateCalls).toEqual([
        ["alpha", "copy-mount"],
        ["copy-mount"],
      ]),
    );

    emitOn(
      state,
      state.channels[1],
      completed(
        updateTaskResult("success", [
          skillResult("copy-mount", "updated", {
            backupPath: "/tmp/backups/3-update-copy-mount",
          }),
        ]),
        "更新完成：1 个 skill 已更新。",
      ),
    );
    const retried = await within(dialog).findByTestId("update-result");
    expect(retried).toHaveAttribute("data-overall", "success");
    expect(within(dialog).getByTestId("update-skill-result")).toHaveAttribute(
      "data-skill-status",
      "updated",
    );
    await waitFor(() => {
      expect(
        within(row("copy-mount")).getByTestId("update-badge"),
      ).toHaveTextContent("已是最新");
    });
  });
});
