import { StrictMode, type ReactNode } from "react";
import { act, cleanup, renderHook } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useBatchOperation } from "./use-batch-operation";
import { createQueryClient } from "../lib/query-client";
import type { BatchPreview, BatchRow } from "../lib/batch";
import type { TaskResult } from "../lib/types";
import { createFakeBackend, type FakeBackend, type FakeBackendOptions } from "../test/fake-backend";
import { attachment, claudeAgent, makeSkill, makeSnapshot } from "../test/inventory-fixtures";

type Options = Parameters<typeof useBatchOperation>[0];
type Operation = ReturnType<typeof useBatchOperation>;
const qoder = { ...claudeAgent, id: "qoder", displayName: "Qoder" };
const agents = [claudeAgent, qoder];
const snapshot = (modified = false) => makeSnapshot({ agents, skills: [
  makeSkill({ name: "alpha", localModification: modified ? "modified" : "clean",
    attachments: [attachment("claude-code", "copy"), attachment("qoder")] }),
  makeSkill({ name: "beta", attachments: [attachment("claude-code")] }),
] });

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

afterEach(() => { cleanup(); vi.useRealTimers(); });

async function setup(overrides: Partial<Options> = {}, backendOptions: FakeBackendOptions = {}, strict = false) {
  vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
  const backend = createFakeBackend({ ...backendOptions,
    data: { snapshot: snapshot(), ...backendOptions.data } }).install();
  const onRunningChange = vi.fn();
  const onCompleted = vi.fn();
  const options: Options = { scope: "global", action: "uninstall", skillIds: ["alpha"],
    selectedAgents: new Set(), sourceAgents: agents, projectRoot: "", disabled: false,
    onRunningChange, onCompleted, ...overrides };
  const client = createQueryClient();
  const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>
    {strict ? <StrictMode>{children}</StrictMode> : children}
  </QueryClientProvider>;
  const renders: { input: Options; value: Operation }[] = [];
  let hook!: ReturnType<typeof renderHook<Operation, Options>>;
  await act(async () => {
    hook = renderHook((input: Options) => {
      const value = useBatchOperation(input);
      renders.push({ input, value });
      return value;
    }, { wrapper, initialProps: options });
  });
  return { hook, backend, options, onRunningChange, onCompleted, renders };
}

async function tick(ms = 2000) {
  await act(async () => { await vi.advanceTimersByTimeAsync(ms); });
}

function authorize(operation: Operation) {
  act(() => { operation.confirmDeletion(true); operation.confirmCopies(true); });
}

async function finish(backend: FakeBackend, rows: BatchRow[], overrides: Partial<TaskResult> = {}) {
  const task = backend.lastTask!;
  const action = backend.callsOf("preview_batch")[0]!.payload.request.action;
  await act(async () => {
    task.emit({ taskId: task.taskId, phase: overrides.overall === "cancelled" ? "cancelled" : "completed",
      message: "批量操作已结束。", result: {
        action: "batch", scope: "global", overall: "partial", skills: [], warnings: [],
        startedAt: 1, finishedAtEpochMs: 2, batch: { action, rows }, ...overrides,
      } });
  });
}

describe("Batch operation interface", () => {
  it("retains confirmation for the same revision and clears it for a changed revision", async () => {
    const { hook, backend } = await setup();
    authorize(hook.result.current);
    await tick();
    expect(hook.result.current.confirmedDelete).toBe(true);
    expect(hook.result.current.confirmedCopies).toBe(true);
    backend.update({ snapshot: snapshot(true) });
    await tick();
    expect(hook.result.current).toMatchObject({ changed: true, confirmedDelete: false,
      confirmedCopies: false, submitDisabled: true });
    expect(backend.count("refresh_batch_preview")).toBe(2);
  });

  it("waits for count refresh and avoids overlapping refresh cycles", async () => {
    const pending = deferred<BatchPreview>();
    let countId = "";
    let refreshed!: BatchPreview;
    const { hook, backend } = await setup({ action: "detach", selectedAgents: new Set(["claude-code"]) }, {
      commands: {
        preview_batch: async (args, backend) => {
          const next = await backend.dispatch("preview_batch", args);
          if (args.request.agentIds.length === 2) countId = next.id;
          return next;
        },
        refresh_batch_preview: async (args, backend) => {
          const next = await backend.dispatch("refresh_batch_preview", args);
          if (args.previewId === countId) { refreshed = next; return pending.promise; }
          return next;
        },
      },
    });
    authorize(hook.result.current);
    backend.update({ snapshot: snapshot(true) });
    await tick();
    expect(backend.count("refresh_batch_preview")).toBe(2);
    expect(hook.result.current.confirmedCopies).toBe(true);
    await tick();
    expect(backend.count("refresh_batch_preview")).toBe(2);
    await act(async () => { pending.resolve(refreshed); });
    expect(hook.result.current.confirmedCopies).toBe(false);
    expect(hook.result.current.changed).toBe(true);
  });

  it.each(["success", "failure"] as const)("ignores an old preview %s and its loading completion", async (reply) => {
    const old = deferred<BatchPreview>();
    const next = deferred<BatchPreview>();
    const values = new Map<string, BatchPreview>();
    const { hook, backend, options } = await setup({}, { commands: {
      preview_batch: async (args, backend) => {
        const name = args.request.skillIds[0]!;
        values.set(name, await backend.dispatch("preview_batch", args));
        return name === "alpha" ? old.promise : next.promise;
      },
    } });
    await act(async () => { hook.rerender({ ...options, skillIds: ["beta"] }); });
    await act(async () => {
      if (reply === "success") old.resolve(values.get("alpha")!); else old.reject(new Error("old failure"));
    });
    expect(hook.result.current).toMatchObject({ preview: null, loading: true, previewError: null, submitDisabled: true });
    await act(async () => { next.resolve(values.get("beta")!); });
    expect(hook.result.current.loading).toBe(false);
    expect(hook.result.current.preview?.targets.map((target) => target.name)).toEqual(["beta"]);
    expect(backend.count("start_batch")).toBe(0);
  });

  it("invalidates authorization in the first render of a changed selection", async () => {
    const pending = deferred<BatchPreview>();
    const { hook, backend, options, renders } = await setup({}, { commands: {
      preview_batch: (args, backend) => args.request.skillIds[0] === "beta"
        ? pending.promise : backend.dispatch("preview_batch", args),
    } });
    authorize(hook.result.current);
    const previous = hook.result.current;
    expect(previous.submitDisabled).toBe(false);
    const input = { ...options, skillIds: ["beta"] };
    await act(async () => { hook.rerender(input); });
    expect(renders.find((render) => render.input === input)!.value).toMatchObject({
      preview: null, loading: true, confirmedDelete: false, confirmedCopies: false, submitDisabled: true,
    });
    await act(async () => { await previous.confirm(); });
    expect(backend.count("start_batch")).toBe(0);
  });

  it.each(["success", "failure"] as const)("isolates an old count preview %s when available Agents change", async (reply) => {
    const old = deferred<BatchPreview>();
    let oldValue!: BatchPreview;
    const { hook, options } = await setup({ action: "detach", selectedAgents: new Set(["claude-code"]) }, { commands: {
      preview_batch: async (args, backend) => {
        const value = await backend.dispatch("preview_batch", args);
        if (args.request.agentIds.length === 2) { oldValue = value; return old.promise; }
        return value;
      },
    } });
    await act(async () => { hook.rerender({ ...options, sourceAgents: [claudeAgent] }); });
    expect(hook.result.current.agentCounts.get("claude-code")).toBe(1);
    await act(async () => {
      if (reply === "success") old.resolve({ ...oldValue, targets: [...oldValue.targets, ...oldValue.targets] });
      else old.reject(new Error("advisory failure"));
    });
    expect(hook.result.current.agentCounts.get("claude-code")).toBe(1);
    expect(hook.result.current.agentCounts.has("qoder")).toBe(false);
    expect(hook.result.current.previewError).toBeNull();
    expect(hook.result.current.candidates).toHaveLength(1);
  });

  it("clears authorization on refresh failure and recovers without restoring confirmation", async () => {
    let fail = true;
    const { hook, backend } = await setup({}, { commands: { refresh_batch_preview: (args, backend) => {
      if (fail) throw { code: "batch.preview_expired", message: "expired", retryable: true };
      return backend.dispatch("refresh_batch_preview", args);
    } } });
    authorize(hook.result.current);
    await tick();
    expect(hook.result.current).toMatchObject({ previewError: { message: "expired" },
      confirmedDelete: false, confirmedCopies: false, submitDisabled: true });
    fail = false;
    await tick();
    expect(hook.result.current).toMatchObject({ previewError: null, confirmedDelete: false, submitDisabled: true });
    expect(backend.count("start_batch")).toBe(0);
  });

  it("keeps count failures advisory and does not reload for equivalent selection objects", async () => {
    let countId = "";
    const { hook, backend, options } = await setup({ action: "detach", selectedAgents: new Set(["claude-code"]) }, {
      commands: {
        preview_batch: async (args, backend) => {
          const next = await backend.dispatch("preview_batch", args);
          if (args.request.agentIds.length === 2) countId = next.id;
          return next;
        },
        refresh_batch_preview: (args, backend) => {
          if (args.previewId === countId) throw new Error("counts unavailable");
          return backend.dispatch("refresh_batch_preview", args);
        },
      },
    });
    authorize(hook.result.current);
    await act(async () => { hook.rerender({ ...options, sourceAgents: [...agents], skillIds: ["alpha"],
      selectedAgents: new Set(["claude-code"]) }); });
    expect(backend.count("preview_batch")).toBe(2);
    expect(hook.result.current.confirmedCopies).toBe(true);
    backend.update({ snapshot: snapshot(true) });
    await tick();
    expect(hook.result.current).toMatchObject({ changed: true, previewError: null, confirmedCopies: false });
  });

  it.each(["success", "failure"] as const)("ignores a refresh %s after a selection change", async (reply) => {
    const pending = deferred<BatchPreview>();
    const { hook, options } = await setup({}, { commands: {
      refresh_batch_preview: () => pending.promise,
    } });
    const old = hook.result.current.preview!;
    await tick();
    await act(async () => { hook.rerender({ ...options, skillIds: ["beta"] }); });
    authorize(hook.result.current);
    const next = hook.result.current.preview;
    await act(async () => {
      if (reply === "success") pending.resolve(old); else pending.reject(new Error("old refresh"));
    });
    expect(hook.result.current).toMatchObject({ preview: next, previewError: null, confirmedDelete: true, changed: false });
  });

  it.each(["success", "failure"] as const)("ignores a refresh %s after starting and prevents duplicate confirmation", async (reply) => {
    const pending = deferred<BatchPreview>();
    const { hook, backend, onRunningChange } = await setup({}, {
      commands: { refresh_batch_preview: () => pending.promise },
    });
    authorize(hook.result.current);
    const preview = hook.result.current.preview!;
    await tick();
    await act(async () => {
      const current = hook.result.current;
      void current.confirm();
      void current.confirm();
      expect(current.canClose()).toBe(false);
    });
    expect(backend.count("start_batch")).toBe(1);
    expect(onRunningChange.mock.calls).toEqual([[true]]);
    expect(backend.callsOf("start_batch")[0]!.payload.confirmation).toEqual({
      previewId: preview.id, revision: preview.revision, confirmedDelete: true, confirmedCopies: true,
    });
    await act(async () => {
      if (reply === "success") pending.resolve({ ...preview, revision: "late-revision" });
      else pending.reject(new Error("late failure"));
    });
    expect(hook.result.current).toMatchObject({ preview, previewError: null, changed: false, task: { running: true } });
    await tick();
    expect(backend.count("refresh_batch_preview")).toBe(1);
    const rows: BatchRow[] = preview.targets.map((target) => ({ ...target, status: "success", error: null, backupPath: null }));
    await finish(backend, rows);
    expect(onRunningChange.mock.calls).toEqual([[true], [false]]);
    expect(hook.result.current.canClose()).toBe(true);
    expect(hook.result.current.result?.outcome).toBe("success");
  });

  it("reloads after start rejection without automatically executing or reusing authorization", async () => {
    const { hook, backend, onRunningChange } = await setup({}, { commands: {
      start_batch: () => { throw { message: "preview changed", code: "batch.preview_expired", retryable: true }; },
    } });
    authorize(hook.result.current);
    const old = hook.result.current.preview!.id;
    await act(async () => { await hook.result.current.confirm(); });
    expect(hook.result.current).toMatchObject({ loading: false, confirmedDelete: false,
      confirmedCopies: false, viewingResult: false, submitDisabled: true,
      task: { startError: { message: "preview changed" }, running: false } });
    expect(hook.result.current.preview!.id).not.toBe(old);
    expect(backend.count("preview_batch")).toBe(2);
    expect(backend.count("start_batch")).toBe(1);
    expect(onRunningChange.mock.calls).toEqual([[true], [false]]);
  });

  it("reloads a terminal failure without Batch rows and keeps its task error", async () => {
    const { hook, backend, onCompleted } = await setup();
    authorize(hook.result.current);
    const oldId = hook.result.current.preview!.id;
    await act(async () => { void hook.result.current.confirm(); });
    const task = backend.lastTask!;
    await act(async () => { task.emit({ taskId: task.taskId, phase: "failed", message: "failed",
      error: { message: "worker failed", retryable: true } }); });
    expect(hook.result.current.preview!.id).not.toBe(oldId);
    expect(hook.result.current).toMatchObject({ result: null, viewingResult: false,
      confirmedDelete: false, task: { running: false, error: { message: "worker failed" } } });
    expect(onCompleted).not.toHaveBeenCalled();
    expect(backend.count("start_batch")).toBe(1);
  });

  it("merges retry history, retains disappeared Agents and does not complete a now-ineligible failed target", async () => {
    const initial = makeSnapshot({ agents, skills: [
      ...snapshot().skills,
      makeSkill({ name: "gamma" }),
      makeSkill({ name: "delta", attachments: [attachment("claude-code")] }),
    ] });
    const { hook, backend, options, onCompleted } = await setup({ action: "uninstall_agents",
      skillIds: ["alpha", "beta", "gamma", "delta"], selectedAgents: new Set(["qoder", "claude-code"]),
    }, { data: { snapshot: initial } });
    expect(hook.result.current.agentIds).toEqual(["claude-code", "qoder"]);
    authorize(hook.result.current);
    const first: BatchRow[] = hook.result.current.preview!.targets.map((target) => ({ ...target,
      status: !target.eligible ? "skipped" : target.name === "alpha" && target.agentId === "qoder" ? "failed"
        : target.name === "delta" ? "not_executed" : "success",
      error: null, backupPath: target.name === "alpha" ? "first-backup" : null,
    }));
    await act(async () => { void hook.result.current.confirm(); });
    await finish(backend, first, { overall: "cancelled", backupPath: "first-backup" });
    expect(hook.result.current.result?.outcome).toBe("cancelled");
    expect(onCompleted.mock.calls).toEqual([[new Set(["beta"])]]);
    backend.update({ snapshot: { ...initial, agents: [claudeAgent] } });
    await act(async () => {
      hook.result.current.retry();
      hook.rerender({ ...options, sourceAgents: [claudeAgent] });
    });
    expect(hook.result.current).toMatchObject({ retrying: true, viewingResult: false,
      confirmedDelete: false, confirmedCopies: false, successfulTargets: 2 });
    expect(hook.result.current.agentIds).toEqual(["claude-code", "qoder"]);
    const unfinished = first.filter((row) => row.status === "failed" || row.status === "not_executed").map((row) => row.key);
    expect(backend.callsOf("preview_batch").slice(-1)[0]!.payload.request.targetKeys).toEqual(unfinished);
    const second: BatchRow[] = hook.result.current.preview!.targets.map((target) => ({ ...target,
      status: target.eligible ? "success" : "skipped", error: null, backupPath: "second-backup" }));
    authorize(hook.result.current);
    await act(async () => { void hook.result.current.confirm(); });
    const warning = { message: "latest warning", retryable: false };
    await finish(backend, second, { backupPath: "second-backup", logPath: "latest-log", warnings: [warning] });
    expect(onCompleted.mock.calls).toEqual([[new Set(["beta"])], [new Set(["beta", "delta"])]]);
    const result = hook.result.current.result!;
    expect(result).toMatchObject({ outcome: "partial", backupPath: "second-backup", logPath: "latest-log", warnings: [warning] });
    expect(result.batch!.rows.map((row) => row.key)).toEqual(first.map((row) => row.key));
    expect(result.batch!.rows.find((row) => row.name === "alpha" && row.agentId === "qoder"))
      .toMatchObject({ status: "skipped", eligible: true });
    expect(result.batch!.rows.find((row) => row.name === "alpha" && row.agentId === "claude-code")?.backupPath).toBe("first-backup");
    expect(hook.result.current.canRetry).toBe(false);
  });

  it("isolates Project preview facts and handles directory-selection errors through the same submission gate", async () => {
    const old = deferred<BatchPreview>();
    let oldValue!: BatchPreview;
    const { hook, backend, options } = await setup({ action: "copy_to_project" }, { commands: {
      preview_batch: async (args, backend) => {
        const value = await backend.dispatch("preview_batch", args);
        if (args.request.projectRoot === "/tmp/old") { oldValue = value; return old.promise; }
        return value;
      },
    } });
    expect(backend.count("preview_batch")).toBe(0);
    expect(hook.result.current).toMatchObject({ loading: false, preview: null, agents: [], submitDisabled: true });
    await act(async () => { hook.rerender({ ...options, projectRoot: "/tmp/old" }); });
    await act(async () => { hook.rerender({ ...options, projectRoot: "/tmp/new" }); });
    const current = hook.result.current.preview!;
    expect(current.project?.root).toBe("/tmp/new");
    await act(async () => { old.resolve({ ...oldValue, agents: [] }); });
    expect(hook.result.current.preview).toEqual(current);
    expect(hook.result.current.agents).toEqual(current.agents);
    expect(hook.result.current.submitDisabled).toBe(false);
    act(() => { hook.result.current.reportSelectionError({ message: "folder picker failed", retryable: true }); });
    expect(hook.result.current.submitDisabled).toBe(true);
    await act(async () => { await hook.result.current.reload(); });
    expect(hook.result.current.previewError).toBeNull();
    expect(hook.result.current.submitDisabled).toBe(false);
    expect(backend.count("add_project")).toBe(0);
    expect((await backend.dispatch("list_projects", {})).projects).toEqual([]);
  });

  it("cleans up polling and ignores pending responses across StrictMode unmount", async () => {
    const pending = deferred<BatchPreview>();
    const { hook, backend, onRunningChange, onCompleted } = await setup({}, { commands: {
      refresh_batch_preview: () => pending.promise,
    } }, true);
    await tick();
    const count = backend.count("refresh_batch_preview");
    expect(count).toBe(1);
    hook.unmount();
    await act(async () => { pending.reject(new Error("late refresh error")); });
    await tick(6000);
    expect(backend.count("refresh_batch_preview")).toBe(count);
    expect(backend.count("start_batch")).toBe(0);
    expect(onRunningChange).not.toHaveBeenCalled();
    expect(onCompleted).not.toHaveBeenCalled();
  });

  it.each(["success", "failure"] as const)("ignores a selected preview %s after unmount", async (reply) => {
    const pending = deferred<BatchPreview>();
    let next!: BatchPreview;
    const { hook, backend, onCompleted, renders } = await setup({}, { commands: {
      preview_batch: async (args, backend) => {
        next = await backend.dispatch("preview_batch", args);
        return pending.promise;
      },
    } });
    const count = renders.length;
    hook.unmount();
    await act(async () => {
      if (reply === "success") pending.resolve(next); else pending.reject(new Error("late preview"));
    });
    await tick();
    expect(renders).toHaveLength(count);
    expect(backend.count("refresh_batch_preview")).toBe(0);
    expect(onCompleted).not.toHaveBeenCalled();
  });
});
