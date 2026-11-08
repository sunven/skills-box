import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import App from "./App";
import { API_COMMANDS } from "./lib/api";
import { createQueryClient } from "./lib/query-client";
import type { InventorySnapshot, PreviewSummary, TaskEvent, TaskPhase } from "./lib/types";
import { createFakeBackend, type FakeBackend, type FakeTask } from "./test/fake-backend";
import { healthyReport } from "./test/fixtures";
import { emptySnapshot, makeSkill, makeSnapshot } from "./test/inventory-fixtures";

interface MockState {
  backend: FakeBackend;
}

interface SetupOptions {
  startError?: { code: string; message: string; retryable: boolean };
  snapshot?: InventorySnapshot;
}

function setup(options: SetupOptions = {}): MockState {
  const backend = createFakeBackend({
    data: {
      snapshot: options.snapshot ?? emptySnapshot(),
      environment: healthyReport(),
    },
    commands: options.startError
      ? {
          start_preview: () => {
            throw options.startError!;
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

async function openWizard(): Promise<HTMLElement> {
  await screen.findByText("环境正常");
  const opener = screen.getByRole("button", { name: "安装 skill" });
  opener.focus();
  fireEvent.click(opener);
  const dialog = await screen.findByRole("dialog");
  expect(dialog).toHaveAttribute("data-testid", "install-wizard");
  return dialog;
}

function previewTask(state: MockState): FakeTask {
  const task = state.backend.tasks.find(
    (entry) => entry.command === API_COMMANDS.startPreview,
  );
  if (!task) throw new Error("preview channel is not available");
  return task;
}

function emit(state: MockState, event: TaskEvent) {
  act(() => previewTask(state).emit(event));
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
      action: "preview",
      overall: "success",
      skills: [],
      warnings: [],
      startedAt: 0,
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
  await waitFor(() => expect(within(dialog).getByRole("button", { name: "预览仓库" })).toBeEnabled());
  fireEvent.click(within(dialog).getByRole("button", { name: "预览仓库" }));
  await waitFor(() =>
    expect(
      state.backend.tasks.some(
        (task) => task.command === API_COMMANDS.startPreview,
      ),
    ).toBe(true),
  );
}

describe("install wizard steps 1-2", () => {
  it("opens command help on click before and after entering a source", async () => {
    const state = setup();
    const dialog = await openWizard();
    const help = within(dialog).getByRole("button", { name: "查看将执行的命令" });

    fireEvent.click(help);
    expect(screen.getByRole("tooltip")).toHaveTextContent("请先输入仓库来源");
    expect(state.backend.count(API_COMMANDS.previewCliCommands)).toBe(0);

    fireEvent.keyDown(help, { key: "Escape" });
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
    expect(dialog).toBeInTheDocument();

    fireEvent.change(within(dialog).getByLabelText("仓库来源"), {
      target: { value: "acme/skills" },
    });
    await waitFor(() => expect(within(dialog).getByRole("button", { name: "预览仓库" })).toBeEnabled());
    fireEvent.click(help);
    expect(screen.getByRole("tooltip")).toHaveTextContent("add acme/skills -l -g");
    expect(help).toHaveAttribute("aria-describedby", screen.getByRole("tooltip").id);
    expect(state.backend.count(API_COMMANDS.startPreview)).toBe(0);
  });

  it("shows backend source validation errors and blocks empty input", async () => {
    const state = setup({
      startError: {
        code: "source.unsupported_subdir",
        message: "仅支持公开 HTTPS Git 根仓库，不支持网页子目录链接。",
        retryable: false,
      },
    });
    const dialog = await openWizard();

    fireEvent.change(within(dialog).getByLabelText("仓库来源"), {
      target: { value: "" },
    });
    await waitFor(() => expect(within(dialog).getByRole("button", { name: "预览仓库" })).toBeEnabled());
    fireEvent.click(within(dialog).getByRole("button", { name: "预览仓库" }));
    expect(
      await within(dialog).findByText("请输入公开 HTTPS Git 仓库，或 GitHub owner/repo。"),
    ).toBeInTheDocument();
    expect(state.backend.count(API_COMMANDS.startPreview)).toBe(0);

    fireEvent.change(within(dialog).getByLabelText("仓库来源"), {
      target: { value: "owner/repo/sub" },
    });
    await waitFor(() => expect(within(dialog).getByRole("button", { name: "预览仓库" })).toBeEnabled());
    fireEvent.click(within(dialog).getByRole("button", { name: "预览仓库" }));
    const alert = await within(dialog).findByRole("alert");
    expect(alert).toHaveTextContent("不支持网页子目录链接");
    expect(within(dialog).getByText("source.unsupported_subdir")).toBeInTheDocument();
    expect(
      state.backend
        .callsOf(API_COMMANDS.startPreview)
        .map((call) => call.payload.source),
    ).toEqual(["owner/repo/sub"]);
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
            { name: "gamma", description: "Gamma description" },
          ],
        }),
      ),
    );

    expect(within(dialog).queryByRole("button", { name: "取消预览" })).not.toBeInTheDocument();
    const alpha = within(dialog).getByLabelText("选择 alpha");
    const beta = within(dialog).getByLabelText("选择 beta");
    const gamma = within(dialog).getByLabelText("选择 gamma");
    expect(alpha).toBeEnabled();
    expect(beta).toBeDisabled();
    expect(within(dialog).getByText("已安装")).toBeInTheDocument();
    expect(within(dialog).getByText("现有来源：other/existing")).toBeInTheDocument();
    expect(within(dialog).getByText("预览完成 · 3/3")).toBeInTheDocument();

    const next = within(dialog).getByRole("button", { name: "下一步" });
    const selectAll = within(dialog).getByRole("button", { name: "全选" });
    const clearSelection = within(dialog).getByRole("button", { name: "取消全选" });
    expect(alpha).toBeChecked();
    expect(beta).not.toBeChecked();
    expect(gamma).toBeChecked();
    expect(selectAll).toBeDisabled();
    expect(clearSelection).toBeEnabled();
    expect(next).toBeEnabled();
    fireEvent.click(clearSelection);
    expect(alpha).not.toBeChecked();
    expect(gamma).not.toBeChecked();
    expect(selectAll).toBeEnabled();
    expect(clearSelection).toBeDisabled();
    expect(next).toBeDisabled();
    fireEvent.click(alpha);
    expect(next).toBeEnabled();
    fireEvent.click(clearSelection);
    expect(alpha).not.toBeChecked();
    expect(next).toBeDisabled();
    fireEvent.click(selectAll);
    expect(alpha).toBeChecked();
    expect(beta).not.toBeChecked();
    expect(beta).toBeDisabled();
    expect(gamma).toBeChecked();
    expect(next).toBeEnabled();
    fireEvent.click(gamma);
    expect(selectAll).toBeEnabled();
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
    await waitFor(() => expect(
      within(dialog).getByRole("button", { name: "开始安装" }),
    ).toBeEnabled());
  });

  it("searches names and descriptions while preserving selection across filters", async () => {
    const state = setup();
    const dialog = await openWizard();
    await startPreview(state, dialog);
    emit(state, completed(preview({ skills: [
      { name: "alpha", description: "Review code", group: "Engineering" },
      {
        name: "beta", description: "Existing skill", group: "Planning",
        conflict: { management: "managed", source: "other/existing" },
      },
      { name: "gamma", description: "Plan a roadmap", group: "Planning" },
    ] })));

    const search = within(dialog).getByRole("searchbox", { name: "搜索待安装的 skill" });
    const next = within(dialog).getByRole("button", { name: "下一步" });
    fireEvent.change(search, { target: { value: "  ALPHA  " } });
    expect(within(dialog).getAllByTestId("preview-skill")).toHaveLength(1);
    expect(within(dialog).getByLabelText("选择 alpha")).toBeChecked();
    expect(within(dialog).getByText("显示 1/3 个 skill · 已选 2 个")).toBeInTheDocument();
    fireEvent.click(within(dialog).getByLabelText("选择 alpha"));

    fireEvent.change(search, { target: { value: "ROADMAP" } });
    expect(within(dialog).getAllByTestId("preview-skill")).toHaveLength(1);
    expect(within(dialog).getByLabelText("选择 gamma")).toBeChecked();
    expect(within(dialog).getByText("Planning")).toBeInTheDocument();
    expect(within(dialog).queryByText("Engineering")).not.toBeInTheDocument();

    fireEvent.change(search, { target: { value: "no-match" } });
    expect(within(dialog).queryAllByTestId("preview-skill")).toHaveLength(0);
    expect(within(dialog).getByText("没有匹配的 skill · 已选 1 个")).toBeInTheDocument();
    expect(next).toBeEnabled();
    fireEvent.click(within(dialog).getByRole("button", { name: "取消全选" }));
    expect(next).toBeDisabled();
    fireEvent.click(within(dialog).getByRole("button", { name: "全选" }));
    expect(within(dialog).getByText("没有匹配的 skill · 已选 2 个")).toBeInTheDocument();

    fireEvent.change(search, { target: { value: "gamma" } });
    fireEvent.click(within(dialog).getByLabelText("选择 gamma"));
    fireEvent.change(search, { target: { value: "" } });
    expect(within(dialog).getAllByTestId("preview-skill")).toHaveLength(3);
    expect(within(dialog).getByLabelText("选择 alpha")).toBeChecked();
    expect(within(dialog).getByLabelText("选择 gamma")).not.toBeChecked();
    expect(within(dialog).getByLabelText("选择 beta")).not.toBeChecked();
    expect(within(dialog).getByLabelText("选择 beta")).toBeDisabled();
    fireEvent.click(next);
    fireEvent.click(within(dialog).getByRole("button", { name: "下一步" }));
    expect(within(dialog).getByTestId("install-confirm")).toHaveTextContent("1 个 · alpha");
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
      expect(
        state.backend
          .callsOf(API_COMMANDS.cancelTask)
          .map((call) => call.payload),
      ).toEqual([{ taskId: "task-1" }]),
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
      expect(
        state.backend
          .callsOf(API_COMMANDS.cancelTask)
          .map((call) => call.payload),
      ).toEqual([{ taskId: "task-1" }]),
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
    await waitFor(() =>
      expect(state.backend.count(API_COMMANDS.startPreview)).toBe(2),
    );
  });
});
