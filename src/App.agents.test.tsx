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

async function openExtraAgentSettings() {
  fireEvent.click(await screen.findByRole("button", { name: "设置" }));
  fireEvent.click(screen.getByRole("button", { name: "额外 Agent" }));
  const settings = screen.getByRole("region", { name: "额外 Agent" });
  await within(settings).findByTestId("extra-agent-settings");
  return settings;
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe("extra agent management", () => {
  it("uses global definitions and attachment counts while a project is active, and refreshes both views", async () => {
    const projectId = "11111111-1111-4111-8111-111111111111";
    const projectScope = `project:${projectId}` as const;
    const backend = createFakeBackend({ data: {
      projects: {
        projects: [{ id: projectId, root: "/tmp/app", displayName: "app",
          addedAt: 1, lastOpenedAt: 2, available: true }],
        lastActiveScope: projectScope,
        warnings: [],
      },
      snapshot: snapshotWith([attachment("acme")], [
        claudeAgent, workbuddyAgent,
        { ...acmeAgent, detected: true, projectSkillsDir: null },
      ]),
      snapshots: { [projectScope]: makeSnapshot({
        skills: [makeSkill({ name: "project-only", attachments: [] })],
        agents: [claudeAgent],
      }) },
    } }).install();
    renderApp();
    await screen.findByText("project-only");
    expect(screen.queryByRole("option", { name: /Acme Agent/ })).toBeNull();

    const settings = await openExtraAgentSettings();
    const acme = within(settings).getAllByTestId("manage-agent-row")
      .find((row) => row.dataset.agentId === "acme")!;
    expect(within(acme).getByText("~/.acme/skills")).toBeInTheDocument();
    expect(within(acme).getByText("1 个挂载")).toBeInTheDocument();
    expect(within(acme).getByTestId("manage-agent-project-dir")).toHaveTextContent("未配置");

    const projectReads = backend.callsOf("get_inventory")
      .filter((call) => call.payload.scope === projectScope).length;
    fireEvent.click(within(acme).getByRole("button", { name: "删除 Acme Agent" }));
    fireEvent.click(within(acme).getByRole("button", { name: "确认删除" }));
    expect(await within(settings).findByTestId("manage-agents-notice")).toHaveTextContent("断开 1 个挂载");
    await waitFor(() => {
      expect(within(settings).queryByText("Acme Agent")).toBeNull();
      expect(backend.callsOf("get_inventory")
        .filter((call) => call.payload.scope === projectScope).length).toBeGreaterThan(projectReads);
    });
    expect(backend.callsOf("get_inventory").some((call) => call.payload.scope === "global")).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "返回 skills" }));
    expect(await screen.findByText("project-only")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "切换到全局" })).toHaveAttribute("aria-pressed", "false");
  });

  it("lists builtin and user extras, adds a definition and confirms deletion", async () => {
    const calls = mockCommands({
      agents: [claudeAgent, workbuddyAgent, acmeAgent],
    });
    renderApp();
    await screen.findByText("alpha");

    fireEvent.change(screen.getByRole("combobox", { name: "Agent 筛选" }), {
      target: { value: "acme" },
    });
    const settings = await openExtraAgentSettings();
    expect(screen.queryByRole("dialog")).toBeNull();
    const rows = within(settings).getAllByTestId("manage-agent-row");
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

    fireEvent.change(within(settings).getByLabelText("id"), {
      target: { value: "acme2" },
    });
    fireEvent.change(within(settings).getByLabelText("显示名"), {
      target: { value: "Acme Two" },
    });
    fireEvent.change(within(settings).getByLabelText("skills 目录"), {
      target: { value: "~/.acme2/skills" },
    });
    fireEvent.change(
      within(settings).getByLabelText("项目内 skills 目录（可选）"),
      {
        target: { value: ".acme2/skills" },
      },
    );
    fireEvent.click(within(settings).getByRole("button", { name: "添加" }));

    expect(
      await within(settings).findByTestId("manage-agents-notice"),
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
    expect(await within(settings).findByText("Acme Two")).toBeInTheDocument();

    fireEvent.click(
      within(settings).getByRole("button", { name: "删除 Acme Agent" }),
    );
    expect(
      within(settings).getByText(/删除会先断开 0 个挂载/),
    ).toBeInTheDocument();
    fireEvent.click(within(settings).getByRole("button", { name: "确认删除" }));

    expect(
      await within(settings).findByTestId("manage-agents-notice"),
    ).toHaveTextContent(/已删除 Acme Agent/);
    const removeCalls = calls.filter(
      (call) => call.command === "remove_extra_agent",
    );
    expect(removeCalls).toHaveLength(1);
    expect(removeCalls[0]!.payload).toEqual({ agentId: "acme" });
    await waitFor(() => expect(within(settings).queryByText("Acme Agent")).toBeNull());
    fireEvent.click(screen.getByRole("button", { name: "返回 skills" }));
    const agentFilter = await screen.findByRole("combobox", { name: "Agent 筛选" });
    expect(agentFilter).toHaveValue("all");
    expect(screen.getByText("alpha")).toBeInTheDocument();
    expect(within(agentFilter).getByRole("option", { name: "Acme Two (0)" })).toBeInTheDocument();
    expect(within(agentFilter).queryByRole("option", { name: /Acme Agent/ })).toBeNull();
  });

  it("rejects invalid input before calling the backend", async () => {
    const calls = mockCommands({
      agents: [claudeAgent, workbuddyAgent, acmeAgent],
    });
    renderApp();
    await screen.findByText("alpha");
    const settings = await openExtraAgentSettings();

    fireEvent.change(within(settings).getByLabelText("id"), {
      target: { value: "-bad" },
    });
    fireEvent.change(within(settings).getByLabelText("显示名"), {
      target: { value: "Bad" },
    });
    fireEvent.change(within(settings).getByLabelText("skills 目录"), {
      target: { value: "relative/skills" },
    });
    fireEvent.click(within(settings).getByRole("button", { name: "添加" }));

    expect(
      await within(settings).findByTestId("manage-agents-add-error"),
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
    const settings = await openExtraAgentSettings();

    fireEvent.change(within(settings).getByLabelText("id"), {
      target: { value: "acme3" },
    });
    fireEvent.change(within(settings).getByLabelText("显示名"), {
      target: { value: "Acme Three" },
    });
    fireEvent.change(within(settings).getByLabelText("skills 目录"), {
      target: { value: "~/.acme3/skills" },
    });
    for (const invalid of ["/abs/skills", "../escape", ".agents/skills"]) {
      fireEvent.change(
        within(settings).getByLabelText("项目内 skills 目录（可选）"),
        { target: { value: invalid } },
      );
      fireEvent.click(within(settings).getByRole("button", { name: "添加" }));
      expect(
        await within(settings).findByTestId("manage-agents-add-error"),
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
    const settings = await openExtraAgentSettings();

    fireEvent.click(
      within(settings).getByRole("button", { name: "编辑 Acme Agent" }),
    );
    const input = within(settings).getByLabelText(
      "项目内 skills 目录 Acme Agent",
    );
    expect(input).toHaveValue(".acme/skills");
    fireEvent.change(input, { target: { value: ".acme2/skills" } });
    fireEvent.click(within(settings).getByRole("button", { name: "保存" }));

    expect(
      await within(settings).findByTestId("manage-agents-notice"),
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
    const settings = await openExtraAgentSettings();

    fireEvent.click(
      within(settings).getByRole("button", { name: "编辑 Acme Agent" }),
    );
    fireEvent.change(
      within(settings).getByLabelText("项目内 skills 目录 Acme Agent"),
      { target: { value: ".acme2/skills" } },
    );
    fireEvent.click(within(settings).getByRole("button", { name: "保存" }));

    expect(
      await within(settings).findByTestId("manage-agents-edit-error"),
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
    const settings = await openExtraAgentSettings();

    fireEvent.click(
      within(settings).getByRole("button", { name: "删除 Acme Agent" }),
    );
    fireEvent.click(within(settings).getByRole("button", { name: "确认删除" }));
    expect(
      await within(settings).findByTestId("manage-agents-error"),
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
