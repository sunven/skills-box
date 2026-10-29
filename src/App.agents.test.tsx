import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { createQueryClient } from "./lib/query-client";
import type {
  AgentAttachment,
  AttachResult,
  InventoryAgent,
  InventorySnapshot,
  SkillDetail,
} from "./lib/types";
import { healthyReport } from "./test/fixtures";
import {
  makeDetail,
  makeDirListing,
  makeFileContent,
} from "./test/detail-fixtures";
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

interface Call {
  command: string;
  payload: unknown;
}

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

function detailWith(current: AgentAttachment[]): SkillDetail {
  return makeDetail({
    name: "alpha",
    description: "Alpha 的描述",
    source: "acme/skills",
    sourceType: "github",
    canonicalPath: "~/.agents/skills/alpha",
    directoryPath: "~/.agents/skills/alpha",
    attachments: current,
  });
}

function mockCommands(options: {
  agents: InventoryAgent[];
  initial?: AgentAttachment[];
  onAttach?: (payload: { skillId: string; agentId: string }) => AttachResult;
  attachError?: { code: string; message: string; detail?: string };
}): Call[] {
  const calls: Call[] = [];
  let current = options.initial ?? [attachment("claude-code")];
  mockIPC((command, payload) => {
    calls.push({ command, payload });
    switch (command) {
      case "get_environment":
        return healthyReport();
      case "get_inventory":
        return snapshotWith(current, options.agents);
      case "get_skill_detail":
        return detailWith(current);
      case "list_skill_dir": {
        const args = payload as { skillId: string; path?: string };
        return makeDirListing(args.skillId, args.path ?? "");
      }
      case "read_skill_file": {
        const args = payload as { skillId: string; path: string };
        return makeFileContent(args.skillId, args.path);
      }
      case "open_skill_folder":
      case "open_skill_file":
        return null;
      case "attach_agent": {
        if (options.attachError) throw options.attachError;
        const request = payload as { skillId: string; agentId: string };
        current = [
          ...current,
          {
            ...attachment(request.agentId, "link", undefined, true),
            agentName:
              options.agents.find((agent) => agent.id === request.agentId)
                ?.displayName ?? request.agentId,
          },
        ];
        return (
          options.onAttach?.(request) ?? {
            skillId: request.skillId,
            agentId: request.agentId,
            agentName: "WorkBuddy",
            attachmentPath: `~/.workbuddy/skills/${request.skillId}`,
            canonicalPath: "~/.agents/skills/alpha",
            logPath: "~/Library/Application Support/com.sunven.skills-box/logs/operations.jsonl",
          }
        );
      }
      case "add_extra_agent": {
        const args = payload as {
          agent: { id: string; displayName: string; skillsDir: string };
        };
        return { ...args.agent, builtin: false };
      }
      case "remove_extra_agent": {
        const args = payload as { agentId: string };
        return {
          id: args.agentId,
          displayName: "Acme Agent",
          detached: [],
        };
      }
      default:
        throw new Error(`unexpected command: ${command}`);
    }
  });
  return calls;
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

afterEach(() => {
  clearMocks();
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

    fireEvent.change(within(dialog).getByLabelText("id"), {
      target: { value: "acme2" },
    });
    fireEvent.change(within(dialog).getByLabelText("显示名"), {
      target: { value: "Acme Two" },
    });
    fireEvent.change(within(dialog).getByLabelText("skills 目录"), {
      target: { value: "~/.acme2/skills" },
    });
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
