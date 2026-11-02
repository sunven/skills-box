import { StrictMode } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import App from "./App";
import { createQueryClient } from "./lib/query-client";
import { discoveryQueryKey, filterDiscoveries } from "./lib/discovery";
import type { PreviewSummary, TaskEvent } from "./lib/types";
import { createFakeBackend, type FakeBackend } from "./test/fake-backend";
import { discovered, discoveryFixture, discoveryProject, discoveryProjectScope } from "./test/discovery-fixtures";
import { healthyReport } from "./test/fixtures";

function setup(options: Parameters<typeof createFakeBackend>[0] = {}) {
  const backend = createFakeBackend({ ...options, data: { environment: healthyReport(), discovery: discoveryFixture(),
    projects: { projects: [discoveryProject], lastActiveScope: "global", warnings: [] }, ...options.data } }).install();
  const client = createQueryClient();
  render(<StrictMode><QueryClientProvider client={client}><App /></QueryClientProvider></StrictMode>);
  return { backend, client };
}
async function openDiscovery() {
  fireEvent.click(await screen.findByRole("button", { name: "发现 (2)" }));
  await screen.findByRole("heading", { name: "发现" });
}
function completePreview(backend: FakeBackend, preview: PreviewSummary) {
  const task = backend.tasks.filter((t) => t.command === "start_preview").slice(-1)[0]!;
  const event: TaskEvent = { taskId: task.taskId, phase: "completed", message: "预览完成", result: {
    action: "preview", scope: backend.callsOf("start_preview").slice(-1)[0]!.payload.scope, overall: "success", skills: [], warnings: [],
    startedAt: 0, finishedAtEpochMs: 1, preview } };
  act(() => task.emit(event));
}
const preview: PreviewSummary = { source: "acme/skills", parseFailed: false, noSkills: false,
  skills: ["alpha", "beta", "gamma"].map((name) => ({ name, description: `${name} description` })) };

describe("source discovery", () => {
  it("projects the 60/20 baseline and scope-independent unread filtering", () => {
    const skills = Array.from({ length: 60 }, (_, i) => discovered(`skill-${i}`, i < 20 ? "installed" : "notInstalled"));
    expect(filterDiscoveries(skills, "global", "uninstalled", "", "all")).toHaveLength(40);
    expect(filterDiscoveries(skills, "global", "new", "", "all")).toHaveLength(0);
    skills.push(discovered("new", "notInstalled", { unread: true, isNew: true }));
    expect(filterDiscoveries(skills, "global", "uninstalled", "", "all")).toHaveLength(41);
    expect(filterDiscoveries(skills, "global", "new", "", "all")).toHaveLength(1);
  });
  it("shows current scope uninstalled by default and all badge items in the new view without acknowledging the list", async () => {
    const { backend } = setup(); await openDiscovery();
    expect(screen.getByRole("button", { name: "gamma" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "beta" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "查看全部新增" }));
    expect(screen.getByRole("button", { name: "beta" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "安装 beta" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("发现目标作用域"), { target: { value: discoveryProjectScope } });
    expect(screen.getByRole("button", { name: "beta" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "发现 (2)" })).toBeInTheDocument();
    expect(backend.count("acknowledge_discoveries")).toBe(0);
    fireEvent.click(screen.getByRole("button", { name: "返回 skills" }));
    expect(await screen.findByRole("button", { name: "发现 (2)" })).toBeInTheDocument();
  });
  it("acknowledges details, persists ignore/restore and handles all read independently of search", async () => {
    const { backend } = setup(); await openDiscovery();
    fireEvent.click(screen.getByRole("button", { name: "gamma" }));
    expect(await screen.findByRole("complementary", { name: "发现项详情" })).toHaveTextContent("示例项目");
    await screen.findByRole("button", { name: "发现 (1)" });
    fireEvent.click(screen.getByRole("button", { name: "忽略 gamma" }));
    await waitFor(() => expect(screen.queryByRole("button", { name: "gamma" })).not.toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "已忽略" }));
    fireEvent.click(screen.getByRole("button", { name: "恢复 gamma" }));
    await waitFor(() => expect(screen.queryByRole("button", { name: "gamma" })).not.toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "全部" }));
    await screen.findByRole("button", { name: "gamma" });
    fireEvent.change(screen.getByLabelText("搜索发现的 skills"), { target: { value: "no-match" } });
    fireEvent.click(screen.getByRole("button", { name: "全部已读" }));
    await screen.findByRole("button", { name: "发现 (0)" });
    expect(backend.callsOf("acknowledge_discoveries").slice(-1)[0]!.payload.ids).toBeNull();
  });
  it("previews the clicked source once and preselects only its intent; scope changes preview again", async () => {
    const { backend } = setup(); await openDiscovery();
    fireEvent.click(screen.getByRole("button", { name: "安装 gamma" }));
    await waitFor(() => expect(backend.callsOf("start_preview")).toHaveLength(1));
    expect(backend.callsOf("start_preview")[0]!.payload).toMatchObject({ source: "acme/skills", scope: "global" });
    completePreview(backend, preview);
    const rows = screen.getAllByTestId("preview-skill");
    for (const row of rows) expect(within(row).getByRole("checkbox")).toHaveProperty("checked", row.dataset.skillName === "gamma");
    expect(backend.count("start_install")).toBe(0);
    fireEvent.change(screen.getByLabelText("目标作用域"), { target: { value: discoveryProjectScope } });
    await waitFor(() => expect(backend.callsOf("start_preview")).toHaveLength(2));
    completePreview(backend, preview);
    expect(backend.callsOf("start_preview")[1]!.payload.scope).toBe(discoveryProjectScope);
    expect(backend.count("start_install")).toBe(0);
    fireEvent.click(screen.getByRole("button", { name: "关闭安装向导" }));
    expect(screen.getByRole("heading", { name: "发现" })).toBeInTheDocument();
  });
  it.each(["missing", "duplicates", "parseFailed"])("blocks a %s discovery intent and invalidates the cached catalog", async (mode) => {
    const { backend } = setup(); await openDiscovery(); fireEvent.click(screen.getByRole("button", { name: "安装 gamma" }));
    await waitFor(() => expect(backend.count("start_preview")).toBe(1));
    completePreview(backend, mode === "parseFailed" ? { ...preview, parseFailed: true, rawOutput: "unknown" } : { ...preview,
      skills: mode === "missing" ? preview.skills.slice(0, 2) : [...preview.skills, { name: "GAMMA", description: "duplicate" }] });
    expect(await screen.findByText(/此发现项在实际预览中已不存在/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "下一步" })).toBeDisabled();
    await waitFor(() => expect(backend.count("invalidate_discovery_source")).toBe(1));
    expect(backend.count("start_install")).toBe(0);
  });
  it("preserves reminder state and reports a failed user state write", async () => {
    setup({ commands: { acknowledge_discoveries: () => { throw { code: "discovery.write_failed", message: "发现记录保存失败", retryable: true }; } } });
    await openDiscovery(); fireEvent.click(screen.getByRole("button", { name: "全部已读" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("发现记录保存失败");
    expect(screen.getByRole("button", { name: "发现 (2)" })).toBeInTheDocument();
  });
  it("manual scan is application-level while unknown/conflicting scopes disable install", async () => {
    const { backend, client } = setup({ data: { discovery: discoveryFixture({ skills: [discovered("gamma", "unknown", { unread: true })] }) } });
    await openDiscovery(); expect(screen.getByRole("button", { name: "安装 gamma" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "立即检查上游" }));
    await waitFor(() => expect(backend.callsOf("start_discovery_scan").some((c) => c.payload.manual === true)).toBe(true));
    backend.update({ discovery: discoveryFixture({ skills: [discovered("gamma", "conflict", { unread: true })] }) });
    await act(async () => { await client.invalidateQueries({ queryKey: discoveryQueryKey }); });
    expect(await screen.findByText("同名冲突")).toBeInTheDocument(); expect(screen.getByRole("button", { name: "安装 gamma" })).toBeDisabled();
  });
});
