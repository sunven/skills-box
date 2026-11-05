import { describe, expect, it } from "vitest";
import {
  defaultFilters,
  type InventoryFilters,
} from "./inventory-view";
import {
  buildWorkspaceView,
  createWorkspaceViewSelector,
  type WorkspaceViewInput,
} from "./workspace-view";
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
import { discovered, discoveryFixture, discoveryProjectScope } from "../test/discovery-fixtures";

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
  it.each([
    ["all", 4], ["acme/skills", 2], ["Acme/Skills", 2],
    ["https://git.example.com/Group/Skills.git", 1],
    ["https://git.example.com/group/skills.git", 1],
    ["__unknown__", 0],
  ])("counts installable discoveries for source %s without changing inventory totals", (source, expected) => {
    const discovery = discoveryFixture();
    const view = buildWorkspaceView({
      activeScope: GLOBAL_SCOPE, snapshot: sampleSnapshot(),
      discovery: discoveryFixture({ skills: [...discovery.skills,
        discovered("baseline", "notInstalled"),
        discovered("upper", "notInstalled", { unread: true, id: { source: "https://git.example.com/Group/Skills.git", name: "upper" } }),
        discovered("lower", "notInstalled", { unread: true, id: { source: "https://git.example.com/group/skills.git", name: "lower" } }),
        discovered("ignored", "notInstalled", { ignored: true }),
        discovered("invalid", "notInstalled", { installIssue: "名称不可安装" }),
        discovered("conflict", "conflict"), discovered("stale", "stale"), discovered("unknown", "unknown"),
      ] }),
      filters: filters({ source }), sort: "recent", selectedName: null, updateMode: null, updateSkillName: null,
    });
    expect(view.availableDiscoveryCount).toBe(expected);
    expect(view.totalSkills).toBe(4);
  });
  it("counts available skills in the selected scope", () => {
    const input: WorkspaceViewInput = {
      activeScope: GLOBAL_SCOPE, snapshot: sampleSnapshot(), discovery: discoveryFixture(),
      filters: filters({ source: "acme/skills" }), sort: "recent", selectedName: null, updateMode: null, updateSkillName: null,
    };
    expect(buildWorkspaceView(input).availableDiscoveryCount).toBe(1);
    expect(buildWorkspaceView({ ...input, activeScope: discoveryProjectScope }).availableDiscoveryCount).toBe(2);
  });
  it("reuses inventory rows while selection, filters, sort and dialog targets change", () => {
    const select = createWorkspaceViewSelector();
    const input: WorkspaceViewInput = {
      activeScope: GLOBAL_SCOPE,
      snapshot: sampleSnapshot(),
      updateReport: report([result("alpha", "update")]),
      filters: filters(),
      sort: "recent",
      selectedName: null,
      updateMode: null,
      updateSkillName: null,
    };
    const initial = select(input);
    const selected = select({ ...input, selectedName: "alpha" });
    const filtered = select({
      ...input,
      filters: filters({ query: "alpha" }),
      sort: "name",
      selectedName: "alpha",
      updateMode: "single",
      updateSkillName: "alpha",
    });

    expect(selected.skills).toBe(initial.skills);
    expect(filtered.skills).toBe(initial.skills);
    expect(selected.selectedSkill).toBe(
      initial.skills.find((skill) => skill.name === "alpha"),
    );
    expect(filtered.visibleSkills).toEqual([selected.selectedSkill]);
    expect(filtered.visibleSkills[0]).toBe(selected.selectedSkill);
    expect(filtered.statusCounts).toBe(initial.statusCounts);
    expect(filtered.agentOptions).toBe(initial.agentOptions);
    expect(filtered.updateDialog?.initialNames).toEqual(["alpha"]);
  });

  it("counts update anomalies within the selected source after the inventory is cached", () => {
    const select = createWorkspaceViewSelector();
    const input: WorkspaceViewInput = {
      activeScope: GLOBAL_SCOPE,
      snapshot: sampleSnapshot(),
      updateReport: report([
        result("alpha", "path_missing"),
        result("beta", "error"),
      ]),
      filters: filters({ status: "anomaly" }),
      sort: "recent",
      selectedName: null,
      updateMode: null,
      updateSkillName: null,
    };
    const initial = select(input);
    expect(initial.statusCounts.anomaly).toBe(4);

    const filtered = select({
      ...input,
      filters: filters({ source: "acme/skills", status: "anomaly" }),
    });
    expect(filtered.skills).toBe(initial.skills);
    expect(filtered.statusCounts.all).toBe(2);
    expect(filtered.statusCounts.anomaly).toBe(2);
    expect(filtered.statusCounts.unmanaged).toBe(0);
    expect(filtered.visibleSkills.map((skill) => skill.name)).toEqual([
      "beta", "alpha",
    ]);

    const searched = select({
      ...input,
      filters: filters({
        source: "acme/skills",
        query: "beta",
        agent: "gemini-cli",
        status: "anomaly",
      }),
    });
    expect(searched.visibleSkills.map((skill) => skill.name)).toEqual(["beta"]);
    expect(searched.statusCounts).toEqual(filtered.statusCounts);
    expect(select(input).statusCounts).toBe(initial.statusCounts);
  });

  it("recomputes cached inventory data when a report or snapshot changes", () => {
    const select = createWorkspaceViewSelector();
    const snapshot = sampleSnapshot();
    const input: WorkspaceViewInput = {
      activeScope: GLOBAL_SCOPE,
      snapshot,
      updateReport: report([result("alpha", "update")]),
      filters: filters(),
      sort: "recent",
      selectedName: "alpha",
      updateMode: null,
      updateSkillName: null,
    };
    const initial = select(input);
    const updated = select({
      ...input,
      updateReport: report([result("alpha", "current")]),
    });
    expect(updated.selectedSkill?.update).toBe("current");
    expect(updated.candidateCount).toBe(0);
    expect(updated.skills).not.toBe(initial.skills);

    const refreshed = select({
      ...input,
      snapshot: {
        ...snapshot,
        skills: snapshot.skills.filter((skill) => skill.name !== "alpha"),
      },
    });
    expect(refreshed.selectedSkill).toBeNull();
    expect(refreshed.totalSkills).toBe(3);
    expect(refreshed.skills.some((skill) => skill.name === "alpha")).toBe(false);
  });

  it("overlays the report and derives rows, counts, options and last check", () => {
    const view = buildWorkspaceView({
      activeScope: GLOBAL_SCOPE,
      snapshot: sampleSnapshot(),
      updateReport: report([result("alpha", "update"), result("beta", "current")]),
      lastFullCheck: report([result("alpha", "update")]),
      updateCheckState: {
        version: 1,
        lastAttemptAt: 1000,
        lastSuccessAt: 900,
        lastOutcome: "ok",
        failedSourceCount: 0,
        cachedSourceCount: 0,
        skippedSourceCount: 0,
        candidateCount: 1,
      },
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
