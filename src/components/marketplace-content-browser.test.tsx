import type { ComponentProps } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { createQueryClient } from "../lib/query-client";
import type { SkillFileContent } from "../lib/types";
import { createFakeBackend, type FakeBackendOptions } from "../test/fake-backend";
import { marketplaceFileContent, marketplaceSkill } from "../test/marketplace-fixtures";
import { clawhubDetail, clawhubFileContent, clawhubVersionOne } from "../test/clawhub-fixtures";
import { skillsShSkill } from "../test/skills-sh-fixtures";
import { MarketplaceContentBrowser } from "./marketplace-content-browser";

type Source = ComponentProps<typeof MarketplaceContentBrowser>["source"];
const skillhub: Source = { kind: "skillhub", detail: marketplaceSkill };
const clawhub: Source = {
  kind: "clawhub", coordinate: clawhubDetail.coordinate, version: clawhubVersionOne,
  canonicalUrl: clawhubDetail.canonicalUrl, changelog: clawhubDetail.versions[0].changelog,
};
const skillsSh: Source = { kind: "skills-sh", skill: skillsShSkill };

function setup(source: Source, options: FakeBackendOptions = {}) {
  const backend = createFakeBackend(options).install();
  const client = createQueryClient();
  const content = (next: Source) => (
    <QueryClientProvider client={client}><MarketplaceContentBrowser source={next} active /></QueryClientProvider>
  );
  const view = render(content(source));
  return { backend, rerender: (next: Source) => view.rerender(content(next)) };
}

const panel = () => within(screen.getByRole("tabpanel", { name: "文件" }));
const filesTab = () => screen.getByRole("tab", { name: "文件" });
const overviewTab = () => screen.getByRole("tab", { name: "概述" });
function selectGuide() {
  fireEvent.click(panel().getByRole("button", { name: "references" }));
  fireEvent.click(panel().getByRole("button", { name: /guide.md/ }));
}

describe("Marketplace Content browser interface", () => {
  it("resolves ClawHub Overview links from SKILL.md after reading a nested file", async () => {
    setup(clawhub);
    await screen.findByRole("heading", { name: "Weather 1.2.0" });
    fireEvent.click(screen.getByRole("link", { name: "阅读指南" }));
    await panel().findByRole("heading", { name: "天气指南 1.2.0" });
    fireEvent.click(overviewTab());
    fireEvent.click(screen.getByRole("link", { name: "阅读指南" }));
    await panel().findByRole("heading", { name: "天气指南 1.2.0" });
    expect(panel().getByRole("button", { name: /guide.md/ })).toHaveAttribute("aria-current", "true");
    expect(panel().queryByText("references/references/guide.md")).not.toBeInTheDocument();
    // File links still resolve relative to the displayed nested file.
    fireEvent.click(panel().getByRole("link", { name: "查看脚本" }));
    await panel().findByLabelText("scripts/forecast.py 原文");
  });

  it("releases unobserved skills.sh snapshots after five minutes", async () => {
    createFakeBackend().install();
    const client = createQueryClient();
    const view = render(<QueryClientProvider client={client}>
      {["first", "second"].map((skillId) => <MarketplaceContentBrowser key={skillId} active
        source={{ kind: "skills-sh", skill: { ...skillsShSkill, coordinate: { ...skillsShSkill.coordinate, skillId } } }} />)}
    </QueryClientProvider>);
    const snapshots = () => client.getQueryCache().findAll({ queryKey: ["marketplace", "skills-sh", "snapshot"] });
    await waitFor(() => {
      expect(snapshots()).toHaveLength(2);
      expect(snapshots().every((query) => query.state.data)).toBe(true);
    });
    vi.useFakeTimers();
    try {
      view.unmount();
      await act(async () => { await vi.advanceTimersByTimeAsync(299_999); });
      expect(snapshots()).toHaveLength(2);
      await act(async () => { await vi.advanceTimersByTimeAsync(1); });
      expect(snapshots()).toHaveLength(0);
    } finally {
      client.clear();
      vi.useRealTimers();
    }
  });

  it("preserves SkillHub Overview raw mode and resets file raw mode across paths and tabs", async () => {
    const { backend, rerender } = setup(skillhub);
    fireEvent.click(screen.getByRole("button", { name: "查看原文" }));
    expect(screen.getByLabelText("SKILL.md 原文")).toBeInTheDocument();
    fireEvent.click(filesTab());
    await panel().findByRole("heading", { name: "Workspace Notes" });
    fireEvent.click(panel().getByRole("button", { name: "查看原文" }));
    selectGuide();
    await panel().findByRole("heading", { name: "笔记指南" });
    fireEvent.click(panel().getByRole("button", { name: "查看原文" }));
    fireEvent.click(overviewTab());
    expect(screen.getByLabelText("SKILL.md 原文")).toBeInTheDocument();
    fireEvent.click(filesTab());
    await panel().findByRole("heading", { name: "笔记指南" });
    expect(panel().getByRole("button", { name: "references" })).toHaveAttribute("aria-expanded", "true");
    rerender({ kind: "skillhub", detail: { ...marketplaceSkill } });
    expect(filesTab()).toHaveAttribute("aria-selected", "true");
    expect(panel().getByRole("heading", { name: "笔记指南" })).toBeInTheDocument();
    expect(backend.count("list_marketplace_files")).toBe(1);
    expect(backend.count("read_marketplace_file")).toBe(2);
  });

  it("keeps ClawHub file raw mode across paths and resets both reading modes when switching tabs", async () => {
    setup(clawhub);
    await screen.findByRole("heading", { name: "Weather 1.2.0" });
    fireEvent.click(screen.getByRole("button", { name: "查看原文" }));
    fireEvent.keyDown(overviewTab(), { key: "End" });
    expect(filesTab()).toHaveFocus();
    await panel().findByRole("heading", { name: "Weather 1.2.0" });
    fireEvent.click(panel().getByRole("button", { name: "查看原文" }));
    selectGuide();
    await panel().findByLabelText("references/guide.md 原文");
    expect(panel().queryByRole("heading", { name: "天气指南 1.2.0" })).not.toBeInTheDocument();
    fireEvent.keyDown(filesTab(), { key: "Home" });
    expect(overviewTab()).toHaveFocus();
    expect(screen.getByRole("heading", { name: "Weather 1.2.0" })).toBeInTheDocument();
    fireEvent.keyDown(overviewTab(), { key: "ArrowRight" });
    expect(filesTab()).toHaveFocus();
    await panel().findByRole("heading", { name: "天气指南 1.2.0" });
    expect(panel().getByRole("button", { name: "references" })).toHaveAttribute("aria-expanded", "true");
  });

  it("resets skills.sh raw mode on tab activation and resets browsing after a successful unchanged snapshot", async () => {
    const { backend } = setup(skillsSh);
    await screen.findByRole("heading", { name: "Project Notes" });
    fireEvent.click(screen.getByRole("button", { name: "查看原文" }));
    fireEvent.keyDown(overviewTab(), { key: "ArrowRight" });
    expect(filesTab()).toHaveFocus();
    expect(panel().getByRole("heading", { name: "Project Notes" })).toBeInTheDocument();
    selectGuide();
    expect(panel().getByRole("region", { name: "文件预览" })).toHaveFocus();
    fireEvent.click(panel().getByRole("button", { name: "查看原文" }));
    fireEvent.keyDown(filesTab(), { key: "Home" });
    expect(overviewTab()).toHaveFocus();
    expect(screen.getByRole("heading", { name: "Project Notes" })).toBeInTheDocument();
    fireEvent.keyDown(overviewTab(), { key: "End" });
    expect(panel().getByRole("heading", { name: "笔记指南" })).toBeInTheDocument();
    fireEvent.click(panel().getByRole("button", { name: "查看原文" }));
    fireEvent.click(screen.getByRole("button", { name: "刷新内容快照" }));
    await waitFor(() => expect(overviewTab()).toHaveAttribute("aria-selected", "true"));
    expect(screen.getByRole("heading", { name: "Project Notes" })).toBeInTheDocument();
    fireEvent.click(filesTab());
    expect(panel().getByRole("heading", { name: "Project Notes" })).toBeInTheDocument();
    expect(panel().getByRole("button", { name: "references" })).toHaveAttribute("aria-expanded", "false");
    expect(backend.count("get_skills_sh_snapshot")).toBe(2);
    expect(backend.count("read_marketplace_file")).toBe(0);
    expect(backend.count("read_clawhub_version_file")).toBe(0);
  });

  it.each(["skillhub", "clawhub"] as const)("ignores a delayed %s file response after selecting another path", async (kind) => {
    let release: ((file: SkillFileContent) => void) | undefined;
    function read(path: string, file: SkillFileContent) {
      return path === "references/guide.md" ? new Promise<SkillFileContent>((resolve) => { release = resolve; }) : file;
    }
    setup(kind === "skillhub" ? skillhub : clawhub, { commands: {
      read_marketplace_file: (args) => read(args.path, marketplaceFileContent(args.path)),
      read_clawhub_version_file: (args) => read(args.path, clawhubFileContent(args.version, args.path)),
    } });
    fireEvent.click(filesTab());
    const heading = kind === "skillhub" ? "Workspace Notes" : "Weather 1.2.0";
    await panel().findByRole("heading", { name: heading });
    selectGuide();
    await waitFor(() => expect(release).toBeDefined());
    fireEvent.click(panel().getByRole("button", { name: /^SKILL.md/ }));
    await panel().findByRole("heading", { name: heading });
    const guide = kind === "skillhub" ? marketplaceFileContent("references/guide.md")
      : clawhubFileContent(clawhubVersionOne, "references/guide.md");
    await act(async () => release!(guide));
    expect(panel().getByRole("heading", { name: heading })).toBeInTheDocument();
    expect(panel().queryByLabelText("references/guide.md 原文")).not.toBeInTheDocument();
    expect(panel().getByRole("button", { name: /^SKILL.md/ })).toHaveAttribute("aria-current", "true");
  });
});
