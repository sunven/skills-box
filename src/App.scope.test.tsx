import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { pickProjectFolder } from "./lib/folder-picker";
import { createQueryClient } from "./lib/query-client";
import type {
  ProjectEntry,
  ProjectList,
  ScopeId,
  UpdateCheckState,
} from "./lib/types";
import { GLOBAL_SCOPE } from "./lib/types";
import { createFakeBackend } from "./test/fake-backend";
import { makeSkill, makeSnapshot } from "./test/inventory-fixtures";

vi.mock("./lib/folder-picker", () => ({
  pickProjectFolder: vi.fn(),
}));

const PROJECT_ID = "11111111-1111-4111-8111-111111111111";
const PROJECT_SCOPE: ScopeId = `project:${PROJECT_ID}`;
const PROJECT_ROOT = "/tmp/skills-proj";
const PROJECT_CANONICAL = "/tmp/skills-proj/.agents/skills";

function project(overrides: Partial<ProjectEntry> = {}): ProjectEntry {
  return {
    id: PROJECT_ID,
    root: PROJECT_ROOT,
    displayName: "skills-proj",
    addedAt: 1,
    lastOpenedAt: 2,
    available: true,
    ...overrides,
  };
}

function projectList(
  projects: ProjectEntry[],
  lastActiveScope: ScopeId = GLOBAL_SCOPE,
): ProjectList {
  return { projects, lastActiveScope, warnings: [] };
}

function renderApp() {
  const queryClient = createQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>,
  );
}

function scopeRow(name: string): HTMLElement {
  return screen.getByRole("button", { name: `切换到项目 ${name}` });
}

function checkState(
  overrides: Partial<UpdateCheckState> = {},
): UpdateCheckState {
  return {
    version: 1,
    lastAttemptAt: 1793163988000,
    lastSuccessAt: 1793163988000,
    lastOutcome: "ok",
    failedSourceCount: 0,
    cachedSourceCount: 0,
    skippedSourceCount: 0,
    ...overrides,
  };
}

describe("scope switcher", () => {
  beforeEach(() => {
    vi.mocked(pickProjectFolder).mockReset();
  });

  it("restores the last active project and shows its view header", async () => {
    const backend = createFakeBackend({
      data: {
        projects: projectList([project()], PROJECT_SCOPE),
        snapshots: {
          [PROJECT_SCOPE]: makeSnapshot({
            skills: [
              makeSkill({
                id: `${PROJECT_SCOPE}/alpha`,
                name: "alpha",
                canonicalPath: `${PROJECT_CANONICAL}/alpha`,
              }),
            ],
          }),
        },
      },
    }).install();

    renderApp();

    expect(await screen.findByText("alpha")).toBeInTheDocument();
    expect(scopeRow("skills-proj")).toHaveAttribute("aria-pressed", "true");
    expect(
      screen.getByRole("button", { name: "切换到全局" }),
    ).toHaveAttribute("aria-pressed", "false");

    const header = await screen.findByTestId("project-header");
    expect(within(header).getByText("skills-proj")).toBeInTheDocument();
    expect(within(header).getByText(PROJECT_ROOT)).toBeInTheDocument();
    expect(within(header).getByText(PROJECT_CANONICAL)).toBeInTheDocument();
    expect(within(header).getByTestId("project-lock-state")).toHaveTextContent(
      "正常",
    );
    // The active scope is checked at launch when it has managed skills and no
    // recorded attempt, so the header moves from "never checked" to a time.
    await waitFor(() => {
      expect(backend.callsOf("check_updates")[0]?.payload.scope).toBe(
        PROJECT_SCOPE,
      );
    });
    await waitFor(() => {
      expect(
        within(header).getByTestId("project-last-check"),
      ).toHaveTextContent("上次检查");
    });

    const inventories = backend.callsOf("get_inventory");
    expect(inventories).toHaveLength(1);
    expect(inventories[0]?.payload.scope).toBe(PROJECT_SCOPE);
  });

  it("falls back to global when the stored project is unavailable", async () => {
    const unavailable = project({
      available: false,
      unavailableReason: "目录不存在：/tmp/skills-proj",
    });
    const backend = createFakeBackend({
      data: {
        projects: projectList([unavailable], PROJECT_SCOPE),
      },
      commands: {
        list_projects: () => projectList([unavailable], PROJECT_SCOPE),
      },
    }).install();

    renderApp();

    expect(
      await screen.findByRole("button", { name: "切换到全局" }),
    ).toHaveAttribute("aria-pressed", "true");
    const row = await screen.findByRole("button", {
      name: "切换到项目 skills-proj",
    });
    expect(row).toHaveAttribute("aria-pressed", "false");
    expect(
      within(row).getByTestId("scope-unavailable-badge"),
    ).toHaveTextContent("不可用");

    await waitFor(() => {
      expect(backend.count("get_inventory")).toBe(1);
    });
    expect(backend.callsOf("get_inventory")[0]?.payload.scope).toBe(
      GLOBAL_SCOPE,
    );
  });

  it("switches scopes through set_active_scope and resets selection, search and filters", async () => {
    const backend = createFakeBackend({
      data: {
        projects: projectList([project()], GLOBAL_SCOPE),
        snapshot: makeSnapshot({
          skills: [
            makeSkill({ name: "alpha" }),
            makeSkill({ name: "beta" }),
          ],
        }),
        snapshots: {
          [PROJECT_SCOPE]: makeSnapshot({
            skills: [
              makeSkill({
                id: `${PROJECT_SCOPE}/gamma`,
                name: "gamma",
                canonicalPath: `${PROJECT_CANONICAL}/gamma`,
              }),
            ],
          }),
        },
      },
    }).install();

    renderApp();
    await screen.findByText("alpha");
    const search = screen.getByLabelText("搜索 skill、来源或描述");
    fireEvent.change(search, { target: { value: "alpha" } });
    fireEvent.click(screen.getByTestId("skill-row"));
    expect(
      await screen.findByRole("complementary", { name: "详情" }),
    ).toBeInTheDocument();

    fireEvent.click(scopeRow("skills-proj"));

    expect(await screen.findByText("gamma")).toBeInTheDocument();
    expect(screen.getByLabelText("搜索 skill、来源或描述")).toHaveValue("");
    expect(
      screen.queryByRole("complementary", { name: "详情" }),
    ).not.toBeInTheDocument();
    expect(scopeRow("skills-proj")).toHaveAttribute("aria-pressed", "true");
    const setCalls = backend.callsOf("set_active_scope");
    expect(setCalls).toHaveLength(1);
    expect(setCalls[0]?.payload.scope).toBe(PROJECT_SCOPE);

    fireEvent.click(screen.getByTestId("skill-row"));
    await waitFor(() => {
      const details = backend.callsOf("get_skill_detail");
      expect(details.length).toBeGreaterThanOrEqual(2);
      expect(details[details.length - 1]?.payload).toEqual({
        scope: PROJECT_SCOPE,
        skillId: `${PROJECT_SCOPE}/gamma`,
      });
    });

    fireEvent.click(screen.getByRole("button", { name: "切换到全局" }));

    expect(await screen.findByText("alpha")).toBeInTheDocument();
    expect(screen.getByLabelText("搜索 skill、来源或描述")).toHaveValue("");
    expect(
      screen.queryByRole("complementary", { name: "详情" }),
    ).not.toBeInTheDocument();
    // Each scope has its own cache entry; returning to a scope reuses it
    // instead of refetching the other scope's data.
    expect(backend.callsOf("get_inventory").map((call) => call.payload.scope)).toEqual([
      GLOBAL_SCOPE,
      PROJECT_SCOPE,
    ]);
  });

  it("shows per-scope update badges, check failures and unavailable rows", async () => {
    const healthy = project();
    const otherId = "22222222-2222-4222-8222-222222222222";
    const otherScope: ScopeId = `project:${otherId}`;
    const broken = project({
      id: otherId,
      root: "/tmp/missing-proj",
      displayName: "missing-proj",
      available: false,
      unavailableReason: "目录不存在",
    });
    createFakeBackend({
      data: {
        projects: projectList([healthy, broken], GLOBAL_SCOPE),
        updateCheckStates: {
          [PROJECT_SCOPE]: checkState({ candidateCount: 2 }),
          [otherScope]: checkState({ lastOutcome: "failed" }),
        },
      },
    }).install();

    renderApp();

    const healthyRow = await screen.findByRole("button", {
      name: "切换到项目 skills-proj",
    });
    expect(
      within(healthyRow).getByTestId("scope-update-badge"),
    ).toHaveTextContent("2");
    const brokenRow = screen.getByRole("button", {
      name: "切换到项目 missing-proj",
    });
    const brokenContainer = brokenRow.parentElement as HTMLElement;
    expect(
      within(brokenContainer).getByTestId("scope-unavailable-badge"),
    ).toHaveTextContent("不可用");
    expect(
      within(brokenContainer).getByTestId("scope-error-badge"),
    ).toHaveTextContent("错误");
    expect(
      within(brokenContainer).getByRole("button", {
        name: "重新定位项目 missing-proj",
      }),
    ).toBeInTheDocument();
  });

  it("auto-checks a stale project scope on switch without rechecking the global scope", async () => {
    const backend = createFakeBackend({
      data: {
        projects: projectList([project()], GLOBAL_SCOPE),
        snapshots: {
          [PROJECT_SCOPE]: makeSnapshot({
            skills: [
              makeSkill({
                id: `${PROJECT_SCOPE}/alpha`,
                name: "alpha",
                canonicalPath: `${PROJECT_CANONICAL}/alpha`,
              }),
            ],
          }),
        },
        updateCheckStates: {
          [PROJECT_SCOPE]: checkState({
            lastAttemptAt: Date.now() - 7 * 60 * 60 * 1000,
            candidateCount: 1,
          }),
        },
      },
    }).install();

    renderApp();
    await screen.findByRole("button", { name: "切换到项目 skills-proj" });
    // Global has no managed skills, so it does not check at launch.
    expect(backend.callsOf("check_updates")).toHaveLength(0);

    fireEvent.click(scopeRow("skills-proj"));
    await waitFor(() => {
      expect(
        backend.callsOf("check_updates").map((call) => call.payload.scope),
      ).toContain(PROJECT_SCOPE);
    });
    const scopes = backend
      .callsOf("check_updates")
      .map((call) => call.payload.scope);
    expect(scopes.filter((scope) => scope === PROJECT_SCOPE)).toHaveLength(1);

    // Returning to global does not add a check for either scope.
    fireEvent.click(screen.getByRole("button", { name: "切换到全局" }));
    await screen.findByRole("button", { name: "切换到项目 skills-proj" });
    expect(backend.callsOf("check_updates")).toHaveLength(scopes.length);
  });

  it("leaves a fresh project scope alone until the user refreshes it", async () => {
    const backend = createFakeBackend({
      data: {
        projects: projectList([project()], PROJECT_SCOPE),
        snapshots: {
          [PROJECT_SCOPE]: makeSnapshot({
            skills: [
              makeSkill({
                id: `${PROJECT_SCOPE}/alpha`,
                name: "alpha",
                canonicalPath: `${PROJECT_CANONICAL}/alpha`,
              }),
            ],
          }),
        },
        updateCheckStates: {
          [PROJECT_SCOPE]: checkState({ lastAttemptAt: Date.now() }),
        },
      },
    }).install();

    renderApp();
    await screen.findByText("alpha");
    await waitFor(() => {
      expect(backend.callsOf("get_inventory").length).toBeGreaterThan(0);
    });
    // A fresh persisted attempt means switching to the scope does not check.
    expect(backend.callsOf("check_updates")).toHaveLength(0);

    fireEvent.click(
      await screen.findByRole("button", { name: "刷新项目" }),
    );
    await waitFor(() => {
      expect(backend.callsOf("check_updates")).toHaveLength(1);
    });
    expect(backend.callsOf("check_updates")[0]?.payload.scope).toBe(
      PROJECT_SCOPE,
    );
  });

  it("keeps project lock errors inside the project view and offers a retry", async () => {
    const backend = createFakeBackend({
      data: { projects: projectList([project()], PROJECT_SCOPE) },
      commands: {
        get_inventory: ({ scope }) => {
          if (scope === PROJECT_SCOPE) {
            throw {
              code: "project.lock_corrupt",
              message: "项目 lock 已损坏。",
              retryable: false,
            };
          }
          return makeSnapshot();
        },
      },
    }).install();

    renderApp();

    expect(await screen.findByText("项目读取失败")).toBeInTheDocument();
    expect(screen.getByText("项目 lock 已损坏。")).toBeInTheDocument();
    // No global lock recovery screen.
    expect(
      screen.queryByRole("heading", { name: /lock/i }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "筛选" })).toBeInTheDocument();
    const header = await screen.findByTestId("project-header");
    expect(within(header).getByTestId("project-lock-state")).toHaveTextContent(
      "锁损坏",
    );
    expect(
      within(scopeRow("skills-proj")).getByTestId("scope-error-badge"),
    ).toHaveTextContent("错误");

    const before = backend.count("get_inventory");
    fireEvent.click(screen.getByRole("button", { name: "重试读取" }));
    await waitFor(() => {
      expect(backend.count("get_inventory")).toBe(before + 1);
    });
    expect(
      backend.callsOf("get_inventory").every(
        (call) => call.payload.scope === PROJECT_SCOPE,
      ),
    ).toBe(true);
  });

  it("adds a project through the folder picker and activates it", async () => {
    vi.mocked(pickProjectFolder).mockResolvedValue("/tmp/newproj");
    const backend = createFakeBackend().install();

    renderApp();
    fireEvent.click(
      await screen.findByRole("button", { name: "添加项目" }),
    );
    const dialog = await screen.findByTestId("project-dialog");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "选择文件夹" }),
    );
    await waitFor(() => {
      expect(within(dialog).getByText("/tmp/newproj")).toBeInTheDocument();
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "添加项目" }),
    );

    expect(
      await screen.findByRole("button", { name: "切换到项目 newproj" }),
    ).toHaveAttribute("aria-pressed", "true");
    await waitFor(() => {
      expect(screen.queryByTestId("project-dialog")).not.toBeInTheDocument();
    });
    expect(backend.callsOf("add_project")[0]?.payload.root).toBe(
      "/tmp/newproj",
    );
    const header = await screen.findByTestId("project-header");
    expect(within(header).getByText("newproj")).toBeInTheDocument();
    await waitFor(() => {
      expect(
        backend.callsOf("get_inventory").some(
          (call) => call.payload.scope === "project:project-1",
        ),
      ).toBe(true);
    });
  });

  it("relocates an unavailable project without changing its id", async () => {
    vi.mocked(pickProjectFolder).mockResolvedValue("/tmp/moved-proj");
    const unavailable = project({
      available: false,
      unavailableReason: "目录不存在：/tmp/skills-proj",
    });
    const backend = createFakeBackend({
      data: { projects: projectList([unavailable], GLOBAL_SCOPE) },
    }).install();

    renderApp();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "重新定位项目 skills-proj",
      }),
    );
    const dialog = await screen.findByTestId("project-dialog");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "选择文件夹" }),
    );
    await waitFor(() => {
      expect(within(dialog).getByText("/tmp/moved-proj")).toBeInTheDocument();
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "重新定位" }),
    );

    expect(
      await screen.findByRole("button", { name: "切换到项目 moved-proj" }),
    ).toHaveAttribute("aria-pressed", "true");
    expect(backend.callsOf("relocate_project")[0]?.payload).toEqual({
      id: PROJECT_ID,
      root: "/tmp/moved-proj",
    });
    const header = await screen.findByTestId("project-header");
    expect(within(header).getByText("/tmp/moved-proj")).toBeInTheDocument();
  });

  it("forgets a project after confirming that files are not deleted", async () => {
    const backend = createFakeBackend({
      data: { projects: projectList([project()], PROJECT_SCOPE) },
    }).install();

    renderApp();
    fireEvent.click(
      await screen.findByRole("button", { name: "遗忘项目 skills-proj" }),
    );
    const dialog = await screen.findByTestId("project-dialog");
    expect(
      within(dialog).getByTestId("project-forget-copy"),
    ).toHaveTextContent("不会删除磁盘上的任何文件");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "遗忘项目" }),
    );

    await waitFor(() => {
      expect(
        screen.queryByRole("button", { name: "切换到项目 skills-proj" }),
      ).not.toBeInTheDocument();
    });
    expect(backend.callsOf("forget_project")[0]?.payload.id).toBe(PROJECT_ID);
    expect(
      screen.getByRole("button", { name: "切换到全局" }),
    ).toHaveAttribute("aria-pressed", "true");
  });

  it("renders project empty states: install CTA and unavailable root", async () => {
    createFakeBackend({
      data: {
        projects: projectList([project()], PROJECT_SCOPE),
      },
    }).install();

    renderApp();
    expect(await screen.findByText("项目暂无 skill")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("project-empty-install"));
    expect(await screen.findByTestId("install-wizard")).toBeInTheDocument();
  });

  it("shows relocate and forget actions for an unavailable active project", async () => {
    const unavailable = project({
      available: false,
      unavailableReason: "目录不存在：/tmp/skills-proj",
    });
    const backend = createFakeBackend({
      data: { projects: projectList([unavailable], GLOBAL_SCOPE) },
    }).install();

    renderApp();
    fireEvent.click(
      await screen.findByRole("button", { name: "切换到项目 skills-proj" }),
    );

    expect(await screen.findByText("项目路径不可用")).toBeInTheDocument();
    expect(screen.getByText("目录不存在：/tmp/skills-proj")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "重新定位" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "遗忘项目" }),
    ).toBeInTheDocument();
    expect(
      backend.callsOf("get_inventory").every(
        (call) => call.payload.scope !== PROJECT_SCOPE,
      ),
    ).toBe(true);
  });
});
