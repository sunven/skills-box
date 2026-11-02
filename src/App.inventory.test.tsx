import { fireEvent, render, screen, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import App from "./App";
import { createQueryClient } from "./lib/query-client";
import type {
  AppError,
  InventorySkill,
  InventorySnapshot,
} from "./lib/types";
import { createFakeBackend } from "./test/fake-backend";
import { healthyReport, cliMissingReport } from "./test/fixtures";
import {
  attachment,
  codexAgent,
  defaultAgents,
  emptySnapshot,
  group,
  makeAnomalies,
  makeSkill,
  makeSnapshot,
  sampleSnapshot,
} from "./test/inventory-fixtures";

function renderApp() {
  const queryClient = createQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>,
  );
}

function mockCommands(
  inventory: InventorySnapshot,
  options: {
    inventoryError?: Partial<AppError> & { message: string };
    env?: ReturnType<typeof healthyReport>;
  } = {},
) {
  return createFakeBackend({
    data: {
      snapshot: inventory,
      environment: options.env ?? healthyReport(),
    },
    commands: options.inventoryError
      ? {
          get_inventory: () => {
            throw options.inventoryError;
          },
        }
      : undefined,
  }).install();
}

function rows(): HTMLElement[] {
  return screen.queryAllByTestId("skill-row");
}

function rowNames(): string[] {
  return rows().map((row) => row.getAttribute("data-skill-name") ?? "");
}

function row(name: string): HTMLElement {
  const found = rows().find(
    (candidate) => candidate.getAttribute("data-skill-name") === name,
  );
  if (!found) throw new Error(`missing row ${name}`);
  return found;
}

function sidebar() {
  return within(screen.getByRole("complementary", { name: "筛选" }));
}

function sidebarButton(name: RegExp): HTMLElement {
  return sidebar().getByRole("button", { name });
}

function detail() {
  return within(screen.getByRole("complementary", { name: "详情" }));
}

describe("inventory list", () => {
  it("renders rows with description, source, orthogonal badges, avatars and time", async () => {
    mockCommands(sampleSnapshot());
    renderApp();

    expect(await screen.findByText("beta")).toBeInTheDocument();
    expect(rowNames()).toEqual(["beta", "alpha", "delta", "gamma"]);

    const alpha = within(row("alpha"));
    expect(alpha.getByText("Alpha 的单行描述")).toBeInTheDocument();
    expect(alpha.getByText("acme/skills")).toBeInTheDocument();
    expect(alpha.getByText("2026-10-01")).toBeInTheDocument();
    expect(alpha.getByText("未知")).toBeInTheDocument();
    expect(alpha.getAllByTestId("agent-avatar")).toHaveLength(2);
    expect(alpha.queryByText("未受管")).not.toBeInTheDocument();

    const delta = within(row("delta"));
    expect(delta.getByText("未知")).toBeInTheDocument();
    expect(delta.getByText("仅副本")).toBeInTheDocument();
    expect(delta.getByText("other/repo")).toBeInTheDocument();

    const gamma = within(row("gamma"));
    expect(gamma.getByText("未受管")).toBeInTheDocument();
    expect(gamma.getByText("来源未知")).toBeInTheDocument();
    // Missing description and unmanaged updated time both render as —.
    expect(gamma.getAllByText("—")).toHaveLength(2);

    // Stale lock entries never appear as rows.
    expect(screen.queryByText("ghost")).not.toBeInTheDocument();
  });

  it("caps agent avatars at five and shows the remaining count", async () => {
    const agents = Array.from({ length: 6 }, (_, index) => ({
      id: `agent-${index}`,
      displayName: `Agent ${index}`,
      universal: false,
      shared: false,
      globalSkillsDir: `~/.agent-${index}/skills`,
      origin: "catalog" as const,
      detected: true,
    }));
    const skill = makeSkill({
      name: "many-mounts",
      updatedAt: "2026-10-01T00:00:00.000Z",
      attachments: agents.map((agent) => ({
        agentId: agent.id,
        agentName: agent.displayName,
        path: `~/.agent-${agent.id}/skills/many-mounts`,
        mode: "link" as const,
        detachable: true,
      })),
    });
    mockCommands(makeSnapshot({ skills: [skill], agents }));
    renderApp();

    const mounted = within(await screen.findByTestId("skill-row"));
    expect(mounted.getAllByTestId("agent-avatar")).toHaveLength(5);
    expect(mounted.getByTestId("agent-more")).toHaveTextContent("+1");
  });

  it("searches name, description and source but not paths", async () => {
    mockCommands(sampleSnapshot());
    renderApp();
    await screen.findByText("beta");

    const search = screen.getByLabelText("搜索 skill、来源或描述");
    fireEvent.change(search, { target: { value: "beta" } });
    expect(rowNames()).toEqual(["beta"]);
    expect(screen.getByText("1 个")).toBeInTheDocument();

    fireEvent.change(search, { target: { value: "单行描述" } });
    expect(rowNames()).toEqual(["beta", "alpha"]);

    fireEvent.change(search, { target: { value: "acme/skills" } });
    expect(rowNames()).toEqual(["beta", "alpha"]);

    // Status counts stay based on the full inventory.
    expect(screen.getByRole("option", { name: "未受管 (1)" })).toBeInTheDocument();

    fireEvent.change(search, { target: { value: "~/.agents" } });
    expect(rows()).toHaveLength(0);
    expect(screen.getByText("没有匹配的 skill")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "清除筛选" })).toBeInTheDocument();
  });

  it("filters by source, agent and status, with an independent no-results state", async () => {
    mockCommands(sampleSnapshot());
    renderApp();
    await screen.findByText("beta");

    fireEvent.click(sidebarButton(/acme\/skills/));
    expect(rowNames()).toEqual(["beta", "alpha"]);
    expect(screen.getByText("2 个")).toBeInTheDocument();
    // Filter counts remain tied to the full inventory.
    expect(screen.getByRole("option", { name: "未受管 (1)" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Claude Code (4)" })).toBeInTheDocument();

    fireEvent.change(screen.getByRole("combobox", { name: "Agent 筛选" }), {
      target: { value: "gemini-cli" },
    });
    expect(rowNames()).toEqual(["beta"]);

    fireEvent.change(screen.getByRole("combobox", { name: "状态筛选" }), {
      target: { value: "unmanaged" },
    });
    expect(screen.getByText("没有匹配的 skill")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "清除筛选" }));
    expect(rowNames()).toEqual(["beta", "alpha", "delta", "gamma"]);
  });

  it("shows unmanaged, copy-only and broken-attachment rows under the anomaly filter", async () => {
    const broken = makeSkill({
      name: "broken",
      updatedAt: "2026-09-01T00:00:00.000Z",
      attachments: [attachment("claude-code", "broken")],
    });
    mockCommands(
      makeSnapshot({
        skills: [...sampleSnapshot().skills, broken],
        anomalies: makeAnomalies({
          total: 4,
          stale: group(1, ["ghost"]),
          unmanaged: group(1, ["gamma"]),
          broken: group(2, ["broken · Claude Code"]),
        }),
      }),
    );
    renderApp();
    await screen.findByText("beta");

    fireEvent.change(screen.getByRole("combobox", { name: "状态筛选" }), {
      target: { value: "anomaly" },
    });
    expect(rowNames()).toEqual(["delta", "broken", "gamma"]);
    expect(within(row("broken")).getByText("断链")).toBeInTheDocument();
    expect(within(row("delta")).getByText("仅副本")).toBeInTheDocument();
  });

  it("sorts by updated time, name and source", async () => {
    const zeta = makeSkill({
      name: "zeta",
      source: "aaa/first",
      updatedAt: "2026-08-01T00:00:00.000Z",
    });
    mockCommands(
      makeSnapshot({ skills: [...sampleSnapshot().skills, zeta] }),
    );
    renderApp();
    await screen.findByText("beta");

    expect(rowNames()).toEqual(["beta", "alpha", "delta", "zeta", "gamma"]);

    const sort = screen.getByLabelText("排序");
    fireEvent.change(sort, { target: { value: "name" } });
    expect(rowNames()).toEqual(["alpha", "beta", "delta", "gamma", "zeta"]);

    fireEvent.change(sort, { target: { value: "source" } });
    expect(rowNames()).toEqual(["zeta", "alpha", "beta", "delta", "gamma"]);
  });

  it("keeps the selected row when visible and selects the first remaining row after filtering", async () => {
    mockCommands(sampleSnapshot());
    renderApp();
    await screen.findByText("beta");

    fireEvent.click(row("alpha"));
    expect(detail().getByRole("heading", { name: "alpha" })).toBeInTheDocument();

    const search = screen.getByLabelText("搜索 skill、来源或描述");
    fireEvent.change(search, { target: { value: "beta" } });
    expect(detail().getByRole("heading", { name: "beta" })).toBeInTheDocument();

    fireEvent.change(search, { target: { value: "no-such-skill" } });
    expect(
      screen.queryByRole("complementary", { name: "详情" }),
    ).not.toBeInTheDocument();

    fireEvent.change(search, { target: { value: "" } });
    // The drawer only reopens on an explicit row click.
    expect(
      screen.queryByRole("complementary", { name: "详情" }),
    ).not.toBeInTheDocument();
  });

  it("opens the detail drawer only for a selected skill and closes it again", async () => {
    mockCommands(sampleSnapshot());
    renderApp();
    await screen.findByText("beta");
    expect(
      screen.queryByRole("complementary", { name: "详情" }),
    ).not.toBeInTheDocument();

    fireEvent.click(row("alpha"));
    expect(
      detail().getByRole("heading", { name: "alpha" }),
    ).toBeInTheDocument();
    expect(row("alpha")).toHaveAttribute("aria-pressed", "true");

    fireEvent.click(screen.getByRole("button", { name: "关闭详情" }));
    expect(
      screen.queryByRole("complementary", { name: "详情" }),
    ).not.toBeInTheDocument();
    expect(row("alpha")).toHaveAttribute("aria-pressed", "false");

    fireEvent.click(row("alpha"));
    expect(
      detail().getByRole("heading", { name: "alpha" }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("detail-scrim"));
    expect(
      screen.queryByRole("complementary", { name: "详情" }),
    ).not.toBeInTheDocument();

    fireEvent.click(row("alpha"));
    expect(
      detail().getByRole("heading", { name: "alpha" }),
    ).toBeInTheDocument();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(
      screen.queryByRole("complementary", { name: "详情" }),
    ).not.toBeInTheDocument();
  });

  it("keeps the drawer open while Escape closes a stacked dialog", async () => {
    mockCommands(sampleSnapshot());
    renderApp();
    await screen.findByText("beta");

    fireEvent.click(row("alpha"));
    fireEvent.click(await screen.findByRole("tab", { name: "操作" }));
    fireEvent.click(
      await detail().findByRole("button", { name: "彻底卸载" }),
    );
    expect(screen.getByTestId("uninstall-dialog")).toBeInTheDocument();

    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByTestId("uninstall-dialog")).not.toBeInTheDocument();
    expect(screen.getByTestId("detail-drawer")).toHaveTextContent("alpha");
  });

  it("shows independent empty states for an empty inventory", async () => {
    mockCommands(emptySnapshot());
    renderApp();

    expect(await screen.findByText("暂无 skill")).toBeInTheDocument();
    expect(
      screen.queryByRole("complementary", { name: "详情" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText(/^异常 \d+$/)).not.toBeInTheDocument();
    expect(screen.getByText("0 个")).toBeInTheDocument();
    expect(sidebar().getByText("暂无来源")).toBeInTheDocument();
  });

  it("renders all 100 rows directly without pagination", async () => {
    const skills: InventorySkill[] = Array.from({ length: 100 }, (_, index) =>
      makeSkill({
        name: `skill-${String(index).padStart(3, "0")}`,
        description: `bulk ${index}`,
      }),
    );
    mockCommands(makeSnapshot({ skills }));
    renderApp();

    await screen.findByText("skill-000");
    expect(rows()).toHaveLength(100);
    expect(screen.getByText("100 个")).toBeInTheDocument();
    expect(row("skill-099")).toBeInTheDocument();
  });

  it("keeps a row with a missing SKILL.md while other rows stay readable", async () => {
    mockCommands(
      makeSnapshot({
        skills: [
          makeSkill({
            name: "alpha",
            description: "ok",
            attachments: [attachment("claude-code")],
          }),
          makeSkill({
            name: "beta",
            description: null,
            warnings: ["未找到 SKILL.md"],
          }),
        ],
      }),
    );
    renderApp();
    await screen.findByText("alpha");

    const beta = within(row("beta"));
    expect(beta.getByText("警告")).toBeInTheDocument();
    // Missing description and absent updated time both render as —.
    expect(beta.getAllByText("—")).toHaveLength(2);
    expect(within(row("alpha")).getByText("ok")).toBeInTheDocument();
  });
});

describe("anomaly banner and modal", () => {
  it("counts stale entries, lists examples and excludes stale from the rows", async () => {
    mockCommands(sampleSnapshot());
    renderApp();
    await screen.findByText("beta");

    const banner = screen.getByRole("button", { name: /异常 3/ });
    expect(banner).toHaveTextContent("1 条 stale lock");
    expect(banner).toHaveTextContent("1 个未受管");
    expect(banner).toHaveTextContent("1 个断链");
    expect(screen.queryByText("ghost")).not.toBeInTheDocument();

    banner.focus();
    fireEvent.click(banner);
    const dialog = screen.getByRole("dialog", { name: "异常与只读提示" });
    expect(within(dialog).getByText("失效 lock 条目")).toBeInTheDocument();
    expect(within(dialog).getByText("未受管")).toBeInTheDocument();
    expect(within(dialog).getByText("断链")).toBeInTheDocument();
    expect(within(dialog).getByText("ghost")).toBeInTheDocument();
    expect(within(dialog).getByText("gamma")).toBeInTheDocument();
    expect(
      within(dialog).getByText("delta · Claude Code"),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByText(/不提供一键修复/),
    ).toBeInTheDocument();

    fireEvent.keyDown(document, { key: "Escape" });
    expect(
      screen.queryByRole("dialog", { name: "异常与只读提示" }),
    ).not.toBeInTheDocument();
    expect(document.activeElement).toBe(banner);
  });
});

describe("corrupt lock", () => {
  it.each([
    {
      label: "corrupt",
      error: {
        code: "lock.corrupt",
        message: "Skill lock 文件损坏，无法解析。",
        detail: "invalid json",
        hint: "请检查 ~/.agents/.skill-lock.json。",
        retryable: false,
      },
    },
    {
      label: "unsupported version",
      error: {
        code: "lock.unsupported_version",
        message: "Skill lock 版本不受支持（v2），需要 v3。",
        detail: "version = 2",
        hint: "请使用 skills@1.7.4 重建 lock。",
        retryable: false,
      },
    },
  ])("shows a full-screen $label error instead of an empty list", async ({ error }) => {
    mockCommands(emptySnapshot(), { inventoryError: error });
    renderApp();

    expect(
      await screen.findByRole("heading", { name: "Skill lock 无法读取" }),
    ).toBeInTheDocument();
    expect(screen.getByText(error.message)).toBeInTheDocument();
    expect(screen.getByText(/invalid json|version = 2/)).toBeInTheDocument();
    expect(screen.getByText(error.hint)).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "安装 skill" }),
    ).toBeDisabled();

    // No empty inventory, no list chrome, no mutation entry.
    expect(screen.queryByText("暂无 skill")).not.toBeInTheDocument();
    expect(screen.queryByTestId("skill-row")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("complementary", { name: "筛选" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByLabelText("搜索 skill、来源或描述"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /异常 \d/ }),
    ).not.toBeInTheDocument();
  });
});

describe("inventory source of truth", () => {
  it("keeps the read-only inventory when the CLI is missing and disables mutations", async () => {
    mockCommands(sampleSnapshot(), { env: cliMissingReport() });
    renderApp();

    // Rows still render from the native read model; only mutations are off.
    expect(await screen.findByText("beta")).toBeInTheDocument();
    expect(rowNames()).toContain("alpha");
    expect(
      screen.getByRole("button", { name: "安装 skill" }),
    ).toBeDisabled();
    expect(
      screen.getAllByText(/skills CLI 不可用/).length,
    ).toBeGreaterThan(0);
  });

  it("does not trust cli over-reporting: agent filter counts physical attachments", async () => {
    // Gemini is catalog-universal, but only alpha exists physically in its dir.
    const snapshot = makeSnapshot({
      skills: [
        makeSkill({
          name: "alpha",
          attachments: [attachment("gemini-cli")],
        }),
        makeSkill({ name: "beta", attachments: [] }),
      ],
      agents: [defaultAgents[0]!, { ...defaultAgents[1]! }, codexAgent],
    });
    mockCommands(snapshot);
    renderApp();
    await screen.findByText("alpha");

    expect(screen.getByRole("option", { name: "Gemini CLI (1)" })).toBeInTheDocument();
    fireEvent.change(screen.getByRole("combobox", { name: "Agent 筛选" }), {
      target: { value: "gemini-cli" },
    });
    expect(rowNames()).toEqual(["alpha"]);
  });
});
