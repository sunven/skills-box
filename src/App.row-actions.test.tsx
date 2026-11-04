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
import type {
  ScopeId,
  UpdateCheckReport,
  UpdateCheckState,
  UpdateSkillResult,
} from "./lib/types";
import {
  createFakeBackend,
  type FakeBackendOptions,
} from "./test/fake-backend";
import { attachment, makeSkill, makeSnapshot } from "./test/inventory-fixtures";

const PROJECT_SCOPE: ScopeId = "project:11111111-1111-4111-8111-111111111111";

function freshCheck(): UpdateCheckState {
  return {
    version: 1,
    checkRevision: "initial",
    lastAttemptAt: Date.now(),
    lastSuccessAt: Date.now(),
    lastOutcome: "ok",
    failedSourceCount: 0,
    cachedSourceCount: 0,
    skippedSourceCount: 0,
  };
}

function updateResult(
  name: string,
  state: UpdateSkillResult["state"],
): UpdateSkillResult {
  return {
    name,
    state,
    source: "acme/skills",
    ref: "main",
    lockHash: "a".repeat(40),
    remoteHash: "b".repeat(40),
    cached: false,
    lastSuccessAtEpochMs: Date.now(),
    previousState: null,
    error: null,
    note: null,
  };
}

function report(skills: UpdateSkillResult[]): UpdateCheckReport {
  return {
    skills,
    sources: [],
    checkedAtEpochMs: Date.now(),
    lastSuccessAtEpochMs: Date.now(),
    skippedSourceCount: 0,
    cachedSourceCount: 0,
    warnings: [],
  };
}

function renderApp(
  options: FakeBackendOptions = {},
  results?: UpdateCheckReport,
) {
  const backend = createFakeBackend({
    ...options,
    data: {
      snapshot: makeSnapshot({
        skills: [makeSkill({ name: "alpha", source: "acme/skills" })],
      }),
      updateCheckState: freshCheck(),
      // Keep startup auto-checks out of these user-action scenarios.
      activeMutation: {
        taskId: "other-task",
        kind: "update",
        skills: [],
        startedAtEpochMs: Date.now(),
        snapshotPaths: [],
      },
      ...options.data,
    },
  }).install();
  const client = createQueryClient();
  if (results) client.setQueryData(updateReportQueryKey("global"), results);
  render(
    <QueryClientProvider client={client}>
      <App />
    </QueryClientProvider>,
  );
  return backend;
}

async function openMenu(name = "alpha") {
  fireEvent.click(
    await screen.findByRole("button", { name: `更多操作 ${name}` }),
  );
  return within(screen.getByRole("menu", { name: `${name} 的操作` }));
}

describe("skill row actions", () => {
  it("opens the skill folder without selecting the row and prevents repeated opens", async () => {
    let finish!: () => void;
    const backend = renderApp({
      commands: {
        open_skill_folder: () =>
          new Promise<void>((resolve) => {
            finish = resolve;
          }),
      },
    });
    const folder = await screen.findByRole("button", {
      name: "打开目录 alpha",
    });
    fireEvent.click(folder);
    fireEvent.click(folder);
    expect(folder).toBeDisabled();
    expect(
      backend.calls.filter((call) => call.command === "open_skill_folder"),
    ).toEqual([
      {
        command: "open_skill_folder",
        payload: { skillId: "alpha", scope: "global" },
      },
    ]);
    expect(screen.queryByRole("complementary", { name: "详情" })).toBeNull();
    await act(async () => finish());
    expect(folder).not.toBeDisabled();
  });

  it("reports ambiguous copy paths without choosing an attachment", async () => {
    const backend = renderApp({
      data: {
        snapshot: makeSnapshot({
          skills: [
            makeSkill({
              name: "alpha",
              management: "copy_only",
              canonicalPath: null,
              attachments: [
                attachment("claude-code", "copy"),
                attachment("qoder", "copy"),
              ],
            }),
          ],
        }),
      },
      commands: {
        open_skill_folder: () => {
          throw {
            message: "无法确定技能目录",
            detail: "存在多个 Agent 副本路径",
            retryable: false,
          };
        },
      },
    });
    fireEvent.click(
      await screen.findByRole("button", { name: "打开目录 alpha" }),
    );
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("alpha：无法确定技能目录");
    expect(alert).toHaveTextContent("存在多个 Agent 副本路径");
    expect(
      backend.calls.filter((call) => call.command === "open_skill_folder"),
    ).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "关闭操作提示" }));
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("jumps to content and attachments while retaining the existing detail and confirmation flows", async () => {
    renderApp();
    let menu = await openMenu();
    fireEvent.click(menu.getByRole("menuitem", { name: "查看内容" }));
    expect(screen.getByRole("tab", { name: "内容" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(await screen.findByTestId("content-toolbar")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "关闭详情" }));
    menu = await openMenu();
    fireEvent.click(menu.getByRole("menuitem", { name: "管理 Agent 挂载" }));
    expect(screen.getByRole("tab", { name: "操作" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByRole("region", { name: "Agent 挂载" })).toHaveFocus();
    expect(screen.getByTestId("detail-attach")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "关闭详情" }));
    fireEvent.click(screen.getByTestId("skill-row"));
    expect(screen.getByRole("tab", { name: "操作" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("shows updates only for candidates and keeps local modification confirmation", async () => {
    const backend = renderApp(
      {
        data: {
          snapshot: makeSnapshot({
            skills: [
              makeSkill({
                name: "alpha",
                source: "acme/skills",
                localModification: "modified",
              }),
              makeSkill({ name: "beta", source: "acme/skills" }),
            ],
          }),
        },
      },
      report([
        updateResult("alpha", "update"),
        updateResult("beta", "current"),
      ]),
    );
    const update = await screen.findByRole("button", { name: "更新 alpha" });
    expect(screen.queryByRole("button", { name: "更新 beta" })).toBeNull();
    fireEvent.click(update);
    expect(screen.getByTestId("update-single-confirm")).toBeInTheDocument();
    expect(
      screen.getByRole("checkbox", { name: "确认本地改动" }),
    ).not.toBeChecked();
    expect(screen.getByRole("button", { name: "开始更新" })).toBeDisabled();
    expect(
      backend.calls.filter((call) => call.command === "start_update"),
    ).toHaveLength(0);
    expect(screen.queryByRole("complementary", { name: "详情" })).toBeNull();
  });

  it("keeps uninstall behind its existing explicit confirmation", async () => {
    const backend = renderApp();
    const menu = await openMenu();
    fireEvent.click(menu.getByRole("menuitem", { name: "卸载" }));
    expect(screen.getByRole("button", { name: "确认卸载" })).toBeDisabled();
    expect(
      backend.calls.filter((call) => call.command === "start_uninstall"),
    ).toHaveLength(0);
    fireEvent.click(screen.getByRole("checkbox", { name: "确认彻底卸载" }));
    fireEvent.click(screen.getByRole("button", { name: "确认卸载" }));
    await waitFor(() =>
      expect(
        backend.calls.filter((call) => call.command === "start_uninstall"),
      ).toHaveLength(1),
    );
    expect(
      backend.calls.find((call) => call.command === "start_uninstall")?.payload,
    ).toMatchObject({
      skillId: "alpha",
      scope: "global",
    });
  });

  it.each(["unmanaged", "copy_only"] as const)(
    "explains unavailable actions for %s skills",
    async (management) => {
      const backend = renderApp({
        data: {
          snapshot: makeSnapshot({
            skills: [
              makeSkill({
                name: "alpha",
                management,
                canonicalPath: null,
                attachments: [
                  attachment("claude-code", "copy", undefined, false),
                ],
              }),
            ],
          }),
        },
      });
      const menu = await openMenu();
      const check = menu.getByRole("menuitem", { name: /^检查更新/ });
      const attachments = menu.getByRole("menuitem", {
        name: /^管理 Agent 挂载/,
      });
      expect(check).toHaveAttribute("aria-disabled", "true");
      expect(attachments).toHaveAttribute("aria-disabled", "true");
      fireEvent.click(check);
      expect(
        backend.calls.filter((call) => call.command === "check_updates"),
      ).toHaveLength(0);
      expect(menu.getByRole("menuitem", { name: /^卸载/ })).toHaveAttribute(
        "aria-disabled",
        "false",
      );
      expect(screen.queryByRole("button", { name: "更新 alpha" })).toBeNull();
    },
  );

  it("checks only the chosen skill, blocks a repeated check and reports its failure", async () => {
    let fail!: (cause: unknown) => void;
    const backend = renderApp({
      commands: {
        check_updates: () =>
          new Promise((_resolve, reject) => {
            fail = reject;
          }),
      },
    });
    let menu = await openMenu();
    fireEvent.click(menu.getByRole("menuitem", { name: "检查更新" }));
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "更多操作 alpha" }),
      ).toHaveAttribute("aria-busy", "true"),
    );
    menu = await openMenu();
    const check = menu.getByRole("menuitem", { name: /^检查更新/ });
    expect(check).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(check);
    expect(
      backend.calls.filter((call) => call.command === "check_updates"),
    ).toEqual([
      {
        command: "check_updates",
        payload: { scope: "global", skillIds: ["alpha"] },
      },
    ]);
    await act(async () => fail({ message: "网络不可用", retryable: true }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "alpha：网络不可用",
    );
  });

  it("supports menu keyboard navigation, Escape focus return and outside dismissal", async () => {
    renderApp();
    const trigger = await screen.findByRole("button", {
      name: "更多操作 alpha",
    });
    trigger.focus();
    fireEvent.keyDown(trigger, { key: "ArrowDown" });
    const menu = screen.getByRole("menu");
    expect(
      within(menu).getByRole("menuitem", { name: "查看内容" }),
    ).toHaveFocus();
    // A delayed scroll from bringing a row into view must not dismiss its
    // newly opened menu.
    fireEvent.scroll(screen.getByTestId("skill-list"));
    expect(screen.getByRole("menu")).toBe(menu);
    fireEvent.keyDown(menu, { key: "End" });
    expect(within(menu).getByRole("menuitem", { name: "卸载" })).toHaveFocus();
    fireEvent.keyDown(menu, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(trigger).toHaveFocus();
    fireEvent.click(trigger);
    fireEvent.pointerDown(screen.getByRole("heading", { name: "全局 Skills" }));
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("uses project skill identity and discards its menu and late failures when switching scope", async () => {
    let fail!: (cause: unknown) => void;
    const backend = renderApp({
      data: {
        projects: {
          projects: [
            {
              id: PROJECT_SCOPE.slice(8),
              root: "/tmp/project",
              displayName: "project",
              addedAt: 1,
              lastOpenedAt: 1,
              available: true,
              checkRevision: "initial",
            },
          ],
          lastActiveScope: PROJECT_SCOPE,
          warnings: [],
        },
        snapshots: {
          [PROJECT_SCOPE]: makeSnapshot({
            skills: [
              makeSkill({
                id: `${PROJECT_SCOPE}/alpha`,
                name: "alpha",
                canonicalPath: "/tmp/project/.agents/skills/alpha",
              }),
            ],
          }),
        },
        updateCheckStates: { [PROJECT_SCOPE]: freshCheck() },
      },
      commands: {
        open_skill_folder: () =>
          new Promise<void>((_resolve, reject) => {
            fail = reject;
          }),
      },
    });
    fireEvent.click(
      await screen.findByRole("button", { name: "打开目录 alpha" }),
    );
    expect(
      backend.calls.find((call) => call.command === "open_skill_folder")
        ?.payload,
    ).toEqual({
      scope: PROJECT_SCOPE,
      skillId: `${PROJECT_SCOPE}/alpha`,
    });
    await openMenu();
    fireEvent.click(screen.getByRole("button", { name: "切换到全局" }));
    await screen.findByRole("heading", { name: "全局 Skills" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(
      await screen.findByRole("button", { name: "打开目录 alpha" }),
    ).not.toBeDisabled();
    await act(async () =>
      fail({ message: "旧项目目录不可用", retryable: false }),
    );
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
