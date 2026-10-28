import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it } from "vitest";
import App from "./App";
import { createQueryClient } from "./lib/query-client";
import type {
  AppError,
  InventorySnapshot,
  UpdateCheckReport,
  UpdateSkillResult,
  UpdateSourceResult,
  UpdateState,
} from "./lib/types";
import { healthyReport } from "./test/fixtures";
import { detailForSkill } from "./test/detail-fixtures";
import {
  attachment,
  makeAnomalies,
  makeSkill,
  makeSnapshot,
} from "./test/inventory-fixtures";

const LAST_SUCCESS = 1793163988000;
const LAST_SUCCESS_LABEL = "2026-10-28 05:06";

interface Call {
  command: string;
  payload: unknown;
}

function updateResult(
  name: string,
  state: UpdateState,
  overrides: Partial<UpdateSkillResult> = {},
): UpdateSkillResult {
  return {
    name,
    source: "acme/skills",
    ref: "main",
    lockHash: "a".repeat(40),
    remoteHash: state === "current" ? "a".repeat(40) : "b".repeat(40),
    state,
    cached: false,
    lastSuccessAtEpochMs: LAST_SUCCESS,
    previousState: null,
    error: null,
    note: null,
    ...overrides,
  };
}

function sourceResult(
  source: string,
  overrides: Partial<UpdateSourceResult> = {},
): UpdateSourceResult {
  return {
    source,
    ref: "main",
    status: "ok",
    skillCount: 1,
    cached: false,
    truncated: false,
    lastSuccessAtEpochMs: LAST_SUCCESS,
    error: null,
    note: null,
    ...overrides,
  };
}

function report(
  skills: UpdateSkillResult[],
  overrides: Partial<UpdateCheckReport> = {},
): UpdateCheckReport {
  return {
    skills,
    sources: [sourceResult("acme/skills", { skillCount: skills.length })],
    checkedAtEpochMs: LAST_SUCCESS,
    lastSuccessAtEpochMs: LAST_SUCCESS,
    skippedSourceCount: 0,
    cachedSourceCount: 0,
    warnings: [],
    ...overrides,
  };
}

function baseSnapshot(): InventorySnapshot {
  return makeSnapshot({
    skills: [
      makeSkill({
        name: "alpha",
        description: "Alpha 描述",
        source: "acme/skills",
        updatedAt: "2026-10-01T08:00:00.000Z",
        attachments: [attachment("claude-code")],
      }),
      makeSkill({
        name: "beta",
        description: "Beta 描述",
        source: "acme/skills",
        updatedAt: "2026-10-02T08:00:00.000Z",
        attachments: [attachment("codex", "native")],
      }),
      makeSkill({
        name: "gamma",
        description: "仅副本",
        source: "other/repo",
        management: "copy_only",
        canonicalPath: null,
        updatedAt: "2026-09-01T08:00:00.000Z",
        attachments: [attachment("claude-code", "copy")],
      }),
      makeSkill({
        name: "delta",
        description: "未受管",
        management: "unmanaged",
        canonicalPath: null,
        attachments: [attachment("claude-code", "copy")],
      }),
    ],
    anomalies: makeAnomalies(),
  });
}

function mockCommands(options: {
  snapshot?: InventorySnapshot;
  reports?: Array<UpdateCheckReport | (() => UpdateCheckReport)>;
  checkError?: Partial<AppError> & { message: string };
}): Call[] {
  const calls: Call[] = [];
  const snapshot = options.snapshot ?? baseSnapshot();
  let reportIndex = 0;
  mockIPC((command, payload) => {
    calls.push({ command, payload });
    switch (command) {
      case "get_environment":
        return healthyReport();
      case "get_inventory":
        return snapshot;
      case "get_skill_detail": {
        const skillId = (payload as { skillId: string }).skillId;
        const skill = snapshot.skills.find((entry) => entry.name === skillId);
        return skill ? detailForSkill(skill) : null;
      }
      case "check_updates": {
        if (options.checkError) throw options.checkError;
        const reports = options.reports ?? [];
        const entry = reports[Math.min(reportIndex, reports.length - 1)];
        reportIndex += 1;
        if (!entry) throw new Error("no scripted update report");
        return typeof entry === "function" ? entry() : entry;
      }
      case "open_skill_folder":
      case "open_skill_md":
        return null;
      default:
        throw new Error(`unexpected command: ${command}`);
    }
  });
  return calls;
}

function renderApp() {
  const queryClient = createQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>,
  );
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

function header() {
  return within(screen.getByRole("banner"));
}

function detail() {
  return within(screen.getByRole("complementary", { name: "详情" }));
}

function checkCalls(calls: Call[]): Call[] {
  return calls.filter((call) => call.command === "check_updates");
}

afterEach(() => {
  clearMocks();
});

describe("manual update checking", () => {
  it("does not auto-check and overlays live badges, counts and last-check time", async () => {
    const calls = mockCommands({
      reports: [
        report([
          updateResult("alpha", "update"),
          updateResult("beta", "current"),
          updateResult("gamma", "update"),
          updateResult("delta", "current"),
        ]),
      ],
    });
    renderApp();
    await screen.findByText("alpha");

    expect(checkCalls(calls)).toHaveLength(0);
    window.dispatchEvent(new Event("focus"));
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(checkCalls(calls)).toHaveLength(0);
    expect(screen.getByTestId("last-update-check")).toHaveTextContent(
      "尚未检查更新",
    );
    expect(
      within(row("alpha")).getByTestId("update-badge"),
    ).toHaveTextContent("未知");

    fireEvent.click(header().getByRole("button", { name: "检查更新" }));

    await waitFor(() => {
      expect(
        within(row("alpha")).getByTestId("update-badge"),
      ).toHaveTextContent("可更新");
    });
    expect(
      within(row("beta")).getByTestId("update-badge"),
    ).toHaveTextContent("已是最新");
    // Copy-only and unmanaged rows never inherit upstream update states.
    expect(
      within(row("gamma")).getByTestId("update-badge"),
    ).toHaveTextContent("未知");
    expect(
      within(row("delta")).getByTestId("update-badge"),
    ).toHaveTextContent("未知");

    expect(checkCalls(calls)).toHaveLength(1);
    expect(checkCalls(calls)[0]?.payload).toEqual({ skillIds: null });
    expect(screen.getByTestId("last-update-check")).toHaveTextContent(
      `上次检查：${LAST_SUCCESS_LABEL}`,
    );
    expect(screen.getByTestId("last-update-check")).toHaveAttribute(
      "title",
      expect.stringContaining("上次成功检查"),
    );
    expect(
      header().getByRole("button", { name: /更新全部 \(1\)/ }),
    ).toBeInTheDocument();

    // Sidebar counts and status filters follow the effective badges.
    expect(within(sidebarButton(/^可更新/)).getByText("1")).toBeInTheDocument();
    expect(within(sidebarButton(/^已是最新/)).getByText("1")).toBeInTheDocument();
    expect(within(sidebarButton(/^未知/)).getByText("2")).toBeInTheDocument();
    fireEvent.click(sidebarButton(/^可更新/));
    expect(rowNames()).toEqual(["alpha"]);
    fireEvent.click(sidebarButton(/^已是最新/));
    expect(rowNames()).toEqual(["beta"]);
    fireEvent.click(sidebarButton(/^未知/));
    expect(rowNames()).toEqual(["gamma", "delta"]);
  });

  it("marks path-missing and check failures as red anomalies in the modal", async () => {
    const base = baseSnapshot();
    const snapshot = {
      ...base,
      skills: [
        ...base.skills.slice(0, 2),
        makeSkill({
          name: "gamma",
          description: "Gamma 描述",
          source: "acme/skills",
          updatedAt: "2026-09-15T08:00:00.000Z",
          attachments: [attachment("claude-code")],
        }),
      ],
      anomalies: makeAnomalies(),
    };
    mockCommands({
      snapshot,
      reports: [
        report(
          [
            updateResult("alpha", "path_missing", {
              note: "来源中已找不到记录的 skillPath，可能已被删除或移动。",
              remoteHash: null,
            }),
            updateResult("beta", "error", {
              remoteHash: null,
              previousState: "update",
              error: {
                code: "network.offline",
                message: "无法连接 GitHub，已保留上次检查结果。",
                detail: "connection refused",
                hint: "请检查网络。",
                retryable: true,
              },
            }),
            updateResult("gamma", "current"),
          ],
          {
            sources: [
              sourceResult("acme/skills", {
                status: "error",
                skillCount: 2,
                error: {
                  code: "network.offline",
                  message: "无法连接 GitHub，已保留上次检查结果。",
                  retryable: true,
                },
              }),
            ],
            warnings: ["无法连接 GitHub，已保留上次检查结果。"],
          },
        ),
      ],
    });
    renderApp();
    await screen.findByText("alpha");
    fireEvent.click(header().getByRole("button", { name: "检查更新" }));

    await waitFor(() => {
      expect(
        within(row("alpha")).getByTestId("update-badge"),
      ).toHaveTextContent("源路径缺失");
    });
    const failedBadge = within(row("beta")).getByTestId("update-badge");
    expect(failedBadge).toHaveTextContent("检查失败");
    expect(failedBadge).toHaveAttribute(
      "title",
      expect.stringContaining("无法连接 GitHub"),
    );
    expect(failedBadge).toHaveAttribute(
      "title",
      expect.stringContaining("上次结果：可更新"),
    );
    expect(within(row("gamma")).getByTestId("update-badge")).toHaveTextContent(
      "已是最新",
    );

    const banner = screen.getByRole("button", { name: /异常 2/ });
    expect(banner).toHaveTextContent("1 个源路径缺失");
    expect(banner).toHaveTextContent("1 个检查失败");
    fireEvent.click(banner);

    const dialog = screen.getByRole("dialog", { name: "异常与只读提示" });
    expect(within(dialog).getByText("源路径缺失")).toBeInTheDocument();
    expect(within(dialog).getByText("检查失败")).toBeInTheDocument();
    expect(
      within(dialog).getByText("alpha · 源路径缺失"),
    ).toBeInTheDocument();
    expect(within(dialog).getByText("beta · 检查失败")).toBeInTheDocument();
    expect(
      within(dialog).getByText(/保留上次成功结果/),
    ).toBeInTheDocument();

    fireEvent.keyDown(document, { key: "Escape" });
    expect(
      screen.queryByRole("dialog", { name: "异常与只读提示" }),
    ).not.toBeInTheDocument();

    expect(within(sidebarButton(/^异常/)).getByText("2")).toBeInTheDocument();
    fireEvent.click(sidebarButton(/^异常/));
    expect(rowNames()).toEqual(["beta", "alpha"]);
  });

  it("scopes a detail check, merges the patch and recovers after a failure", async () => {
    const calls = mockCommands({
      reports: [
        report([updateResult("alpha", "update"), updateResult("beta", "current")]),
        report(
          [
            updateResult("alpha", "error", {
              previousState: "update",
              error: {
                code: "network.rate_limited",
                message: "GitHub 检查额度已用尽，已保留上次检查结果。",
                retryable: true,
              },
            }),
          ],
          {
            sources: [
              sourceResult("acme/skills", {
                status: "error",
                skillCount: 1,
                error: {
                  code: "network.rate_limited",
                  message: "GitHub 检查额度已用尽，已保留上次检查结果。",
                  retryable: true,
                },
              }),
            ],
          },
        ),
        report([updateResult("alpha", "current")]),
      ],
    });
    renderApp();
    await screen.findByText("alpha");
    fireEvent.click(header().getByRole("button", { name: "检查更新" }));
    await waitFor(() => {
      expect(
        within(row("alpha")).getByTestId("update-badge"),
      ).toHaveTextContent("可更新");
    });

    fireEvent.click(row("alpha"));
    await waitFor(() => {
      expect(
        detail().getByTestId("detail-update-badge"),
      ).toHaveTextContent("可更新");
    });
    await waitFor(() => {
      expect(
        detail().getByTestId("detail-last-update-check"),
      ).toHaveTextContent(LAST_SUCCESS_LABEL);
    });

    fireEvent.click(detail().getByRole("button", { name: "检查更新" }));
    await waitFor(() => {
      expect(
        detail().getByTestId("update-check-error"),
      ).toHaveTextContent("检查失败");
    });
    expect(detail().getByTestId("update-check-error")).toHaveTextContent(
      "上次成功检查",
    );
    expect(
      within(row("alpha")).getByTestId("update-badge"),
    ).toHaveTextContent("检查失败");
    // The scoped patch must not erase other rows from the full report.
    expect(
      within(row("beta")).getByTestId("update-badge"),
    ).toHaveTextContent("已是最新");

    fireEvent.click(detail().getByRole("button", { name: "检查更新" }));
    await waitFor(() => {
      expect(
        detail().getByTestId("detail-update-badge"),
      ).toHaveTextContent("已是最新");
    });
    expect(detail().queryByTestId("update-check-error")).not.toBeInTheDocument();

    const scoped = checkCalls(calls);
    expect(scoped).toHaveLength(3);
    expect(scoped[0]?.payload).toEqual({ skillIds: null });
    expect(scoped[1]?.payload).toEqual({ skillIds: ["alpha"] });
    expect(scoped[2]?.payload).toEqual({ skillIds: ["alpha"] });
  });

  it("summarizes update-all candidates and skipped items without mutating", async () => {
    const snapshot = makeSnapshot({
      skills: [
        makeSkill({
          name: "candidate",
          source: "acme/skills",
          attachments: [attachment("claude-code")],
        }),
        makeSkill({
          name: "modified",
          source: "acme/skills",
          localModification: "modified",
          attachments: [attachment("claude-code")],
        }),
        makeSkill({
          name: "failed",
          source: "acme/skills",
          attachments: [attachment("claude-code")],
        }),
        makeSkill({
          name: "missing",
          source: "acme/skills",
          attachments: [attachment("claude-code")],
        }),
        makeSkill({
          name: "unknown",
          source: "acme/skills",
          attachments: [attachment("claude-code")],
        }),
        makeSkill({
          name: "copy",
          source: "other/repo",
          management: "copy_only",
          canonicalPath: null,
          attachments: [attachment("claude-code", "copy")],
        }),
        makeSkill({
          name: "unmanaged",
          management: "unmanaged",
          canonicalPath: null,
          attachments: [attachment("claude-code", "copy")],
        }),
      ],
      anomalies: makeAnomalies(),
    });
    const calls = mockCommands({
      snapshot,
      reports: [
        report(
          [
            updateResult("candidate", "update"),
            updateResult("modified", "update"),
            updateResult("failed", "error", {
              error: {
                code: "network.error",
                message: "检查更新失败。",
                retryable: true,
              },
            }),
            updateResult("missing", "path_missing"),
            updateResult("unknown", "unknown"),
            updateResult("copy", "update"),
          ],
          {
            sources: [
              sourceResult("acme/skills", {
                status: "error",
                skillCount: 5,
                error: {
                  code: "network.error",
                  message: "检查更新失败。",
                  retryable: true,
                },
              }),
              sourceResult("other/repo", { skillCount: 1 }),
            ],
          },
        ),
      ],
    });
    renderApp();
    await screen.findByText("candidate");
    fireEvent.click(header().getByRole("button", { name: "检查更新" }));

    const updateAll = await header().findByRole("button", {
      name: /更新全部 \(1\)/,
    });
    fireEvent.click(updateAll);

    const dialog = screen.getByRole("dialog", { name: "更新全部" });
    const candidates = within(dialog).getByTestId("update-candidates");
    expect(candidates).toHaveTextContent("candidate");
    expect(candidates).not.toHaveTextContent("modified");
    const skipped = within(dialog).getByTestId("update-skipped");
    for (const [name, label] of [
      ["modified", "本地改动"],
      ["failed", "检查失败"],
      ["missing", "源路径缺失"],
      ["unknown", "未知"],
      ["copy", "仅副本"],
    ] as const) {
      expect(within(skipped).getAllByText(name).length).toBeGreaterThan(0);
      expect(
        within(skipped).getAllByText(new RegExp(label)).length,
      ).toBeGreaterThan(0);
    }
    expect(
      within(dialog).getByText(/更新执行（ticket 07）/),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByText(/不会调用 skills update/),
    ).toBeInTheDocument();

    fireEvent.keyDown(document, { key: "Escape" });
    expect(
      screen.queryByRole("dialog", { name: "更新全部" }),
    ).not.toBeInTheDocument();
    // Only the read-only check ran; no execution command exists yet.
    expect(calls.every((call) => !call.command.startsWith("start_"))).toBe(true);
  });

  it("shows cached results instantly and labels the tooltip", async () => {
    const calls = mockCommands({
      reports: [
        report([updateResult("alpha", "current", { cached: true })], {
          sources: [
            sourceResult("acme/skills", {
              status: "cached",
              cached: true,
              skillCount: 1,
            }),
          ],
          cachedSourceCount: 1,
        }),
      ],
    });
    renderApp();
    await screen.findByText("alpha");
    fireEvent.click(header().getByRole("button", { name: "检查更新" }));

    await waitFor(() => {
      expect(
        within(row("alpha")).getByTestId("update-badge"),
      ).toHaveTextContent("已是最新");
    });
    expect(within(row("alpha")).getByTestId("update-badge")).toHaveAttribute(
      "title",
      expect.stringContaining("缓存结果"),
    );
    expect(screen.getByTestId("last-update-check")).toHaveAttribute(
      "title",
      expect.stringContaining("1 个来源来自 6 小时缓存"),
    );
    expect(checkCalls(calls)).toHaveLength(1);
  });

  it("surfaces a command-level failure and keeps the inventory usable", async () => {
    mockCommands({
      checkError: {
        code: "lock.corrupt",
        message: "Skill lock 文件损坏，无法解析。",
        retryable: false,
      },
    });
    renderApp();
    await screen.findByText("alpha");
    fireEvent.click(header().getByRole("button", { name: "检查更新" }));

    expect(
      await screen.findByText(/更新检查失败：Skill lock 文件损坏/),
    ).toBeInTheDocument();
    expect(row("alpha")).toBeInTheDocument();
    expect(
      within(row("alpha")).getByTestId("update-badge"),
    ).toHaveTextContent("未知");
    expect(
      header().getByRole("button", { name: "检查更新" }),
    ).toBeEnabled();
  });
});
