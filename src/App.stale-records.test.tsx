import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import App from "./App";
import { createQueryClient } from "./lib/query-client";
import { updateReportQueryKey } from "./lib/update-view";
import { listStaleRecords, getInventory, getDiscoveryState } from "./lib/api";
import type { StaleRecord } from "./lib/batch";
import { createFakeBackend, type FakeBackendOptions } from "./test/fake-backend";
import { makeSkill, makeSnapshot } from "./test/inventory-fixtures";
import { healthyReport } from "./test/fixtures";
import { discovered, discoveryFixture, discoveryProject, discoveryProjectScope } from "./test/discovery-fixtures";

const records: StaleRecord[] = Array.from({ length: 21 }, (_, index) => ({
  id: `missing-${index + 1}`, name: `missing-${index + 1}`, source: "original/skills", reference: "release/stable", cleanReason: null, restoreReason: null,
}));
function setup(options: FakeBackendOptions = {}, selected = records) {
  const snapshot = makeSnapshot({ skills: [makeSkill({ name: "retained", source: "other/skills" })], anomalies: {
    ...makeSnapshot().anomalies, total: selected.length, stale: { count: selected.length, examples: selected.slice(0, 3).map((r) => r.name) },
  } });
  const backend = createFakeBackend({ autoTasks: { delayMs: 5 }, ...options, data: { snapshot, staleRecords: { global: selected }, ...options.data } }).install();
  const client = createQueryClient();
  render(<QueryClientProvider client={client}><App /></QueryClientProvider>);
  return { backend, client, snapshot };
}
async function openRecords() {
  fireEvent.click(await screen.findByRole("button", { name: /异常提示：异常/ }));
  fireEvent.click(screen.getByRole("button", { name: "处理" }));
  return screen.findByTestId("stale-records-dialog");
}
async function submit(name: string) {
  const button = await screen.findByRole("button", { name });
  await waitFor(() => expect(button).toBeEnabled()); fireEvent.click(button);
}

describe("失效安装记录", () => {
  it("opens all 21 records unselected and cleans them without Node, preserving unrelated rows", async () => {
    const report = { skills: [{ name: "retained", state: "current" as const, source:"other/skills", ref:null, lockHash:null, remoteHash:null, cached:false, lastSuccessAtEpochMs:null, previousState:null, error:null, note:null }], sources: [], warnings: [], checkedAtEpochMs:Date.now(), lastSuccessAtEpochMs:Date.now(), skippedSourceCount:0, cachedSourceCount:0 };
    const { backend, client } = setup({ data: { environment: { ...healthyReport(), mutationsEnabled: false }, updateReport:report } });
    client.setQueryData(updateReportQueryKey("global"), report);
    const dialog = await openRecords();
    await waitFor(() => expect(within(dialog).getAllByTestId("stale-record")).toHaveLength(21));
    expect(within(dialog).getByRole("checkbox", { name: "全选失效记录" })).not.toBeChecked();
    expect(within(dialog).getByRole("button", { name: "清理记录" })).toBeDisabled();
    fireEvent.click(within(dialog).getByRole("checkbox", { name: "全选失效记录" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "清理记录" }));
    await submit("清理 21 条记录");
    await screen.findByText("记录清理完成");
    expect(backend.callsOf("start_batch")).toHaveLength(1);
    expect(await listStaleRecords("global")).toEqual([]);
    expect((await getInventory("global")).skills.map((s) => s.name)).toEqual(["retained"]);
    expect(client.getQueryData(updateReportQueryKey("global"))).toEqual(report);
    expect(backend.callsOf("start_install")).toHaveLength(0);
  });

  it("restores from the original source/ref and defaults to canonical only", async () => {
    const { backend } = setup({}, records.slice(0, 1));
    const dialog = await openRecords();
    fireEvent.click(await within(dialog).findByRole("checkbox", { name: "选择记录 missing-1" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "恢复技能" }));
    await screen.findByText("original/skills · release/stable");
    expect(screen.getByRole("checkbox", { name: "选择 Agent Claude Code" })).not.toBeChecked();
    await submit("恢复 1 个技能");
    await screen.findByText("技能恢复完成");
    const skill = (await getInventory("global")).skills.find((s) => s.name === "missing-1")!;
    expect(skill.source).toBe("original/skills"); expect(skill.attachments.filter((a) => a.mode !== "native")).toEqual([]);
    expect(await listStaleRecords("global")).toEqual([]);
    expect(backend.callsOf("preview_batch")[backend.callsOf("preview_batch").length - 1]?.payload.request.agentIds).toEqual([]);
    expect(backend.callsOf("start_install")).toHaveLength(0);
  });

  it("counts only eligible records and retains blocked records in the result", async () => {
    setup({}, [records[0]!, { ...records[1]!, cleanReason: "目录无法完整读取", restoreReason: "目录无法完整读取" }]);
    const dialog = await openRecords();
    fireEvent.click(await within(dialog).findByRole("checkbox", { name: "全选失效记录" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "清理记录" }));
    await submit("清理 1 条记录");
    await screen.findByText("记录部分清理");
    expect((await listStaleRecords("global")).map((r) => r.name)).toEqual(["missing-2"]);
    expect(screen.getAllByText("目录无法完整读取").length).toBeGreaterThan(0);
  });

  it("rejects files appearing after preview without overwriting them", async () => {
    const { backend, snapshot } = setup({}, records.slice(0, 1));
    const dialog = await openRecords();
    fireEvent.click(await within(dialog).findByRole("checkbox", { name: "选择记录 missing-1" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "恢复技能" }));
    const button = await screen.findByRole("button", { name: "恢复 1 个技能" });
    await waitFor(() => expect(button).toBeEnabled());
    backend.update({ snapshot: { ...snapshot, skills: [...snapshot.skills, makeSkill({ name: "missing-1", source: "external/skills" })] } });
    fireEvent.click(button);
    await screen.findByText(/预览已变化/);
    expect((await getInventory("global")).skills.find((s) => s.name === "missing-1")?.source).toBe("external/skills");
    expect(await listStaleRecords("global")).toHaveLength(1);
  });

  it("refreshes discovery installation status within the current scope and preserves history", async () => {
    const source = records[0]!.source;
    const skill = discovered("missing-1", "stale", { id: { source, name: "missing-1" }, ignored: true, isNew: true,
      installations: [{ scope: "global", label: "Global", status: "stale" }, { scope: discoveryProjectScope, label: "示例项目", status: "installed" }] });
    setup({ data: {
      discovery: discoveryFixture({ skills: [skill], sources: [{ ...discoveryFixture().sources[0]!, source }], unreadCount: 0 }),
      projects: { projects: [discoveryProject], lastActiveScope: "global", warnings: [] },
      snapshots: { [discoveryProjectScope]: makeSnapshot({ skills: [makeSkill({ name: "missing-1", source })] }) },
    } }, records.slice(0, 1));
    const dialog = await openRecords();
    fireEvent.click(await within(dialog).findByRole("checkbox", { name: "全选失效记录" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "清理记录" }));
    await submit("清理 1 条记录");
    await screen.findByText("记录清理完成");
    expect((await getDiscoveryState()).skills).toEqual([{ ...skill, installations: [
      { scope: "global", label: "Global", status: "notInstalled" }, skill.installations[1]!,
    ] }]);
  });

  it("deactivates the last discovery source while retaining its decisions for reactivation", async () => {
    const source = records[0]!.source;
    const skill = discovered("missing-1", "stale", { id: { source, name: "missing-1" }, ignored: true, isNew: true });
    const { backend } = setup({ data: { discovery: discoveryFixture({ skills: [skill], sources: [{ ...discoveryFixture().sources[0]!, source }], unreadCount: 0 }) } }, records.slice(0, 1));
    const dialog = await openRecords();
    fireEvent.click(await within(dialog).findByRole("checkbox", { name: "全选失效记录" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "清理记录" }));
    await submit("清理 1 条记录");
    await screen.findByText("记录清理完成");
    expect(await getDiscoveryState()).toMatchObject({ skills: [], sources: [] });
    backend.update({ staleRecords: { global: [{ ...records[0]!, id: "another", name: "another" }] } });
    expect((await getDiscoveryState()).skills[0]).toMatchObject({ ignored: true, unread: false, isNew: true });
  });
});
