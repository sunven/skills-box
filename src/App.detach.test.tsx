import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { createQueryClient } from "./lib/query-client";
import type {
  AgentAttachment,
  DetachResult,
  InventorySnapshot,
  SkillDetail,
} from "./lib/types";
import { healthyReport } from "./test/fixtures";
import { makeDetail } from "./test/detail-fixtures";
import {
  attachment,
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

function renderApp() {
  const queryClient = createQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>,
  );
}

const attachments: AgentAttachment[] = [
  attachment("claude-code", "link", "~/.claude/skills/alpha", true),
  attachment("qoder", "copy", "~/.qoder/skills/alpha", true),
  attachment("codex", "native", "~/.agents/skills/alpha", false),
  attachment("gemini-cli", "broken", "~/.gemini/skills/alpha", false),
];

function snapshotWith(current: AgentAttachment[]): InventorySnapshot {
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

function detachResult(agentId: string, agentName: string): DetachResult {
  return {
    skillId: "alpha",
    agentId,
    agentName,
    mode: "copy",
    attachmentPath: `~/.${agentId}/skills/alpha`,
    canonicalPath: "~/.agents/skills/alpha",
    backupPath: "~/Library/Application Support/com.sunven.skills-box/backups/1-detach-alpha",
    logPath: "~/Library/Application Support/com.sunven.skills-box/logs/operations.jsonl",
    remainingAgents: [],
    unmounted: true,
  };
}

function mockCommands(options: {
  initial?: AgentAttachment[];
  onDetach?: (payload: { skillId: string; agentId: string }) => DetachResult;
  detachError?: { code: string; message: string; detail?: string; hint?: string };
}): Call[] {
  const calls: Call[] = [];
  let current = options.initial ?? attachments;
  mockIPC((command, payload) => {
    calls.push({ command, payload });
    switch (command) {
      case "get_environment":
        return healthyReport();
      case "get_inventory":
        return snapshotWith(current);
      case "get_skill_detail":
        return detailWith(current);
      case "detach_agent": {
        if (options.detachError) throw options.detachError;
        const request = payload as { skillId: string; agentId: string };
        current = current.filter(
          (entry) => entry.agentId !== request.agentId,
        );
        return options.onDetach
          ? options.onDetach(request)
          : detachResult(request.agentId, "Qoder");
      }
      default:
        throw new Error(`unexpected command: ${command}`);
    }
  });
  return calls;
}

function detailPane() {
  return within(screen.getByRole("complementary", { name: "详情" }));
}

async function selectAlpha() {
  await screen.findByText("alpha");
  const row = screen
    .queryAllByTestId("skill-row")
    .find((candidate) => candidate.getAttribute("data-skill-name") === "alpha");
  if (!row) throw new Error("missing alpha row");
  fireEvent.click(row);
  await screen.findByText("概览");
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  clearMocks();
});

describe("detach from one agent", () => {
  it("offers detach only for managed link/copy rows and confirms agent, mode and retained state", async () => {
    mockCommands({});
    renderApp();
    await selectAlpha();

    const pane = detailPane();
    expect(pane.getAllByTestId("detail-agent-row")).toHaveLength(4);
    // Native, broken and non-detachable rows stay read-only.
    expect(
      pane.getByRole("button", { name: "断开 Claude Code" }),
    ).toBeInTheDocument();
    expect(pane.getByRole("button", { name: "断开 Qoder" })).toBeInTheDocument();
    expect(
      pane.queryByRole("button", { name: /断开 Codex|断开 Gemini CLI/ }),
    ).toBeNull();
    // Ticket 09 adds a deliberate uninstall action; detach still stays scoped
    // to the agent rows and never introduces a repair action.
    expect(pane.queryByRole("button", { name: /修复/ })).toBeNull();
    expect(
      pane.getByRole("button", { name: "彻底卸载" }),
    ).toBeInTheDocument();

    const detachButton = pane.getByRole("button", {
      name: "断开 Claude Code",
    });
    detachButton.focus();
    fireEvent.click(detachButton);

    const dialog = screen.getByRole("dialog", { name: /断开 agent 挂载/ });
    expect(within(dialog).getByText("Claude Code")).toBeInTheDocument();
    expect(within(dialog).getByText("链接")).toBeInTheDocument();
    expect(
      within(dialog).getAllByText("~/.claude/skills/alpha").length,
    ).toBeGreaterThan(0);
    expect(
      within(dialog).getByText("~/.agents/skills/alpha"),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByText(/canonical 与 lock 将保留/),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByText("Qoder、Codex、Gemini CLI"),
    ).toBeInTheDocument();

    fireEvent.click(within(dialog).getByRole("button", { name: "取消" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(detachButton);

    fireEvent.click(pane.getByRole("button", { name: "断开 Qoder" }));
    const copyDialog = screen.getByRole("dialog", { name: /断开 agent 挂载/ });
    expect(within(copyDialog).getByText("副本")).toBeInTheDocument();
    expect(
      within(copyDialog).getByText(/copy 挂载断开前会创建备份/),
    ).toBeInTheDocument();
    expect(
      within(copyDialog).getByText(/被阻止/),
    ).toBeInTheDocument();
  });

  it("backs up a copy detach, refreshes detail/list and shows 未挂载 when nothing remains", async () => {
    const calls = mockCommands({ initial: [attachments[1]!] });
    renderApp();
    await selectAlpha();

    fireEvent.click(
      detailPane().getByRole("button", { name: "断开 Qoder" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "确认断开" }));

    expect(
      await screen.findByTestId("detach-result"),
    ).toHaveTextContent(/已从 Qoder 断开/);
    const notice = screen.getByTestId("detach-result");
    expect(notice).toHaveTextContent(/canonical 与 lock/);
    expect(notice).toHaveTextContent("1-detach-alpha");

    await waitFor(() => {
      expect(detailPane().queryAllByTestId("detail-agent-row")).toHaveLength(0);
      expect(detailPane().getAllByText("未挂载").length).toBeGreaterThan(0);
    });
    expect(screen.queryByRole("dialog")).toBeNull();

    const detachCalls = calls.filter((call) => call.command === "detach_agent");
    expect(detachCalls).toHaveLength(1);
    expect(detachCalls[0]!.payload).toEqual({
      skillId: "alpha",
      agentId: "qoder",
    });
    expect(Object.keys(detachCalls[0]!.payload as object)).toEqual([
      "skillId",
      "agentId",
    ]);
    expect(
      calls.filter((call) => call.command === "get_skill_detail").length,
    ).toBeGreaterThanOrEqual(2);
    expect(
      calls.filter((call) => call.command === "get_inventory").length,
    ).toBeGreaterThanOrEqual(2);
  });

  it("explains a copy dependency conflict and leaves the attachment in place", async () => {
    const calls = mockCommands({
      initial: [attachments[1]!],
      detachError: {
        code: "agent.dependency_conflict",
        message:
          "skill「alpha」的 copy 挂载仍被其他 agent 的符号链接引用，已阻止断开。",
        detail:
          "依赖此 copy 目录的符号链接：Gemini CLI（~/.gemini/skills/alpha）",
        hint: "v1 不会自动改指；请先处理这些 agent 的链接，或改用「彻底卸载」。",
      },
    });
    renderApp();
    await selectAlpha();

    fireEvent.click(
      detailPane().getByRole("button", { name: "断开 Qoder" }),
    );
    const dialog = screen.getByRole("dialog", { name: /断开 agent 挂载/ });
    fireEvent.click(within(dialog).getByRole("button", { name: "确认断开" }));

    const alert = await within(dialog).findByRole("alert");
    expect(alert).toHaveTextContent("已阻止断开");
    expect(alert).toHaveTextContent("Gemini CLI");
    expect(alert).toHaveTextContent("v1 不会自动改指");
    expect(screen.queryByTestId("detach-result")).toBeNull();
    // The dialog stays open so the user can read the explanation.
    expect(screen.getByRole("dialog", { name: /断开 agent 挂载/ })).toBeInTheDocument();

    fireEvent.click(within(dialog).getByRole("button", { name: "取消" }));
    expect(
      detailPane().getByRole("button", { name: "断开 Qoder" }),
    ).toBeInTheDocument();
    expect(detailPane().queryByText("未挂载")).toBeNull();
    expect(calls.filter((call) => call.command === "detach_agent")).toHaveLength(
      1,
    );
  });

  it("keeps copy-only and unmanaged agent-only skills read-only", async () => {
    const copyAttachment = attachment(
      "claude-code",
      "copy",
      "~/.claude/skills/beta",
      false,
    );
    mockIPC((command, payload) => {
      if (command === "get_environment") return healthyReport();
      if (command === "get_inventory") {
        return makeSnapshot({
          skills: [
            makeSkill({
              name: "beta",
              management: "copy_only",
              canonicalPath: null,
              attachments: [copyAttachment],
            }),
            makeSkill({
              name: "gamma",
              management: "unmanaged",
              source: null,
              canonicalPath: null,
              attachments: [copyAttachment],
            }),
          ],
          anomalies: makeAnomalies(),
        });
      }
      if (command === "get_skill_detail") {
        const skillId = (payload as { skillId: string }).skillId;
        return makeDetail({
          name: skillId,
          management: skillId === "beta" ? "copy_only" : "unmanaged",
          canonicalPath: null,
          directoryPath: `~/.claude/skills/${skillId}`,
          attachments: [copyAttachment],
        });
      }
      throw new Error(`unexpected command: ${command}`);
    });
    renderApp();
    await screen.findByText("beta");
    for (const name of ["beta", "gamma"]) {
      fireEvent.click(
        screen
          .queryAllByTestId("skill-row")
          .find((row) => row.getAttribute("data-skill-name") === name)!,
      );
      await waitFor(() => {
        expect(detailPane().getAllByTestId("detail-agent-row")).toHaveLength(1);
      });
      expect(
        detailPane().queryByRole("button", { name: /断开/ }),
      ).toBeNull();
    }
  });
});
