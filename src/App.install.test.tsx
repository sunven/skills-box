import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { Channel } from "@tauri-apps/api/core";
import { afterEach, describe, expect, it } from "vitest";
import App from "./App";
import { createQueryClient } from "./lib/query-client";
import type {
  InstallRequest,
  InventorySnapshot,
  PreviewSummary,
  TaskEvent,
  TaskPhase,
} from "./lib/types";
import { healthyReport } from "./test/fixtures";
import {
  emptySnapshot,
  makeSkill,
  makeSnapshot,
} from "./test/inventory-fixtures";

interface MockState {
  channel: Channel<TaskEvent> | null;
  installChannel: Channel<TaskEvent> | null;
  index: number;
  startCalls: Array<{ source: string }>;
  installCalls: Array<{ request: InstallRequest }>;
  cancelCalls: Array<{ taskId: string }>;
}

interface SetupOptions {
  startError?: { code: string; message: string; retryable: boolean };
  snapshot?: InventorySnapshot;
}

function setup(options: SetupOptions = {}): MockState {
  const state: MockState = {
    channel: null,
    installChannel: null,
    index: 0,
    startCalls: [],
    installCalls: [],
    cancelCalls: [],
  };
  mockIPC((command, payload) => {
    if (command === "get_environment") return healthyReport();
    if (command === "get_inventory") return options.snapshot ?? emptySnapshot();
    if (command === "start_preview") {
      const args = payload as {
        source: string;
        onEvent: Channel<TaskEvent>;
      };
      state.startCalls.push({ source: args.source });
      if (options.startError) throw options.startError;
      state.channel = args.onEvent;
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
      return { taskId: "task-1", accepted: true, message: "已请求取消" };
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
  const opener = screen.getByRole("button", { name: "安装 skill" });
  opener.focus();
  fireEvent.click(opener);
  const dialog = await screen.findByRole("dialog");
  expect(dialog).toHaveAttribute("data-testid", "install-wizard");
  return dialog;
}

function emit(state: MockState, event: TaskEvent) {
  const channel = state.channel;
  if (!channel) throw new Error("preview channel is not available");
  emitOn(state, channel, event);
}

function emitOn(state: MockState, channel: Channel<TaskEvent>, event: TaskEvent) {
  const internals = (
    window as unknown as {
      __TAURI_INTERNALS__: {
        runCallback: (id: number, data: unknown) => void;
      };
    }
  ).__TAURI_INTERNALS__;
  const index = state.index++;
  act(() => {
    internals.runCallback(channel.id, { index, message: event });
  });
}

function event(
  phase: TaskPhase,
  overrides: Partial<TaskEvent> = {},
): TaskEvent {
  return {
    taskId: "task-1",
    phase,
    message: "",
    ...overrides,
  };
}

function completed(
  preview: PreviewSummary,
  overrides: Partial<TaskEvent> = {},
): TaskEvent {
  return event("completed", {
    message: "预览完成",
    completed: Math.max(1, preview.skills.length),
    total: Math.max(1, preview.skills.length),
    result: {
      overall: "success",
      skills: [],
      warnings: [],
      finishedAtEpochMs: 1,
      preview,
    },
    ...overrides,
  });
}

function preview(overrides: Partial<PreviewSummary> = {}): PreviewSummary {
  return {
    source: "acme/skills",
    skills: [],
    parseFailed: false,
    noSkills: false,
    ...overrides,
  };
}

async function startPreview(
  state: MockState,
  dialog: HTMLElement,
  source = "https://github.com/acme/skills",
) {
  fireEvent.change(within(dialog).getByLabelText("仓库来源"), {
    target: { value: source },
  });
  fireEvent.click(within(dialog).getByRole("button", { name: "预览仓库" }));
  await waitFor(() => expect(state.channel).not.toBeNull());
}

afterEach(() => {
  clearMocks();
});

describe("install wizard steps 1-2", () => {
  it("shows backend source validation errors and blocks empty input", async () => {
    const state = setup({
      startError: {
        code: "source.unsupported_subdir",
        message: "仅支持公开 GitHub 根仓库，不支持子目录。",
        retryable: false,
      },
    });
    const dialog = await openWizard();

    fireEvent.change(within(dialog).getByLabelText("仓库来源"), {
      target: { value: "" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "预览仓库" }));
    expect(
      await within(dialog).findByText("请输入公开 GitHub 仓库，例如 owner/repo。"),
    ).toBeInTheDocument();
    expect(state.startCalls).toHaveLength(0);

    fireEvent.change(within(dialog).getByLabelText("仓库来源"), {
      target: { value: "owner/repo/sub" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "预览仓库" }));
    const alert = await within(dialog).findByRole("alert");
    expect(alert).toHaveTextContent("不支持子目录");
    expect(within(dialog).getByText("source.unsupported_subdir")).toBeInTheDocument();
    expect(state.startCalls).toEqual([{ source: "owner/repo/sub" }]);
  });

  it("renders preview progress, conflicts, selection, agents and the confirm page", async () => {
    const state = setup();
    const dialog = await openWizard();
    await startPreview(state, dialog);

    emit(state, event("preparing", { message: "正在准备预览…", completed: 0, total: 1 }));
    emit(state, event("cloning", { message: "正在克隆仓库并读取 skill 列表…", completed: 0, total: 1 }));

    const progress = within(dialog).getByTestId("task-progress");
    expect(within(progress).getByText("正在克隆仓库")).toBeInTheDocument();
    expect(within(progress).getByText("0/1")).toBeInTheDocument();
    expect(within(progress).getByRole("button", { name: "取消预览" })).toBeInTheDocument();

    emit(
      state,
      completed(
        preview({
          skills: [
            { name: "alpha", description: "Alpha description" },
            {
              name: "beta",
              description: "Beta description",
              conflict: { management: "managed", source: "other/existing" },
            },
          ],
        }),
      ),
    );

    expect(within(dialog).queryByRole("button", { name: "取消预览" })).not.toBeInTheDocument();
    const alpha = within(dialog).getByLabelText("选择 alpha");
    const beta = within(dialog).getByLabelText("选择 beta");
    expect(alpha).toBeEnabled();
    expect(beta).toBeDisabled();
    expect(within(dialog).getByText("已安装")).toBeInTheDocument();
    expect(within(dialog).getByText("现有来源：other/existing")).toBeInTheDocument();
    expect(within(dialog).getByText("预览完成 · 2/2")).toBeInTheDocument();

    const next = within(dialog).getByRole("button", { name: "下一步" });
    expect(alpha).toBeChecked();
    expect(beta).not.toBeChecked();
    expect(next).toBeEnabled();
    fireEvent.click(alpha);
    expect(next).toBeDisabled();
    fireEvent.click(alpha);
    expect(next).toBeEnabled();
    fireEvent.click(next);

    expect(
      within(dialog).getByText(/只显示本机检测到的 agent/),
    ).toBeInTheDocument();
    expect(within(dialog).getByTestId("shared-agents")).toHaveTextContent(
      "共享 canonical",
    );
    expect(within(dialog).getByText(/后台始终追加/)).toHaveTextContent(
      "-a universal",
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "下一步" }));

    const confirm = within(dialog).getByTestId("install-confirm");
    expect(confirm).toHaveTextContent("acme/skills");
    expect(confirm).toHaveTextContent("1 个 · alpha");
    expect(confirm).toHaveTextContent("~/.agents/skills");
    expect(within(dialog).getByTestId("writing-warning")).toHaveTextContent(
      "写入阶段不可取消",
    );
    expect(
      within(dialog).getByRole("button", { name: "开始安装" }),
    ).toBeEnabled();
  });

  it("auto-skips selection for a single installable skill but still shows agents and confirm", async () => {
    const state = setup();
    const dialog = await openWizard();
    await startPreview(state, dialog);

    emit(
      state,
      completed(
        preview({ skills: [{ name: "solo", description: "Only skill" }] }),
      ),
    );

    expect(
      await within(dialog).findByText(/只显示本机检测到的 agent/),
    ).toBeInTheDocument();
    expect(within(dialog).queryByLabelText("选择 solo")).not.toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "下一步" }));
    const confirm = within(dialog).getByTestId("install-confirm");
    expect(confirm).toHaveTextContent("1 个 · solo");
    expect(within(dialog).getByTestId("writing-warning")).toHaveTextContent(
      "写入阶段不可取消",
    );
  });

  it("shows an empty state and disables install when the repo has no SKILL.md", async () => {
    const state = setup();
    const dialog = await openWizard();
    await startPreview(state, dialog);

    emit(
      state,
      completed(preview({ noSkills: true, skills: [] }), {
        message: "仓库中没有可安装的 skill（缺少 SKILL.md）。",
      }),
    );

    expect(await within(dialog).findByTestId("preview-empty")).toHaveTextContent(
      "没有可安装的 skill",
    );
    expect(within(dialog).getByRole("button", { name: "下一步" })).toBeDisabled();
  });

  it("falls back to raw output and allows manual skill names", async () => {
    const state = setup();
    const dialog = await openWizard();
    await startPreview(state, dialog);

    emit(
      state,
      completed(
        preview({
          parseFailed: true,
          rawOutput: "◒ spinner garbage\n◇  something unexpected",
        }),
      ),
    );

    expect(await within(dialog).findByLabelText("预览原始输出")).toHaveTextContent(
      "spinner garbage",
    );
    const next = within(dialog).getByRole("button", { name: "下一步" });
    expect(next).toBeDisabled();
    fireEvent.change(within(dialog).getByLabelText("手动输入 skill 名"), {
      target: { value: "manual-a, manual-b" },
    });
    expect(next).toBeEnabled();
    fireEvent.click(next);
    fireEvent.click(within(dialog).getByRole("button", { name: "下一步" }));

    const confirm = within(dialog).getByTestId("install-confirm");
    expect(confirm).toHaveTextContent("2 个 · manual-a、manual-b");
  });

  it("blocks manual names that collide with installed skills", async () => {
    const state = setup({
      snapshot: makeSnapshot({
        skills: [
          makeSkill({
            name: "manual-a",
            management: "managed",
            source: "acme/existing",
          }),
        ],
      }),
    });
    const dialog = await openWizard();
    await startPreview(state, dialog);
    emit(state, completed(preview({ parseFailed: true, rawOutput: "raw" })));

    fireEvent.change(await within(dialog).findByLabelText("手动输入 skill 名"), {
      target: { value: "manual-a" },
    });
    expect(within(dialog).getByText(/同名冲突：manual-a/)).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "下一步" })).toBeDisabled();
  });

  it("cancels a running preview through the API", async () => {
    const state = setup();
    const dialog = await openWizard();
    await startPreview(state, dialog);

    emit(state, event("preparing", { completed: 0, total: 1 }));
    emit(state, event("cloning", { message: "正在克隆仓库并读取 skill 列表…", completed: 0, total: 1 }));
    fireEvent.click(within(dialog).getByRole("button", { name: "取消预览" }));

    await waitFor(() =>
      expect(state.cancelCalls).toEqual([{ taskId: "task-1" }]),
    );
    emit(state, event("cancelled", { message: "预览已取消。" }));
    expect(await within(dialog).findByText(/预览已取消/)).toBeInTheDocument();
    expect(within(dialog).queryByRole("button", { name: "取消预览" })).not.toBeInTheDocument();
  });

  it("closes on Escape and cancels a running preview", async () => {
    const state = setup();
    const dialog = await openWizard();
    await startPreview(state, dialog);
    emit(state, event("cloning", { message: "正在克隆仓库并读取 skill 列表…", completed: 0, total: 1 }));

    fireEvent.keyDown(document, { key: "Escape" });

    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    await waitFor(() =>
      expect(state.cancelCalls).toEqual([{ taskId: "task-1" }]),
    );
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: "安装 skill" }),
    );
  });

  it("shows a network failure and allows retrying the preview", async () => {
    const state = setup();
    const dialog = await openWizard();
    await startPreview(state, dialog);

    emit(
      state,
      event("failed", {
        message: "无法访问仓库，请检查网络后重试。",
        error: {
          code: "network.fetch_failed",
          message: "无法访问仓库，请检查网络后重试。",
          retryable: true,
        },
      }),
    );

    const alert = await within(dialog).findByRole("alert");
    expect(alert).toHaveTextContent("无法访问仓库");
    const previewButton = within(dialog).getByRole("button", { name: "预览仓库" });
    expect(previewButton).toBeEnabled();

    fireEvent.click(previewButton);
    await waitFor(() => expect(state.startCalls).toHaveLength(2));
  });
});
