import { QueryObserver } from "@tanstack/react-query";
import type { QueryKey } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { act, waitFor } from "@testing-library/react";
import { refreshMutationQueries } from "./mutation-refresh";
import type { MutationAction, MutationResultView } from "./mutation";
import type { BatchRow } from "./batch";
import { createQueryClient } from "./query-client";
import type { ProjectList, ScopeId, UpdateCheckReport, UpdateCheckResponse } from "./types";
import { checkRunQueryKey, updateCheckLifecycle } from "./update-check-lifecycle";
import { createFakeBackend, emptyUpdateReport } from "../test/fake-backend";

const PROJECT: ScopeId = "project:one";
const SCOPES: ScopeId[] = ["global", PROJECT, "project:two"];

function observations(scope: ScopeId): Record<string, QueryKey> {
  return {
    inventory: ["inventory", scope],
    alpha: ["skill", scope, "alpha"],
    beta: ["skill", scope, "beta"],
    alphaDir: ["skill-dir", scope, "alpha", "references"],
    betaDir: ["skill-dir", scope, "beta", ""],
    alphaFile: ["skill-file", scope, "alpha", "references/guide.md"],
    betaFile: ["skill-file", scope, "beta", "index.md"],
    report: ["updateReport", scope],
    checkState: ["updateCheckState", scope],
    lastCheck: ["lastFullCheck", scope],
    lastAttempt: ["lastFullAttempt", scope],
    staleRecords: ["staleRecords", scope],
    packs: ["expertPacks", "local", scope],
  };
}

function fixture() {
  const client = createQueryClient();
  for (const scope of SCOPES) {
    for (const [name, key] of Object.entries(observations(scope))) {
      client.setQueryData(key, { name, scope });
    }
  }
  const projects: ProjectList = {
    projects: ["one", "two"].map((id) => ({
      id, root: `/tmp/${id}`, displayName: id, addedAt: 1,
      lastOpenedAt: 1, checkRevision: "initial", available: true,
    })),
    lastActiveScope: "global",
    warnings: [],
  };
  client.setQueryData(["projects"], projects);
  client.setQueryData(["discovery"], []);
  client.setQueryData(["updateCheckStates"], { [PROJECT]: { candidateCount: 1 } });
  client.setQueryData(["expertPacks", "marketplace"], []);
  return client;
}

function result(outcome: MutationResultView["outcome"]): MutationResultView {
  return { outcome, rows: [], warnings: [], scope: PROJECT };
}

function updateReport(names = ["alpha", "beta"]): UpdateCheckReport {
  return emptyUpdateReport({ skills: names.map((name) => ({
    name, source: "acme/skills", ref: null, lockHash: "old", remoteHash: "new",
    state: "update", cached: false, lastSuccessAtEpochMs: 1,
    previousState: null, error: null, note: null,
  })) });
}

function batchRow(name: string, status: BatchRow["status"]): BatchRow {
  return {
    key: name, skillId: `${PROJECT}/${name}`, name, status, agentId: null,
    agentName: null, eligible: true, reason: null, localModification: "clean",
    deletesCopy: false, paths: [], error: null, backupPath: null,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

describe("complete mutation refresh", () => {
  const coverage: { action: MutationAction; local: string[]; shared: string[] }[] = [
    { action: "install", local: ["inventory", "alpha", "beta", "packs"], shared: ["discovery"] },
    { action: "update", local: ["inventory", "alpha", "beta", "report", "alphaDir", "alphaFile", "packs"], shared: [] },
    { action: "uninstall", local: ["inventory", "alpha", "beta", "report", "packs"], shared: ["discovery"] },
    { action: "uninstall_agents", local: ["inventory", "alpha", "beta", "report", "alphaDir", "alphaFile", "packs"], shared: [] },
    ...(["attach", "detach"] as const).map((action) => ({
      action, local: ["inventory", "alpha", "alphaDir", "alphaFile", "packs"], shared: [],
    })),
    ...(["clean_records", "restore"] as const).map((action) => ({
      action, local: ["inventory", "alpha", "beta", "staleRecords", "packs"], shared: ["discovery"],
    })),
    { action: "copy_to_project", local: ["inventory", "alpha", "beta", "report", "packs"], shared: ["projects", "updateCheckStates"] },
    ...(["project_add", "project_relocate"] as const).map((action) => ({
      action, local: ["inventory", "alpha", "beta", "report", "checkState", "lastCheck", "lastAttempt", "packs"],
      shared: ["projects", "updateCheckStates", "discovery"],
    })),
    { action: "project_forget", local: Object.keys(observations(PROJECT)), shared: ["projects", "updateCheckStates", "discovery"] },
    ...(["manage_add", "manage_remove"] as const).map((action) => ({ action, local: ["packs"], shared: [] })),
  ];

  describe.each(["global", PROJECT] as const)("terminal refresh in %s", (target) => {
    it.each(coverage)("refreshes the complete observations for $action after failure", ({ action, local, shared }) => {
      const client = fixture();
      const before = client.getQueriesData({});
      const onEnvironmentRecheck = vi.fn();
      refreshMutationQueries(client, { action, scope: target, skillId: "alpha", onEnvironmentRecheck }, null);
      expect(client.getQueriesData({})).toEqual(before);
      for (const scope of SCOPES) {
        for (const [name, key] of Object.entries(observations(scope))) {
          expect(client.getQueryState(key)?.isInvalidated, JSON.stringify(key))
            .toBe(scope === target && local.includes(name));
        }
      }
      for (const name of ["projects", "discovery", "updateCheckStates"]) {
        expect(client.getQueryState([name])?.isInvalidated, name).toBe(shared.includes(name));
      }
      expect(client.getQueryState(["expertPacks", "marketplace"])?.isInvalidated).toBe(false);
      expect(onEnvironmentRecheck).toHaveBeenCalledTimes(action.startsWith("manage_") ? 1 : 0);
    });
  });

  it.each(["update", "uninstall_agents", "attach", "detach"] as const)(
    "refreshes all Skill content for %s without a selected Skill",
    (action) => {
      const client = fixture();
      refreshMutationQueries(client, { action, scope: PROJECT }, null);
      for (const name of ["alpha", "beta", "alphaDir", "betaDir", "alphaFile", "betaFile"]) {
        expect(client.getQueryState(observations(PROJECT)[name])?.isInvalidated, name).toBe(true);
      }
    },
  );

  it.each(["success", "partial", "cancelled"] as const)(
    "patches only updated rows before report invalidation for an Update with %s outcome",
    (outcome) => {
      const client = createQueryClient();
      const key = observations(PROJECT).report;
      client.setQueryData(key, updateReport(["alpha", "beta", "gamma", "delta", "native"]));
      const observed: unknown[] = [];
      const unsubscribe = client.getQueryCache().subscribe((event) => {
        if (event.type === "updated" && event.action.type === "invalidate" && event.query.queryKey[0] === "updateReport") {
          observed.push(event.query.state.data);
        }
      });
      refreshMutationQueries(client, { action: "update", scope: PROJECT }, {
        ...result(outcome), rows: [
          { name: "alpha", status: "success", statusLabel: "已更新", taskStatus: "updated" },
          { name: "beta", status: "failed", statusLabel: "失败", taskStatus: "failed" },
          { name: "gamma", status: "skipped", statusLabel: "跳过", taskStatus: "skipped" },
          { name: "delta", status: "success", statusLabel: "已卸载", taskStatus: "removed" },
          { name: "native", status: "success", statusLabel: "已更新" },
        ],
      });
      unsubscribe();
      const report = client.getQueryData<UpdateCheckReport>(key)!;
      expect(report.skills.map(({ state, lockHash }) => [state, lockHash])).toEqual([
        ["current", "new"], ["update", "old"], ["update", "old"], ["update", "old"], ["current", "new"],
      ]);
      expect(observed).toEqual([report]);
      expect(client.getQueryState(key)?.isInvalidated).toBe(true);
    },
  );

  it("preserves the report's candidates after a non-Update action", () => {
    const client = createQueryClient();
    const key = observations(PROJECT).report;
    const report = updateReport();
    client.setQueryData(key, report);
    refreshMutationQueries(client, { action: "uninstall", scope: PROJECT }, {
      ...result("success"), rows: [{ name: "alpha", status: "success", statusLabel: "已卸载", taskStatus: "removed" }],
    });
    expect(client.getQueryData(key)).toEqual(report);
    expect(client.getQueryState(key)?.isInvalidated).toBe(true);
  });

  it.each(["clean_records", "restore"] as const)(
    "clears only successful %s rows and checks only restored Skills",
    async (action) => {
      const pending = deferred<UpdateCheckResponse>();
      const client = fixture();
      const key = observations(PROJECT).report;
      client.setQueryData(key, updateReport(["alpha", "beta", "gamma", "delta"]));
      const backend = createFakeBackend({ commands: { check_updates: () => pending.promise } }).install();
      refreshMutationQueries(client, { action, scope: PROJECT }, {
        ...result("cancelled"), batch: { action, rows: [
          batchRow("alpha", "success"), batchRow("beta", "failed"),
          batchRow("gamma", "skipped"), batchRow("delta", "not_executed"),
        ] },
      });
      expect(client.getQueryData<UpdateCheckReport>(key)?.skills.map(({ name }) => name)).toEqual(["beta", "gamma", "delta"]);
      if (action === "restore") {
        expect(backend.callsOf("check_updates")).toMatchObject([
          { payload: { scope: PROJECT, checkRevision: "initial", skillIds: [`${PROJECT}/alpha`] } },
        ]);
        pending.resolve({ report: updateReport(["alpha"]), scope: PROJECT, checkRevision: "initial" });
        await waitFor(() => expect(client.getQueryData(checkRunQueryKey(PROJECT, "initial"))).toMatchObject({ isPending: false }));
        expect(client.getQueryData<UpdateCheckReport>(key)?.skills.map(({ name }) => name).sort()).toEqual(["alpha", "beta", "delta", "gamma"]);
      } else expect(backend.callsOf("check_updates")).toHaveLength(0);
    },
  );

  it("does not start a Restore check when no row succeeded", () => {
    const backend = createFakeBackend().install();
    const client = fixture();
    const report = updateReport();
    client.setQueryData(observations(PROJECT).report, report);
    refreshMutationQueries(client, { action: "restore", scope: PROJECT }, {
      ...result("failed"), batch: { action: "restore", rows: [batchRow("alpha", "failed")] },
    });
    expect(client.getQueryData(observations(PROJECT).report)).toEqual(report);
    expect(backend.callsOf("check_updates")).toHaveLength(0);
  });

  it.each(["relocated", "forgotten", "forget failed"] as const)(
    "coordinates a pending Update check when the Project is %s",
    async (change) => {
      const client = fixture();
      const pending = deferred<UpdateCheckResponse>();
      const backend = createFakeBackend({ commands: { check_updates: () => pending.promise } }).install();
      const lifecycle = updateCheckLifecycle(client);
      expect(lifecycle.start(PROJECT, "initial")).toBe(true);
      const key = observations(PROJECT).report;
      const atRefetch: unknown[] = [];
      const observer = new QueryObserver(client, {
        queryKey: observations(PROJECT).inventory,
        queryFn: async () => {
          atRefetch.push({
            revision: client.getQueryData<ProjectList>(["projects"])?.projects[0]?.checkRevision,
            report: client.getQueryData(key),
          });
          return [];
        },
      });
      const unsubscribe = observer.subscribe(() => {});
      try {
        refreshMutationQueries(client, {
          action: change === "relocated" ? "project_relocate" : "project_forget", scope: PROJECT,
        }, change === "forget failed" ? null : { ...result("success"), checkRevision: "next" });
        if (change === "relocated") {
          expect(atRefetch).toEqual([{ revision: "next", report: null }]);
        } else if (change === "forgotten") expect(atRefetch).toEqual([]);
        const report = updateReport();
        await act(async () => {
          pending.resolve({ report, scope: PROJECT, checkRevision: "initial" });
        });
        await waitFor(() => {
          expect(client.getQueryData(checkRunQueryKey(PROJECT, "initial"))).toMatchObject({ isPending: false });
          expect(client.getQueryData(key)).toEqual(
            change === "relocated" ? null : change === "forgotten" ? undefined : report,
          );
        });
        if (change === "forgotten") {
          expect(lifecycle.start(PROJECT, "initial")).toBe(false);
          expect(backend.callsOf("check_updates")).toHaveLength(1);
        }
      } finally {
        unsubscribe();
        client.clear();
      }
    },
  );

  it.each([null, "success", "failed", "partial", "cancelled"] as const)(
    "leaves all observations untouched after preview %s",
    (outcome) => {
      const client = fixture();
      const before = client.getQueryCache().getAll().map((query) => query.state);
      const onEnvironmentRecheck = vi.fn();
      refreshMutationQueries(client, {
        action: "preview", scope: PROJECT, onEnvironmentRecheck,
      }, outcome === null ? null : result(outcome));
      expect(client.getQueryCache().getAll().map((query) => query.state)).toEqual(before);
      expect(onEnvironmentRecheck).not.toHaveBeenCalled();
    },
  );

  it.each([null, "failed", "partial", "cancelled"] as const)(
    "preserves cached data when Forget is not confirmed successful (%s)",
    (outcome) => {
      const client = fixture();
      const before = client.getQueriesData({});
      refreshMutationQueries(client, { action: "project_forget", scope: PROJECT },
        outcome === null ? null : result(outcome));
      expect(client.getQueriesData({})).toEqual(before);
      for (const scope of SCOPES) {
        for (const key of Object.values(observations(scope))) {
          expect(client.getQueryState(key)?.isInvalidated, JSON.stringify(key))
            .toBe(scope === PROJECT);
        }
      }
      for (const key of [["projects"], ["discovery"], ["updateCheckStates"]]) {
        expect(client.getQueryState(key)?.isInvalidated).toBe(true);
      }
    },
  );

  it("removes forgotten observations without refetching active Pack or content queries", async () => {
    const client = fixture();
    const keys = observations(PROJECT);
    const fetch = vi.fn(async () => ({ stale: true }));
    const observers = [keys.packs, keys.alphaDir, keys.alphaFile].map((queryKey) => {
      const observer = new QueryObserver(client, { queryKey, queryFn: fetch });
      return observer.subscribe(() => {});
    });
    try {
      refreshMutationQueries(client, { action: "project_forget", scope: "global" }, result("success"));
      await Promise.resolve();
      for (const scope of SCOPES) {
        for (const [name, key] of Object.entries(observations(scope))) {
          if (scope === PROJECT) expect(client.getQueryState(key), name).toBeUndefined();
          else {
            expect(client.getQueryData(key)).toEqual({ name, scope });
            expect(client.getQueryState(key)?.isInvalidated).toBe(false);
          }
        }
      }
      expect(client.getQueryData<ProjectList>(["projects"])?.projects.map((project) => project.id)).toEqual(["two"]);
      expect(fetch).not.toHaveBeenCalled();
    } finally {
      observers.forEach((unsubscribe) => unsubscribe());
      client.clear();
    }
  });
});
