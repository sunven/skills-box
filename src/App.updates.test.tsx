import { API_COMMANDS } from "./lib/api";
import { StrictMode } from "react";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import App from "./App";
import { createQueryClient } from "./lib/query-client";
import type {
  ActiveMutationInfo,
  AppError,
  InventorySnapshot,
  UpdateCheckReport,
  UpdateCheckState,
  UpdateSkillResult,
  UpdateSourceResult,
  UpdateState,
} from "./lib/types";
import { formatEpochDateTime } from "./lib/update-view";
import {
  createFakeBackend,
  type FakeBackendData,
  type FakeCall,
  type FakeHandlers,
} from "./test/fake-backend";
import {
  attachment,
  makeAnomalies,
  makeSkill,
  makeSnapshot,
} from "./test/inventory-fixtures";

const LAST_SUCCESS = 1793163988000;
const LAST_SUCCESS_LABEL = "2026-10-28 13:06";
const RUN_AT = Date.now();
const RUN_AT_LABEL = formatEpochDateTime(RUN_AT);
const SCOPED_CHECKED_AT = RUN_AT + 60_000;
const PERSISTED_SUCCESS = LAST_SUCCESS - 60_000;
const PERSISTED_SUCCESS_LABEL = "2026-10-28 13:05";

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
    checkedAtEpochMs: RUN_AT,
    lastSuccessAtEpochMs: LAST_SUCCESS,
    skippedSourceCount: 0,
    cachedSourceCount: 0,
    warnings: [],
    ...overrides,
  };
}

function emptyReport(
  overrides: Partial<UpdateCheckReport> = {},
): UpdateCheckReport {
  return {
    skills: [],
    sources: [],
    checkedAtEpochMs: RUN_AT,
    lastSuccessAtEpochMs: LAST_SUCCESS,
    skippedSourceCount: 0,
    cachedSourceCount: 0,
    warnings: [],
    ...overrides,
  };
}

const activeUpdateMutation: ActiveMutationInfo = {
  taskId: "task-auto",
  kind: "update",
  skills: ["alpha"],
  startedAtEpochMs: LAST_SUCCESS,
  snapshotPaths: [],
};

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
  autoReport?: UpdateCheckReport | (() => UpdateCheckReport) | null;
  checkError?: Partial<AppError> & { message: string };
  initialState?: UpdateCheckState | null;
  activeMutation?: ActiveMutationInfo | null;
}): readonly FakeCall[] {
  const autoReport =
    options.autoReport === undefined ? emptyReport() : options.autoReport;
  let autoPending = autoReport !== null;
  let reportIndex = 0;
  const data: Partial<FakeBackendData> = {
    snapshot: options.snapshot ?? baseSnapshot(),
    updateCheckState: options.initialState ?? null,
    activeMutation: options.activeMutation ?? null,
    updateReport: () => {
      if (autoPending && autoReport !== null) {
        autoPending = false;
        return typeof autoReport === "function" ? autoReport() : autoReport;
      }
      const reports = options.reports ?? [];
      const entry = reports[Math.min(reportIndex, reports.length - 1)];
      reportIndex += 1;
      if (!entry) throw new Error("no scripted update report");
      return typeof entry === "function" ? entry() : entry;
    },
  };
  const commands: Partial<FakeHandlers> = options.checkError
    ? {
        check_updates: () => {
          throw options.checkError!;
        },
      }
    : {};
  return createFakeBackend({ data, commands }).install().calls;
}

function renderApp({ strict = false }: { strict?: boolean } = {}) {
  const queryClient = createQueryClient();
  const tree = (
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>
  );
  return render(strict ? <StrictMode>{tree}</StrictMode> : tree);
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

function selectStatus(value: string) {
  fireEvent.change(screen.getByRole("combobox", { name: "状态筛选" }), {
    target: { value },
  });
}

function header() {
  return within(screen.getByRole("banner"));
}

function detail() {
  return within(screen.getByRole("complementary", { name: "详情" }));
}

function checkCalls(calls: readonly FakeCall[]): readonly FakeCall[] {
  return calls.filter((call) => call.command === "check_updates");
}

describe("manual update checking", () => {
  it("auto-checks once on mount and overlays live badges, counts and last-check time", async () => {
    const calls = mockCommands({
      autoReport: report([
        updateResult("alpha", "update"),
        updateResult("beta", "current"),
        updateResult("gamma", "update"),
        updateResult("delta", "current"),
      ]),
    });
    renderApp();
    await screen.findByText("alpha");

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
    expect(checkCalls(calls)[0]?.payload).toEqual({
      skillIds: null,
      scope: "global",
    });
    expect(screen.getByTestId("last-update-check")).toHaveTextContent(
      `上次检查：${RUN_AT_LABEL}`,
    );
    expect(screen.getByTestId("last-update-check")).toHaveAttribute(
      "title",
      expect.stringContaining("上次成功检查"),
    );
    expect(screen.getByTestId("last-update-check")).toHaveAttribute(
      "title",
      expect.stringContaining("启动和每 6 小时自动检查"),
    );
    expect(
      header().getByRole("button", { name: /更新全部 \(1\)/ }),
    ).toBeInTheDocument();

    // Window focus must not start a second check.
    window.dispatchEvent(new Event("focus"));
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(checkCalls(calls)).toHaveLength(1);

    // Status counts and filters follow the effective badges.
    expect(screen.getByRole("option", { name: "可更新 (1)" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "已是最新 (1)" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "未知 (2)" })).toBeInTheDocument();
    selectStatus("update");
    expect(rowNames()).toEqual(["alpha"]);
    selectStatus("current");
    expect(rowNames()).toEqual(["beta"]);
    selectStatus("unknown");
    expect(rowNames()).toEqual(["gamma", "delta"]);
  });

  it("starts at most one automatic check under StrictMode", async () => {
    const calls = mockCommands({
      autoReport: report([updateResult("alpha", "current")]),
    });
    renderApp({ strict: true });
    await screen.findByText("alpha");

    await waitFor(() => {
      expect(
        within(row("alpha")).getByTestId("update-badge"),
      ).toHaveTextContent("已是最新");
    });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(checkCalls(calls)).toHaveLength(1);
  });

  it("skips the automatic check while a mutation is active", async () => {
    const calls = mockCommands({
      autoReport: null,
      activeMutation: activeUpdateMutation,
    });
    renderApp();
    await screen.findByText("alpha");
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(checkCalls(calls)).toHaveLength(0);
    expect(screen.getByTestId("last-update-check")).toHaveTextContent(
      "尚未检查更新",
    );
  });

  it("does not auto-check when there is no managed skill", async () => {
    const calls = mockCommands({
      autoReport: null,
      snapshot: makeSnapshot({
        skills: [
          makeSkill({
            name: "copy",
            management: "copy_only",
            canonicalPath: null,
          }),
          makeSkill({
            name: "loose",
            management: "unmanaged",
            canonicalPath: null,
          }),
        ],
      }),
    });
    renderApp();
    await screen.findByText("copy");
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(checkCalls(calls)).toHaveLength(0);
    expect(screen.getByTestId("last-update-check")).toHaveTextContent(
      "尚未检查更新",
    );
  });

  it("shows the persisted last check before a new check can start", async () => {
    const persisted: UpdateCheckState = {
      version: 1,
      lastAttemptAt: LAST_SUCCESS,
      lastSuccessAt: PERSISTED_SUCCESS,
      lastOutcome: "partial",
      failedSourceCount: 1,
      cachedSourceCount: 2,
      skippedSourceCount: 0,
    };
    mockCommands({
      autoReport: null,
      activeMutation: activeUpdateMutation,
      initialState: persisted,
    });
    renderApp();
    await screen.findByText("alpha");

    await waitFor(() => {
      expect(screen.getByTestId("last-update-check")).toHaveTextContent(
        `上次检查：${LAST_SUCCESS_LABEL}（部分失败）`,
      );
    });
    const stamp = screen.getByTestId("last-update-check");
    expect(stamp).toHaveAttribute(
      "title",
      expect.stringContaining(`上次成功检查：${PERSISTED_SUCCESS_LABEL}`),
    );
    expect(stamp).toHaveAttribute(
      "title",
      expect.stringContaining("1 个来源本次检查失败"),
    );
    expect(stamp).toHaveAttribute(
      "title",
      expect.stringContaining("2 个来源来自 6 小时缓存"),
    );
    expect(stamp).toHaveAttribute(
      "title",
      expect.stringContaining("启动和每 6 小时自动检查"),
    );
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

    expect(screen.getByRole("option", { name: "异常 (2)" })).toBeInTheDocument();
    selectStatus("anomaly");
    expect(rowNames()).toEqual(["beta", "alpha"]);
  });

  it("scopes a detail check, merges the patch and recovers after a failure", async () => {
    const calls = mockCommands({
      reports: [
        report([
          updateResult("alpha", "update"),
          updateResult("beta", "current"),
        ]),
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
            checkedAtEpochMs: SCOPED_CHECKED_AT,
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
    fireEvent.click(await screen.findByRole("tab", { name: "操作" }));
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

    fireEvent.click(detail().getByRole("tab", { name: "内容" }));
    const updateSummary = within(detail().getByTestId("detail-update-summary"));
    expect(
      updateSummary.getByRole("button", { name: "更新" }),
    ).toBeVisible();
    fireEvent.click(updateSummary.getByRole("button", { name: "检查更新" }));
    await waitFor(() => {
      expect(
        detail().getByTestId("update-check-error"),
      ).toHaveTextContent("检查失败");
    });
    expect(detail().getByTestId("update-check-error")).toBeVisible();
    expect(detail().getByTestId("detail-alerts")).toContainElement(
      detail().getByTestId("update-check-error"),
    );
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
    // Scoped checks never advance the header's full-check time.
    expect(screen.getByTestId("last-update-check")).toHaveTextContent(
      `上次检查：${RUN_AT_LABEL}`,
    );

    fireEvent.click(detail().getByRole("button", { name: "检查更新" }));
    await waitFor(() => {
      expect(
        detail().getByTestId("detail-update-badge"),
      ).toHaveTextContent("已是最新");
    });
    expect(detail().queryByTestId("update-check-error")).not.toBeInTheDocument();

    const scoped = checkCalls(calls);
    expect(scoped).toHaveLength(4);
    expect(scoped[0]?.payload).toEqual({ skillIds: null, scope: "global" });
    expect(scoped[1]?.payload).toEqual({ skillIds: null, scope: "global" });
    expect(scoped[2]?.payload).toEqual({
      skillIds: ["alpha"],
      scope: "global",
    });
    expect(scoped[3]?.payload).toEqual({
      skillIds: ["alpha"],
      scope: "global",
    });
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
    updateAll.focus();
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
      within(dialog).getByRole("button", { name: /开始更新 \(1\)/ }),
    ).toBeInTheDocument();
    expect(
      within(dialog).getAllByText(/不调用 skills update/).length,
    ).toBeGreaterThan(0);

    fireEvent.keyDown(document, { key: "Escape" });
    expect(
      screen.queryByRole("dialog", { name: "更新全部" }),
    ).not.toBeInTheDocument();
    expect(document.activeElement).toBe(updateAll);
    // Only the read-only check ran; no execution command exists yet.
    expect(calls.every((call) => call.command === API_COMMANDS.startDiscoveryScan || !call.command.startsWith("start_"))).toBe(true);
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
    expect(checkCalls(calls)).toHaveLength(2);
    expect(checkCalls(calls)[1]?.payload).toEqual({
      skillIds: null,
      scope: "global",
    });
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
