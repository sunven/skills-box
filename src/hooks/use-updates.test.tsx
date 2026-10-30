import type { PropsWithChildren } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import { createQueryClient } from "../lib/query-client";
import { updateReportQueryKey } from "../lib/update-view";
import {
  GLOBAL_SCOPE,
  type ProjectList,
  type ScopeId,
  type UpdateCheckResponse,
} from "../lib/types";
import {
  lastFullAttemptQueryKey,
  lastFullCheckQueryKey,
  updateCheckLifecycle,
  updateCheckStateQueryKey,
  updateCheckStatesQueryKey,
} from "../lib/update-check-lifecycle";
import { createFakeBackend, emptyUpdateReport } from "../test/fake-backend";
import {
  useCheckUpdates,
  useLastFullAttemptQuery,
  useLastFullCheckQuery,
  useUpdateReportQuery,
} from "./use-updates";

const PROJECT: ScopeId = "project:11111111-1111-4111-8111-111111111111";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

function setup() {
  const client = createQueryClient();
  client.setQueryData<ProjectList>(["projects"], {
    projects: [
      {
        id: PROJECT.slice("project:".length),
        root: "/tmp/project",
        displayName: "project",
        addedAt: 1,
        lastOpenedAt: 1,
        checkRevision: "initial",
        available: true,
      },
    ],
    lastActiveScope: PROJECT,
    warnings: [],
  });
  const wrapper = ({ children }: PropsWithChildren) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  return { client, wrapper };
}

describe("Update check lifecycle", () => {
  it("deduplicates checks shared by hook instances within the same revision", async () => {
    const pending = deferred<UpdateCheckResponse>();
    const backend = createFakeBackend({
      commands: { check_updates: () => pending.promise },
    }).install();
    const { wrapper } = setup();
    const first = renderHook(() => useCheckUpdates(PROJECT), { wrapper });
    const second = renderHook(() => useCheckUpdates(PROJECT), { wrapper });
    act(() => {
      expect(first.result.current.mutate()).toBe(true);
      expect(second.result.current.mutate([`${PROJECT}/alpha`])).toBe(false);
    });
    await waitFor(() => expect(second.result.current.isPending).toBe(true));
    expect(backend.callsOf("check_updates")).toHaveLength(1);
    await act(async () => pending.resolve({ report: emptyUpdateReport() }));
    await waitFor(() => expect(first.result.current.isPending).toBe(false));
    expect(second.result.current.isPending).toBe(false);
  });

  it("clears reports and Last check in mounted observers on Relocate", async () => {
    const { client, wrapper } = setup();
    const report = emptyUpdateReport({ checkedAtEpochMs: 100 });
    client.setQueryData(updateReportQueryKey(PROJECT), report);
    client.setQueryData(lastFullCheckQueryKey(PROJECT), report);
    client.setQueryData(lastFullAttemptQueryKey(PROJECT), 100);
    client.setQueryData(updateCheckStateQueryKey(PROJECT), {
      candidateCount: 2,
    });
    client.setQueryData(updateCheckStatesQueryKey, {
      [PROJECT]: { candidateCount: 2 },
      global: { candidateCount: 3 },
    });
    const hook = renderHook(
      () => ({
        report: useUpdateReportQuery(PROJECT).data,
        lastCheck: useLastFullCheckQuery(PROJECT).data,
        lastAttempt: useLastFullAttemptQuery(PROJECT).data,
      }),
      { wrapper },
    );
    expect(hook.result.current.report).toEqual(report);
    act(() => updateCheckLifecycle(client).invalidate(PROJECT, "relocated"));
    await waitFor(() => expect(hook.result.current.report).toBeNull());
    expect(hook.result.current.lastCheck).toBeNull();
    expect(hook.result.current.lastAttempt).toBeNull();
    expect(client.getQueryData(updateCheckStateQueryKey(PROJECT))).toBeNull();
    expect(client.getQueryData(updateCheckStatesQueryKey)).toEqual({
      global: { candidateCount: 3 },
    });
  });

  it.each(["response", "error"] as const)(
    "ignores an old %s while the new revision is checking",
    async (completion) => {
      const old = deferred<UpdateCheckResponse>();
      const next = deferred<UpdateCheckResponse>();
      const backend = createFakeBackend({
        commands: {
          check_updates: ({ checkRevision }) =>
            checkRevision === "initial" ? old.promise : next.promise,
        },
      }).install();
      const { client, wrapper } = setup();
      const hook = renderHook(
        ({ revision }) => useCheckUpdates(PROJECT, revision),
        { wrapper, initialProps: { revision: "initial" } },
      );
      act(() => hook.result.current.mutate());
      act(() => updateCheckLifecycle(client).invalidate(PROJECT, "relocated"));
      hook.rerender({ revision: "relocated" });
      act(() => expect(hook.result.current.mutate()).toBe(true));
      await waitFor(() =>
        expect(backend.callsOf("check_updates")).toHaveLength(2),
      );
      await act(async () => {
        if (completion === "response")
          old.resolve({
            scope: PROJECT,
            checkRevision: "initial",
            report: emptyUpdateReport({ checkedAtEpochMs: 100 }),
          });
        else
          old.reject({
            code: "network.error",
            message: "old root failed",
            retryable: true,
          });
      });
      expect(hook.result.current.isPending).toBe(true);
      expect(hook.result.current.error).toBeNull();
      expect(client.getQueryData(updateReportQueryKey(PROJECT))).toBeNull();
      expect(client.getQueryData(lastFullAttemptQueryKey(PROJECT))).toBeNull();
      const report = emptyUpdateReport({ checkedAtEpochMs: 200 });
      await act(async () =>
        next.resolve({ scope: PROJECT, checkRevision: "relocated", report }),
      );
      await waitFor(() => expect(hook.result.current.isPending).toBe(false));
      expect(client.getQueryData(updateReportQueryKey(PROJECT))).toEqual(
        report,
      );
    },
  );

  it("does not recreate forgotten Project results when a response arrives", async () => {
    const pending = deferred<UpdateCheckResponse>();
    const backend = createFakeBackend({
      commands: { check_updates: () => pending.promise },
    }).install();
    const { client, wrapper } = setup();
    const hook = renderHook(() => useCheckUpdates(PROJECT), { wrapper });
    act(() => hook.result.current.mutate());
    act(() => updateCheckLifecycle(client).invalidate(PROJECT, null));
    await act(async () => pending.resolve({ report: emptyUpdateReport() }));
    await waitFor(() => expect(hook.result.current.isPending).toBe(false));
    expect(hook.result.current.error).toBeNull();
    expect(client.getQueryData(updateReportQueryKey(PROJECT))).toBeNull();
    act(() => expect(hook.result.current.mutate()).toBe(false));
    expect(backend.callsOf("check_updates")).toHaveLength(1);
  });

  it("settles an Invalidated check normally without publishing state", async () => {
    createFakeBackend({
      commands: {
        check_updates: () => ({
          invalidated: true,
          scope: PROJECT,
          checkRevision: "initial",
        }),
      },
    }).install();
    const { client, wrapper } = setup();
    const hook = renderHook(() => useCheckUpdates(PROJECT), { wrapper });
    act(() => hook.result.current.mutate());
    await waitFor(() => expect(hook.result.current.isPending).toBe(false));
    expect(hook.result.current.error).toBeNull();
    expect(client.getQueryData(updateReportQueryKey(PROJECT))).toBeUndefined();
    expect(
      client.getQueryData(lastFullAttemptQueryKey(PROJECT)),
    ).toBeUndefined();
  });

  it("writes a late response to its original Scope after switching", async () => {
    const pending = deferred<UpdateCheckResponse>();
    createFakeBackend({
      commands: { check_updates: () => pending.promise },
    }).install();
    const { client, wrapper } = setup();
    const hook = renderHook(({ scope }) => useCheckUpdates(scope), {
      wrapper,
      initialProps: { scope: GLOBAL_SCOPE as ScopeId },
    });
    act(() => hook.result.current.mutate());
    hook.rerender({ scope: PROJECT });
    const report = emptyUpdateReport({ checkedAtEpochMs: 100 });
    await act(async () => pending.resolve({ report }));
    await waitFor(() =>
      expect(client.getQueryData(updateReportQueryKey(GLOBAL_SCOPE))).toEqual(
        report,
      ),
    );
    expect(client.getQueryData(updateReportQueryKey(PROJECT))).toBeUndefined();
  });

  it("lets another Scope check while the first is pending", async () => {
    const first = deferred<UpdateCheckResponse>();
    const second = deferred<UpdateCheckResponse>();
    const backend = createFakeBackend({
      commands: {
        check_updates: ({ scope }) =>
          scope === GLOBAL_SCOPE ? first.promise : second.promise,
      },
    }).install();
    const { wrapper } = setup();
    const hook = renderHook(({ scope }) => useCheckUpdates(scope), {
      wrapper,
      initialProps: { scope: GLOBAL_SCOPE as ScopeId },
    });
    act(() => hook.result.current.mutate());
    await waitFor(() =>
      expect(backend.callsOf("check_updates")).toHaveLength(1),
    );
    hook.rerender({ scope: PROJECT });
    act(() => hook.result.current.mutate());
    await waitFor(() =>
      expect(backend.callsOf("check_updates")).toHaveLength(2),
    );
    await act(async () => first.resolve({ report: emptyUpdateReport() }));
    expect(hook.result.current.isPending).toBe(true);
    await act(async () => second.resolve({ report: emptyUpdateReport() }));
    await waitFor(() => expect(hook.result.current.isPending).toBe(false));
  });
});
