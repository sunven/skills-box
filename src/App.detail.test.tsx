import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { openUrl } from "@tauri-apps/plugin-opener";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { createQueryClient } from "./lib/query-client";
import type {
  AgentAttachment,
  InventorySnapshot,
  SkillDetail,
} from "./lib/types";
import { healthyReport } from "./test/fixtures";
import { makeDetail } from "./test/detail-fixtures";
import {
  attachment,
  group,
  makeAnomalies,
  makeSkill,
  makeSnapshot,
} from "./test/inventory-fixtures";

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(() => Promise.resolve()),
}));

const openUrlMock = vi.mocked(openUrl);

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
  attachment("claude-code", "link", "~/.claude/skills/alpha"),
  attachment("codex", "native", "~/.agents/skills/alpha"),
  attachment("gemini-cli", "broken", "~/.gemini/skills/alpha"),
];

function baseSnapshot(
  overrides: Partial<InventorySnapshot> = {},
): InventorySnapshot {
  return makeSnapshot({
    skills: [
      makeSkill({
        name: "alpha",
        description: "Alpha 的单行描述",
        source: "acme/skills",
        updatedAt: "2026-10-01T08:00:00.000Z",
        attachments,
        warnings: ["断链：Gemini CLI"],
      }),
    ],
    anomalies: makeAnomalies({
      total: 1,
      broken: group(1, ["alpha · Gemini CLI"]),
    }),
    ...overrides,
  });
}

function baseDetail(overrides: Partial<SkillDetail> = {}): SkillDetail {
  return makeDetail({
    name: "alpha",
    description: "Alpha 的描述",
    source: "acme/skills",
    sourceType: "github",
    ref: "main",
    skillPath: "skills/alpha/SKILL.md",
    installedAt: "2026-01-01T00:00:00.000Z",
    updatedAt: "2026-02-03T04:05:06.000Z",
    canonicalPath: "~/.agents/skills/alpha",
    directoryPath: "~/.agents/skills/alpha",
    directoryMtimeEpochMs: 1793163988000,
    hash: "0123456789abcdef0123456789abcdef01234567",
    attachments,
    warnings: ["断链：Gemini CLI"],
    ...overrides,
  });
}

function mockCommands(options: {
  snapshot?: InventorySnapshot | (() => InventorySnapshot);
  detail?: SkillDetail | ((skillId: string) => SkillDetail);
  detailError?: { code: string; message: string; hint?: string };
  openError?: { code: string; message: string };
}): Call[] {
  const calls: Call[] = [];
  const snapshot = options.snapshot ?? baseSnapshot();
  mockIPC((command, payload) => {
    calls.push({ command, payload });
    switch (command) {
      case "get_environment":
        return healthyReport();
      case "get_inventory":
        return typeof snapshot === "function" ? snapshot() : snapshot;
      case "get_skill_detail": {
        if (options.detailError) throw options.detailError;
        const skillId = (payload as { skillId: string }).skillId;
        const detail = options.detail ?? baseDetail();
        return typeof detail === "function" ? detail(skillId) : detail;
      }
      case "open_skill_folder":
      case "open_skill_md":
        if (options.openError) throw options.openError;
        return null;
      default:
        throw new Error(`unexpected command: ${command}`);
    }
  });
  return calls;
}

function detailPane() {
  return within(screen.getByRole("complementary", { name: "详情" }));
}

function row(name: string): HTMLElement {
  const found = screen
    .queryAllByTestId("skill-row")
    .find((candidate) => candidate.getAttribute("data-skill-name") === name);
  if (!found) throw new Error(`missing row ${name}`);
  return found;
}

function selectAlphaSync() {
  const target = row("alpha");
  target.focus();
  fireEvent.click(target);
  return target;
}

async function selectAlpha() {
  await screen.findByText("alpha");
  const target = selectAlphaSync();
  await screen.findByText("概览");
  return target;
}

beforeEach(() => {
  openUrlMock.mockClear();
});

afterEach(() => {
  clearMocks();
});

describe("skill detail panel", () => {
  it("renders overview metadata, orthogonal badges, attachments and rendered SKILL.md", async () => {
    mockCommands({
      detail: baseDetail({
        localModification: "modified",
        localModificationDetail: "当前文件与基线指纹不一致。",
        skillMd: {
          status: "ok",
          path: "~/.agents/skills/alpha/SKILL.md",
          content:
            "---\nname: alpha\ndescription: Alpha\n---\n\n# Alpha 标题\n\n正文第一段。\n\n- 条目一\n- 条目二\n",
          sizeBytes: 120,
          limitBytes: 1024 * 1024,
          error: null,
        },
      }),
    });
    renderApp();
    await selectAlpha();

    const pane = detailPane();
    expect(
      pane.getByRole("heading", { name: "alpha" }),
    ).toBeInTheDocument();
    // Update, management and warning badges stay orthogonal.
    expect(pane.getAllByText("未知").length).toBeGreaterThan(0);
    expect(pane.getAllByText("本地改动").length).toBeGreaterThan(0);
    expect(pane.getAllByText("断链").length).toBeGreaterThan(0);
    expect(pane.getAllByText("警告").length).toBeGreaterThan(0);
    expect(
      pane.getByText(
        "检测到本地改动：共享副本与基线记录不一致。更新会覆盖修改，请先备份。",
        { exact: false },
      ),
    ).toBeInTheDocument();

    expect(pane.getByText("acme/skills")).toBeInTheDocument();
    expect(pane.getByText("· 公开 GitHub")).toBeInTheDocument();
    expect(pane.getByText("main")).toBeInTheDocument();
    expect(pane.getByText("skills/alpha/SKILL.md")).toBeInTheDocument();
    expect(pane.getByText("canonical 路径")).toBeInTheDocument();
    expect(pane.getByText("2026-01-01")).toBeInTheDocument();
    expect(pane.getByText("2026-02-03")).toBeInTheDocument();
    expect(pane.getAllByText("~/.agents/skills/alpha").length).toBeGreaterThan(
      0,
    );
    expect(pane.getByText("2026-10-28")).toBeInTheDocument();
    expect(pane.getByText("0123456789ab…")).toBeInTheDocument();
    expect(pane.queryByText("未检测到改动")).not.toBeInTheDocument();
    expect(pane.getByText("受管")).toBeInTheDocument();

    // Hash can be expanded to the full value.
    fireEvent.click(pane.getByRole("button", { name: "查看完整 hash" }));
    expect(
      pane.getByText("0123456789abcdef0123456789abcdef01234567"),
    ).toBeInTheDocument();

    // Attachment rows expose detach only for detachable link/copy mounts;
    // native/broken rows stay read-only.
    expect(pane.getAllByTestId("detail-agent-row")).toHaveLength(3);
    expect(pane.getByText("链接")).toBeInTheDocument();
    expect(pane.getByText("共享")).toBeInTheDocument();
    expect(pane.getByText("链接目标缺失；只读提示，不自动修复。")).toBeInTheDocument();
    expect(
      pane.getByRole("button", { name: "断开 Claude Code" }),
    ).toBeInTheDocument();
    expect(
      pane.queryByRole("button", { name: /^更新$|卸载|断开 Codex|断开 Gemini/ }),
    ).toBeNull();
    expect(
      pane.getByRole("button", { name: "检查更新" }),
    ).toBeInTheDocument();

    // Rendered SKILL.md hides frontmatter and renders Markdown.
    const rendered = pane.getByTestId("skill-md-rendered");
    expect(within(rendered).getByRole("heading", { name: "Alpha 标题" })).toBeInTheDocument();
    expect(within(rendered).getByText("正文第一段。")).toBeInTheDocument();
    expect(within(rendered).getByText("条目一")).toBeInTheDocument();
    expect(rendered).not.toHaveTextContent("name: alpha");
  });

  it("toggles between rendered and complete raw SKILL.md without losing focus", async () => {
    const content =
      "---\nname: alpha\ndescription: Alpha\n---\n\n# Alpha 标题\n\n正文。\n";
    mockCommands({
      detail: baseDetail({
        skillMd: {
          status: "ok",
          path: "~/.agents/skills/alpha/SKILL.md",
          content,
          sizeBytes: content.length,
          limitBytes: 1024 * 1024,
          error: null,
        },
      }),
    });
    renderApp();
    await selectAlpha();

    const pane = detailPane();
    expect(pane.getByTestId("skill-md-rendered")).toBeInTheDocument();
    const rawButton = pane.getByRole("button", { name: "原文" });
    rawButton.focus();
    fireEvent.click(rawButton);

    const raw = pane.getByTestId("skill-md-raw");
    expect(raw).toHaveTextContent("---");
    expect(raw).toHaveTextContent("name: alpha");
    expect(raw.textContent).toBe(content);
    expect(rawButton).toHaveAttribute("aria-pressed", "true");
    expect(document.activeElement).toBe(rawButton);

    const renderedButton = pane.getByRole("button", { name: "渲染" });
    fireEvent.click(renderedButton);
    expect(pane.getByTestId("skill-md-rendered")).toBeInTheDocument();
    expect(pane.queryByTestId("skill-md-raw")).toBeNull();
    expect(renderedButton).toHaveAttribute("aria-pressed", "true");
  });

  it("does not execute raw HTML, load remote images, or open non-https links", async () => {
    const content = [
      "# Alpha",
      "",
      "<script>window.__skills_box_xss = true</script>",
      '<img src="https://evil.test/raw.png" onerror="window.__skills_box_xss = true" />',
      "",
      "[HTTPS 文档](https://example.com/docs)",
      "[HTTP 链接](http://example.com/insecure)",
      "[JS 链接](javascript:alert(1))",
      "",
      "![架构图](https://example.com/diagram.png)",
      "![不安全图](http://example.com/bad.png)",
    ].join("\n");
    mockCommands({
      detail: baseDetail({
        skillMd: {
          status: "ok",
          path: "~/.agents/skills/alpha/SKILL.md",
          content,
          sizeBytes: content.length,
          limitBytes: 1024 * 1024,
          error: null,
        },
      }),
    });
    renderApp();
    await selectAlpha();

    const pane = detailPane();
    const rendered = pane.getByTestId("skill-md-rendered");
    expect(rendered.querySelector("script")).toBeNull();
    expect(rendered.querySelector("img")).toBeNull();
    expect(
      (window as unknown as { __skills_box_xss?: boolean }).__skills_box_xss,
    ).toBeUndefined();
    expect(rendered).not.toHaveTextContent("window.__skills_box_xss");

    const httpsLink = within(rendered).getByRole("link", { name: "HTTPS 文档" });
    expect(httpsLink).toHaveAttribute("href", "https://example.com/docs");
    fireEvent.click(httpsLink);
    expect(openUrlMock).toHaveBeenCalledWith("https://example.com/docs");

    // Non-https links render as plain text, never as openable links.
    expect(
      within(rendered).queryByRole("link", { name: "HTTP 链接" }),
    ).toBeNull();
    expect(
      within(rendered).queryByRole("link", { name: "JS 链接" }),
    ).toBeNull();

    // Remote images become links/placeholders; nothing loads them.
    expect(within(rendered).getByText(/架构图/)).toBeInTheDocument();
    expect(within(rendered).getByText(/不安全图/)).toBeInTheDocument();
    const imageLink = within(rendered).getByRole("link", { name: "打开图片" });
    expect(imageLink).toHaveAttribute("href", "https://example.com/diagram.png");
    fireEvent.click(imageLink);
    expect(openUrlMock).toHaveBeenCalledWith("https://example.com/diagram.png");
    expect(openUrlMock).toHaveBeenCalledTimes(2);
    expect(rendered.querySelector("img")).toBeNull();
  });

  it("shows a clear warning when SKILL.md is missing", async () => {
    mockCommands({
      detail: baseDetail({
        skillMd: {
          status: "missing",
          path: "~/.agents/skills/alpha/SKILL.md",
          content: null,
          sizeBytes: null,
          limitBytes: 1024 * 1024,
          error: null,
        },
        warnings: ["未找到 SKILL.md"],
      }),
    });
    renderApp();
    await selectAlpha();
    const pane = detailPane();
    expect(
      pane.getByText(
        "未找到 SKILL.md。该 skill 目录可能不完整；不会渲染内容。",
      ),
    ).toBeInTheDocument();
    expect(pane.getByTestId("detail-warnings")).toHaveTextContent(
      "未找到 SKILL.md",
    );
    expect(pane.queryByRole("button", { name: "原文" })).toBeNull();
  });

  it("refuses to load SKILL.md over 1 MiB and shows unreadable errors", async () => {
    mockCommands({
      detail: baseDetail({
        skillMd: {
          status: "too_large",
          path: "~/.agents/skills/alpha/SKILL.md",
          content: null,
          sizeBytes: 1024 * 1024 + 1,
          limitBytes: 1024 * 1024,
          error: null,
        },
        warnings: ["SKILL.md 超过 1 MiB，未加载内容。"],
      }),
    });
    renderApp();
    await selectAlpha();
    expect(
      detailPane().getAllByText(/SKILL.md 超过 1 MiB/).length,
    ).toBeGreaterThan(0);
    expect(
      detailPane().queryByRole("button", { name: "原文" }),
    ).toBeNull();
  });

  it("shows an unreadable SKILL.md error", async () => {
    mockCommands({
      detail: baseDetail({
        skillMd: {
          status: "unreadable",
          path: "~/.agents/skills/alpha/SKILL.md",
          content: null,
          sizeBytes: 3,
          limitBytes: 1024 * 1024,
          error: "SKILL.md 不是有效的 UTF-8",
        },
        warnings: ["无法读取 SKILL.md：SKILL.md 不是有效的 UTF-8"],
      }),
    });
    renderApp();
    await selectAlpha();
    expect(
      detailPane().getAllByText(
        /无法读取 SKILL.md：SKILL.md 不是有效的 UTF-8/,
      ).length,
    ).toBeGreaterThan(0);
    expect(detailPane().queryByTestId("skill-md-rendered")).toBeNull();
  });

  it("warns about malformed frontmatter but still hides the block in the rendered view", async () => {
    const content = "---\ndescription: \"unterminated\n---\n# Alpha\n";
    mockCommands({
      detail: baseDetail({
        skillMd: {
          status: "ok",
          path: "~/.agents/skills/alpha/SKILL.md",
          content,
          sizeBytes: content.length,
          limitBytes: 1024 * 1024,
          error: null,
        },
        warnings: ['SKILL.md frontmatter 解析失败：description 的引号未闭合'],
      }),
    });
    renderApp();
    await selectAlpha();

    const pane = detailPane();
    expect(pane.getByTestId("detail-warnings")).toHaveTextContent("frontmatter");
    expect(
      pane.getByText(
        "渲染视图可能无法完整隐藏 frontmatter：解析失败，原文可查看完整内容。",
      ),
    ).toBeInTheDocument();
    expect(pane.getByTestId("skill-md-rendered")).not.toHaveTextContent(
      "description:",
    );
    expect(pane.getByTestId("skill-md-rendered")).toHaveTextContent("Alpha");

    fireEvent.click(pane.getByRole("button", { name: "原文" }));
    expect(pane.getByTestId("skill-md-raw").textContent).toBe(content);
  });

  it("shows local-modification badges and banners, and keeps unmanaged read-only", async () => {
    const modified = makeSkill({
      name: "alpha",
      localModification: "modified",
      attachments: [attachment("claude-code")],
    });
    const unknown = makeSkill({
      name: "beta",
      localModification: "unknown",
      attachments: [attachment("claude-code")],
    });
    const unmanaged = makeSkill({
      name: "gamma",
      management: "unmanaged",
      source: null,
      localModification: "unknown",
      attachments: [attachment("claude-code", "copy")],
      canonicalPath: null,
    });
    const copyOnly = makeSkill({
      name: "delta",
      management: "copy_only",
      localModification: "clean",
      canonicalPath: null,
      attachments: [attachment("claude-code", "copy")],
    });
    mockCommands({
      snapshot: makeSnapshot({
        skills: [modified, unknown, unmanaged, copyOnly],
        anomalies: makeAnomalies({
          total: 4,
          unmanaged: group(1, ["gamma"]),
          modified: group(2, ["alpha · 本地改动", "beta · 本地改动?"]),
        }),
      }),
      detail: (skillId) =>
        makeDetail({
          name: skillId,
          management:
            skillId === "gamma"
              ? "unmanaged"
              : skillId === "delta"
                ? "copy_only"
                : "managed",
          canonicalPath:
            skillId === "gamma" || skillId === "delta"
              ? null
              : `~/.agents/skills/${skillId}`,
          directoryPath:
            skillId === "gamma" || skillId === "delta"
              ? `~/.claude/skills/${skillId}`
              : `~/.agents/skills/${skillId}`,
          localModification:
            skillId === "alpha"
              ? "modified"
              : skillId === "beta" || skillId === "gamma"
                ? "unknown"
                : "clean",
          localModificationDetail:
            skillId === "alpha"
              ? "当前文件与基线指纹不一致。"
              : skillId === "beta"
                ? "lock hash 与当前目录不一致；首次扫描已记录基线但尚未校验。"
                : null,
          attachments: [attachment("claude-code", "copy", undefined, false)],
          warnings: [],
        }),
    });
    renderApp();
    await screen.findByText("alpha");

    expect(within(row("alpha")).getByTestId("local-modification-badge")).toHaveTextContent("本地改动");
    expect(within(row("beta")).getByTestId("local-modification-badge")).toHaveTextContent("本地改动?");
    expect(within(row("gamma")).queryByTestId("local-modification-badge")).toBeNull();
    expect(within(row("delta")).queryByTestId("local-modification-badge")).toBeNull();

    row("alpha").focus();
    fireEvent.click(row("alpha"));
    expect(
      await detailPane().findByText(/检测到本地改动：共享副本与基线记录不一致/),
    ).toBeInTheDocument();

    fireEvent.click(row("beta"));
    expect(
      await detailPane().findByText(/无法确认本地改动/),
    ).toBeInTheDocument();
    expect(detailPane().getAllByText("本地改动?").length).toBeGreaterThan(0);

    fireEvent.click(row("gamma"));
    await waitFor(() => {
      expect(
        detailPane().getByText("未受管：该目录没有 lock 条目，v1 只读展示，不提供更新或卸载。"),
      ).toBeInTheDocument();
    });
    expect(detailPane().getByRole("button", { name: "在 Finder 中查看" })).toBeInTheDocument();
    expect(detailPane().queryByRole("button", { name: "默认编辑器打开" })).toBeNull();
    expect(detailPane().queryByText("本地改动?")).toBeNull();
    expect(detailPane().getByText("~/.claude/skills/gamma")).toBeInTheDocument();
    expect(detailPane().getByText("canonical 路径")).toBeInTheDocument();

    fireEvent.click(row("delta"));
    await waitFor(() => {
      expect(detailPane().getByText(/仅副本：没有 canonical 目录/)).toBeInTheDocument();
    });
    expect(detailPane().getByRole("button", { name: "在 Finder 中查看" })).toBeInTheDocument();
    // copy-only keeps Finder only until uninstall lands in ticket 09.
    expect(detailPane().queryByRole("button", { name: "默认编辑器打开" })).toBeNull();
    expect(detailPane().getByText("agent 路径")).toBeInTheDocument();
    expect(
      detailPane().queryByRole("button", { name: /检查更新|^更新$|卸载|断开/ }),
    ).toBeNull();

    // The anomaly filter includes modified and unknown local changes.
    fireEvent.click(screen.getByRole("button", { name: /^异常/ }));
    await waitFor(() => {
      expect(
        screen
          .queryAllByTestId("skill-row")
          .map((element) => element.getAttribute("data-skill-name")),
      ).toEqual(["alpha", "beta", "gamma"]);
    });
  });

  it("extends the anomaly dialog with the modified category", async () => {
    mockCommands({
      snapshot: baseSnapshot({
        anomalies: makeAnomalies({
          total: 4,
          stale: group(1, ["ghost"]),
          unmanaged: group(1, ["gamma"]),
          broken: group(1, ["alpha · Gemini CLI"]),
          modified: group(1, ["alpha · 本地改动"]),
        }),
      }),
    });
    renderApp();
    await screen.findByText("alpha");

    const banner = screen.getByRole("button", { name: /异常 4/ });
    expect(banner).toHaveTextContent("1 个本地改动");
    fireEvent.click(banner);

    const dialog = screen.getByRole("dialog", { name: "异常与只读提示" });
    expect(within(dialog).getByText("本地改动")).toBeInTheDocument();
    expect(within(dialog).getByText("alpha · 本地改动")).toBeInTheDocument();
    expect(within(dialog).getByText(/本地改动\?/)).toBeInTheDocument();
  });

  it("opens Finder and the editor through skillId-only commands and keeps focus", async () => {
    const calls = mockCommands({});
    renderApp();
    const selectedRow = await selectAlpha();
    expect(document.activeElement).toBe(selectedRow);

    const pane = detailPane();
    const finder = pane.getByRole("button", { name: "在 Finder 中查看" });
    finder.focus();
    fireEvent.click(finder);
    await waitFor(() => {
      expect(calls.some((call) => call.command === "open_skill_folder")).toBe(true);
    });
    const folderCall = calls.find((call) => call.command === "open_skill_folder")!;
    expect(folderCall.payload).toEqual({ skillId: "alpha" });
    expect(Object.keys(folderCall.payload as object)).toEqual(["skillId"]);
    expect(document.activeElement).toBe(finder);

    const editor = pane.getByRole("button", { name: "默认编辑器打开" });
    editor.focus();
    fireEvent.click(editor);
    await waitFor(() => {
      expect(calls.some((call) => call.command === "open_skill_md")).toBe(true);
    });
    const mdCall = calls.find((call) => call.command === "open_skill_md")!;
    expect(mdCall.payload).toEqual({ skillId: "alpha" });
    expect(document.activeElement).toBe(editor);
  });

  it("surfaces opener failures in a focused alert and clears them when selection changes", async () => {
    const alpha = baseSnapshot().skills[0]!;
    const beta = makeSkill({
      name: "beta",
      description: "Beta",
      attachments: [attachment("claude-code")],
    });
    mockCommands({
      snapshot: makeSnapshot({ skills: [alpha, beta] }),
      detail: (skillId) => baseDetail({ name: skillId }),
      openError: {
        code: "open.failed",
        message: "无法打开文件或目录。",
      },
    });
    renderApp();
    await selectAlpha();

    fireEvent.click(
      detailPane().getByRole("button", { name: "在 Finder 中查看" }),
    );
    const alert = await detailPane().findByRole("alert");
    expect(alert).toHaveTextContent("无法打开文件或目录。");
    await waitFor(() => expect(alert).toHaveFocus());

    fireEvent.click(row("beta"));
    await waitFor(() => {
      expect(detailPane().queryByRole("alert")).toBeNull();
      expect(
        detailPane().getByRole("heading", { name: "beta" }),
      ).toBeInTheDocument();
    });
  });

  it("keeps the summary usable when detail loading fails and offers a retry", async () => {
    let attempts = 0;
    mockIPC((command) => {
      if (command === "get_environment") return healthyReport();
      if (command === "get_inventory") return baseSnapshot();
      if (command === "get_skill_detail") {
        attempts += 1;
        if (attempts === 1) {
          throw {
            code: "skill.read_failed",
            message: "读取 skill 详情失败。",
            hint: "请检查该 skill 目录的权限后重试。",
          };
        }
        return baseDetail();
      }
      if (command === "open_skill_folder" || command === "open_skill_md") {
        return null;
      }
      throw new Error(`unexpected command: ${command}`);
    });
    renderApp();
    await screen.findByText("alpha");
    const selectedRow = selectAlphaSync();
    expect(document.activeElement).toBe(selectedRow);

    const pane = detailPane();
    expect(pane.getByRole("heading", { name: "alpha" })).toBeInTheDocument();
    const alert = await pane.findByRole("alert");
    expect(alert).toHaveTextContent("读取 skill 详情失败。");
    expect(alert).toHaveTextContent("请检查该 skill 目录的权限后重试。");
    fireEvent.click(within(alert).getByRole("button", { name: "重试读取" }));
    await waitFor(() => {
      expect(pane.queryByRole("alert")).toBeNull();
      expect(pane.getByText("概览")).toBeInTheDocument();
    });
    expect(attempts).toBe(2);
  });

  it("focuses the selected row and never steals focus into the detail pane", async () => {
    mockCommands({});
    renderApp();
    const selectedRow = await selectAlpha();
    expect(document.activeElement).toBe(selectedRow);

    const pane = detailPane();
    const raw = pane.getByRole("button", { name: "原文" });
    raw.focus();
    fireEvent.click(raw);
    expect(document.activeElement).toBe(raw);
    fireEvent.click(pane.getByRole("button", { name: "渲染" }));
    expect(document.activeElement).toBe(raw);
  });
});
