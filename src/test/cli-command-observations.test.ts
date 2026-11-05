import { Channel } from "@tauri-apps/api/core";
import { describe, expect, it, vi } from "vitest";
import { createFakeBackend } from "./fake-backend";
import { makeSkill, makeSnapshot } from "./inventory-fixtures";
import { discoveryFixture } from "./discovery-fixtures";
import type { TaskEvent } from "../lib/types";

describe("CLI invocation observations", () => {
  it("rejects a stale command before registering a fake task", () => {
    const backend = createFakeBackend();
    expect(() => backend.dispatch("start_install", { scope: "global", request: { source: "owner/repo", skills: ["alpha"], agents: [] }, commandPreviewId: "expired", onEvent: new Channel<TaskEvent>() })).toThrow();
    expect(backend.tasks).toHaveLength(0);
    expect(backend.dispatch("get_cli_commands", {})).toEqual([]);
  });

  it("retains environment calls on cache hits and replaces them on refresh", () => {
    const backend = createFakeBackend();
    backend.dispatch("get_environment", { refresh: false });
    const first = backend.dispatch("get_cli_commands", {});
    backend.dispatch("get_environment", { refresh: false });
    expect(backend.dispatch("get_cli_commands", {})).toEqual(first);
    backend.dispatch("get_environment", { refresh: true });
    expect(backend.dispatch("get_cli_commands", {})).toHaveLength(1);
    expect(first).toMatchObject([{ context: { origin: "environment" }, command: expect.stringContaining("skills@latest --version") }]);
  });

  it("records a discovery list invocation only when the catalog needs scanning", () => {
    const discovery = discoveryFixture();
    const backend = createFakeBackend({ data: { discovery } });
    backend.dispatch("start_discovery_scan", {});
    expect(backend.dispatch("get_cli_commands", {})).toEqual([]);
    backend.update({ discovery: { ...discovery, sources: discovery.sources.map((source) => ({ ...source, needsRevalidation: true })) } });
    backend.dispatch("start_discovery_scan", {});
    const calls = backend.dispatch("get_cli_commands", {});
    expect(calls).toMatchObject([{ status: "succeeded", context: { origin: "discovery", source: "acme/skills" }, command: expect.stringContaining("skills@latest add acme/skills -l -g") }]);
    backend.dispatch("start_discovery_scan", {});
    expect(backend.dispatch("get_cli_commands", {})).toEqual(calls);
  });

  it("does not report a cancelled task or a native uninstall as a CLI invocation", () => {
    const backend = createFakeBackend({ data: { snapshot: makeSnapshot({ skills: [makeSkill({ name: "local", management: "unmanaged" })] }) } }).install();
    backend.dispatch("start_uninstall", { scope: "global", skillId: "local", onEvent: new Channel<TaskEvent>() });
    backend.lastTask!.emit({ taskId: backend.lastTask!.taskId, phase: "running_cli", message: "删除文件" });
    expect(backend.dispatch("get_cli_commands", {})).toEqual([]);
    backend.dispatch("start_preview", { scope: "global", source: "owner/repo", onEvent: new Channel<TaskEvent>() });
    backend.lastTask!.emit({ taskId: backend.lastTask!.taskId, phase: "cancelled", message: "取消" });
    expect(backend.dispatch("get_cli_commands", {})).toEqual([]);
  });

  it("records only attempted batch targets when later targets are stopped", async () => {
    vi.useFakeTimers();
    try {
      const backend = createFakeBackend({ data: { snapshot: makeSnapshot({ skills: ["alpha", "beta"].map((name) => makeSkill({ name, source: "owner/repo" })) }) }, autoTasks: { delayMs: 10 } }).install();
      const preview = await backend.dispatch("preview_batch", { scope: "global", request: { action: "uninstall", skillIds: ["alpha", "beta", "missing"], agentIds: [] } });
      const taskId = await backend.dispatch("start_batch", { scope: "global", confirmation: { previewId: preview.id, revision: preview.revision, confirmedDelete: true, confirmedCopies: true }, onEvent: new Channel<TaskEvent>() });
      await vi.advanceTimersByTimeAsync(10);
      backend.dispatch("cancel_task", { taskId });
      await vi.runAllTimersAsync();
      const records = await backend.dispatch("get_cli_commands", {});
      expect(records).toHaveLength(1);
      expect(records[0]).toMatchObject({ status: "succeeded", command: preview.targets[0]!.command });
      expect(records[0]!.command).toContain("-s alpha");
      backend.reset();
      expect(backend.dispatch("get_cli_commands", {})).toEqual([]);
    } finally {
      vi.useRealTimers();
    }
  });
});
