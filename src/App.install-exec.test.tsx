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
  DetectedAgent,
  InstallRequest,
  InventorySnapshot,
  PreviewSummary,
  TaskEvent,
  TaskOverall,
  TaskPhase,
  TaskResult,
  TaskSkillResult,
} from "./lib/types";
import { healthyReport } from "./test/fixtures";
import {
  attachment,
  makeSkill,
  makeSnapshot,
} from "./test/inventory-fixtures";

interface ExecState {
  previewChannel: Channel<TaskEvent> | null;
  installChannel: Channel<TaskEvent> | null;
  indexes: Map<number, number>;
  previewCalls: Array<{ source: string }>;
  installCalls: Array<{ request: InstallRequest }>;
  cancelCalls: Array<{ taskId: string }>;
  inventoryCalls: number;
}

const detectedAgents: DetectedAgent[] = [
  {
    id: "claude-code",
    displayName: "Claude Code",
    globalSkillsDir: "~/.claude/skills",
    universal: false,
  },
  {
    id: "antigravity",
    displayName: "Antigravity",
    globalSkillsDir: "~/.gemini/antigravity/skills",
    universal: false,
  },
  {
    id: "codex",
    displayName: "Codex",
    globalSkillsDir: "~/.agents/skills",
    universal: true,
  },
];

function setup(snapshot: InventorySnapshot): ExecState {
  const state: ExecState = {
    previewChannel: null,
    installChannel: null,
    indexes: new Map(),
    previewCalls: [],
    installCalls: [],
    cancelCalls: [],
    inventoryCalls: 0,
  };
  mockIPC((command, payload) => {
    if (command === "get_environment") {
      return healthyReport({ agents: detectedAgents });
    }
    if (command === "get_inventory") {
      state.inventoryCalls += 1;
      return snapshot;
    }
    if (command === "start_preview") {
      const args = payload as {
        source: string;
        onEvent: Channel<TaskEvent>;
      };
      state.previewCalls.push({ source: args.source });
      state.previewChannel = args.onEvent;
      return "task-1";
    }
    if (command === "start_install") {
      const args = payload as {
        request: InstallRequest;
        onEvent: Channel<TaskEvent>;
      };
      state.installCalls.push({ request: args.request });
      state.installChannel = args.onEvent;
      return "task-2";
    }
    if (command === "cancel_task") {
      state.cancelCalls.push(payload as { taskId: string });
      return { taskId: "task-2", accepted: true, message: "已请求取消" };
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

async function openWizard(): Promise<HTMLElement> {
  await screen.findByText("环境正常");
  fireEvent.click(screen.getByRole("button", { name: "安装 skill" }));
  return screen.findByTestId("install-wizard");
}

function emitOn(
  state: ExecState,
  channel: Channel<TaskEvent> | null,
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

function emitPreview(state: ExecState, event: TaskEvent) {
  emitOn(state, state.previewChannel, event);
}

function emitInstall(state: ExecState, event: TaskEvent) {
  emitOn(state, state.installChannel, event);
}

function taskEvent(
  phase: TaskPhase,
  overrides: Partial<TaskEvent> = {},
): TaskEvent {
  return { taskId: "task-2", phase, message: "", ...overrides };
}

function preview(skills: PreviewSummary["skills"]): PreviewSummary {
  return {
    source: "acme/skills",
    skills,
    parseFailed: false,
    noSkills: false,
  };
}

function previewCompleted(summary: PreviewSummary): TaskEvent {
  return taskEvent("completed", {
    message: "预览完成",
    completed: Math.max(1, summary.skills.length),
    total: Math.max(1, summary.skills.length),
    result: {
      overall: "success",
      skills: [],
      warnings: [],
      finishedAtEpochMs: 1,
      preview: summary,
    },
  });
}

function skillResult(
  name: string,
  status: TaskSkillResult["status"],
  message?: string,
): TaskSkillResult {
  return message
    ? {
        name,
        status,
        error: { code: "skill.install_failed", message, retryable: true },
      }
    : { name, status };
}

function installResult(
  overall: TaskOverall,
  skills: TaskSkillResult[],
  warnings: TaskResult["warnings"] = [],
): TaskResult {
  return {
    overall,
    skills,
    warnings,
    finishedAtEpochMs: 2,
    backupPath: "/tmp/backups/1-install-task-2",
  };
}

function installCompleted(result: TaskResult, message: string): TaskEvent {
  return taskEvent("completed", {
    message,
    completed: result.skills.length,
    total: Math.max(1, result.skills.length),
    result,
  });
}

function snapshotWithDefaultAgents(): InventorySnapshot {
  return makeSnapshot({
    skills: [
      makeSkill({
        name: "alpha",
        source: "acme/skills",
        attachments: [
          attachment("claude-code"),
          attachment("codex", "native"),
        ],
      }),
      makeSkill({
        name: "gamma",
        description: "Agent-only unmanaged",
        management: "unmanaged",
        source: null,
        canonicalPath: null,
        attachments: [attachment("antigravity", "copy")],
      }),
    ],
  });
}

async function startPreviewAndReachSkills(
  state: ExecState,
  dialog: HTMLElement,
  skills: PreviewSummary["skills"],
) {
  fireEvent.change(within(dialog).getByLabelText("仓库来源"), {
    target: { value: "https://github.com/acme/skills" },
  });
  fireEvent.click(within(dialog).getByRole("button", { name: "预览仓库" }));
  await waitFor(() => expect(state.previewChannel).not.toBeNull());
  emitPreview(state, previewCompleted(preview(skills)));
}

afterEach(() => {
  clearMocks();
});

describe("install wizard steps 3-5", () => {
  it("defaults agents by managed skills, labels universal and writes canonical", async () => {
    const state = setup(snapshotWithDefaultAgents());
    const dialog = await openWizard();
    await startPreviewAndReachSkills(state, dialog, [
      { name: "alpha", description: "Alpha description" },
    ]);

    const claude = await within(dialog).findByLabelText("选择 Claude Code");
    const antigravity = within(dialog).getByLabelText("选择 Antigravity");
    expect(claude).toBeChecked();
    expect(antigravity).not.toBeChecked();
    const antigravityRow = within(dialog)
      .getByLabelText("选择 Antigravity")
      .closest('[data-testid="agent-option"]');
    expect(antigravityRow).toHaveAttribute("data-only-unmanaged", "true");
    expect(
      within(antigravityRow as HTMLElement).getByText("仅含未受管 skill"),
    ).toBeInTheDocument();
    expect(
      within(antigravityRow as HTMLElement).getByText(
        "目录中只有未受管 skill，默认不勾选",
      ),
    ).toBeInTheDocument();

    const shared = within(dialog).getByTestId("shared-agents");
    expect(shared).toHaveTextContent("共享 canonical");
    expect(shared).toHaveTextContent("Codex");
    expect(within(dialog).getByText(/始终追加/)).toHaveTextContent(
      "-a universal",
    );

    fireEvent.click(within(dialog).getByRole("button", { name: "下一步" }));
    const confirm = within(dialog).getByTestId("install-confirm");
    expect(confirm).toHaveTextContent("acme/skills");
    expect(confirm).toHaveTextContent("1 个 · alpha");
    expect(confirm).toHaveTextContent("Claude Code");
    expect(confirm).toHaveTextContent("共享 canonical（Codex）");
    expect(confirm).toHaveTextContent("~/.agents/skills");
    expect(confirm).toHaveTextContent("~/.claude/skills");
    expect(within(dialog).getByTestId("writing-warning")).toHaveTextContent(
      "写入阶段不可取消",
    );

    fireEvent.click(within(dialog).getByRole("button", { name: "开始安装" }));
    await waitFor(() => expect(state.installCalls).toHaveLength(1));
    expect(state.installCalls[0]!.request).toEqual({
      source: "acme/skills",
      skills: ["alpha"],
      agents: ["claude-code"],
    });

    emitInstall(
      state,
      taskEvent("preparing", { message: "正在准备安装…", completed: 0, total: 1 }),
    );
    const progress = within(dialog).getByTestId("task-progress");
    expect(within(progress).getByText("准备中")).toBeInTheDocument();
    expect(
      within(progress).getByRole("button", { name: "取消安装" }),
    ).toBeInTheDocument();

    emitInstall(
      state,
      taskEvent("running_cli", {
        message: "正在执行 skills add…",
        completed: 0,
        total: 1,
      }),
    );
    expect(within(dialog).getByText("正在执行 CLI")).toBeInTheDocument();
    expect(
      within(dialog).queryByRole("button", { name: "取消安装" }),
    ).not.toBeInTheDocument();
    expect(within(dialog).getByTestId("install-lock-note")).toHaveTextContent(
      "写入阶段不可取消",
    );
    expect(
      within(dialog).getByRole("button", { name: "关闭安装向导" }),
    ).toBeDisabled();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByTestId("install-wizard")).toBeInTheDocument();

    emitInstall(
      state,
      taskEvent("verifying", {
        message: "正在校验 canonical、agent 挂载与 lock…",
        completed: 0,
        total: 1,
      }),
    );
    emitInstall(
      state,
      installCompleted(
        installResult("success", [skillResult("alpha", "installed")], [
          {
            code: "verify.copy_fallback",
            message: "「alpha」在 agent「Claude Code」上是副本而非符号链接。",
            retryable: false,
          },
        ]),
        "安装完成，已验证 1 个 skill。",
      ),
    );

    const result = await within(dialog).findByTestId("install-result");
    expect(result).toHaveAttribute("data-overall", "success");
    expect(result).toHaveTextContent("安装完成");
    expect(result).toHaveTextContent("lock 快照：/tmp/backups/1-install-task-2");
    expect(within(dialog).getByTestId("install-warnings")).toHaveTextContent(
      "副本而非符号链接",
    );
    expect(
      within(dialog).getByTestId("install-skill-result"),
    ).toHaveAttribute("data-skill-status", "installed");

    await waitFor(() => expect(state.inventoryCalls).toBeGreaterThanOrEqual(2));
    expect(
      within(dialog).getByRole("button", { name: "关闭安装向导" }),
    ).toBeEnabled();
    fireEvent.click(within(dialog).getByRole("button", { name: "完成" }));
    await waitFor(() =>
      expect(screen.queryByTestId("install-wizard")).not.toBeInTheDocument(),
    );
  });

  it("shows partial results and retries a single failed skill", async () => {
    const state = setup(snapshotWithDefaultAgents());
    const dialog = await openWizard();
    await startPreviewAndReachSkills(state, dialog, [
      { name: "alpha", description: "Alpha" },
      { name: "beta", description: "Beta" },
    ]);

    fireEvent.click(within(dialog).getByRole("button", { name: "下一步" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "下一步" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "开始安装" }));
    await waitFor(() => expect(state.installCalls).toHaveLength(1));
    expect(state.installCalls[0]!.request.skills).toEqual(["alpha", "beta"]);

    emitInstall(
      state,
      taskEvent("verifying", { message: "校验中", completed: 0, total: 2 }),
    );
    emitInstall(
      state,
      installCompleted(
        installResult("partial", [
          skillResult("alpha", "installed"),
          skillResult("beta", "failed", "agent 挂载缺失"),
        ]),
        "部分安装完成：1 个成功，1 个未完成。",
      ),
    );

    const result = await within(dialog).findByTestId("install-result");
    expect(result).toHaveAttribute("data-overall", "partial");
    expect(result).toHaveTextContent("安装部分失败");
    const rows = within(dialog).getAllByTestId("install-skill-result");
    expect(rows[0]).toHaveAttribute("data-skill-status", "installed");
    expect(rows[1]).toHaveAttribute("data-skill-status", "failed");
    expect(rows[1]).toHaveTextContent("agent 挂载缺失");
    expect(
      within(dialog).getByRole("button", { name: "重试失败项" }),
    ).toBeInTheDocument();

    fireEvent.click(within(rows[1]!).getByRole("button", { name: "重试" }));
    await waitFor(() => expect(state.installCalls).toHaveLength(2));
    expect(state.installCalls[1]!.request).toEqual({
      source: "acme/skills",
      skills: ["beta"],
      agents: ["claude-code"],
    });

    emitInstall(
      state,
      taskEvent("preparing", { message: "正在准备安装…", completed: 0, total: 1 }),
    );
    emitInstall(
      state,
      installCompleted(
        installResult("success", [skillResult("beta", "installed")]),
        "安装完成，已验证 1 个 skill。",
      ),
    );
    const retried = await within(dialog).findByTestId("install-result");
    expect(retried).toHaveAttribute("data-overall", "success");
    expect(within(dialog).getByTestId("install-skill-result")).toHaveAttribute(
      "data-skill-status",
      "installed",
    );
  });

  it("cancels before the CLI spawns and reports the cancelled result", async () => {
    const state = setup(snapshotWithDefaultAgents());
    const dialog = await openWizard();
    await startPreviewAndReachSkills(state, dialog, [
      { name: "alpha", description: "Alpha" },
    ]);

    fireEvent.click(within(dialog).getByRole("button", { name: "下一步" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "开始安装" }));
    await waitFor(() => expect(state.installCalls).toHaveLength(1));

    emitInstall(
      state,
      taskEvent("preparing", { message: "正在准备安装…", completed: 0, total: 1 }),
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "取消安装" }));
    await waitFor(() =>
      expect(state.cancelCalls).toEqual([{ taskId: "task-2" }]),
    );

    emitInstall(state, taskEvent("cancelled", { message: "安装已取消。" }));
    expect(await within(dialog).findByTestId("install-cancelled")).toHaveTextContent(
      "未写入任何文件",
    );
    expect(
      within(dialog).queryByTestId("install-result"),
    ).not.toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "返回确认" }));
    expect(within(dialog).getByTestId("install-confirm")).toBeInTheDocument();
  });

  it("surfaces task-level failures and retries the same request", async () => {
    const state = setup(snapshotWithDefaultAgents());
    const dialog = await openWizard();
    await startPreviewAndReachSkills(state, dialog, [
      { name: "alpha", description: "Alpha" },
    ]);

    fireEvent.click(within(dialog).getByRole("button", { name: "下一步" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "开始安装" }));
    await waitFor(() => expect(state.installCalls).toHaveLength(1));

    emitInstall(
      state,
      taskEvent("failed", {
        message: "skills CLI 执行失败。",
        error: {
          code: "cli.failed",
          message: "skills CLI 执行失败。",
          retryable: true,
        },
      }),
    );

    const alert = await within(dialog).findByTestId("install-error");
    expect(alert).toHaveTextContent("skills CLI 执行失败");
    expect(
      within(dialog).queryByRole("button", { name: "关闭" }),
    ).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "重试" }));
    await waitFor(() => expect(state.installCalls).toHaveLength(2));
    expect(state.installCalls[1]!.request).toEqual(
      state.installCalls[0]!.request,
    );
  });
});
