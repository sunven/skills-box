import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it } from "vitest";
import App from "./App";
import { API_COMMANDS } from "./lib/api";
import { createQueryClient } from "./lib/query-client";
import type {
  AppError,
  InventorySnapshot,
  TaskEvent,
  TaskPhase,
  TaskResult,
  TaskSkillStatus,
} from "./lib/types";
import {
  createFakeBackend,
  type FakeBackend,
  type FakeTask,
} from "./test/fake-backend";
import { healthyReport } from "./test/fixtures";
import {
  attachment,
  group,
  makeAnomalies,
  makeSkill,
  makeSnapshot,
} from "./test/inventory-fixtures";

interface UninstallState {
  backend: FakeBackend;
}

function baseSnapshot(): InventorySnapshot {
  return makeSnapshot({
    skills: [
      makeSkill({
        name: "alpha",
        description: "Alpha 描述",
        source: "acme/skills",
        updatedAt: "2026-10-01T08:00:00.000Z",
        attachments: [
          attachment("claude-code", "link", "~/.claude/skills/alpha"),
          attachment("gemini-cli", "copy", "~/.gemini/skills/alpha"),
          attachment("codex", "native", "~/.agents/skills/alpha"),
        ],
      }),
      makeSkill({
        name: "copy-only",
        description: "仅副本",
        source: "other/repo",
        management: "copy_only",
        canonicalPath: null,
        attachments: [
          attachment("claude-code", "copy", "~/.claude/skills/copy-only"),
        ],
      }),
      makeSkill({
        name: "unmanaged",
        description: "未受管",
        source: null,
        management: "unmanaged",
        canonicalPath: null,
        attachments: [
          attachment("claude-code", "copy", "~/.claude/skills/unmanaged"),
        ],
      }),
    ],
    anomalies: makeAnomalies({
      total: 2,
      unmanaged: group(1, ["unmanaged"]),
      modified: group(1, ["alpha · 本地改动?"]),
    }),
  });
}

function snapshotWithoutAlpha(): InventorySnapshot {
  return makeSnapshot({
    skills: baseSnapshot().skills.filter((skill) => skill.name !== "alpha"),
    anomalies: makeAnomalies({
      total: 1,
      unmanaged: group(1, ["unmanaged"]),
    }),
  });
}

function setup(startError?: AppError): UninstallState {
  const backend = createFakeBackend({
    data: {
      environment: healthyReport(),
      snapshot: baseSnapshot(),
    },
    commands: startError
      ? {
          start_uninstall: () => {
            throw startError;
          },
        }
      : undefined,
  }).install();
  render(
    <QueryClientProvider client={createQueryClient()}>
      <App />
    </QueryClientProvider>,
  );
  return { backend };
}

function uninstallTask(state: UninstallState, index: number): FakeTask {
  const task = state.backend.tasks.filter(
    (entry) => entry.command === API_COMMANDS.startUninstall,
  )[index];
  if (!task) throw new Error("channel is not available");
  return task;
}

function emitOn(state: UninstallState, index: number, event: TaskEvent) {
  act(() => uninstallTask(state, index).emit(event));
}

function uninstallCallsOf(state: UninstallState) {
  return state.backend
    .callsOf(API_COMMANDS.startUninstall)
    .map((call) => ({ skillId: call.payload.skillId }));
}

function revealCallsOf(state: UninstallState) {
  return state.backend
    .callsOf(API_COMMANDS.revealAppData)
    .map((call) => call.payload.kind);
}

function taskEvent(
  phase: TaskPhase,
  overrides: Partial<TaskEvent> = {},
): TaskEvent {
  return { taskId: "task-uninstall-1", phase, message: "", ...overrides };
}

function uninstallResult(
  overall: TaskResult["overall"],
  status: TaskSkillStatus,
  overrides: Partial<TaskResult> = {},
): TaskResult {
  return {
    action: "uninstall",
    overall,
    skills: [
      {
        name: "alpha",
        status,
        backupPath: "/tmp/backups/1-uninstall-alpha",
        ...(status === "failed"
          ? {
              error: {
                code: "verify.attachment_present",
                message:
                  "卸载后 agent「Claude Code」的挂载仍然存在（alpha）。",
                retryable: false,
              },
            }
          : {}),
      },
    ],
    warnings: [],
    backupPath: "/tmp/backups/1-uninstall-alpha",
    logPath: "/tmp/logs/operations.jsonl",
    startedAt: 1,
    finishedAtEpochMs: 2,
    ...overrides,
  };
}

function row(name: string): HTMLElement {
  const found = screen
    .queryAllByTestId("skill-row")
    .find((candidate) => candidate.getAttribute("data-skill-name") === name);
  if (!found) throw new Error(`missing row ${name}`);
  return found;
}

function detail() {
  return within(screen.getByRole("complementary", { name: "详情" }));
}

async function openUninstall(name: string) {
  await waitFor(() => {
    expect(
      screen
        .queryAllByTestId("skill-row")
        .some((candidate) => candidate.getAttribute("data-skill-name") === name),
    ).toBe(true);
  });
  fireEvent.click(row(name));
  fireEvent.click(await screen.findByRole("tab", { name: "内容" }));
  await waitFor(() => {
    expect(
      detail().getByRole("button", { name: "彻底卸载" }),
    ).toBeInTheDocument();
  });
  const opener = detail().getByRole("button", { name: "彻底卸载" });
  opener.focus();
  fireEvent.click(opener);
  return screen.getByRole("dialog", { name: `彻底卸载 · ${name}` });
}

async function confirmUninstall(dialog: HTMLElement) {
  const confirm = within(dialog).getByRole("button", { name: "确认卸载" });
  expect(confirm).toBeDisabled();
  fireEvent.click(confirm);
  expect(uninstallCalls()).toHaveLength(0);
  fireEvent.click(within(dialog).getByLabelText("确认彻底卸载"));
  expect(confirm).toBeEnabled();
  fireEvent.click(confirm);
}

let currentState: UninstallState | null = null;
function uninstallCalls() {
  return currentState ? uninstallCallsOf(currentState) : [];
}

afterEach(() => {
  currentState = null;
});

describe("uninstall", () => {
  it("offers uninstall only for managed and copy-only rows", async () => {
    currentState = setup();
    await screen.findByText("alpha");

    fireEvent.click(row("unmanaged"));
    await waitFor(() => {
      expect(detail().getByText(/未受管：该目录没有 lock 条目/)).toBeInTheDocument();
    });
    fireEvent.click(await screen.findByRole("tab", { name: "操作" }));
    expect(
      detail().queryByRole("button", { name: "彻底卸载" }),
    ).toBeNull();

    fireEvent.click(row("copy-only"));
    await waitFor(() => {
      expect(detail().getByText(/仅副本：没有 canonical 目录/)).toBeInTheDocument();
    });
    fireEvent.click(await screen.findByRole("tab", { name: "操作" }));
    await waitFor(() => {
      expect(
        detail().getByRole("button", { name: "彻底卸载" }),
      ).toBeInTheDocument();
    });
    const dialog = await openUninstall("copy-only");
    const scope = within(dialog).getByTestId("uninstall-scope");
    expect(scope).toHaveTextContent("无 canonical");
    expect(scope).toHaveTextContent("Claude Code");
    expect(scope).toHaveTextContent("~/.claude/skills/copy-only");
    expect(scope).toHaveTextContent("lock 条目：copy-only");
    expect(scope).toHaveTextContent("基线");
    expect(within(dialog).getByTestId("uninstall-danger")).toHaveTextContent(
      "不可撤销",
    );

    fireEvent.click(within(dialog).getByRole("button", { name: "取消" }));
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: "彻底卸载 · copy-only" }),
      ).not.toBeInTheDocument(),
    );
    expect(document.activeElement).toBe(
      detail().getByRole("button", { name: "彻底卸载" }),
    );
  });

  it("requires the checkbox, backs up, removes and refreshes inventory and anomalies", async () => {
    const state = setup();
    currentState = state;
    const dialog = await openUninstall("alpha");

    const scope = within(dialog).getByTestId("uninstall-scope");
    expect(scope).toHaveTextContent("~/.agents/skills/alpha");
    expect(scope).toHaveTextContent("Claude Code");
    expect(scope).toHaveTextContent("Gemini CLI");
    expect(scope).toHaveTextContent("Codex");
    expect(scope).toHaveTextContent("lock 条目：alpha");
    expect(scope).toHaveTextContent("基线");

    await confirmUninstall(dialog);
    await waitFor(() =>
      expect(uninstallCallsOf(state)).toEqual([{ skillId: "alpha" }]),
    );
    expect(uninstallCallsOf(state)[0]).toEqual({ skillId: "alpha" });

    emitOn(
      state,
      0,
      taskEvent("backup", {
        message: "正在备份 alpha…",
        skill: "alpha",
        completed: 0,
        total: 1,
      }),
    );
    expect(within(dialog).getByText("正在备份")).toBeInTheDocument();

    state.backend.update({ snapshot: snapshotWithoutAlpha() });
    emitOn(
      state,
      0,
      taskEvent("completed", {
        message: "已彻底卸载「alpha」。",
        completed: 1,
        total: 1,
        result: uninstallResult("success", "removed"),
      }),
    );

    const result = await within(dialog).findByTestId("uninstall-result");
    expect(result).toHaveAttribute("data-overall", "success");
    expect(result).toHaveTextContent("卸载完成");
    expect(result).not.toHaveTextContent("卸载失败");
    expect(within(dialog).getByTestId("uninstall-skill-result")).toHaveAttribute(
      "data-skill-status",
      "removed",
    );
    expect(result).toHaveTextContent("已卸载");
    expect(within(dialog).getByTestId("uninstall-backup-path")).toHaveTextContent(
      "/tmp/backups/1-uninstall-alpha",
    );
    expect(within(dialog).getByTestId("uninstall-log-path")).toHaveTextContent(
      "/tmp/logs/operations.jsonl",
    );

    await waitFor(() => {
      expect(
        screen
          .queryAllByTestId("skill-row")
          .map((element) => element.getAttribute("data-skill-name")),
      ).not.toContain("alpha");
    });
    await waitFor(() =>
      expect(state.backend.count(API_COMMANDS.getInventory)).toBeGreaterThanOrEqual(2),
    );
    await waitFor(() => expect(state.backend.count(API_COMMANDS.getSkillDetail)).toBeGreaterThanOrEqual(2));
    expect(
      screen.getByRole("button", { name: /异常 1/ }),
    ).toBeInTheDocument();

    fireEvent.click(
      within(dialog).getByRole("button", { name: "打开备份目录" }),
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "打开日志目录" }),
    );
    await waitFor(() =>
      expect(revealCallsOf(state)).toEqual(["backups", "logs"]),
    );

    fireEvent.click(within(dialog).getByRole("button", { name: "完成" }));
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: "彻底卸载 · alpha" }),
      ).not.toBeInTheDocument(),
    );
  });

  it("never claims success when verification fails", async () => {
    const state = setup();
    currentState = state;
    const dialog = await openUninstall("alpha");
    await confirmUninstall(dialog);

    emitOn(
      state,
      0,
      taskEvent("completed", {
        message: "卸载未完成：「alpha」仍保留部分文件或 lock 条目。",
        completed: 1,
        total: 1,
        result: uninstallResult("failed", "failed"),
      }),
    );

    const result = await within(dialog).findByTestId("uninstall-result");
    expect(result).toHaveAttribute("data-overall", "failed");
    expect(result).toHaveTextContent("卸载失败");
    expect(result).not.toHaveTextContent("卸载完成");
    expect(within(dialog).getByTestId("uninstall-skill-result")).toHaveAttribute(
      "data-skill-status",
      "failed",
    );
    expect(result).toHaveTextContent(
      "卸载后 agent「Claude Code」的挂载仍然存在（alpha）。",
    );
    expect(within(dialog).getByTestId("uninstall-backup-path")).toHaveTextContent(
      "/tmp/backups/1-uninstall-alpha",
    );
    // The row is not removed by a failed uninstall.
    expect(row("alpha")).toBeInTheDocument();
    expect(within(dialog).queryByText("卸载完成")).toBeNull();
  });

  it("surfaces dependency conflicts from the backend before starting a task", async () => {
    const state = setup({
      code: "agent.dependency_conflict",
      message:
        "agent「Claude Code」上的副本仍被其他 agent 的挂载引用，无法彻底卸载。",
      detail: "引用该副本的挂载：Gemini CLI · ~/.gemini/skills/alpha",
      hint: "请先在相关 agent 中断开这些挂载，再重试卸载。",
      retryable: false,
    });
    currentState = state;
    const dialog = await openUninstall("alpha");
    await confirmUninstall(dialog);

    const alert = await within(dialog).findByTestId("uninstall-start-error");
    expect(alert).toHaveTextContent("无法彻底卸载");
    expect(alert).toHaveTextContent("Gemini CLI");
    expect(alert).toHaveTextContent("请先在相关 agent 中断开");
    expect(uninstallCallsOf(state)).toEqual([{ skillId: "alpha" }]);
    expect(within(dialog).queryByTestId("uninstall-result")).toBeNull();
    expect(
      within(dialog).getByRole("button", { name: "确认卸载" }),
    ).toBeEnabled();
  });
});
