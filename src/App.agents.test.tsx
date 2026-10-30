import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { createQueryClient } from "./lib/query-client";
import type {
  AgentAttachment,
  AppError,
  InventoryAgent,
  InventorySnapshot,
} from "./lib/types";
import {
  createFakeBackend,
  type FakeCall,
  type FakeHandlers,
} from "./test/fake-backend";
import { healthyReport } from "./test/fixtures";
import {
  attachment,
  claudeAgent,
  makeAnomalies,
  makeSkill,
  makeSnapshot,
} from "./test/inventory-fixtures";

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(() => Promise.resolve()),
}));

const workbuddyAgent: InventoryAgent = {
  id: "workbuddy",
  displayName: "WorkBuddy",
  universal: false,
  shared: false,
  globalSkillsDir: "~/.workbuddy/skills",
  origin: "builtin",
  detected: true,
};

const acmeAgent: InventoryAgent = {
  id: "acme",
  displayName: "Acme Agent",
  universal: false,
  shared: false,
  globalSkillsDir: "~/.acme/skills",
  origin: "user",
  detected: false,
  projectSkillsDir: ".acme/skills",
};

function renderApp() {
  const queryClient = createQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>,
  );
}

function snapshotWith(
  current: AgentAttachment[],
  agents: InventoryAgent[],
): InventorySnapshot {
  return makeSnapshot({
    skills: [
      makeSkill({
        name: "alpha",
        description: "Alpha 的单行描述",
        source: "acme/skills",
        updatedAt: "2026-10-01T08:00:00.000Z",
        attachments: current,
      }),
    ],
    agents,
    anomalies: makeAnomalies(),
  });
}

function mockCommands(options: {
  agents: InventoryAgent[];
  initial?: AgentAttachment[];
  attachError?: { code: string; message: string; detail?: string };
  commands?: Partial<FakeHandlers>;
}): readonly FakeCall[] {
  const commands: Partial<FakeHandlers> = {
    ...(options.attachError
      ? {
          attach_agent: () => {
            throw options.attachError!;
          },
        }
      : {}),
    ...options.commands,
  };
  return createFakeBackend({
    data: {
      environment: healthyReport(),
      snapshot: snapshotWith(
        options.initial ?? [attachment("claude-code")],
        options.agents,
      ),
    },
    commands,
  }).install().calls;
}

async function selectAlpha() {
  await screen.findByText("alpha");
  const row = screen
    .queryAllByTestId("skill-row")
    .find((candidate) => candidate.getAttribute("data-skill-name") === "alpha");
  if (!row) throw new Error("missing alpha row");
  fireEvent.click(row);
  fireEvent.click(await screen.findByRole("tab", { name: "操作" }));
  await screen.findByText("概览");
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe("extra agent management", () => {
  it("lists builtin and user extras, adds a definition and confirms deletion", async () => {
    const calls = mockCommands({
      agents: [claudeAgent, workbuddyAgent, acmeAgent],
    });
    renderApp();
    await screen.findByText("alpha");

    fireEvent.click(screen.getByRole("button", { name: "管理 agent" }));
    const dialog = screen.getByRole("dialog", { name: /管理 agent/ });
    const rows = within(dialog).getAllByTestId("manage-agent-row");
    expect(rows).toHaveLength(2);
    expect(within(rows[0]!).getByText("内置")).toBeInTheDocument();
    expect(
      within(rows[0]!).queryByRole("button", { name: /删除/ }),
    ).toBeNull();
    expect(within(rows[1]!).getByText("自定义")).toBeInTheDocument();
    expect(within(rows[1]!).getByText("未检测到")).toBeInTheDocument();
    expect(
      within(rows[1]!).getByTestId("manage-agent-project-dir"),
    ).toHaveTextContent("项目目录：.acme/skills");

    fireEvent.change(within(dialog).getByLabelText("id"), {
      target: { value: "acme2" },
    });
    fireEvent.change(within(dialog).getByLabelText("显示名"), {
      target: { value: "Acme Two" },
    });
    fireEvent.change(within(dialog).getByLabelText("skills 目录"), {
      target: { value: "~/.acme2/skills" },
    });
    fireEvent.change(
      within(dialog).getByLabelText("项目内 skills 目录（可选）"),
      {
        target: { value: ".acme2/skills" },
      },
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "添加" }));

    expect(
      await within(dialog).findByTestId("manage-agents-notice"),
    ).toHaveTextContent(/已添加 Acme Two/);
    const addCalls = calls.filter((call) => call.command === "add_extra_agent");
    expect(addCalls).toHaveLength(1);
    expect(addCalls[0]!.payload).toEqual({
      agent: {
        id: "acme2",
        displayName: "Acme Two",
        skillsDir: "~/.acme2/skills",
        projectSkillsDir: ".acme2/skills",
      },
    });

    fireEvent.click(
      within(dialog).getByRole("button", { name: "删除 Acme Agent" }),
    );
    expect(
      within(dialog).getByText(/删除会先断开 0 个挂载/),
    ).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "确认删除" }));

    expect(
      await within(dialog).findByTestId("manage-agents-notice"),
    ).toHaveTextContent(/已删除 Acme Agent/);
    const removeCalls = calls.filter(
      (call) => call.command === "remove_extra_agent",
    );
    expect(removeCalls).toHaveLength(1);
    expect(removeCalls[0]!.payload).toEqual({ agentId: "acme" });
  });

  it("rejects invalid input before calling the backend", async () => {
    const calls = mockCommands({
      agents: [claudeAgent, workbuddyAgent, acmeAgent],
    });
    renderApp();
    await screen.findByText("alpha");
    fireEvent.click(screen.getByRole("button", { name: "管理 agent" }));
    const dialog = screen.getByRole("dialog", { name: /管理 agent/ });

    fireEvent.change(within(dialog).getByLabelText("id"), {
      target: { value: "-bad" },
    });
    fireEvent.change(within(dialog).getByLabelText("显示名"), {
      target: { value: "Bad" },
    });
    fireEvent.change(within(dialog).getByLabelText("skills 目录"), {
      target: { value: "relative/skills" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "添加" }));

    expect(
      await within(dialog).findByTestId("manage-agents-add-error"),
    ).toHaveTextContent("id 不合法");
    expect(
      calls.filter((call) => call.command === "add_extra_agent"),
    ).toHaveLength(0);
  });

  it("validates the optional project directory before calling the backend", async () => {
    const calls = mockCommands({
      agents: [claudeAgent, workbuddyAgent, acmeAgent],
    });
    renderApp();
    await screen.findByText("alpha");
    fireEvent.click(screen.getByRole("button", { name: "管理 agent" }));
    const dialog = screen.getByRole("dialog", { name: /管理 agent/ });

    fireEvent.change(within(dialog).getByLabelText("id"), {
      target: { value: "acme3" },
    });
    fireEvent.change(within(dialog).getByLabelText("显示名"), {
      target: { value: "Acme Three" },
    });
    fireEvent.change(within(dialog).getByLabelText("skills 目录"), {
      target: { value: "~/.acme3/skills" },
    });
    for (const invalid of ["/abs/skills", "../escape", ".agents/skills"]) {
      fireEvent.change(
        within(dialog).getByLabelText("项目内 skills 目录（可选）"),
        { target: { value: invalid } },
      );
      fireEvent.click(within(dialog).getByRole("button", { name: "添加" }));
      expect(
        await within(dialog).findByTestId("manage-agents-add-error"),
      ).toHaveTextContent(/项目内 skills 目录/);
    }
    expect(
      calls.filter((call) => call.command === "add_extra_agent"),
    ).toHaveLength(0);
  });

  it("edits the project directory and surfaces the guard error", async () => {
    const calls = mockCommands({
      agents: [claudeAgent, workbuddyAgent, acmeAgent],
    });
    renderApp();
    await screen.findByText("alpha");
    fireEvent.click(screen.getByRole("button", { name: "管理 agent" }));
    const dialog = screen.getByRole("dialog", { name: /管理 agent/ });

    fireEvent.click(
      within(dialog).getByRole("button", { name: "编辑 Acme Agent" }),
    );
    const input = within(dialog).getByLabelText(
      "项目内 skills 目录 Acme Agent",
    );
    expect(input).toHaveValue(".acme/skills");
    fireEvent.change(input, { target: { value: ".acme2/skills" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "保存" }));

    expect(
      await within(dialog).findByTestId("manage-agents-notice"),
    ).toHaveTextContent(/已更新 Acme Agent 的项目目录为 .acme2\/skills/);
    const updateCalls = calls.filter(
      (call) => call.command === "update_extra_agent",
    );
    expect(updateCalls).toHaveLength(1);
    expect(updateCalls[0]!.payload).toEqual({
      agentId: "acme",
      projectSkillsDir: ".acme2/skills",
    });
  });

  it("shows the project guard while changing a directory with mounts", async () => {
    mockCommands({
      agents: [claudeAgent, workbuddyAgent, acmeAgent],
      commands: {
        update_extra_agent: () => {
          throw {
            code: "extra.project_blocked",
            message: "无法修改额外 agent 的项目目录：项目可能仍保留挂载。",
            detail: "app（/tmp/app）：仍有 1 个挂载（alpha）",
            hint: "请先在相关项目中卸载该 agent 的挂载。",
            retryable: false,
          } satisfies AppError;
        },
      },
    });
    renderApp();
    await screen.findByText("alpha");
    fireEvent.click(screen.getByRole("button", { name: "管理 agent" }));
    const dialog = screen.getByRole("dialog", { name: /管理 agent/ });

    fireEvent.click(
      within(dialog).getByRole("button", { name: "编辑 Acme Agent" }),
    );
    fireEvent.change(
      within(dialog).getByLabelText("项目内 skills 目录 Acme Agent"),
      { target: { value: ".acme2/skills" } },
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "保存" }));

    expect(
      await within(dialog).findByTestId("manage-agents-edit-error"),
    ).toHaveTextContent(/app（\/tmp\/app）：仍有 1 个挂载/);
  });

  it("shows the removal guard for unavailable projects", async () => {
    mockCommands({
      agents: [claudeAgent, workbuddyAgent, acmeAgent],
      commands: {
        remove_extra_agent: () => {
          throw {
            code: "extra.project_blocked",
            message: "无法删除额外 agent：仍有项目不可用。",
            detail: "gone（/tmp/gone）：目录不存在",
            retryable: false,
          } satisfies AppError;
        },
      },
    });
    renderApp();
    await screen.findByText("alpha");
    fireEvent.click(screen.getByRole("button", { name: "管理 agent" }));
    const dialog = screen.getByRole("dialog", { name: /管理 agent/ });

    fireEvent.click(
      within(dialog).getByRole("button", { name: "删除 Acme Agent" }),
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "确认删除" }));
    expect(
      await within(dialog).findByTestId("manage-agents-error"),
    ).toHaveTextContent(/gone（\/tmp\/gone）：目录不存在/);
  });
});

describe("attach an existing skill", () => {
  it("offers detected, unattached extras and links the selected agent", async () => {
    const calls = mockCommands({
      agents: [claudeAgent, workbuddyAgent, acmeAgent],
    });
    renderApp();
    await selectAlpha();

    const pane = within(screen.getByRole("complementary", { name: "详情" }));
    fireEvent.click(pane.getByRole("button", { name: /挂载到/ }));

    const dialog = screen.getByRole("dialog", { name: /挂载到 agent/ });
    expect(
      within(dialog).getByRole("checkbox", { name: "挂载到 WorkBuddy" }),
    ).toBeInTheDocument();
    // Undetected extras and already attached agents are not offered.
    expect(
      within(dialog).queryByRole("checkbox", { name: /挂载到 Acme Agent/ }),
    ).toBeNull();
    expect(
      within(dialog).queryByRole("checkbox", { name: /挂载到 Claude Code/ }),
    ).toBeNull();

    const confirm = within(dialog).getByRole("button", { name: /^挂载/ });
    expect(confirm).toBeDisabled();
    fireEvent.click(
      within(dialog).getByRole("checkbox", { name: "挂载到 WorkBuddy" }),
    );
    fireEvent.click(within(dialog).getByRole("button", { name: /挂载（1）/ }));

    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    const attachCalls = calls.filter((call) => call.command === "attach_agent");
    expect(attachCalls).toHaveLength(1);
    expect(attachCalls[0]!.payload).toEqual({
      skillId: "alpha",
      agentId: "workbuddy",
      scope: "global",
    });
    await waitFor(() => {
      expect(pane.getByText("WorkBuddy")).toBeInTheDocument();
    });
  });

  it("keeps failures visible and refreshes the detail", async () => {
    mockCommands({
      agents: [claudeAgent, workbuddyAgent, acmeAgent],
      attachError: {
        code: "agent.attach_failed",
        message: "创建 agent 挂载失败。",
        detail: "~/.workbuddy/skills/alpha 已存在且不是符号链接，已停止（不覆盖现有内容）。",
      },
    });
    renderApp();
    await selectAlpha();
    const pane = within(screen.getByRole("complementary", { name: "详情" }));
    fireEvent.click(pane.getByRole("button", { name: /挂载到/ }));
    const dialog = screen.getByRole("dialog", { name: /挂载到 agent/ });
    fireEvent.click(
      within(dialog).getByRole("checkbox", { name: "挂载到 WorkBuddy" }),
    );
    fireEvent.click(within(dialog).getByRole("button", { name: /挂载（1）/ }));

    const alert = await within(dialog).findByRole("alert");
    expect(alert).toHaveTextContent("挂载失败");
    expect(alert).toHaveTextContent("不覆盖现有内容");
    expect(screen.getByRole("dialog", { name: /挂载到 agent/ })).toBeInTheDocument();
  });
});
