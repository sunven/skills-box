import { describe, expect, it } from "vitest";
import {
  defaultFilters,
  type InventoryFilters,
} from "./inventory-view";
import { buildWorkspaceView } from "./workspace-view";
import {
  makeSnapshot,
  makeUpdateViewSkill,
  sampleSnapshot,
} from "../test/inventory-fixtures";
import type {
  UpdateCheckReport,
  UpdateCheckState,
  UpdateSkillResult,
  UpdateState,
} from "./types";
import { GLOBAL_SCOPE } from "./types";

function result(name: string, state: UpdateState): UpdateSkillResult {
  return {
    name,
    source: "acme/skills",
    ref: "main",
    lockHash: "a".repeat(40),
    remoteHash: state === "current" ? "a".repeat(40) : "b".repeat(40),
    state,
    cached: false,
    lastSuccessAtEpochMs: 900,
    previousState: null,
    error: null,
    note: null,
  };
}

function report(skills: UpdateSkillResult[]): UpdateCheckReport {
  return {
    skills,
    sources: [],
    checkedAtEpochMs: 1000,
    lastSuccessAtEpochMs: 900,
    skippedSourceCount: 0,
    cachedSourceCount: 0,
    warnings: [],
  };
}

function filters(overrides: Partial<InventoryFilters> = {}): InventoryFilters {
  return { ...defaultFilters(), ...overrides };
}

describe("workspace projection", () => {
  it("overlays the report and derives rows, counts, options and last check", () => {
    const view = buildWorkspaceView({
      activeScope: GLOBAL_SCOPE,
      snapshot: sampleSnapshot(),
      updateReport: report([result("alpha", "update"), result("beta", "current")]),
      lastFullCheck: report([result("alpha", "update")]),
      filters: filters(),
      sort: "recent",
      selectedName: "alpha",
      updateMode: null,
      updateSkillName: null,
    });

    expect(view.totalSkills).toBe(4);
    expect(view.skills.find((skill) => skill.name === "alpha")?.update).toBe(
      "update",
    );
    expect(view.skills.find((skill) => skill.name === "beta")?.update).toBe(
      "current",
    );
    expect(view.visibleSkills).toHaveLength(4);
    expect(view.selectedSkill?.name).toBe("alpha");
    expect(view.statusCounts.all).toBe(4);
    expect(view.statusCounts.update).toBe(1);
    expect(view.statusCounts.current).toBe(1);
    expect(view.statusCounts.unmanaged).toBe(1);
    expect(view.sourceCount).toBe(3);
    expect(view.candidateCount).toBe(1);
    expect(view.updateSummary.candidates.map((skill) => skill.name)).toEqual([
      "alpha",
    ]);
    expect(view.updateSummary.skipped.map((item) => item.name)).toContain(
      "delta",
    );
    expect(view.anomalies.total).toBe(3);
    expect(view.lastCheck?.lastAttemptAt).toBe(1000);
    expect(view.hasManagedSkills).toBe(true);
    expect(view.updateDialog).toBeNull();
  });

  it("applies deferred filters, status and sort, and keeps selection exact", () => {
    const base = {
      activeScope: GLOBAL_SCOPE,
      snapshot: sampleSnapshot(),
      updateReport: report([result("alpha", "update")]),
      lastFullCheck: null,
      filters: filters({ query: "beta" }),
      sort: "recent" as const,
      selectedName: "alpha",
      updateMode: null,
      updateSkillName: null,
    };
    const filtered = buildWorkspaceView(base);
    expect(filtered.visibleSkills.map((skill) => skill.name)).toEqual(["beta"]);
    expect(filtered.selectedSkill).toBeNull();

    const byStatus = buildWorkspaceView({
      ...base,
      filters: filters({ status: "update" }),
      selectedName: "beta",
    });
    expect(byStatus.visibleSkills.map((skill) => skill.name)).toEqual(["alpha"]);
    expect(byStatus.selectedSkill).toBeNull();

    const byName = buildWorkspaceView({
      ...base,
      filters: filters(),
      sort: "name",
      selectedName: "beta",
    });
    expect(byName.visibleSkills.map((skill) => skill.name)).toEqual([
      "alpha",
      "beta",
      "delta",
      "gamma",
    ]);
    expect(byName.selectedSkill?.name).toBe("beta");
  });

  it("preserves the single-vs-all eligibility divergence for modified rows", () => {
    const modified = makeUpdateViewSkill({
      name: "modified",
      management: "managed",
      localModification: "modified",
    });
    const snapshot = makeSnapshot({ skills: [modified] });
    const updateReport = report([result("modified", "update")]);
    const base = {
      activeScope: GLOBAL_SCOPE,
      snapshot,
      updateReport,
      lastFullCheck: null,
      filters: filters(),
      sort: "recent" as const,
      selectedName: "modified",
      updateSkillName: "modified",
    };

    const all = buildWorkspaceView({ ...base, updateMode: "all" });
    expect(all.updateSummary.candidates).toEqual([]);
    expect(all.updateSummary.skipped[0]?.reason).toBe("local_modified");
    expect(all.updateDialog?.initialNames).toEqual([]);

    const single = buildWorkspaceView({ ...base, updateMode: "single" });
    expect(single.updateDialog?.singleEligible).toBe(true);
    expect(single.updateDialog?.singleNeedsConfirmation).toBe(true);
    expect(single.updateDialog?.initialNames).toEqual(["modified"]);
  });

  it("falls back to the persisted update check state", () => {
    const state: UpdateCheckState = {
      version: 1,
      lastAttemptAt: 800,
      lastSuccessAt: null,
      lastOutcome: "partial",
      failedSourceCount: 1,
      cachedSourceCount: 2,
      skippedSourceCount: 3,
    };
    const view = buildWorkspaceView({
      activeScope: GLOBAL_SCOPE,
      snapshot: sampleSnapshot(),
      updateCheckState: state,
      filters: filters(),
      sort: "recent",
      selectedName: null,
      updateMode: null,
      updateSkillName: null,
    });
    expect(view.lastCheck?.lastAttemptAt).toBe(800);
    expect(view.lastCheck?.outcome).toBe("partial");
    expect(view.lastCheck?.warnings).toEqual([]);
  });

  it("maps the real lock.corrupt code and flags an inactive project lock error", () => {
    const id = "11111111-1111-4111-8111-111111111111";
    const scope = `project:${id}` as const;
    const projects = {
      projects: [
        {
          id,
          root: "/tmp/skills-proj",
          displayName: "skills-proj",
          addedAt: 1,
          lastOpenedAt: 2,
          available: true,
          lockError: "项目 lock 已损坏。",
        },
      ],
      lastActiveScope: GLOBAL_SCOPE,
      warnings: [],
    };
    const inactive = buildWorkspaceView({
      activeScope: GLOBAL_SCOPE,
      projects,
      filters: filters(),
      sort: "recent",
      selectedName: null,
      updateMode: null,
      updateSkillName: null,
    });
    expect(inactive.scopeProjects[0]?.error).toBe(true);

    const active = buildWorkspaceView({
      activeScope: scope,
      projects,
      snapshot: makeSnapshot(),
      scopeError: {
        code: "lock.corrupt",
        message: "项目 lock 已损坏。",
        retryable: false,
      },
      filters: filters(),
      sort: "recent",
      selectedName: null,
      updateMode: null,
      updateSkillName: null,
    });
    expect(active.projectHeader?.lockLabel).toBe("锁损坏");
    expect(active.projectHeader?.lockTone).toBe("negative");
  });
});
