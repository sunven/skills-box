import { StrictMode } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import App from "./App";
import { createQueryClient } from "./lib/query-client";
import type { PreviewSummary, TaskEvent } from "./lib/types";
import type { SkillsShListing } from "./lib/skills-sh";
import { createFakeBackend, type FakeBackend, type FakeBackendOptions } from "./test/fake-backend";
import { discoveryProject, discoveryProjectScope } from "./test/discovery-fixtures";
import { nodeMissingReport } from "./test/fixtures";
import { makeSkill, makeSnapshot } from "./test/inventory-fixtures";
import { marketplaceSkill } from "./test/marketplace-fixtures";
import { skillsShPreview, skillsShSkill, skillsShSnapshot, skillsShWebsiteSkill } from "./test/skills-sh-fixtures";

function setup(options: FakeBackendOptions = {}) {
  const backend = createFakeBackend({ ...options, data: {
    snapshot: makeSnapshot(), preview: skillsShPreview,
    projects: { projects: [discoveryProject], lastActiveScope: "global", warnings: [] },
    ...options.data,
  } }).install();
  const client = createQueryClient();
  const view = render(<StrictMode><QueryClientProvider client={client}><App /></QueryClientProvider></StrictMode>);
  return { backend, client, ...view };
}
async function openMarket() {
  fireEvent.click(await screen.findByRole("button", { name: "技能市场" }));
  fireEvent.click(await screen.findByRole("button", { name: "skills.sh" }));
  await screen.findByRole("button", { name: skillsShSkill.name });
}
async function openDetail() {
  await openMarket();
  fireEvent.click(screen.getByRole("button", { name: skillsShSkill.name }));
  return screen.findByRole("main", { name: skillsShSkill.name });
}
function completePreview(backend: FakeBackend, preview: PreviewSummary) {
  const task = backend.tasks.filter((task) => task.command === "start_preview").slice(-1)[0]!;
  const event: TaskEvent = { taskId: task.taskId, phase: "completed", message: "预览完成", result: {
    action: "preview", scope: backend.callsOf("start_preview").slice(-1)[0]!.payload.scope,
    overall: "success", skills: [], warnings: [], startedAt: 0, finishedAtEpochMs: 1, preview,
  } };
  act(() => task.emit(event));
}

describe("skills.sh marketplace", () => {
  it("defaults to SkillHub and preserves separate searches, pages, scroll and source preference", async () => {
    const { backend } = setup({ commands: {
      list_skills_sh: ({ page }) => ({ skills: [skillsShSkill], total: 600, page, hasMore: page < 2 }),
    } });
    fireEvent.click(await screen.findByRole("button", { name: "技能市场" }));
    await screen.findByRole("button", { name: marketplaceSkill.name });
    expect(screen.getByRole("button", { name: "SkillHub" })).toHaveAttribute("aria-pressed", "true");
    expect(backend.count("list_skills_sh")).toBe(0);
    fireEvent.change(screen.getByLabelText("搜索 SkillHub"), { target: { value: "hub draft" } });
    const hubList = screen.getByRole("main", { name: "技能市场" });
    fireEvent.scroll(hubList, { target: { scrollTop: 120 } });
    fireEvent.click(screen.getByRole("button", { name: "skills.sh" }));
    await screen.findByRole("button", { name: skillsShSkill.name });
    fireEvent.click(screen.getByRole("button", { name: "24 小时趋势榜" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "下一页" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "下一页" }));
    await screen.findByText(/榜单可浏览 600 项 · 第 2 页/);
    fireEvent.change(screen.getByLabelText("搜索 skills.sh"), { target: { value: "sh draft" } });
    const list = screen.getByRole("main", { name: "技能市场" });
    fireEvent.scroll(list, { target: { scrollTop: 420 } });
    const opener = screen.getByRole("button", { name: skillsShSkill.name });
    fireEvent.click(opener);
    fireEvent.click(await screen.findByRole("button", { name: "返回列表" }));
    expect(list.scrollTop).toBe(420);
    expect(opener).toHaveFocus();
    fireEvent.click(screen.getByRole("button", { name: "SkillHub" }));
    expect(screen.getByLabelText("搜索 SkillHub")).toHaveValue("hub draft");
    expect(hubList.scrollTop).toBe(120);
    fireEvent.click(screen.getByRole("button", { name: "skills.sh" }));
    expect(screen.getByLabelText("搜索 skills.sh")).toHaveValue("sh draft");
    expect(list.scrollTop).toBe(420);
    expect(screen.queryByRole("group", { name: "市场类型" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "专家包" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "SkillHub" }));
    fireEvent.click(screen.getByRole("button", { name: "专家包" }));
    expect(within(screen.getByRole("group", { name: "市场类型" })).getByText("来源 · SkillHub")).toBeVisible();
    fireEvent.click(within(screen.getByRole("group", { name: "市场类型" })).getByRole("button", { name: "技能" }));
    expect(screen.getByRole("button", { name: "SkillHub" })).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(screen.getByRole("button", { name: "skills.sh" }));
    expect(screen.getByRole("button", { name: "skills.sh" })).toHaveAttribute("aria-pressed", "true");
    expect(localStorage.getItem("skills-box.marketplace-source")).toBe("skills-sh");
    fireEvent.click(screen.getByRole("button", { name: "返回 skills" }));
    fireEvent.click(screen.getByRole("button", { name: "技能市场" }));
    expect(screen.getByLabelText("搜索 skills.sh")).toHaveValue("sh draft");
    expect(screen.getByRole("main", { name: "技能市场" }).scrollTop).toBe(420);
    expect(backend.callsOf("list_skills_sh").slice(-1)[0]!.payload).toEqual({ keyword: "", view: "trending", page: 1 });
  });

  it("restores skills.sh on a fresh app and searches in upstream order without fake pagination", async () => {
    localStorage.setItem("skills-box.marketplace-source", "skills-sh");
    const rows = Array.from({ length: 200 }, (_, index) => ({ ...skillsShSkill,
      name: `result-${200 - index}`, coordinate: { ...skillsShSkill.coordinate, skillId: `result-${index}` },
    }));
    const { backend } = setup({ commands: {
      list_skills_sh: ({ keyword, page }) => ({ skills: keyword ? rows : [skillsShSkill], total: 200, page, hasMore: !keyword }),
    } });
    await openMarket();
    expect(backend.count("list_marketplace_skills")).toBe(0);
    const input = screen.getByLabelText("搜索 skills.sh");
    fireEvent.change(input, { target: { value: "x" } });
    expect(screen.getByRole("button", { name: "搜索" })).toBeDisabled();
    fireEvent.change(input, { target: { value: "notes" } });
    fireEvent.click(screen.getByRole("button", { name: "搜索" }));
    await screen.findByText("已达到搜索返回上限，请缩小搜索范围。");
    expect(screen.getAllByRole("article").map((card) => within(card).getByRole("button", { name: /^result-/ }).textContent)).toEqual(rows.map((skill) => skill.name));
    expect(screen.queryByRole("button", { name: "下一页" })).not.toBeInTheDocument();
    expect(backend.callsOf("list_skills_sh").slice(-1)[0]!.payload).toEqual({ keyword: "notes", view: "all-time", page: 0 });
    fireEvent.click(screen.getByRole("button", { name: "热榜" }));
    await screen.findByRole("button", { name: skillsShSkill.name });
    expect(screen.getByLabelText("搜索 skills.sh")).toHaveValue("");
    expect(screen.getByText(/热榜 · 榜单可浏览/)).toHaveTextContent("最近一小时安装量");
  });

  it("labels the old query after a failed search and never substitutes SkillHub results", async () => {
    let failed = true;
    const { backend } = setup({ commands: {
      list_skills_sh: ({ keyword, page }) => {
        if (keyword && failed) throw { code: "marketplace.unavailable", message: "目录读取失败", detail: "HTTP 503" };
        return { skills: [skillsShSkill], total: 1, page, hasMore: false };
      },
    } });
    await openMarket();
    fireEvent.change(screen.getByLabelText("搜索 skills.sh"), { target: { value: "missing" } });
    fireEvent.click(screen.getByRole("button", { name: "搜索" }));
    await screen.findByText(/保留上次成功结果：累计榜，第 1 页/);
    expect(screen.getByRole("button", { name: skillsShSkill.name })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: marketplaceSkill.name })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "官方页面" }));
    expect(backend.callsOf("open_external_url").slice(-1)[0]!.payload.url).toBe("https://www.skills.sh/");
    failed = false;
    fireEvent.click(screen.getByRole("button", { name: "重试" }));
    await screen.findByText(/搜索「missing」 · 返回 1 项/);
    expect(screen.queryByText(/保留上次成功结果/)).not.toBeInTheDocument();
  });

  it("ignores a late search response after changing the query and marketplace source", async () => {
    let release: ((listing: SkillsShListing) => void) | undefined;
    setup({ commands: { list_skills_sh: ({ keyword }) => keyword === "slow"
      ? new Promise((resolve) => { release = resolve; })
      : { skills: [{ ...skillsShSkill, name: keyword || skillsShSkill.name }], total: 1, page: 0, hasMore: false },
    } });
    await openMarket();
    const input = screen.getByLabelText("搜索 skills.sh");
    fireEvent.change(input, { target: { value: "slow" } });
    fireEvent.click(screen.getByRole("button", { name: "搜索" }));
    await waitFor(() => expect(release).toBeDefined());
    fireEvent.change(input, { target: { value: "new-query" } });
    fireEvent.click(screen.getByRole("button", { name: "搜索" }));
    await screen.findByRole("button", { name: "new-query" });
    fireEvent.click(screen.getByRole("button", { name: "SkillHub" }));
    await act(async () => release!({ skills: [{ ...skillsShSkill, name: "obsolete" }], total: 1, page: 0, hasMore: false }));
    expect(screen.getByRole("button", { name: marketplaceSkill.name })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "skills.sh" }));
    expect(screen.getByRole("button", { name: "new-query" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "obsolete" })).not.toBeInTheDocument();
  });

  it("browses one read-only snapshot, handles absent files and replaces the entire snapshot on refresh", async () => {
    let revision = 0;
    setup({ commands: { get_skills_sh_snapshot: () => {
      if (revision === 1) throw { code: "marketplace.unavailable", message: "暂时离线" };
      return revision === 2 ? { hash: "new", files: [{ path: "SKILL.md", contents: "# New snapshot", sizeBytes: 14 }] }
        : { ...skillsShSnapshot, files: skillsShSnapshot.files!.map((file) => file.path === "SKILL.md"
          ? { ...file, contents: `${file.contents}\n\n[快照未收录](references/missing.md)` } : file) };
    } } });
    const detail = await openDetail();
    await within(detail).findByText("记录项目决策与后续事项。");
    expect(within(detail).getByText(/可能与 Git 上游当前内容不同/)).toBeInTheDocument();
    fireEvent.click(within(detail).getByRole("link", { name: "快照未收录" }));
    expect(within(detail).getByText(/此快照未包含该文件/)).toBeInTheDocument();
    fireEvent.click(within(detail).getByRole("tab", { name: "概述" }));
    fireEvent.click(within(detail).getByRole("link", { name: "阅读指南" }));
    await within(detail).findByRole("heading", { name: "笔记指南" });
    expect(within(detail).getByRole("tab", { name: "文件" })).toHaveAttribute("aria-selected", "true");
    expect(within(detail).getByRole("region", { name: "文件预览" })).toHaveFocus();
    revision = 1;
    fireEvent.click(within(detail).getByRole("button", { name: "刷新内容快照" }));
    await within(detail).findByText(/保留上次成功的快照/);
    expect(within(detail).getByRole("heading", { name: "笔记指南" })).toBeInTheDocument();
    revision = 2;
    fireEvent.click(within(detail).getByRole("button", { name: "重试内容快照" }));
    await within(detail).findByRole("heading", { name: "New snapshot" });
    fireEvent.click(within(detail).getByRole("tab", { name: "文件" }));
    expect(within(detail).queryByText("guide.md")).not.toBeInTheDocument();
    expect(within(detail).queryByText(/保留上次成功的快照/)).not.toBeInTheDocument();
  });

  it("can browse without Git dependencies and keeps website entries outside the installer", async () => {
    const { backend } = setup({ data: { environment: nodeMissingReport() } });
    const detail = await openDetail();
    await within(detail).findByText("记录项目决策与后续事项。");
    expect(within(detail).getByRole("button", { name: "安装" })).toBeDisabled();
    const before = backend.count("get_environment");
    fireEvent.click(within(detail).getByRole("button", { name: "重新检测环境" }));
    await waitFor(() => expect(backend.count("get_environment")).toBeGreaterThan(before));
    fireEvent.click(within(detail).getByRole("button", { name: "返回列表" }));
    fireEvent.click(screen.getByRole("button", { name: skillsShWebsiteSkill.name }));
    const website = await screen.findByRole("main", { name: skillsShWebsiteSkill.name });
    expect(within(website).getByText(/此来源暂不支持应用内安装，可前往官方页面查看/)).toBeInTheDocument();
    expect(within(website).queryByRole("button", { name: "GitHub 仓库" })).not.toBeInTheDocument();
    expect(within(website).getByRole("button", { name: "安装" })).toBeDisabled();
    expect(backend.count("start_preview")).toBe(0);
    expect(backend.count("get_marketplace_cli")).toBe(0);
  });

  it("can retry Git preview when snapshot retrieval and the first Git preview both fail", async () => {
    const { backend } = setup({ commands: {
      get_skills_sh_snapshot: () => { throw { code: "marketplace.unavailable", message: "快照离线" }; },
    } });
    const detail = await openDetail();
    await within(detail).findByText(/未获取到可浏览的快照/);
    fireEvent.click(within(detail).getByRole("button", { name: "安装" }));
    await waitFor(() => expect(backend.count("start_preview")).toBe(1));
    const task = backend.tasks.filter((task) => task.command === "start_preview")[0];
    act(() => task.emit({ taskId: task.taskId, phase: "failed", message: "Git 无法连接", error: {
      code: "task.preview_failed", message: "Git 无法连接", retryable: true,
    } }));
    const wizard = screen.getByTestId("install-wizard");
    await within(wizard).findByText("Git 无法连接");
    expect(within(wizard).queryByRole("button", { name: "下一步" })).not.toBeInTheDocument();
    expect(backend.count("start_install")).toBe(0);
    await waitFor(() => expect(within(wizard).getByRole("button", { name: "预览仓库" })).toBeEnabled());
    fireEvent.click(within(wizard).getByRole("button", { name: "预览仓库" }));
    await waitFor(() => expect(backend.count("start_preview")).toBe(2));
    completePreview(backend, { ...skillsShPreview, skills: [{ name: "PROJECT-NOTES", description: "actual Git name" }] });
    await within(wizard).findByRole("checkbox", { name: "选择 Claude Code" });
    fireEvent.click(within(wizard).getByRole("button", { name: "下一步" }));
    await waitFor(() => expect(within(wizard).getByRole("button", { name: "开始安装" })).toBeEnabled());
    const command = backend.callsOf("preview_cli_commands").filter((call) => call.payload.request.action === "install").slice(-1)[0];
    expect(command.payload.request).toMatchObject({ request: { skills: ["PROJECT-NOTES"] } });
    expect(backend.count("invalidate_discovery_source")).toBe(0);
  });

  it.each(["global", discoveryProjectScope] as const)("installs exactly the selected Git skill in %s despite a missing snapshot", async (scope) => {
    const { backend } = setup({ autoTasks: { delayMs: 5 }, data: {
      projects: { projects: [discoveryProject], lastActiveScope: scope, warnings: [] },
      snapshots: { [discoveryProjectScope]: makeSnapshot() },
    }, commands: { get_skills_sh_snapshot: () => ({ files: null, hash: null }) } });
    const detail = await openDetail();
    await within(detail).findByText(/仍可通过 Git 仓库预览后安装/);
    fireEvent.click(within(detail).getByRole("button", { name: "安装" }));
    const wizard = await screen.findByTestId("install-wizard");
    await within(wizard).findByRole("checkbox", { name: "选择 Claude Code" });
    expect(within(wizard).queryByText("other-skill")).not.toBeInTheDocument();
    expect(within(wizard).queryByRole("button", { name: "全选" })).not.toBeInTheDocument();
    expect(within(wizard).getByText(/按 Git 默认分支安装/)).toBeInTheDocument();
    expect(detail.closest(".marketplace-workspace")).toHaveAttribute("inert");
    fireEvent.click(within(wizard).getByRole("checkbox", { name: "选择 Claude Code" }));
    fireEvent.click(within(wizard).getByRole("button", { name: "下一步" }));
    await waitFor(() => expect(within(wizard).getByRole("button", { name: "开始安装" })).toBeEnabled());
    const command = backend.callsOf("preview_cli_commands").filter((call) => call.payload.request.action === "install").slice(-1)[0]!;
    expect(command.payload).toMatchObject({ scope, request: { action: "install", request: {
      source: "example/skills", skills: ["project-notes"], agents: ["claude-code"],
    } } });
    fireEvent.click(within(wizard).getByRole("button", { name: "开始安装" }));
    await within(wizard).findByRole("button", { name: "完成" });
    expect(backend.callsOf("start_install")[0].payload).toMatchObject({ scope, request: {
      source: "example/skills", skills: ["project-notes"], agents: ["claude-code"],
    } });
    fireEvent.click(within(wizard).getByRole("button", { name: "完成" }));
    await within(detail).findByRole("button", { name: "查看已安装技能" });
    expect(backend.count("prepare_marketplace_install")).toBe(0);
    expect(backend.count("get_marketplace_cli")).toBe(0);
    expect(backend.count("invalidate_discovery_source")).toBe(0);
    const inventory = await backend.dispatch("get_inventory", { scope });
    expect(inventory.skills.map((skill) => skill.name)).toEqual(["project-notes"]);
    expect(inventory.skills[0].sourceType).toBe("github");
  });

  it.each(["missing", "duplicates", "parseFailed"])("blocks %s Git matches without invalidating discovery history", async (mode) => {
    const { backend } = setup();
    const detail = await openDetail();
    fireEvent.click(within(detail).getByRole("button", { name: "安装" }));
    await waitFor(() => expect(backend.count("start_preview")).toBe(1));
    completePreview(backend, { ...skillsShPreview, parseFailed: mode === "parseFailed",
      skills: mode === "missing" ? [{ name: "renamed", description: "" }] : [...skillsShPreview.skills, { name: "PROJECT-NOTES", description: "duplicate" }],
    });
    const wizard = screen.getByTestId("install-wizard");
    await within(wizard).findByText(/此市场条目在 Git 预览中不存在/);
    expect(within(wizard).getByRole("button", { name: "下一步" })).toBeDisabled();
    expect(within(wizard).queryByLabelText("手动输入 skill 名")).not.toBeInTheDocument();
    expect(within(wizard).queryByText("other-skill")).not.toBeInTheDocument();
    expect(backend.count("invalidate_discovery_source")).toBe(0);
    expect(backend.count("start_install")).toBe(0);
  });

  it.each([
    { source: "https://github.com/Example/Skills.git", management: "managed" as const },
    { source: "https://www.github.com/Example/Skills.git/", management: "managed" as const },
    { source: "Example/Skills", management: "copy_only" as const },
  ])("recognizes direct Git installs ($source, $management) in the correct scope", async ({ source, management }) => {
    setup({ data: {
      snapshot: makeSnapshot({ skills: [makeSkill({ name: "PROJECT-NOTES", source, management, sourceType: "git" })] }),
      snapshots: { [discoveryProjectScope]: makeSnapshot() },
    } });
    const detail = await openDetail();
    expect(within(detail).getByRole("button", { name: "查看已安装技能" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "切换到项目 示例项目", hidden: true }));
    await waitFor(() => expect(within(detail).getByRole("button", { name: "安装" })).toBeEnabled());
    expect(within(detail).getByText("安装到：项目 · 示例项目")).toBeInTheDocument();
  });

  it("retains conflicts found by Git preview and never offers another repository skill", async () => {
    const { backend } = setup();
    const detail = await openDetail();
    fireEvent.click(within(detail).getByRole("button", { name: "安装" }));
    await waitFor(() => expect(backend.count("start_preview")).toBe(1));
    completePreview(backend, { ...skillsShPreview, skills: skillsShPreview.skills.map((skill) => skill.name === skillsShSkill.name
      ? { ...skill, conflict: { management: "managed", source: "old/repository" } } : skill) });
    const wizard = screen.getByTestId("install-wizard");
    expect(await within(wizard).findByRole("checkbox", { name: "选择 project-notes" })).toBeDisabled();
    expect(within(wizard).getByRole("button", { name: "下一步" })).toBeDisabled();
    expect(within(wizard).queryByRole("checkbox", { name: "选择 other-skill" })).not.toBeInTheDocument();
    expect(backend.count("start_install")).toBe(0);
    expect(backend.count("invalidate_discovery_source")).toBe(0);
  });

  it.each(["skillhub", "unmanaged", "other-repo"])("blocks same-name %s collisions without launching Git", async (kind) => {
    const { backend } = setup({ data: { snapshot: makeSnapshot({ skills: [makeSkill({ name: "project-notes",
      source: kind === "other-repo" ? "other/skills" : "example/skills",
      sourceType: kind === "skillhub" ? "skillhub" : "github", management: kind === "unmanaged" ? "unmanaged" : "managed",
    })] }) } });
    const detail = await openDetail();
    expect(within(detail).getByRole("alert")).toHaveTextContent("当前作用域已有同名技能");
    expect(within(detail).getByRole("button", { name: "安装" })).toBeDisabled();
    expect(backend.count("start_preview")).toBe(0);
  });
});
