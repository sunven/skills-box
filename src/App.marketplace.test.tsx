import {
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
import {
  createFakeBackend,
  type FakeBackendOptions,
} from "./test/fake-backend";
import { nodeMissingReport } from "./test/fixtures";
import { marketplaceCli, marketplaceSkill } from "./test/marketplace-fixtures";
import { makeSkill, makeSnapshot } from "./test/inventory-fixtures";

function setup(options: FakeBackendOptions = {}) {
  const backend = createFakeBackend(options).install();
  render(
    <QueryClientProvider client={createQueryClient()}>
      <App />
    </QueryClientProvider>,
  );
  return backend;
}
async function openMarket() {
  fireEvent.click(await screen.findByRole("button", { name: "技能市场" }));
  await screen.findByRole("button", { name: marketplaceSkill.name });
}
async function openDetail() {
  await openMarket();
  fireEvent.click(screen.getByRole("button", { name: marketplaceSkill.name }));
  const dialog = await screen.findByRole("dialog", {
    name: marketplaceSkill.name,
  });
  await within(dialog).findByText(/平台版本：1.2.0/);
  return dialog;
}

describe("SkillHub marketplace", () => {
  it("browses without Node or SkillHub CLI and gives CLI-only guidance at installation", async () => {
    const missing = {
      code: "marketplace.cli_unavailable",
      message: "SkillHub CLI 未安装或不可用。",
      detail: "未找到 skillhub 命令",
      retryable: true,
    };
    const backend = setup({
      data: { environment: nodeMissingReport() },
      commands: {
        prepare_marketplace_install: () => {
          throw missing;
        },
        get_marketplace_cli: () => ({
          ...marketplaceCli,
          available: false,
          error: missing,
        }),
      },
    });
    const dialog = await openDetail();
    expect(backend.count("get_marketplace_cli")).toBe(0);
    fireEvent.click(within(dialog).getByLabelText("Claude Code"));
    fireEvent.click(within(dialog).getByRole("button", { name: "预览安装" }));
    expect(await within(dialog).findByText(/--cli-only/)).toBeInTheDocument();
    await waitFor(() =>
      expect(
        within(dialog).getByRole("button", { name: "重新检测" }),
      ).toBeEnabled(),
    );
    expect(backend.count("start_marketplace_install")).toBe(0);
    fireEvent.click(
      within(dialog).getByRole("button", { name: "关闭市场详情" }),
    );
    expect(screen.getByRole("button", { name: "刷新目录" })).toBeEnabled();
  });

  it("clears the dependency failure after CLI recheck and allows installation preview", async () => {
    let available = false;
    const missing = {
      code: "marketplace.cli_unavailable",
      message: "SkillHub CLI 不可用。",
      retryable: true,
    };
    const backend = setup({
      commands: {
        prepare_marketplace_install: (args, fake) => {
          if (!available) throw missing;
          return fake.dispatch("prepare_marketplace_install", args);
        },
        get_marketplace_cli: () => ({
          ...marketplaceCli,
          available,
          error: available ? null : missing,
        }),
      },
    });
    const dialog = await openDetail();
    fireEvent.click(within(dialog).getByLabelText("Claude Code"));
    fireEvent.click(within(dialog).getByRole("button", { name: "预览安装" }));
    const recheck = await within(dialog).findByRole("button", {
      name: "重新检测",
    });
    await waitFor(() => expect(recheck).toBeEnabled());
    available = true;
    fireEvent.click(recheck);
    await waitFor(() =>
      expect(
        within(dialog).queryByRole("button", { name: "重新检测" }),
      ).not.toBeInTheDocument(),
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "预览安装" }));
    expect(
      await within(dialog).findByRole("button", { name: "确认安装" }),
    ).toBeEnabled();
    expect(backend.count("start_marketplace_install")).toBe(0);
  });

  it("installs into the selected registered project without changing global inventory", async () => {
    const scope = "project:11111111-1111-4111-8111-111111111111" as const;
    const backend = setup({
      autoTasks: { delayMs: 5 },
      data: {
        projects: {
          projects: [
            {
              id: scope.slice(8),
              root: "/tmp/project",
              displayName: "Example",
              addedAt: 1,
              lastOpenedAt: 1,
              available: true,
            },
          ],
          lastActiveScope: scope,
          warnings: [],
        },
        snapshots: { [scope]: makeSnapshot({ skills: [] }) },
      },
    });
    const dialog = await openDetail();
    expect(within(dialog).getByText(/安装到 当前项目/)).toBeInTheDocument();
    fireEvent.click(within(dialog).getByLabelText("Claude Code"));
    fireEvent.click(within(dialog).getByRole("button", { name: "预览安装" }));
    fireEvent.click(
      await within(dialog).findByRole("button", { name: "确认安装" }),
    );
    await waitFor(() =>
      expect(screen.getByTestId("marketplace-result")).toHaveAttribute(
        "data-overall",
        "success",
      ),
    );
    expect(
      backend.callsOf("prepare_marketplace_install")[0].payload.scope,
    ).toBe(scope);
    expect(
      (await backend.dispatch("get_inventory", { scope })).skills[0].id,
    ).toBe(`${scope}/workspace-notes`);
    expect(
      (await backend.dispatch("get_inventory", { scope: "global" })).skills,
    ).toHaveLength(0);
  });

  it("uninstalls market skills natively without Node or SkillHub CLI", async () => {
    const backend = setup({
      autoTasks: { delayMs: 5 },
      data: {
        environment: nodeMissingReport(),
        snapshot: makeSnapshot({
          skills: [
            makeSkill({
              name: "workspace-notes",
              source: "@example/workspace-notes",
              sourceType: "skillhub",
              sourceVersion: "1.2.0",
            }),
          ],
        }),
      },
    });
    await openMarket();
    fireEvent.click(screen.getByRole("button", { name: "已安装 · 查看" }));
    fireEvent.click(await screen.findByRole("tab", { name: "内容" }));
    fireEvent.click(await screen.findByRole("button", { name: "彻底卸载" }));
    const dialog = await screen.findByRole("dialog", {
      name: "彻底卸载 · workspace-notes",
    });
    fireEvent.click(within(dialog).getByLabelText("确认彻底卸载"));
    const confirm = within(dialog).getByRole("button", { name: "确认卸载" });
    await waitFor(() => expect(confirm).toBeEnabled());
    fireEvent.click(confirm);
    await waitFor(() => expect(backend.count("start_uninstall")).toBe(1));
    await waitFor(async () =>
      expect(
        (await backend.dispatch("get_inventory", { scope: "global" })).skills,
      ).toHaveLength(0),
    );
    expect(backend.count("get_marketplace_cli")).toBe(0);
  });

  it("keeps native batch uninstall available when Node is missing", async () => {
    const backend = setup({
      data: {
        environment: nodeMissingReport(),
        snapshot: makeSnapshot({
          skills: [
            makeSkill({
              name: "market-skill",
              sourceType: "skillhub",
              source: "@publisher/slug",
            }),
          ],
        }),
      },
    });
    fireEvent.click(
      await screen.findByRole("checkbox", { name: "选择技能 market-skill" }),
    );
    const uninstall = screen.getByRole("button", { name: "批量卸载" });
    expect(uninstall).toBeEnabled();
    fireEvent.click(uninstall);
    fireEvent.click(screen.getByRole("radio", { name: "完全卸载" }));
    await waitFor(() =>
      expect(backend.count("preview_batch")).toBeGreaterThan(0),
    );
    fireEvent.click(screen.getByRole("checkbox", { name: "确认批量卸载" }));
    const confirm = await screen.findByRole("button", {
      name: "确认完全卸载 (1)",
    });
    await waitFor(() => expect(confirm).toBeEnabled());
    expect(backend.count("get_marketplace_cli")).toBe(0);
  });

  it("confirms a backend preview with multiple agents and returns a managed installation", async () => {
    const backend = setup({ autoTasks: { delayMs: 5 } });
    const dialog = await openDetail();
    fireEvent.click(within(dialog).getByLabelText("Claude Code"));
    fireEvent.click(within(dialog).getByLabelText("Codex（共享目录）"));
    fireEvent.click(within(dialog).getByRole("button", { name: "预览安装" }));
    const confirm = await within(dialog).findByRole("button", {
      name: "确认安装",
    });
    expect(
      within(dialog).getByText(/workspace-notes@1.2.0 --namespace example/),
    ).toBeInTheDocument();
    expect(backend.callsOf("prepare_marketplace_install")[0].payload).toEqual({
      scope: "global",
      coordinate: marketplaceSkill.coordinate,
      agents: ["claude-code", "codex"],
    });
    fireEvent.click(confirm);
    await waitFor(() =>
      expect(screen.getByTestId("marketplace-result")).toHaveAttribute(
        "data-overall",
        "success",
      ),
    );
    expect(
      backend.callsOf("start_marketplace_install")[0].payload.previewId,
    ).toContain("marketplace-global");
    const snapshot = await backend.dispatch("get_inventory", {
      scope: "global",
    });
    expect(snapshot.skills[0]).toMatchObject({
      name: "workspace-notes",
      source: "@example/workspace-notes",
      sourceType: "skillhub",
      sourceVersion: "1.2.0",
      management: "managed",
    });
  });

  it("retains the successful catalog with its original query label when a new search fails", async () => {
    setup({
      commands: {
        list_marketplace_skills: ({ keyword, page }) => {
          if (keyword)
            throw {
              code: "marketplace.unavailable",
              message: "请求失败",
              retryable: true,
            };
          return { skills: [marketplaceSkill], total: 21, page, pageSize: 20 };
        },
      },
    });
    await openMarket();
    fireEvent.change(screen.getByRole("textbox", { name: "搜索 SkillHub" }), {
      target: { value: "other query" },
    });
    fireEvent.click(screen.getByRole("button", { name: "搜索" }));
    expect(
      await screen.findByText(/保留上次成功结果：默认目录，第 1 页/),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: marketplaceSkill.name }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "下一页" })).toBeDisabled();
  });

  it("shows a same-name conflict rather than offering overwrite", async () => {
    const backend = setup({
      data: {
        snapshot: makeSnapshot({
          skills: [makeSkill({ name: "workspace-notes", source: "other/git" })],
        }),
      },
    });
    const dialog = await openDetail();
    expect(
      await within(dialog).findByText(/当前作用域已有同名技能/),
    ).toBeInTheDocument();
    expect(
      within(dialog).queryByRole("button", { name: "预览安装" }),
    ).not.toBeInTheDocument();
    expect(backend.count("prepare_marketplace_install")).toBe(0);
  });

  it("recognizes the full market coordinate as installed and opens its inventory", async () => {
    setup({
      data: {
        snapshot: makeSnapshot({
          skills: [
            makeSkill({
              name: "original-local-name",
              source: "@example/workspace-notes",
              sourceType: "skillhub",
              sourceVersion: "1.0.0",
            }),
          ],
        }),
      },
    });
    await openMarket();
    fireEvent.click(screen.getByRole("button", { name: "已安装 · 查看" }));
    expect(
      await screen.findByRole("heading", { name: "original-local-name" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "技能市场" }),
    ).not.toBeInTheDocument();
  });

  it("discards late search responses and requests pages with distinct keys", async () => {
    let release:
      | ((value: {
          skills: (typeof marketplaceSkill)[];
          total: number;
          page: number;
          pageSize: number;
        }) => void)
      | undefined;
    const backend = setup({
      commands: {
        list_marketplace_skills: ({ keyword, page }) => {
          if (keyword === "slow")
            return new Promise((resolve) => {
              release = resolve;
            });
          return {
            skills: [
              { ...marketplaceSkill, name: keyword || marketplaceSkill.name },
            ],
            total: 21,
            page,
            pageSize: 20,
          };
        },
      },
    });
    await openMarket();
    fireEvent.click(screen.getByRole("button", { name: "下一页" }));
    await waitFor(() =>
      expect(
        backend.callsOf("list_marketplace_skills").slice(-1)[0]?.payload.page,
      ).toBe(2),
    );
    const search = screen.getByRole("textbox", { name: "搜索 SkillHub" });
    fireEvent.change(search, { target: { value: "slow" } });
    fireEvent.click(screen.getByRole("button", { name: "搜索" }));
    await waitFor(() => expect(release).toBeDefined());
    fireEvent.change(search, { target: { value: "new result" } });
    fireEvent.click(screen.getByRole("button", { name: "搜索" }));
    await screen.findByRole("button", { name: "new result" });
    release!({
      skills: [{ ...marketplaceSkill, name: "late result" }],
      total: 1,
      page: 1,
      pageSize: 20,
    });
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "new result" }),
      ).toBeInTheDocument(),
    );
    expect(
      screen.queryByRole("button", { name: "late result" }),
    ).not.toBeInTheDocument();
  });
});
