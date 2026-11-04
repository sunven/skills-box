import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import App from "./App";
import { createQueryClient } from "./lib/query-client";
import { updateReportQueryKey } from "./lib/update-view";
import {
  createFakeBackend,
  type FakeBackend,
  type FakeBackendOptions,
} from "./test/fake-backend";
import {
  attachment,
  claudeAgent,
  makeSkill,
  makeSnapshot,
} from "./test/inventory-fixtures";
import type { BatchAction, BatchPreview, BatchRow } from "./lib/batch";
import type { ScopeId, UpdateCheckReport, UpdateCheckState } from "./lib/types";

const PROJECT: ScopeId = "project:11111111-1111-4111-8111-111111111111";
const state: UpdateCheckState = {
  version: 1,
  lastAttemptAt: Date.now(),
  lastSuccessAt: Date.now(),
  lastOutcome: "ok",
  failedSourceCount: 0,
  cachedSourceCount: 0,
  skippedSourceCount: 0,
};
const report: UpdateCheckReport = {
  skills: ["alpha", "beta", "gamma"].map((name) => ({
    name,
    source: "acme/skills",
    ref: "main",
    lockHash: "a".repeat(40),
    remoteHash: "b".repeat(40),
    state: "update",
    cached: false,
    lastSuccessAtEpochMs: Date.now(),
    previousState: null,
    error: null,
    note: null,
  })),
  sources: [],
  checkedAtEpochMs: Date.now(),
  lastSuccessAtEpochMs: Date.now(),
  skippedSourceCount: 0,
  cachedSourceCount: 0,
  warnings: [],
};
function setup(options: FakeBackendOptions = {}) {
  const backend = createFakeBackend({
    ...options,
    data: {
      snapshot: makeSnapshot({
        skills: [
          makeSkill({ name: "alpha" }),
          makeSkill({ name: "beta" }),
          makeSkill({ name: "gamma" }),
        ],
      }),
      updateReport: report,
      updateCheckState: state,
      ...options.data,
    },
  }).install();
  const client = createQueryClient();
  client.setQueryData(updateReportQueryKey("global"), options.data?.updateReport ?? report);
  render(
    <QueryClientProvider client={client}>
      <App />
    </QueryClientProvider>,
  );
  return backend;
}
async function select(name: string) {
  fireEvent.click(
    await screen.findByRole("checkbox", { name: `选择技能 ${name}` }),
  );
}
async function confirm(label: string) {
  const button = await screen.findByRole("button", { name: label });
  await waitFor(() => expect(button).toBeEnabled());
  fireEvent.click(button);
}
function row(
  name: string,
  status: BatchRow["status"],
  extra: Partial<BatchRow> = {},
): BatchRow {
  return {
    key: JSON.stringify([name, null]),
    skillId: name,
    name,
    agentId: null,
    agentName: null,
    eligible: true,
    reason: null,
    localModification: "clean",
    deletesCopy: false,
    paths: [],
    status,
    error:
      status === "failed"
        ? { message: "目标失败", code: "verify.failed", retryable: true }
        : null,
    backupPath: null,
    ...extra,
  };
}
function finish(
  backend: FakeBackend,
  action: BatchAction,
  rows: BatchRow[],
  cancelled = false,
) {
  const task = backend.lastTask!;
  act(() =>
    task.emit({
      taskId: task.taskId,
      phase: cancelled ? "cancelled" : "completed",
      completed: rows.filter(
        (row) => row.eligible && row.status !== "not_executed",
      ).length,
      total: rows.filter((row) => row.eligible).length,
      message: "批量操作已结束。",
      result: {
        action: "batch",
        scope: "global",
        overall: cancelled ? "cancelled" : "partial",
        skills: [],
        warnings: [],
        startedAt: Date.now(),
        finishedAtEpochMs: Date.now(),
        batch: { action, rows },
      },
    }),
  );
}

describe("inventory batch operations", () => {
  it("defaults uninstall to Agent selection and removes only selected attachments", async () => {
    const backend = setup({ autoTasks: true, data: { snapshot: makeSnapshot({ skills: [
      makeSkill({ name: "alpha", attachments: [attachment("claude-code"), attachment("gemini-cli")] }),
      makeSkill({ name: "beta", attachments: [attachment("gemini-cli")] }),
    ] }) } });
    await select("alpha");
    await select("beta");
    fireEvent.click(screen.getByRole("button", { name: "批量卸载" }));
    expect(screen.getByRole("radio", { name: "按 Agent 移除" })).toBeChecked();
    expect(screen.getByRole("checkbox", { name: "选择 Agent Claude Code" })).not.toBeChecked();
    expect(screen.getByRole("checkbox", { name: "选择 Agent Codex" })).toBeDisabled();
    expect(screen.getByRole("checkbox", { name: "选择 Agent Gemini CLI" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "确认移除 (0)" })).toBeDisabled();
    fireEvent.click(screen.getByRole("checkbox", { name: "选择 Agent Claude Code" }));
    const button = await screen.findByRole("button", { name: "确认移除 (1)" });
    expect(button).toBeDisabled();
    expect(screen.getByText("没有对应的 Agent 挂载。")).toBeInTheDocument();
    expect(within(screen.getByTestId("batch-dialog")).queryByText("~/.agents/skills/alpha")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("checkbox", { name: "确认按 Agent 移除" }));
    await confirm("确认移除 (1)");
    await screen.findByTestId("batch-result");
    const snapshot = await backend.dispatch("get_inventory", { scope: "global" });
    expect(snapshot.skills).toHaveLength(2);
    expect(snapshot.skills[0]!.attachments.map((a) => a.agentId)).toEqual(["gemini-cli"]);
    expect(snapshot.skills[0]!.canonicalPath).toBe("~/.agents/skills/alpha");
    expect(backend.callsOf("preview_batch").slice(-1)[0]!.payload.request.action).toBe("uninstall_agents");
  });

  it("confirms the last unmanaged copy and supports broken and copy-only attachments", async () => {
    const backend = setup({ autoTasks: true, data: { snapshot: makeSnapshot({ skills: [
      makeSkill({ name: "alpha", management: "unmanaged", canonicalPath: null,
        attachments: [attachment("claude-code", "copy", "~/.claude/skills/alpha", false)] }),
      makeSkill({ name: "beta", management: "copy_only", canonicalPath: null,
        attachments: [attachment("claude-code", "copy", "~/.claude/skills/beta", false),
          attachment("gemini-cli", "copy", "~/.gemini/skills/beta", false)] }),
      makeSkill({ name: "gamma", attachments: [attachment("claude-code", "broken", "~/.claude/skills/gamma", false)] }),
    ] }) } });
    for (const name of ["alpha", "beta", "gamma"]) await select(name);
    fireEvent.click(screen.getByRole("button", { name: "批量卸载" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "选择 Agent Claude Code" }));
    await screen.findByRole("button", { name: "确认移除 (3)" });
    expect(screen.getByText("最后一份内容，会先备份再删除。")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("checkbox", { name: "确认按 Agent 移除" }));
    expect(screen.getByRole("button", { name: "确认移除 (3)" })).toBeDisabled();
    fireEvent.click(screen.getByRole("checkbox", { name: "确认删除副本挂载" }));
    await confirm("确认移除 (3)");
    await screen.findByTestId("batch-result");
    const snapshot = await backend.dispatch("get_inventory", { scope: "global" });
    expect(snapshot.skills.map((s) => s.name)).toEqual(["beta", "gamma"]);
    expect(snapshot.skills[0]!.attachments.map((a) => a.agentId)).toEqual(["gemini-cli"]);
  });

  it("clears confirmations when changing Agents or switching uninstall mode", async () => {
    setup({ data: { snapshot: makeSnapshot({ skills: [makeSkill({ name: "alpha",
      attachments: [attachment("claude-code", "copy"), attachment("gemini-cli")] })] }) } });
    await select("alpha");
    fireEvent.click(screen.getByRole("button", { name: "批量卸载" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "选择 Agent Claude Code" }));
    await screen.findByRole("button", { name: "确认移除 (1)" });
    fireEvent.click(screen.getByRole("checkbox", { name: "确认按 Agent 移除" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "确认删除副本挂载" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "选择 Agent Gemini CLI" }));
    await screen.findByRole("button", { name: "确认移除 (2)" });
    expect(screen.getByRole("checkbox", { name: "确认按 Agent 移除" })).not.toBeChecked();
    expect(screen.getByRole("checkbox", { name: "确认删除副本挂载" })).not.toBeChecked();
    fireEvent.click(screen.getByRole("radio", { name: "完全卸载" }));
    await screen.findByRole("button", { name: "确认完全卸载 (1)" });
    expect(screen.queryByRole("checkbox", { name: "选择 Agent Claude Code" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("checkbox", { name: "确认批量卸载" }));
    fireEvent.click(screen.getByRole("radio", { name: "按 Agent 移除" }));
    await screen.findByRole("button", { name: "确认移除 (2)" });
    expect(screen.getByRole("checkbox", { name: "确认按 Agent 移除" })).not.toBeChecked();
  });

  it("ignores a late Agent preview after switching to complete uninstall", async () => {
    let release: ((preview: BatchPreview) => void) | undefined;
    let delayed: BatchPreview | undefined;
    setup({ data: { snapshot: makeSnapshot({ skills: [makeSkill({ name: "alpha",
      attachments: [attachment("claude-code")] })] }) }, commands: {
      preview_batch: async (args, backend) => {
        const preview = await backend.dispatch("preview_batch", args);
        if (args.request.action === "uninstall_agents" && args.request.agentIds.length === 1) {
          delayed = preview;
          return new Promise<BatchPreview>((resolve) => { release = resolve; });
        }
        return preview;
      },
    } });
    await select("alpha");
    fireEvent.click(screen.getByRole("button", { name: "批量卸载" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "选择 Agent Claude Code" }));
    await waitFor(() => expect(release).toBeDefined());
    fireEvent.click(screen.getByRole("radio", { name: "完全卸载" }));
    await screen.findByRole("button", { name: "确认完全卸载 (1)" });
    await act(async () => { release!(delayed!); });
    expect(screen.getByRole("button", { name: "确认完全卸载 (1)" })).toBeDisabled();
    expect(within(screen.getByTestId("batch-dialog")).getByText("~/.agents/skills/alpha")).toBeInTheDocument();
    expect(screen.queryByText("alpha · Claude Code")).not.toBeInTheDocument();
  });

  it("selects all filtered results, prunes hidden selections, retains sorting and clears on Scope switch", async () => {
    setup({
      data: {
        projects: {
          projects: [
            {
              id: PROJECT.slice(8),
              root: "/tmp/project",
              displayName: "Example",
              addedAt: 1,
              lastOpenedAt: 1,
              available: true,
            },
          ],
          lastActiveScope: "global",
          warnings: [],
        },
        snapshots: {
          [PROJECT]: makeSnapshot({
            skills: [makeSkill({ id: `${PROJECT}/alpha`, name: "alpha" })],
          }),
        },
      },
    });
    await waitFor(() =>
      expect(
        screen.getByRole("checkbox", { name: "全选当前筛选结果" }),
      ).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("checkbox", { name: "全选当前筛选结果" }));
    expect(screen.getByText("已选 3 个技能")).toBeInTheDocument();
    expect(screen.queryByTestId("detail-pane")).not.toBeInTheDocument();
    fireEvent.change(screen.getByRole("combobox", { name: "排序" }), {
      target: { value: "name" },
    });
    expect(screen.getByText("已选 3 个技能")).toBeInTheDocument();
    fireEvent.change(
      screen.getByRole("textbox", { name: "搜索 skill、来源或描述" }),
      { target: { value: "alpha" } },
    );
    await waitFor(() =>
      expect(screen.getByText("已选 1 个技能")).toBeInTheDocument(),
    );
    fireEvent.change(
      screen.getByRole("textbox", { name: "搜索 skill、来源或描述" }),
      { target: { value: "" } },
    );
    await waitFor(() =>
      expect(
        screen.getByRole("checkbox", { name: "选择技能 beta" }),
      ).not.toBeChecked(),
    );
    expect(
      screen.getByRole("checkbox", { name: "全选当前筛选结果" }),
    ).toHaveAttribute("aria-checked", "mixed");
    fireEvent.click(screen.getByRole("button", { name: "切换项目" }));
    fireEvent.click(
      await screen.findByRole("button", {
        name: "切换到项目 Example",
        hidden: true,
      }),
    );
    await waitFor(() =>
      expect(
        screen.queryByRole("toolbar", { name: "批量操作" }),
      ).not.toBeInTheDocument(),
    );
    expect(
      await screen.findByRole("checkbox", { name: "选择技能 alpha" }),
    ).not.toBeChecked();
  });

  it("previews mixed update eligibility and clears only successful selections", async () => {
    const backend = setup({
      data: {
        snapshot: makeSnapshot({
          skills: [
            makeSkill({ name: "alpha" }),
            makeSkill({ name: "beta", localModification: "modified" }),
            makeSkill({ name: "gamma", management: "unmanaged" }),
          ],
        }),
      },
    });
    await waitFor(() =>
      expect(
        screen.getByRole("checkbox", { name: "全选当前筛选结果" }),
      ).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("checkbox", { name: "全选当前筛选结果" }));
    fireEvent.click(screen.getByRole("button", { name: "批量更新" }));
    await confirm("确认更新 (1)");
    await waitFor(() => expect(backend.count("start_batch")).toBe(1));
    expect(screen.getByRole("button", { name: "切换项目" })).toBeDisabled();
    expect(
      screen.getByRole("checkbox", { name: "选择技能 beta" }),
    ).toBeDisabled();
    finish(backend, "update", [
      row("alpha", "success"),
      row("beta", "skipped", { eligible: false, reason: "本地改动" }),
      row("gamma", "skipped", { eligible: false, reason: "未受管" }),
    ]);
    await waitFor(() =>
      expect(screen.getByTestId("batch-result")).toBeInTheDocument(),
    );
    expect(
      screen.getByRole("checkbox", { name: "选择技能 alpha" }),
    ).not.toBeChecked();
    expect(
      screen.getByRole("checkbox", { name: "选择技能 beta" }),
    ).toBeChecked();
    expect(
      screen.getByRole("checkbox", { name: "选择技能 gamma" }),
    ).toBeChecked();
    expect(
      screen.queryByRole("button", { name: "重试失败及未执行项" }),
    ).not.toBeInTheDocument();
    expect(
      within(screen.getByTestId("batch-result")).getByText("本地改动"),
    ).toBeInTheDocument();
    expect(
      within(screen.getByTestId("batch-result")).getByText("未受管"),
    ).toBeInTheDocument();
  });

  it("requires deletion confirmation and rejects a changed preview without starting a task", async () => {
    const backend = setup();
    await select("alpha");
    fireEvent.click(screen.getByRole("button", { name: "批量卸载" }));
    fireEvent.click(screen.getByRole("radio", { name: "完全卸载" }));
    const button = await screen.findByRole("button", { name: "确认完全卸载 (1)" });
    await waitFor(() =>
      expect(screen.queryByText("正在核对操作目标…")).not.toBeInTheDocument(),
    );
    expect(button).toBeDisabled();
    fireEvent.click(screen.getByRole("checkbox", { name: "确认批量卸载" }));
    backend.update({
      snapshot: makeSnapshot({
        skills: [makeSkill({ name: "alpha", localModification: "modified" })],
      }),
    });
    fireEvent.click(button);
    await screen.findByText("预览已变化，请重新预览并确认。");
    await waitFor(() =>
      expect(
        screen.getByRole("checkbox", { name: "确认批量卸载" }),
      ).not.toBeChecked(),
    );
    expect(backend.tasks).toHaveLength(0);
    expect(
      screen.getByText("存在本地改动，卸载将删除这些改动。"),
    ).toBeInTheDocument();
  });

  it("previews and uninstalls unmanaged canonical and Agent-only skills after deletion confirmation", async () => {
    const backend = setup({
      autoTasks: true,
      data: {
        snapshot: makeSnapshot({
          skills: [
            makeSkill({
              name: "gitnexus-cli",
              management: "unmanaged",
              source: null,
              localModification: "unknown",
            }),
            makeSkill({
              name: "gitnexus-debugging",
              management: "unmanaged",
              source: null,
              localModification: "unknown",
              canonicalPath: null,
              attachments: [
                attachment(
                  "claude-code",
                  "copy",
                  "~/.claude/skills/gitnexus-debugging",
                ),
              ],
            }),
          ],
        }),
      },
    });
    await select("gitnexus-cli");
    await select("gitnexus-debugging");
    fireEvent.click(screen.getByRole("button", { name: "批量卸载" }));
    fireEvent.click(screen.getByRole("radio", { name: "完全卸载" }));
    const button = await screen.findByRole("button", { name: "确认完全卸载 (2)" });
    expect(button).toBeDisabled();
    expect(
      screen.getByText("~/.claude/skills/gitnexus-debugging"),
    ).toBeInTheDocument();
    expect(
      screen.getAllByText("无法确认本地改动，卸载将删除现有内容。"),
    ).toHaveLength(2);
    expect(backend.count("start_batch")).toBe(0);
    fireEvent.click(screen.getByRole("checkbox", { name: "确认批量卸载" }));
    await confirm("确认完全卸载 (2)");
    await screen.findByTestId("batch-result");
    expect(backend.count("start_batch")).toBe(1);
    expect(
      within(screen.getByTestId("batch-result")).getAllByText("已卸载"),
    ).toHaveLength(2);
    await waitFor(() =>
      expect(
        screen.queryByRole("checkbox", { name: "选择技能 gitnexus-cli" }),
      ).not.toBeInTheDocument(),
    );
  });

  it("uninstalls source-path-missing skills with their copy and legacy link paths after confirmation", async () => {
    const names = [
      "faceless-explainer",
      "hyperframes-creative",
      "hyperframes-keyframes",
      "hyperframes-core",
      "hyperframes-audio",
      "hyperframes-animation",
    ];
    const backend = setup({
      autoTasks: true,
      data: {
        snapshot: makeSnapshot({
          skills: names.map((name) => makeSkill({
            name,
            source: "heygen-com/hyperframes",
            canonicalPath: `~/.agents/skills/${name}`,
            attachments: [
              attachment("claude-code", "copy", `~/.claude/skills/${name}`),
              attachment("gemini-cli", "link", `~/.gemini/skills/${name}`),
              attachment("codex-legacy", "link", `~/.codex/skills/${name}`),
            ],
          })),
        }),
        updateReport: {
          ...report,
          skills: names.map((name) => ({
            ...report.skills[0]!,
            name,
            source: "heygen-com/hyperframes",
            state: "path_missing",
            remoteHash: null,
          })),
        },
      },
    });
    await waitFor(() => expect(screen.getAllByText("源路径缺失")).toHaveLength(6));
    fireEvent.click(screen.getByRole("checkbox", { name: "全选当前筛选结果" }));
    fireEvent.click(screen.getByRole("button", { name: "批量卸载" }));
    fireEvent.click(screen.getByRole("radio", { name: "完全卸载" }));
    const button = await screen.findByRole("button", { name: "确认完全卸载 (6)" });
    expect(button).toBeDisabled();
    for (const name of names) {
      expect(screen.getByText(`~/.codex/skills/${name}`)).toBeInTheDocument();
      expect(screen.getByText(`~/.claude/skills/${name}`)).toBeInTheDocument();
      expect(screen.getByText(`~/.gemini/skills/${name}`)).toBeInTheDocument();
    }
    expect(backend.count("start_batch")).toBe(0);
    fireEvent.click(screen.getByRole("checkbox", { name: "确认批量卸载" }));
    await confirm("确认完全卸载 (6)");
    const result = await screen.findByTestId("batch-result");
    expect(within(result).getAllByText("已卸载")).toHaveLength(6);
    expect(backend.count("start_batch")).toBe(1);
    await waitFor(() => expect(screen.queryByTestId("skill-row")).not.toBeInTheDocument());
  });

  it("defaults Agent selection to empty and confirms deleting copy attachments", async () => {
    const backend = setup({
      data: {
        snapshot: makeSnapshot({
          skills: [
            makeSkill({
              name: "alpha",
              attachments: [attachment("claude-code", "copy")],
            }),
            makeSkill({ name: "beta" }),
          ],
        }),
      },
    });
    await select("alpha");
    await select("beta");
    fireEvent.click(
      screen.getByRole("button", { name: "批量解除 Agent 挂载" }),
    );
    const agent = await screen.findByRole("checkbox", {
      name: "选择 Agent Claude Code",
    });
    expect(agent).not.toBeChecked();
    expect(
      screen.getByRole("checkbox", { name: "选择 Agent Codex" }),
    ).toBeDisabled();
    fireEvent.click(agent);
    await screen.findByRole("button", { name: "确认解除 Agent 挂载 (1)" });
    expect(
      screen.getByRole("button", { name: "确认解除 Agent 挂载 (1)" }),
    ).toBeDisabled();
    fireEvent.click(screen.getByRole("checkbox", { name: "确认删除副本挂载" }));
    await confirm("确认解除 Agent 挂载 (1)");
    await waitFor(() => expect(backend.lastTask?.command).toBe("start_batch"));
    expect(
      backend.callsOf("start_batch")[0]!.payload.confirmation.confirmedCopies,
    ).toBe(true);
  });

  it("stops later targets, retains results and retries only failed/unexecuted pairs", async () => {
    const backend = setup();
    await waitFor(() =>
      expect(
        screen.getByRole("checkbox", { name: "全选当前筛选结果" }),
      ).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("checkbox", { name: "全选当前筛选结果" }));
    fireEvent.click(screen.getByRole("button", { name: "批量更新" }));
    await confirm("确认更新 (3)");
    await waitFor(() => expect(backend.lastTask).not.toBeNull());
    const stop = screen.getAllByRole("button", { name: "停止后续操作" })[0]!;
    fireEvent.click(stop);
    await waitFor(() => expect(backend.count("cancel_task")).toBe(1));
    finish(
      backend,
      "update",
      [
        row("alpha", "success"),
        row("beta", "failed"),
        row("gamma", "not_executed"),
      ],
      true,
    );
    await screen.findByTestId("batch-result");
    expect(
      within(screen.getByTestId("batch-result")).getByText("未执行"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "重试失败及未执行项" }));
    await confirm("确认更新 (2)");
    expect(
      backend.callsOf("preview_batch").slice(-1)[0]!.payload.request.targetKeys,
    ).toEqual([
      JSON.stringify(["beta", null]),
      JSON.stringify(["gamma", null]),
    ]);
    await waitFor(() => expect(backend.count("start_batch")).toBe(2));
    finish(backend, "update", [
      row("beta", "success"),
      row("gamma", "success"),
    ]);
    await waitFor(() =>
      expect(
        within(screen.getByTestId("batch-result")).getAllByText("已更新"),
      ).toHaveLength(3),
    );
    expect(
      screen.queryByRole("toolbar", { name: "批量操作" }),
    ).not.toBeInTheDocument();
  });

  it("uses project skill identities when opening a batch", async () => {
    const backend = setup({
      data: {
        projects: {
          projects: [
            {
              id: PROJECT.slice(8),
              root: "/tmp/project",
              displayName: "Example",
              addedAt: 1,
              lastOpenedAt: 1,
              available: true,
            },
          ],
          lastActiveScope: PROJECT,
          warnings: [],
        },
        snapshots: {
          [PROJECT]: makeSnapshot({
            agents: [claudeAgent],
            skills: [makeSkill({ id: `${PROJECT}/alpha`, name: "alpha" })],
          }),
        },
        updateCheckStates: { [PROJECT]: state },
      },
    });
    await select("alpha");
    fireEvent.click(screen.getByRole("button", { name: "批量卸载" }));
    fireEvent.click(screen.getByRole("radio", { name: "完全卸载" }));
    await waitFor(() =>
      expect(backend.count("preview_batch")).toBeGreaterThan(0),
    );
    expect(backend.callsOf("preview_batch")[0]!.payload).toMatchObject({
      scope: PROJECT,
      request: { skillIds: [`${PROJECT}/alpha`] },
    });
  });
});

describe("project batch Agent targets", () => {
  it("can detach a project-only Agent absent from install targets", async () => {
    setup({
      data: {
        projects: {
          projects: [
            {
              id: PROJECT.slice(8),
              root: "/tmp/project",
              displayName: "Example",
              addedAt: 1,
              lastOpenedAt: 1,
              available: true,
            },
          ],
          lastActiveScope: PROJECT,
          warnings: [],
        },
        snapshots: {
          [PROJECT]: makeSnapshot({
            agents: [claudeAgent],
            installTargets: [],
            skills: [
              makeSkill({
                id: `${PROJECT}/alpha`,
                name: "alpha",
                attachments: [attachment("claude-code")],
              }),
            ],
          }),
        },
      },
    });
    await select("alpha");
    fireEvent.click(
      screen.getByRole("button", { name: "批量解除 Agent 挂载" }),
    );
    fireEvent.click(
      await screen.findByRole("checkbox", { name: "选择 Agent Claude Code" }),
    );
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "确认解除 Agent 挂载 (1)" }),
      ).toBeEnabled(),
    );
  });
});
