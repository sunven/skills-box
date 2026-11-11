import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import App from "./App";
import { createQueryClient } from "./lib/query-client";
import { marketplaceKey } from "./lib/marketplace";
import { createFakeBackend, type FakeBackendOptions } from "./test/fake-backend";
import { marketplaceSkill } from "./test/marketplace-fixtures";
import { discoveryProject } from "./test/discovery-fixtures";
import { makeSnapshot } from "./test/inventory-fixtures";
import type { SkillFileContent } from "./lib/types";

function setup(options: FakeBackendOptions = {}) {
  const backend = createFakeBackend(options).install();
  const client = createQueryClient();
  render(<QueryClientProvider client={client}><App /></QueryClientProvider>);
  return { backend, client };
}
async function openDetail() {
  fireEvent.click(await screen.findByRole("button", { name: "技能市场" }));
  fireEvent.click(await screen.findByRole("button", { name: marketplaceSkill.name }));
  return screen.findByRole("tab", { name: "文件" });
}

describe("marketplace release files", () => {
  it("loads files on demand and browses nested Markdown, source, relative links and images", async () => {
    const { backend } = setup();
    const tab = await openDetail();
    expect(backend.count("list_marketplace_files")).toBe(0);
    fireEvent.click(tab);
    const panel = screen.getByRole("tabpanel", { name: "文件" });
    await within(panel).findByTestId("skill-markdown-rendered");
    fireEvent.click(within(panel).getByRole("button", { name: "references" }));
    fireEvent.click(within(panel).getByRole("button", { name: /guide.md/ }));
    await within(panel).findByRole("heading", { name: "笔记指南" });
    fireEvent.click(within(panel).getByRole("button", { name: "查看原文" }));
    expect(within(panel).getByLabelText("references/guide.md 原文")).toHaveTextContent("[查看示例脚本](../scripts/notes.py)");
    fireEvent.click(within(panel).getByRole("button", { name: "返回渲染" }));
    fireEvent.click(within(panel).getByRole("link", { name: "查看示例脚本" }));
    expect(await within(panel).findByLabelText("scripts/notes.py 原文")).toHaveTextContent("from pathlib import Path");
    expect(within(panel).getByRole("region", { name: "文件预览" })).toHaveFocus();
    expect(within(panel).getByRole("button", { name: "scripts" })).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(within(panel).getByRole("button", { name: "assets" }));
    fireEvent.click(within(panel).getByRole("button", { name: /preview.png/ }));
    expect(await within(panel).findByRole("img", { name: "preview.png" })).toHaveAttribute("src", expect.stringContaining("data:image/png;base64,"));
    expect(backend.callsOf("read_marketplace_file").every((call) => call.payload.version === marketplaceSkill.version)).toBe(true);
    expect(backend.callsOf("list_marketplace_files")[0].payload.coordinate).toEqual(marketplaceSkill.coordinate);
    expect(backend.count("prepare_marketplace_install")).toBe(0);
    expect(backend.count("read_skill_file")).toBe(0);
  });

  it("opens overview file links and keeps the selected file across tabs, scopes and installation", async () => {
    const { backend } = setup({
      data: { snapshot: makeSnapshot(), projects: { projects: [discoveryProject], lastActiveScope: "global", warnings: [] } },
      commands: { get_marketplace_skill: () => ({ ...marketplaceSkill, markdown: marketplaceSkill.markdown + "\n\n[阅读指南](references/guide.md)" }) },
    });
    await openDetail();
    fireEvent.click(screen.getByRole("link", { name: "阅读指南" }));
    const panel = screen.getByRole("tabpanel", { name: "文件" });
    await within(panel).findByRole("heading", { name: "笔记指南" });
    expect(screen.getByRole("tab", { name: "文件" })).toHaveFocus();
    fireEvent.click(screen.getByRole("tab", { name: "概述" }));
    fireEvent.click(screen.getByRole("tab", { name: "文件" }));
    expect(within(panel).getByRole("heading", { name: "笔记指南" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: `切换到项目 ${discoveryProject.displayName}`, hidden: true }));
    await waitFor(() => expect(screen.getByRole("button", { name: "安装" })).toBeEnabled());
    expect(within(panel).getByRole("heading", { name: "笔记指南" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "安装" }));
    fireEvent.click(await screen.findByRole("button", { name: "关闭安装" }));
    expect(within(panel).getByRole("heading", { name: "笔记指南" })).toBeInTheDocument();
    expect(backend.count("list_marketplace_files")).toBe(1);
    expect(backend.count("read_marketplace_file")).toBe(1);
  });

  it("retries failed listings and file reads without displaying the previous file", async () => {
    let listFail = true;
    let readFail = true;
    const { backend } = setup({ commands: {
      list_marketplace_files: (args, fake) => {
        if (listFail) { listFail = false; throw new Error("文件列表暂时不可用"); }
        return fake.dispatch("list_marketplace_files", args);
      },
      read_marketplace_file: (args, fake) => {
        if (args.path === "references/guide.md" && readFail) { readFail = false; throw new Error("文件读取失败"); }
        return fake.dispatch("read_marketplace_file", args);
      },
    } });
    fireEvent.click(await openDetail());
    fireEvent.click(await screen.findByRole("button", { name: "重试文件列表" }));
    const panel = screen.getByRole("tabpanel", { name: "文件" });
    await within(panel).findByTestId("skill-markdown-rendered");
    fireEvent.click(within(panel).getByRole("button", { name: "references" }));
    fireEvent.click(within(panel).getByRole("button", { name: /guide.md/ }));
    await within(panel).findByText("文件读取失败");
    expect(within(panel).queryByTestId("skill-markdown-rendered")).not.toBeInTheDocument();
    fireEvent.click(within(panel).getByRole("button", { name: "重试读取文件" }));
    await within(panel).findByRole("heading", { name: "笔记指南" });
    fireEvent.click(within(panel).getByRole("button", { name: /large.txt/ }));
    await within(panel).findByText(/文件超过 1 MiB 预览上限/);
    fireEvent.click(within(panel).getByRole("button", { name: "assets" }));
    fireEvent.click(within(panel).getByRole("button", { name: /data.bin/ }));
    await within(panel).findByText(/二进制或非 UTF-8/);
    expect(backend.count("list_marketplace_files")).toBe(2);
  });

  it("separates file caches by release and handles missing paths and empty releases", async () => {
    let releaseNew: ((file: SkillFileContent) => void) | undefined;
    const { backend, client } = setup({ commands: {
      get_marketplace_skill: () => ({ ...marketplaceSkill, markdown: "[缺失文件](references/missing.md)" }),
      list_marketplace_files: (args, fake) => args.version === "3.0.0"
        ? { coordinate: args.coordinate, version: args.version, files: [] }
        : fake.dispatch("list_marketplace_files", args),
      read_marketplace_file: (args, fake) => args.version === "2.0.0"
        ? new Promise((resolve) => { releaseNew = resolve; })
        : fake.dispatch("read_marketplace_file", args),
    } });
    await openDetail();
    fireEvent.click(screen.getByRole("link", { name: "缺失文件" }));
    await screen.findByText(/此版本中没有该文件/);
    expect(backend.count("read_marketplace_file")).toBe(0);
    fireEvent.click(screen.getByRole("button", { name: /^SKILL.md/ }));
    await screen.findByTestId("skill-markdown-rendered");
    act(() => client.setQueryData(["marketplace", "detail", marketplaceKey(marketplaceSkill.coordinate)], { ...marketplaceSkill, version: "2.0.0" }));
    await screen.findByText("平台版本：2.0.0");
    fireEvent.click(await screen.findByRole("tab", { name: "文件" }));
    const panel = screen.getByRole("tabpanel", { name: "文件" });
    await waitFor(() => expect(releaseNew).toBeDefined());
    expect(within(panel).queryByTestId("skill-markdown-rendered")).not.toBeInTheDocument();
    const file = await backend.dispatch("read_marketplace_file", { coordinate: marketplaceSkill.coordinate, version: "1.2.0", path: "SKILL.md" });
    await act(async () => releaseNew!({ ...file, content: "# 新版内容" }));
    await within(panel).findByRole("heading", { name: "新版内容" });
    act(() => client.setQueryData(["marketplace", "detail", marketplaceKey(marketplaceSkill.coordinate)], { ...marketplaceSkill, version: "3.0.0" }));
    await screen.findByText("平台版本：3.0.0");
    fireEvent.click(await screen.findByRole("tab", { name: "文件" }));
    await screen.findByText("此版本没有可浏览的文件。");
  });

  it("switches tabs using the keyboard", async () => {
    setup();
    await openDetail();
    const overview = screen.getByRole("tab", { name: "概述" });
    overview.focus();
    fireEvent.keyDown(overview, { key: "ArrowRight" });
    const files = screen.getByRole("tab", { name: "文件" });
    expect(files).toHaveFocus();
    expect(files).toHaveAttribute("aria-selected", "true");
    await screen.findByRole("navigation", { name: "市场文件树" });
    fireEvent.keyDown(files, { key: "Home" });
    expect(overview).toHaveFocus();
    expect(overview).toHaveAttribute("aria-selected", "true");
  });
});
