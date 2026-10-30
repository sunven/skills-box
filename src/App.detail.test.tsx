import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { createQueryClient } from "./lib/query-client";
import type {
  AgentAttachment,
  InventorySnapshot,
  SkillDetail,
  SkillDirListing,
  SkillFileContent,
} from "./lib/types";
import {
  createFakeBackend,
  type FakeBackendData,
  type FakeHandlers,
} from "./test/fake-backend";
import {
  IMAGE_DATA_URL,
  makeDetail,
  makeDirListing,
  makeFileContent,
} from "./test/detail-fixtures";
import {
  attachment,
  group,
  makeAnomalies,
  makeSkill,
  makeSnapshot,
} from "./test/inventory-fixtures";

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
    ...overrides,
  });
}

function mockCommands(
  options: {
    snapshot?: InventorySnapshot | (() => InventorySnapshot);
    detail?: SkillDetail | ((skillId: string) => SkillDetail);
    detailError?: { code: string; message: string; hint?: string };
    listing?: (skillId: string, path: string) => SkillDirListing;
    file?: (skillId: string, path: string) => SkillFileContent;
    fileError?: { code: string; message: string; hint?: string };
    openError?: { code: string; message: string };
  } = {},
) {
  const snapshot =
    typeof options.snapshot === "function"
      ? options.snapshot()
      : (options.snapshot ?? baseSnapshot());
  const data: Partial<FakeBackendData> = {
    snapshot,
    listDir: options.listing ?? makeDirListing,
    readFile: options.file ?? makeFileContent,
  };
  if (options.detail) {
    data.detail = (skillId) =>
      typeof options.detail === "function"
        ? options.detail(skillId)
        : options.detail!;
  }
  const commands: Partial<FakeHandlers> = {};
  if (options.detailError) {
    commands.get_skill_detail = () => {
      throw options.detailError!;
    };
  }
  if (options.fileError) {
    commands.read_skill_file = () => {
      throw options.fileError!;
    };
  }
  if (options.openError) {
    commands.open_skill_folder = () => {
      throw options.openError!;
    };
    commands.open_skill_file = () => {
      throw options.openError!;
    };
  }
  return createFakeBackend({ data, commands }).install().calls;
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

async function selectAlpha(openContent = false) {
  await screen.findByText("alpha");
  const target = selectAlphaSync();
  await waitFor(() =>
    expect(screen.getByRole("complementary", { name: "详情" })).toHaveAttribute(
      "aria-busy",
      "false",
    ),
  );
  if (openContent) {
    fireEvent.click(tab("内容"));
    await detailPane().findByTestId("skill-markdown-rendered");
  }
  return target;
}

function treeRow(path: string): HTMLElement {
  const found = detailPane()
    .getAllByTestId("tree-row")
    .find((candidate) => candidate.getAttribute("data-path") === path);
  if (!found) throw new Error(`missing tree row ${path}`);
  return found;
}

async function waitForTreeRow(path: string) {
  await waitFor(() => {
    const found = detailPane()
      .getAllByTestId("tree-row")
      .some((candidate) => candidate.getAttribute("data-path") === path);
    expect(found).toBe(true);
  });
}

function tab(name: "内容" | "操作"): HTMLElement {
  return detailPane().getByRole("tab", { name });
}

function toolbar() {
  return within(detailPane().getByTestId("content-toolbar"));
}

beforeEach(() => {
  Element.prototype.scrollIntoView = vi.fn();
});

describe("skill detail drawer", () => {
  it("loads content only when the content tab is first opened", async () => {
    const backend = createFakeBackend({
      data: { snapshot: baseSnapshot() },
    }).install();
    renderApp();
    await selectAlpha();

    expect(tab("操作")).toHaveAttribute("aria-selected", "true");
    expect(backend.count("list_skill_dir")).toBe(0);
    expect(backend.count("read_skill_file")).toBe(0);
    expect(detailPane().queryByTestId("content-toolbar")).toBeNull();

    fireEvent.click(tab("内容"));
    await detailPane().findByTestId("skill-markdown-rendered");
    expect(backend.count("list_skill_dir")).toBe(1);
    expect(backend.count("read_skill_file")).toBe(1);
  });

  it("renders identity, pinned alerts, the content tab, and the actions tab composition", async () => {
    const content =
      "---\nname: alpha\ndescription: Alpha\n---\n\n# Alpha 标题\n\n正文第一段。\n\n- 条目一\n- 条目二\n";
    mockCommands({
      detail: baseDetail({
        localModification: "modified",
        localModificationDetail: "当前文件与基线指纹不一致。",
        warnings: ["断链：Gemini CLI"],
      }),
      file: (skillId, path) =>
        path === "SKILL.md"
          ? makeFileContent(skillId, path, {
              content,
              sizeBytes: content.length,
            })
          : makeFileContent(skillId, path),
    });
    renderApp();
    await selectAlpha();

    const pane = detailPane();
    expect(
      pane.getByRole("heading", { name: "alpha" }),
    ).toBeInTheDocument();
    expect(pane.getByTestId("detail-update-badge")).toHaveTextContent("未知");
    const updateSummary = pane.getByTestId("detail-update-summary");
    expect(updateSummary.closest(".detail-head")).not.toBeNull();
    expect(
      within(updateSummary).getByRole("button", { name: "检查更新" }),
    ).toBeVisible();
    expect(within(updateSummary).getByText("上次检查")).toBeVisible();
    const uninstall = within(updateSummary).getByRole("button", {
      name: "彻底卸载",
    });
    expect(uninstall).toBeVisible();
    expect(uninstall).toHaveAttribute(
      "title",
      "删除 canonical、所有 agent 挂载、lock 条目与基线；先备份，不自动回滚",
    );
    expect(pane.queryByRole("heading", { name: "危险操作" })).toBeNull();
    expect(
      within(pane.getByTestId("detail-actions-panel")).queryByRole("button", {
        name: "彻底卸载",
      }),
    ).toBeNull();
    expect(pane.queryByText("远端 tree SHA")).not.toBeInTheDocument();
    expect(pane.queryByTestId("detail-update-section")).not.toBeInTheDocument();
    expect(pane.getAllByText("本地改动").length).toBeGreaterThan(0);
    expect(pane.getAllByText("断链").length).toBeGreaterThan(0);
    expect(pane.getAllByText("警告").length).toBeGreaterThan(0);

    // State notices sit above the tabs and are visible in both panels.
    const alerts = pane.getByTestId("detail-alerts");
    expect(alerts).toHaveTextContent(
      "检测到本地改动：共享副本与基线记录不一致。更新会覆盖修改，请先备份。",
    );
    expect(pane.getByTestId("detail-warnings")).toHaveTextContent(
      "断链：Gemini CLI",
    );

    expect(tab("操作")).toHaveAttribute("aria-selected", "true");
    expect(tab("内容")).toHaveAttribute("aria-selected", "false");

    // Arrow keys move between the tabs and take focus with them.
    fireEvent.keyDown(tab("操作"), { key: "ArrowRight" });
    expect(tab("内容")).toHaveAttribute("aria-selected", "true");
    expect(document.activeElement).toBe(tab("内容"));
    await pane.findByTestId("skill-markdown-rendered");
    fireEvent.keyDown(tab("内容"), { key: "ArrowLeft" });
    expect(tab("操作")).toHaveAttribute("aria-selected", "true");
    expect(document.activeElement).toBe(tab("操作"));

    fireEvent.click(tab("内容"));
    expect(tab("内容")).toHaveAttribute("aria-selected", "true");
    expect(updateSummary).toBeVisible();
    expect(uninstall).toBeVisible();
    expect(
      within(updateSummary).getByRole("button", { name: "检查更新" }),
    ).toBeVisible();

    // Root tree lists directories first and auto-selects SKILL.md.
    const rootRows = pane
      .getAllByTestId("tree-row")
      .map((element) => element.getAttribute("data-path"));
    expect(rootRows).toEqual(["assets", "references", "scripts", "SKILL.md"]);
    expect(treeRow("SKILL.md")).toHaveAttribute("data-selected", "true");

    const rendered = pane.getByTestId("skill-markdown-rendered");
    expect(
      within(rendered).getByRole("heading", { name: "Alpha 标题" }),
    ).toBeInTheDocument();
    expect(within(rendered).getByText("正文第一段。")).toBeInTheDocument();
    expect(within(rendered).getByText("条目一")).toBeInTheDocument();
    expect(rendered).not.toHaveTextContent("name: alpha");

    fireEvent.click(tab("操作"));
    expect(tab("操作")).toHaveAttribute("aria-selected", "true");
    expect(await pane.findByText("概览")).toBeInTheDocument();
    expect(pane.getByText("Agent 挂载")).toBeInTheDocument();
    expect(pane.getByText("3 个")).toBeInTheDocument();
    expect(pane.getAllByTestId("detail-agent-row")).toHaveLength(3);
    expect(
      pane.getByRole("button", { name: "断开 Claude Code" }),
    ).toBeInTheDocument();
    expect(
      pane.queryByRole("button", { name: /^更新$|断开 Codex|断开 Gemini/ }),
    ).toBeNull();
    expect(
      pane.getByRole("button", { name: "检查更新" }),
    ).toBeInTheDocument();
    expect(
      pane.getByRole("button", { name: "彻底卸载" }),
    ).toBeInTheDocument();
    expect(pane.getByText("acme/skills")).toBeInTheDocument();
    expect(pane.getByText("main")).toBeInTheDocument();
    expect(pane.getByText("canonical 路径")).toBeInTheDocument();
    expect(pane.getByText("上次检查")).toBeInTheDocument();
    expect(pane.getByTestId("detail-last-update-check")).toHaveTextContent(
      "尚未检查",
    );
    expect(pane.queryByRole("button", { name: "展开" })).not.toBeInTheDocument();
    // The local-modification notice is still pinned above the tabs.
    expect(pane.getByTestId("detail-alerts")).toHaveTextContent(
      /检测到本地改动/,
    );
  });

  it("clamps long descriptions to three lines and expands on demand", async () => {
    const longDescription = `长描述：${"这是一段用于验证三行折叠的描述。".repeat(20)}`;
    mockCommands({
      snapshot: makeSnapshot({
        skills: [
          makeSkill({
            name: "alpha",
            description: longDescription,
            source: "acme/skills",
            attachments,
          }),
        ],
      }),
      detail: baseDetail({ description: longDescription }),
    });
    renderApp();
    await selectAlpha();

    const pane = detailPane();
    const description = pane.getByTestId("detail-description");
    expect(description).toHaveTextContent(longDescription);
    expect(description).toHaveAttribute("data-expanded", "false");

    const expand = await pane.findByRole("button", { name: "展开" });
    expect(expand).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(expand);

    expect(description).toHaveAttribute("data-expanded", "true");
    const collapse = pane.getByRole("button", { name: "收起" });
    expect(collapse).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(collapse);

    expect(description).toHaveAttribute("data-expanded", "false");
    expect(pane.getByRole("button", { name: "展开" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
  });

  it("browses directories and previews markdown, text, and images", async () => {
    mockCommands({});
    renderApp();
    await selectAlpha(true);
    const pane = detailPane();

    fireEvent.click(treeRow("references"));
    await waitForTreeRow("references/api.md");
    expect(treeRow("references")).toHaveAttribute("aria-expanded", "true");

    fireEvent.click(treeRow("references/api.md"));
    await waitFor(() =>
      expect(pane.getByTestId("content-preview-path")).toHaveTextContent(
        "references/api.md",
      ),
    );
    await waitFor(() =>
      expect(pane.getByTestId("skill-markdown-rendered")).toHaveTextContent(
        "参考文档。",
      ),
    );
    expect(treeRow("references/api.md")).toHaveAttribute(
      "data-selected",
      "true",
    );

    fireEvent.click(treeRow("scripts"));
    await waitForTreeRow("scripts/run.sh");
    fireEvent.click(treeRow("scripts/run.sh"));
    await waitFor(() =>
      expect(pane.getByTestId("content-preview-path")).toHaveTextContent(
        "scripts/run.sh",
      ),
    );
    await waitFor(() =>
      expect(pane.getByTestId("skill-file-text")).toHaveTextContent(
        "#!/bin/sh",
      ),
    );
    expect(pane.queryByRole("button", { name: "原文" })).toBeNull();

    fireEvent.click(treeRow("assets"));
    await waitForTreeRow("assets/logo.png");
    fireEvent.click(treeRow("assets/logo.png"));
    await waitFor(() =>
      expect(pane.getByTestId("skill-file-image")).toBeInTheDocument(),
    );
    const image = pane.getByTestId("skill-file-image") as HTMLImageElement;
    expect(image.src).toBe(IMAGE_DATA_URL);
  });

  it("renders markdown safely and navigates internal links, images, and fragments", async () => {
    const content = [
      "---",
      "name: alpha",
      "description: Alpha",
      "---",
      "",
      "# Alpha 标题",
      "",
      "## Sec",
      "",
      "<script>window.__skills_box_xss = true</script>",
      '<img src="https://evil.test/raw.png" onerror="window.__skills_box_xss = true" />',
      "",
      "[API 文档](references/api.md)",
      "[跳转](#sec)",
      "[HTTPS 文档](https://example.com/docs)",
      "[HTTP 链接](http://example.com/insecure)",
      "[JS 链接](javascript:alert(1))",
      "",
      "![架构图](https://example.com/diagram.png)",
      "![本地图](assets/logo.png)",
    ].join("\n");
    const calls = mockCommands({
      file: (skillId, path) =>
        path === "SKILL.md"
          ? makeFileContent(skillId, path, {
              content,
              sizeBytes: content.length,
            })
          : makeFileContent(skillId, path),
    });
    renderApp();
    await selectAlpha(true);

    const pane = detailPane();
    const rendered = pane.getByTestId("skill-markdown-rendered");
    expect(rendered.querySelector("script")).toBeNull();
    expect(
      (window as unknown as { __skills_box_xss?: boolean }).__skills_box_xss,
    ).toBeUndefined();
    expect(rendered).not.toHaveTextContent("window.__skills_box_xss");
    expect(
      rendered.querySelector('img[src="https://evil.test/raw.png"]'),
    ).toBeNull();

    const localImage = await within(rendered).findByTestId(
      "skill-markdown-image",
    );
    expect(localImage).toHaveAttribute("src", IMAGE_DATA_URL);
    expect(localImage).toHaveAttribute("alt", "本地图");
    expect(within(rendered).getByText(/架构图/)).toBeInTheDocument();

    // Fragment links scroll the current document.
    const heading = within(rendered).getByRole("heading", { name: "Sec" });
    expect(heading).toHaveAttribute("id", "sec");
    fireEvent.click(within(rendered).getByRole("link", { name: "跳转" }));
    expect(Element.prototype.scrollIntoView).toHaveBeenCalled();

    // External https still goes through the opener.
    fireEvent.click(
      within(rendered).getByRole("link", { name: "HTTPS 文档" }),
    );
    await waitFor(() =>
      expect(
        calls.some(
          (call) =>
            call.command === "open_external_url" &&
            (call.payload as { url?: string }).url ===
              "https://example.com/docs",
        ),
      ).toBe(true),
    );
    expect(
      within(rendered).queryByRole("link", { name: "HTTP 链接" }),
    ).toBeNull();
    expect(
      within(rendered).queryByRole("link", { name: "JS 链接" }),
    ).toBeNull();

    // Internal relative links select the target file and reveal it in the tree.
    fireEvent.click(within(rendered).getByRole("link", { name: "API 文档" }));
    await waitFor(() =>
      expect(pane.getByTestId("content-preview-path")).toHaveTextContent(
        "references/api.md",
      ),
    );
    await waitForTreeRow("references/api.md");
    expect(treeRow("references/api.md")).toHaveAttribute(
      "data-selected",
      "true",
    );
  });

  it("shows an empty selection when SKILL.md is missing", async () => {
    const listing = (skillId: string, path: string) =>
      path === ""
        ? makeDirListing(skillId, "", {
            entries: [
              {
                name: "notes.txt",
                path: "notes.txt",
                kind: "file",
                symlink: false,
                sizeBytes: 10,
                markdown: false,
                expandable: false,
              },
            ],
          })
        : makeDirListing(skillId, path);
    mockCommands({
      listing,
      detail: baseDetail({ warnings: ["未找到 SKILL.md"] }),
    });
    renderApp();
    await screen.findByText("alpha");
    selectAlphaSync();

    const pane = detailPane();
    fireEvent.click(tab("内容"));
    expect(
      await pane.findByText("从左侧选择文件查看内容"),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(pane.getByTestId("detail-warnings")).toHaveTextContent(
        "未找到 SKILL.md",
      ),
    );
    expect(treeRow("notes.txt")).toHaveAttribute("data-selected", "false");

    fireEvent.click(treeRow("notes.txt"));
    await waitFor(() =>
      expect(pane.getByTestId("content-preview-path")).toHaveTextContent(
        "notes.txt",
      ),
    );
    await waitFor(() =>
      expect(pane.getByTestId("skill-file-text")).toBeInTheDocument(),
    );
  });

  it("reports oversized, binary, and unreadable files", async () => {
    mockCommands({
      file: (skillId, path) => {
        if (path === "SKILL.md") {
          return makeFileContent(skillId, path, {
            status: "too_large",
            content: null,
            sizeBytes: 1024 * 1024 + 1,
          });
        }
        if (path === "references/notes.txt") {
          return makeFileContent(skillId, path, {
            status: "binary",
            kind: "text",
            content: null,
            sizeBytes: 16,
          });
        }
        if (path === "scripts/run.sh") {
          return makeFileContent(skillId, path, {
            status: "unreadable",
            kind: "text",
            content: null,
            error: "权限不足",
          });
        }
        return makeFileContent(skillId, path);
      },
    });
    renderApp();
    await screen.findByText("alpha");
    selectAlphaSync();
    const pane = detailPane();
    fireEvent.click(tab("内容"));

    expect(await pane.findByText(/文件超过 1 MiB 上限/)).toBeInTheDocument();
    expect(pane.getByText(/当前大小 1 MiB/)).toBeInTheDocument();

    fireEvent.click(treeRow("references"));
    await waitForTreeRow("references/notes.txt");
    fireEvent.click(treeRow("references/notes.txt"));
    expect(
      await pane.findByText("该文件不是 UTF-8 文本"),
    ).toBeInTheDocument();

    fireEvent.click(treeRow("scripts"));
    await waitForTreeRow("scripts/run.sh");
    fireEvent.click(treeRow("scripts/run.sh"));
    expect(await pane.findByText("无法读取该文件")).toBeInTheDocument();
    expect(pane.getByText("权限不足")).toBeInTheDocument();
  });

  it("preserves content state across tabs and resets it when the skill changes", async () => {
    const beta = makeSkill({
      name: "beta",
      description: "Beta",
      attachments: [attachment("claude-code")],
    });
    mockCommands({
      snapshot: makeSnapshot({
        skills: [baseSnapshot().skills[0]!, beta],
      }),
      detail: (skillId) => baseDetail({ name: skillId }),
    });
    renderApp();
    await selectAlpha(true);
    const pane = detailPane();

    fireEvent.click(treeRow("references"));
    await waitForTreeRow("references/api.md");
    fireEvent.click(treeRow("references/api.md"));
    await waitFor(() =>
      expect(pane.getByTestId("content-preview-path")).toHaveTextContent(
        "references/api.md",
      ),
    );

    fireEvent.click(tab("操作"));
    fireEvent.click(tab("内容"));
    expect(pane.getByTestId("content-preview-path")).toHaveTextContent(
      "references/api.md",
    );

    fireEvent.click(row("beta"));
    expect(tab("操作")).toHaveAttribute("aria-selected", "true");
    expect(pane.queryByTestId("content-toolbar")).toBeNull();
    fireEvent.click(tab("内容"));
    expect(tab("内容")).toHaveAttribute("aria-selected", "true");
    await waitFor(() =>
      expect(pane.getByTestId("content-preview-path")).toHaveTextContent(
        "SKILL.md",
      ),
    );
    expect(
      pane
        .getAllByTestId("tree-row")
        .some(
          (element) =>
            element.getAttribute("data-path") === "references/api.md",
        ),
    ).toBe(false);
  });

  it("opens Finder and the selected file with skillId and relative path only", async () => {
    const calls = mockCommands({});
    renderApp();
    await selectAlpha(true);
    const pane = detailPane();

    const finder = toolbar().getByRole("button", {
      name: "在 Finder 中查看",
    });
    fireEvent.click(finder);
    await waitFor(() =>
      expect(calls.some((call) => call.command === "open_skill_folder")).toBe(
        true,
      ),
    );
    const folderCall = calls.find(
      (call) => call.command === "open_skill_folder",
    )!;
    expect(folderCall.payload).toEqual({ skillId: "alpha", scope: "global" });

    const opener = toolbar().getByRole("button", {
      name: "用默认应用打开",
    });
    fireEvent.click(opener);
    await waitFor(() =>
      expect(calls.some((call) => call.command === "open_skill_file")).toBe(
        true,
      ),
    );
    const fileCall = calls.find(
      (call) => call.command === "open_skill_file",
    )!;
    expect(fileCall.payload).toEqual({
      skillId: "alpha",
      path: "SKILL.md",
      scope: "global",
    });

    fireEvent.click(treeRow("references"));
    await waitForTreeRow("references/api.md");
    fireEvent.click(treeRow("references/api.md"));
    await waitFor(() =>
      expect(pane.getByTestId("content-preview-path")).toHaveTextContent(
        "references/api.md",
      ),
    );
    fireEvent.click(opener);
    await waitFor(() =>
      expect(
        calls.filter((call) => call.command === "open_skill_file"),
      ).toHaveLength(2),
    );
    const secondCall = calls.filter(
      (call) => call.command === "open_skill_file",
    )[1]!;
    expect(secondCall.payload).toEqual({
      skillId: "alpha",
      path: "references/api.md",
      scope: "global",
    });
  });

  it("surfaces content-open failures and clears them when the skill changes", async () => {
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
    await selectAlpha(true);
    const pane = detailPane();

    fireEvent.click(
      toolbar().getByRole("button", { name: "在 Finder 中查看" }),
    );
    const alert = await pane.findByRole("alert");
    expect(alert).toHaveTextContent("无法打开文件或目录。");
    await waitFor(() => expect(alert).toHaveFocus());

    fireEvent.click(row("beta"));
    await waitFor(() => {
      expect(pane.queryByRole("alert")).toBeNull();
      expect(
        pane.getByRole("heading", { name: "beta" }),
      ).toBeInTheDocument();
    });
  });

  it("refreshes the directory, the selected file, and the detail", async () => {
    const calls = mockCommands({});
    renderApp();
    await selectAlpha(true);
    const count = (command: string) =>
      calls.filter((call) => call.command === command).length;
    const initialList = count("list_skill_dir");
    const initialRead = count("read_skill_file");
    const initialDetail = count("get_skill_detail");

    fireEvent.click(toolbar().getByRole("button", { name: "刷新" }));
    await waitFor(() => {
      expect(count("list_skill_dir")).toBeGreaterThan(initialList);
      expect(count("read_skill_file")).toBeGreaterThan(initialRead);
      expect(count("get_skill_detail")).toBeGreaterThan(initialDetail);
    });
  });

  it("toggles rendered and complete raw markdown without losing focus", async () => {
    const content =
      "---\nname: alpha\ndescription: Alpha\n---\n\n# Alpha 标题\n\n正文。\n";
    mockCommands({
      file: (skillId, path) =>
        path === "SKILL.md"
          ? makeFileContent(skillId, path, {
              content,
              sizeBytes: content.length,
            })
          : makeFileContent(skillId, path),
    });
    renderApp();
    await selectAlpha(true);
    const pane = detailPane();

    expect(pane.getByTestId("skill-markdown-rendered")).toBeInTheDocument();
    const rawButton = pane.getByRole("button", { name: "原文" });
    rawButton.focus();
    fireEvent.click(rawButton);

    const raw = pane.getByTestId("skill-file-raw");
    expect(raw).toHaveTextContent("---");
    expect(raw).toHaveTextContent("name: alpha");
    expect(raw.textContent).toBe(content);
    expect(rawButton).toHaveAttribute("aria-pressed", "true");
    expect(document.activeElement).toBe(rawButton);

    const renderedButton = pane.getByRole("button", { name: "渲染" });
    fireEvent.click(renderedButton);
    expect(pane.getByTestId("skill-markdown-rendered")).toBeInTheDocument();
    expect(pane.queryByTestId("skill-file-raw")).toBeNull();
    expect(renderedButton).toHaveAttribute("aria-pressed", "true");
  });

  it("keeps the drawer usable when detail loading fails and offers a retry", async () => {
    let attempts = 0;
    createFakeBackend({
      data: {
        snapshot: baseSnapshot(),
        detail: () => {
          attempts += 1;
          if (attempts === 1) {
            throw {
              code: "skill.read_failed",
              message: "读取 skill 详情失败。",
              hint: "请检查该 skill 目录的权限后重试。",
            };
          }
          return baseDetail();
        },
      },
    }).install();
    renderApp();
    await screen.findByText("alpha");
    selectAlphaSync();
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: "关闭详情" }),
    );

    const pane = detailPane();
    expect(
      pane.getByRole("heading", { name: "alpha" }),
    ).toBeInTheDocument();
    const alert = await pane.findByRole("alert");
    expect(alert).toHaveTextContent("读取 skill 详情失败。");
    expect(alert).toHaveTextContent("请检查该 skill 目录的权限后重试。");
    fireEvent.click(within(alert).getByRole("button", { name: "重试读取" }));
    await waitFor(() => {
      expect(pane.queryByRole("alert")).toBeNull();
    });
    fireEvent.click(tab("操作"));
    expect(await pane.findByText("概览")).toBeInTheDocument();
    expect(attempts).toBe(2);
  });

  it("moves focus into the drawer and restores it to the row on close", async () => {
    mockCommands({});
    renderApp();
    const selectedRow = await selectAlpha(true);
    const closeButton = screen.getByRole("button", { name: "关闭详情" });
    expect(document.activeElement).toBe(closeButton);

    const pane = detailPane();
    const raw = pane.getByRole("button", { name: "原文" });
    raw.focus();
    fireEvent.click(raw);
    expect(document.activeElement).toBe(raw);
    fireEvent.click(pane.getByRole("button", { name: "渲染" }));
    expect(document.activeElement).toBe(raw);

    fireEvent.click(closeButton);
    expect(
      screen.queryByRole("complementary", { name: "详情" }),
    ).not.toBeInTheDocument();
    expect(document.activeElement).toBe(selectedRow);
  });
});
