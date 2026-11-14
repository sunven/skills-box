import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import {
  API_COMMANDS,
  startInstall,
  startPreview,
  startUninstall,
  startUpdate,
} from "./api";
import { promiseDriver, taskEventDriver, useMutation } from "./mutation";
import type {
  MutationAction,
  MutationHandle,
  MutationOptions,
  PromiseMutationResult,
  TaskEventDriver,
} from "./mutation";
import { createQueryClient } from "./query-client";
import type { CancelAck, TaskResult } from "./types";
import { createFakeBackend } from "../test/fake-backend";
import type { FakeBackend, FakeBackendOptions } from "../test/fake-backend";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function options(action: MutationAction): MutationOptions {
  return {
    action,
    scope: "global",
    fallbackFailure: "任务失败。",
    fallbackStart: "无法开始任务。",
  };
}

function setup(commands?: FakeBackendOptions["commands"]) {
  const backend = createFakeBackend({ commands }).install();
  const client = createQueryClient();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  return { backend, client, wrapper };
}

async function startTask(
  handle: MutationHandle,
  backend: FakeBackend,
  launch: TaskEventDriver["launch"] = (onEvent) =>
    startUpdate(["alpha"], onEvent, "global"),
) {
  let finished!: ReturnType<MutationHandle["start"]>;
  await act(async () => {
    finished = handle.start(
      taskEventDriver({
        total: 1,
        message: "正在准备任务…",
        launch,
      }),
    );
  });
  return { finished, task: backend.lastTask! };
}

function taskResult(overall: TaskResult["overall"]): TaskResult {
  return {
    action: "update",
    overall,
    skills: [{ name: "alpha", status: "updated" }],
    warnings: [],
    startedAt: 1,
    finishedAtEpochMs: 2,
  };
}

const launchInstall: TaskEventDriver["launch"] = (onEvent) =>
  startInstall(
    { source: "acme/skills", skills: ["alpha"], agents: [] },
    onEvent,
    "global",
  );

describe("mutation lifecycle", () => {
  it.each(["task", "promise"] as const)(
    "uses submitted refresh context for a pending %s after options change",
    async (kind) => {
      const reply = deferred<PromiseMutationResult>();
      const fixture = setup();
      const initialOptions: MutationOptions = {
        ...options("attach"), scope: "project:one", skillId: "alpha",
      };
      const hook = renderHook((props) => useMutation(props), {
        ...fixture, initialProps: initialOptions,
      });
      for (const scope of ["global", "project:one", "project:two"]) {
        for (const id of ["alpha", "beta"]) {
          fixture.client.setQueryData(["skill", scope, id], { id });
          fixture.client.setQueryData(["skill-file", scope, id, "index.md"], { id });
        }
      }
      let finished!: ReturnType<MutationHandle["start"]>;
      if (kind === "task") {
        ({ finished } = await startTask(hook.result.current, fixture.backend));
      } else {
        act(() => {
          finished = hook.result.current.start(promiseDriver({
            total: 1, message: "挂载中", indeterminate: true, run: () => reply.promise,
          }));
        });
      }
      hook.rerender({ ...options("uninstall"), scope: "project:two", skillId: "beta" });
      await act(async () => {
        if (kind === "task") fixture.backend.lastTask!.emit({
          taskId: fixture.backend.lastTask!.taskId, phase: "failed", message: "失败",
        });
        else reply.reject(new Error("failed"));
        await finished;
      });
      await expect(finished).resolves.toBeNull();
      for (const query of fixture.client.getQueryCache().getAll()) {
        const [, scope, id] = query.queryKey;
        expect(query.state.isInvalidated, JSON.stringify(query.queryKey))
          .toBe(scope === "project:one" && id === "alpha");
      }
    },
  );

  it.each(["task", "promise"] as const)(
    "prefers the returned Scope while retaining the submitted action for a %s",
    async (kind) => {
      const reply = deferred<PromiseMutationResult>();
      const fixture = setup();
      const hook = renderHook((props) => useMutation(props), {
        ...fixture, initialProps: { ...options("attach"), skillId: "alpha" },
      });
      for (const scope of ["global", "project:one", "project:two"]) {
        fixture.client.setQueryData(["skill", scope, "alpha"], {});
        fixture.client.setQueryData(["skill", scope, "beta"], {});
      }
      let finished!: ReturnType<MutationHandle["start"]>;
      if (kind === "task") {
        ({ finished } = await startTask(hook.result.current, fixture.backend));
      } else act(() => {
        finished = hook.result.current.start(promiseDriver({
          total: 1, message: "挂载中", indeterminate: true, run: () => reply.promise,
        }));
      });
      hook.rerender({ ...options("uninstall"), scope: "project:two", skillId: "beta" });
      await act(async () => {
        if (kind === "task") fixture.backend.lastTask!.emit({
          taskId: fixture.backend.lastTask!.taskId, phase: "completed", message: "完成",
          result: { ...taskResult("success"), scope: "project:one" },
        });
        else reply.resolve({ rows: [], scope: "project:one" });
        await finished;
      });
      for (const query of fixture.client.getQueryCache().getAll()) {
        const [, scope, id] = query.queryKey;
        expect(query.state.isInvalidated, JSON.stringify(query.queryKey))
          .toBe(scope === "project:one" && id === "alpha");
      }
    },
  );

  it("retains the submitted environment callback when a native action settles", async () => {
    const fixture = setup();
    const reply = deferred<PromiseMutationResult>();
    const original = vi.fn();
    const next = vi.fn();
    const hook = renderHook((props) => useMutation(props), {
      ...fixture, initialProps: { ...options("manage_add"), onEnvironmentRecheck: original },
    });
    let finished!: ReturnType<MutationHandle["start"]>;
    act(() => {
      finished = hook.result.current.start(promiseDriver({
        total: 1, message: "添加中", indeterminate: true, run: () => reply.promise,
      }));
    });
    hook.rerender({ ...options("manage_remove"), onEnvironmentRecheck: next });
    await act(async () => { reply.resolve({ rows: [] }); await finished; });
    expect(original).toHaveBeenCalledOnce();
    expect(next).not.toHaveBeenCalled();
  });

  it.each(["completed", "failed", "cancelled"] as const)(
    "keeps Preview %s cache-free through the task driver",
    async (phase) => {
      const fixture = setup();
      const hook = renderHook(() => useMutation(options("preview")), fixture);
      const keys = [
        ["inventory", "global"], ["expertPacks", "local", "global"],
        ["discovery"], ["updateReport", "global"],
        ["skill-file", "global", "alpha", "index.md"],
      ];
      keys.forEach((key) => fixture.client.setQueryData(key, []));
      const before = fixture.client.getQueryCache().getAll().map((query) => query.state);
      const run = await startTask(hook.result.current, fixture.backend,
        (onEvent) => startPreview("acme/skills", onEvent));
      await act(async () => {
        run.task.emit({
          taskId: run.task.taskId, phase, message: "预览结束",
          result: phase === "failed" ? undefined : taskResult(phase === "cancelled" ? "cancelled" : "success"),
        });
        await run.finished;
      });
      expect(fixture.client.getQueryCache().getAll().map((query) => query.state)).toEqual(before);
    },
  );

  it.each(["task", "promise"] as const)(
    "ignores refresh from superseded and reset %s runs",
    async (kind) => {
      const fixture = setup();
      const hook = renderHook((props) => useMutation(props), {
        ...fixture, initialProps: { ...options("attach"), scope: "project:one" as const },
      });
      for (const scope of ["global", "project:one", "project:two"]) {
        fixture.client.setQueryData(["inventory", scope], []);
        fixture.client.setQueryData(["expertPacks", "local", scope], []);
      }
      const before = fixture.client.getQueryCache().getAll().map((query) => query.state);
      const replies = [deferred<PromiseMutationResult>(), deferred<PromiseMutationResult>()];
      const runs: Awaited<ReturnType<typeof startTask>>[] = [];
      const finished: ReturnType<MutationHandle["start"]>[] = [];
      for (let i = 0; i < 2; i++) {
        if (kind === "task") {
          const run = await startTask(hook.result.current, fixture.backend);
          runs.push(run);
          finished.push(run.finished);
        } else act(() => {
          finished.push(hook.result.current.start(promiseDriver({
            total: 1, message: "挂载中", indeterminate: true, run: () => replies[i].promise,
          })));
        });
      }
      await expect(finished[0]).resolves.toBeNull();
      const complete = async (i: number) => {
        if (kind === "task") runs[i].task.emit({
          taskId: runs[i].task.taskId, phase: "completed", message: "完成", result: taskResult("success"),
        });
        else { replies[i].resolve({ rows: [] }); await replies[i].promise; }
      };
      await act(async () => complete(0));
      expect(hook.result.current.state.running).toBe(true);
      act(() => hook.result.current.reset());
      await expect(finished[1]).resolves.toBeNull();
      await act(async () => complete(1));
      expect(hook.result.current.state.result).toBeNull();
      expect(fixture.client.getQueryCache().getAll().map((query) => query.state)).toEqual(before);
    },
  );

  it("does not refresh when a task launch rejects", async () => {
    const fixture = setup();
    const hook = renderHook(() => useMutation(options("install")), fixture);
    fixture.client.setQueryData(["inventory", "global"], []);
    fixture.client.setQueryData(["expertPacks", "local", "global"], []);
    const before = fixture.client.getQueryCache().getAll().map((query) => query.state);
    await act(async () => {
      const result = await hook.result.current.start(taskEventDriver({
        total: 1, message: "准备中", launch: async () => { throw new Error("launch failed"); },
      }));
      expect(result).toBeNull();
    });
    expect(hook.result.current.state.startError).not.toBeNull();
    expect(fixture.client.getQueryCache().getAll().map((query) => query.state)).toEqual(before);
  });

  it.each(["rejected", "failed"] as const)("ignores an old %s cancellation after reset and a new start", async (replyKind) => {
    const reply = deferred<CancelAck>();
    const fixture = setup({ cancel_task: () => reply.promise });
    const hook = renderHook(() => useMutation(options("update")), fixture);
    const first = await startTask(hook.result.current, fixture.backend);
    let cancellation!: Promise<void>;
    act(() => {
      cancellation = hook.result.current.cancel();
    });
    expect(hook.result.current.state.cancelling).toBe(true);

    act(() => hook.result.current.reset());
    await expect(first.finished).resolves.toBeNull();
    const second = await startTask(hook.result.current, fixture.backend);
    await act(async () => {
      if (replyKind === "failed") {
        reply.reject({ message: "旧取消请求失败。", retryable: true });
      } else {
        reply.resolve({
          taskId: first.task.taskId,
          accepted: false,
          message: "旧任务已进入写入阶段。",
        });
      }
      await cancellation;
    });

    expect(hook.result.current.state).toMatchObject({
      taskId: second.task.taskId,
      running: true,
      error: null,
    });
  });

  it.each(["rejected", "failed"] as const)("keeps a completed result when a %s cancellation arrives afterward", async (replyKind) => {
    const reply = deferred<CancelAck>();
    const fixture = setup({ cancel_task: () => reply.promise });
    const hook = renderHook(() => useMutation(options("update")), fixture);
    const run = await startTask(hook.result.current, fixture.backend);
    let cancellation!: Promise<void>;
    act(() => {
      cancellation = hook.result.current.cancel();
    });
    act(() => {
      run.task.emit({
        taskId: run.task.taskId,
        phase: "completed",
        message: "更新完成。",
        result: taskResult("success"),
      });
    });
    await act(async () => {
      if (replyKind === "failed") {
        reply.reject({ message: "取消请求失败。", retryable: true });
      } else {
        reply.resolve({
          taskId: run.task.taskId,
          accepted: false,
          message: "任务已结束。",
        });
      }
      await cancellation;
    });

    expect(hook.result.current.state).toMatchObject({
      running: false,
      error: null,
      result: { outcome: "success" },
    });
    await expect(run.finished).resolves.toMatchObject({ outcome: "success" });
  });

  it("closes a preview without waiting for cancellation or refreshing queries", async () => {
    const reply = deferred<CancelAck>();
    const fixture = setup({ cancel_task: () => reply.promise });
    const hook = renderHook(() => useMutation(options("preview")), fixture);
    fixture.client.setQueryData(["inventory", "global"], { skills: [] });
    fixture.client.setQueryData(["expertPacks", "local", "global"], []);
    const run = await startTask(
      hook.result.current,
      fixture.backend,
      (onEvent) => startPreview("acme/skills", onEvent),
    );
    let closed!: boolean;
    act(() => {
      expect(hook.result.current.canClose()).toBe(true);
      closed = hook.result.current.requestClose();
    });

    expect(closed).toBe(true);
    expect(hook.result.current.state).toMatchObject({
      running: false,
      taskId: null,
      result: null,
      error: null,
    });
    await expect(run.finished).resolves.toBeNull();
    expect(fixture.backend.callsOf(API_COMMANDS.cancelTask)).toMatchObject([
      { payload: { taskId: run.task.taskId } },
    ]);
    expect(
      fixture.client.getQueryState(["inventory", "global"])?.isInvalidated,
    ).toBe(false);
    expect(
      fixture.client.getQueryState(["expertPacks", "local", "global"])?.isInvalidated,
    ).toBe(false);

    await act(async () => {
      reply.reject({ message: "任务已结束。", retryable: false });
      await reply.promise.catch(() => {});
    });
    expect(hook.result.current.state.error).toBeNull();
  });

  it("closes both wizard handles while prioritizing installation cancellation", async () => {
    const fixture = setup();
    const hook = renderHook(
      () => ({
        install: useMutation(options("install")),
        preview: useMutation(options("preview")),
      }),
      fixture,
    );
    const preview = await startTask(
      hook.result.current.preview,
      fixture.backend,
      (onEvent) => startPreview("acme/skills", onEvent),
    );
    const install = await startTask(
      hook.result.current.install,
      fixture.backend,
      launchInstall,
    );
    act(() => install.task.emit({
      taskId: install.task.taskId,
      phase: "preparing",
      message: "正在准备安装…",
    }));
    let closed!: boolean;
    act(() => {
      closed = hook.result.current.install.requestClose([
        hook.result.current.preview,
      ]);
    });

    expect(closed).toBe(true);
    expect(hook.result.current.install.state.running).toBe(false);
    expect(hook.result.current.preview.state.running).toBe(false);
    await expect(install.finished).resolves.toBeNull();
    await expect(preview.finished).resolves.toBeNull();
    expect(fixture.backend.callsOf(API_COMMANDS.cancelTask)).toMatchObject([
      { payload: { taskId: install.task.taskId } },
    ]);
  });

  it("cancels a returned taskId when closing before React renders it", async () => {
    const fixture = setup();
    const hook = renderHook(() => useMutation(options("preview")), fixture);
    let finished!: ReturnType<MutationHandle["start"]>;
    let launched!: Promise<string>;
    await act(async () => {
      finished = hook.result.current.start(
        taskEventDriver({
          total: 1,
          message: "正在准备预览…",
          launch: (onEvent) => {
            launched = startPreview("acme/skills", onEvent);
            return launched;
          },
        }),
      );
      const taskId = await launched;
      expect(hook.result.current.requestClose()).toBe(true);
      expect(fixture.backend.callsOf(API_COMMANDS.cancelTask)).toMatchObject([
        { payload: { taskId } },
      ]);
    });
    await expect(finished).resolves.toBeNull();
    expect(hook.result.current.state.running).toBe(false);
  });

  it.each([
    { action: "install", phase: null },
    { action: "install", phase: "running_cli" },
    { action: "update", phase: "preparing" },
    { action: "uninstall", phase: "preparing" },
  ] as const)("leaves both handles unchanged when $action in $phase blocks close", async ({ action, phase }) => {
    const fixture = setup();
    const hook = renderHook(
      () => ({
        mutation: useMutation(options(action)),
        preview: useMutation(options("preview")),
      }),
      fixture,
    );
    await startTask(
      hook.result.current.preview,
      fixture.backend,
      (onEvent) => startPreview("acme/skills", onEvent),
    );
    const launch: TaskEventDriver["launch"] = action === "install"
      ? launchInstall
      : action === "uninstall"
        ? (onEvent) => startUninstall("alpha", onEvent, "global")
        : (onEvent) => startUpdate(["alpha"], onEvent, "global");
    const run = await startTask(hook.result.current.mutation, fixture.backend, launch);
    if (phase) act(() => run.task.emit({
      taskId: run.task.taskId,
      phase,
      message: "任务进行中。",
    }));
    const mutationState = hook.result.current.mutation.state;
    const previewState = hook.result.current.preview.state;
    act(() => {
      expect(hook.result.current.mutation.canClose()).toBe(false);
      expect(hook.result.current.preview.requestClose([
        hook.result.current.mutation,
      ])).toBe(false);
    });
    expect(hook.result.current.mutation.state).toEqual(mutationState);
    expect(hook.result.current.preview.state).toEqual(previewState);
    expect(fixture.backend.count(API_COMMANDS.cancelTask)).toBe(0);
  });

  it("cancels a late launch and ignores its events after another preview starts", async () => {
    const reply = deferred<string>();
    const fixture = setup({
      start_preview: (args, backend) => {
        backend.dispatch(API_COMMANDS.startPreview, args);
        const task = backend.lastTask!;
        return task.taskId === "task-1" ? reply.promise : task.taskId;
      },
    });
    const backend = fixture.backend;
    const observed: unknown[] = [];
    const hook = renderHook(
      () => useMutation({
        ...options("preview"),
        onEvent: (event) => observed.push(event),
      }),
      fixture,
    );
    const launch = (onEvent: Parameters<typeof startPreview>[1]) =>
      startPreview("acme/skills", onEvent);
    const first = await startTask(hook.result.current, backend, launch);
    act(() => expect(hook.result.current.requestClose()).toBe(true));
    await expect(first.finished).resolves.toBeNull();
    expect(backend.count(API_COMMANDS.cancelTask)).toBe(0);
    const second = await startTask(hook.result.current, backend, launch);
    await act(async () => {
      reply.resolve(first.task.taskId);
      await reply.promise;
    });
    act(() => first.task.emit({
      taskId: first.task.taskId,
      phase: "failed",
      message: "旧预览失败。",
      error: { message: "旧预览失败。", retryable: true },
    }));
    expect(backend.callsOf(API_COMMANDS.cancelTask)).toMatchObject([
      { payload: { taskId: first.task.taskId } },
    ]);
    expect(hook.result.current.state).toMatchObject({
      running: true,
      taskId: second.task.taskId,
      error: null,
    });
    expect(observed).toEqual([]);
  });

  it("keeps reset state-only for an already launched preview", async () => {
    const fixture = setup();
    const hook = renderHook(() => useMutation(options("preview")), fixture);
    const run = await startTask(
      hook.result.current,
      fixture.backend,
      (onEvent) => startPreview("acme/skills", onEvent),
    );
    act(() => hook.result.current.reset());
    await expect(run.finished).resolves.toBeNull();
    expect(fixture.backend.count(API_COMMANDS.cancelTask)).toBe(0);
  });

  it("does not clear a new run's pending cancellation when the old reply settles", async () => {
    const oldReply = deferred<CancelAck>();
    const newReply = deferred<CancelAck>();
    const fixture = setup({
      cancel_task: ({ taskId }) =>
        taskId === "task-1" ? oldReply.promise : newReply.promise,
    });
    const hook = renderHook(() => useMutation(options("update")), fixture);
    const first = await startTask(hook.result.current, fixture.backend);
    let oldCancellation!: Promise<void>;
    act(() => {
      oldCancellation = hook.result.current.cancel();
    });
    act(() => hook.result.current.reset());
    const second = await startTask(hook.result.current, fixture.backend);
    let newCancellation!: Promise<void>;
    act(() => {
      newCancellation = hook.result.current.cancel();
    });
    await act(async () => {
      oldReply.resolve({ taskId: first.task.taskId, accepted: false });
      await oldCancellation;
    });
    expect(hook.result.current.state).toMatchObject({
      taskId: second.task.taskId,
      cancelling: true,
      error: null,
    });
    await act(async () => {
      newReply.resolve({ taskId: second.task.taskId, accepted: true });
      await newCancellation;
    });
    expect(hook.result.current.state.cancelling).toBe(false);
  });

  it("waits for the terminal result and refresh after explicit cancellation", async () => {
    const fixture = setup();
    fixture.client.setQueryData(["inventory", "global"], { skills: [] });
    const hook = renderHook(() => useMutation(options("update")), fixture);
    const run = await startTask(hook.result.current, fixture.backend);
    await act(async () => {
      const first = hook.result.current.cancel();
      const repeated = hook.result.current.cancel();
      await Promise.all([first, repeated]);
    });
    expect(fixture.backend.count(API_COMMANDS.cancelTask)).toBe(1);
    expect(hook.result.current.state).toMatchObject({
      running: true,
      result: null,
      cancelling: false,
    });
    expect(
      fixture.client.getQueryState(["inventory", "global"])?.isInvalidated,
    ).toBe(false);
    act(() => run.task.emit({
      taskId: run.task.taskId,
      phase: "cancelled",
      message: "更新已取消。",
      result: taskResult("cancelled"),
    }));
    await expect(run.finished).resolves.toMatchObject({
      outcome: "cancelled",
      rows: [{ name: "alpha", status: "success" }],
    });
    expect(
      fixture.client.getQueryState(["inventory", "global"])?.isInvalidated,
    ).toBe(true);
    expect(hook.result.current.state.running).toBe(false);
  });

  it.each(["rejected", "failed"] as const)("reports a current %s cancellation and can reset it", async (replyKind) => {
    const reply = deferred<CancelAck>();
    const fixture = setup({ cancel_task: () => reply.promise });
    const hook = renderHook(() => useMutation(options("update")), fixture);
    const run = await startTask(hook.result.current, fixture.backend);
    await act(async () => {
      const cancellation = hook.result.current.cancel();
      if (replyKind === "failed") reply.reject({ message: "正在写入。", retryable: false });
      else reply.resolve({ taskId: run.task.taskId, accepted: false, message: "正在写入。" });
      await cancellation;
    });
    expect(hook.result.current.state).toMatchObject({
      running: true,
      cancelling: false,
      error: { message: "正在写入。" },
    });
    act(() => hook.result.current.reset());
    await expect(run.finished).resolves.toBeNull();
    expect(hook.result.current.state.error).toBeNull();
  });

  it("blocks close during a native promise and retains its settled result and refresh", async () => {
    const reply = deferred<PromiseMutationResult>();
    const fixture = setup();
    fixture.client.setQueryData(["inventory", "global"], { skills: [] });
    const hook = renderHook(() => useMutation(options("attach")), fixture);
    let finished!: ReturnType<MutationHandle["start"]>;
    act(() => {
      finished = hook.result.current.start(promiseDriver({
        total: 1,
        message: "正在挂载…",
        indeterminate: true,
        run: () => reply.promise,
      }));
      expect(hook.result.current.canClose()).toBe(false);
      expect(hook.result.current.requestClose()).toBe(false);
    });
    expect(hook.result.current.state.running).toBe(true);
    await act(async () => {
      reply.resolve({
        rows: [{ name: "Claude Code", status: "success", statusLabel: "已挂载" }],
      });
      await finished;
    });
    expect(hook.result.current.state.result?.outcome).toBe("success");
    expect(hook.result.current.canClose()).toBe(true);
    expect(
      fixture.client.getQueryState(["inventory", "global"])?.isInvalidated,
    ).toBe(true);
    act(() => expect(hook.result.current.requestClose()).toBe(true));
    expect(hook.result.current.state.result).toBeNull();
    expect(fixture.backend.count(API_COMMANDS.cancelTask)).toBe(0);
  });
});
