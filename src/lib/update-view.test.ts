import { describe, expect, it } from "vitest";
import type {
  AnomalyGroup,
  AnomalySummary,
  InventorySkill,
  UpdateCheckReport,
  UpdateCheckState,
  UpdateSkillResult,
  UpdateState,
  UpdateViewSkill,
} from "./types";
import {
  applyUpdateReport,
  computeUpdateSummary,
  formatEpochDateTime,
  markSkillsCurrent,
  mergeUpdateReports,
  selectUpdateDialog,
  summarizePersistedState,
  updateBadgeTitle,
  withUpdateAnomalies,
} from "./update-view";
import {
  attachment,
  claudeAgent,
  codexAgent,
  geminiAgent,
  makeAnomalies,
  makeSkill,
  makeUpdateViewSkill,
} from "../test/inventory-fixtures";

function updateResult(
  name: string,
  state: UpdateState,
  overrides: Partial<UpdateSkillResult> = {},
): UpdateSkillResult {
  return {
    name,
    source: "acme/skills",
    ref: "main",
    lockHash: "a".repeat(40),
    remoteHash: state === "current" ? "a".repeat(40) : "b".repeat(40),
    state,
    cached: false,
    lastSuccessAtEpochMs: 1793163988000,
    previousState: null,
    error: null,
    note: null,
    ...overrides,
  };
}

function report(
  skills: UpdateSkillResult[],
  overrides: Partial<UpdateCheckReport> = {},
): UpdateCheckReport {
  return {
    skills,
    sources: [
      {
        source: "acme/skills",
        ref: "main",
        status: "ok",
        skillCount: skills.length,
        cached: false,
        truncated: false,
        lastSuccessAtEpochMs: 1793163988000,
        error: null,
        note: null,
      },
    ],
    checkedAtEpochMs: 1793163988000,
    lastSuccessAtEpochMs: 1793163988000,
    skippedSourceCount: 0,
    cachedSourceCount: 0,
    warnings: [],
    ...overrides,
  };
}

function skill(
  name: string,
  overrides: Partial<InventorySkill> & { update?: UpdateState } = {},
): UpdateViewSkill {
  const { update = "unknown", ...rest } = overrides;
  return {
    ...makeSkill({ name, source: "acme/skills", ...rest }),
    update,
  };
}

function group(count: number, examples: string[] = []): AnomalyGroup {
  return { count, examples };
}

function baseAnomalies(): AnomalySummary {
  return makeAnomalies({
    total: 3,
    stale: group(1, ["ghost"]),
    unmanaged: group(1, ["gamma"]),
    broken: group(1, ["delta · Claude Code"]),
  });
}

describe("applyUpdateReport", () => {
  it("overlays effective states on managed rows only", () => {
    const skills = [
      skill("alpha"),
      skill("beta"),
      skill("gamma"),
      skill("delta", { management: "copy_only", canonicalPath: null }),
      skill("epsilon", { management: "unmanaged", canonicalPath: null }),
    ];
    const view = applyUpdateReport(
      skills,
      report([
        updateResult("alpha", "update"),
        updateResult("beta", "current"),
        updateResult("gamma", "error", {
          error: { code: "network.offline", message: "离线", retryable: true },
        }),
        updateResult("delta", "update"),
        updateResult("epsilon", "current"),
      ]),
    );

    expect(view[0]?.update).toBe("update");
    expect(view[0]?.updateResult?.state).toBe("update");
    expect(view[1]?.update).toBe("current");
    expect(view[2]?.update).toBe("error");
    // Copy-only and unmanaged rows are never update candidates.
    expect(view[3]?.update).toBe("unknown");
    expect(view[3]?.updateResult).toBeUndefined();
    expect(view[4]?.update).toBe("unknown");
    expect(view[4]?.updateResult).toBeUndefined();
  });

  it("leaves rows unknown without a report", () => {
    const view = applyUpdateReport([skill("alpha")], null);
    expect(view[0]?.update).toBe("unknown");
    expect(view[0]?.updateResult).toBeUndefined();
  });
});

describe("mergeUpdateReports", () => {
  it("merges scoped checks by skill and source while keeping the latest success time", () => {
    const base = report([updateResult("alpha", "update"), updateResult("beta", "current")], {
      checkedAtEpochMs: 100,
      lastSuccessAtEpochMs: 100,
      warnings: ["旧警告"],
    });
    const patch = report(
      [
        updateResult("alpha", "error", {
          lastSuccessAtEpochMs: 200,
          previousState: "update",
          error: { code: "network.offline", message: "离线", retryable: true },
        }),
      ],
      { checkedAtEpochMs: 200, lastSuccessAtEpochMs: 200, warnings: ["新警告"] },
    );

    const merged = mergeUpdateReports(base, patch);
    expect(merged.skills.map((entry) => entry.name)).toEqual(["alpha", "beta"]);
    expect(merged.skills[0]?.state).toBe("error");
    expect(merged.skills[1]?.state).toBe("current");
    expect(merged.checkedAtEpochMs).toBe(200);
    expect(merged.lastSuccessAtEpochMs).toBe(200);
    expect(merged.warnings).toEqual(["旧警告", "新警告"]);
    expect(merged.sources[0]?.skillCount).toBe(2);
  });
});

describe("withUpdateAnomalies", () => {
  it("adds path-missing and check-failure groups to the anomaly summary", () => {
    const view = withUpdateAnomalies(
      baseAnomalies(),
      [
        skill("alpha", { update: "path_missing" }),
        skill("beta", { update: "error" }),
        skill("gamma", { update: "error" }),
        skill("delta", { update: "update" }),
      ],
    );

    expect(view.pathMissing.count).toBe(1);
    expect(view.pathMissing.examples).toEqual(["alpha · 源路径缺失"]);
    expect(view.checkFailed.count).toBe(2);
    expect(view.checkFailed.examples).toEqual(["beta · 检查失败", "gamma · 检查失败"]);
    expect(view.total).toBe(6);
  });
});

describe("computeUpdateSummary", () => {
  it("splits eligible candidates from skipped items with reasons", () => {
    const view = applyUpdateReport(
      [
        skill("candidate"),
        skill("modified", { localModification: "modified" }),
        skill("unknown-local", { localModification: "unknown" }),
        skill("missing-path"),
        skill("failed"),
        skill("unknown"),
        skill("current"),
        skill("copy", { management: "copy_only", canonicalPath: null }),
        skill("unmanaged", { management: "unmanaged", canonicalPath: null }),
      ],
      report([
        updateResult("candidate", "update"),
        updateResult("modified", "update"),
        updateResult("unknown-local", "update"),
        updateResult("missing-path", "path_missing"),
        updateResult("failed", "error"),
        updateResult("unknown", "unknown"),
        updateResult("current", "current"),
        updateResult("copy", "update"),
      ]),
    );

    const summary = computeUpdateSummary(view);
    expect(summary.candidates.map((entry) => entry.name)).toEqual(["candidate"]);
    expect(summary.skipped.map((entry) => entry.name)).toEqual([
      "modified",
      "unknown-local",
      "missing-path",
      "failed",
      "unknown",
      "copy",
    ]);
    expect(summary.skipped.map((entry) => entry.label)).toEqual([
      "本地改动",
      "本地改动?",
      "源路径缺失",
      "检查失败",
      "未知",
      "仅副本",
    ]);
  });

  it("marks capped sources as not checked", () => {
    const view = applyUpdateReport(
      [skill("alpha")],
      report([
        updateResult("alpha", "unknown", {
          unknownReason: "not_checked",
          note: "本次检查达到来源上限（20 个），该来源尚未检查。",
        }),
      ]),
    );
    expect(computeUpdateSummary(view).skipped[0]?.label).toBe("未检查");
  });
});

describe("badges and formatting", () => {
  it("builds update tooltips", () => {
    const failed = {
      ...skill("alpha", { update: "error" }),
      updateResult: updateResult("alpha", "error", {
        cached: true,
        previousState: "update",
        error: { code: "network.offline", message: "无法连接 GitHub", retryable: true },
      }),
    };
    const title = updateBadgeTitle(failed);
    expect(title).toContain("无法连接 GitHub");
    expect(title).toContain("上次结果：可更新");
    expect(title).toContain("缓存");

    const missing = {
      ...skill("beta", { update: "path_missing" }),
      updateResult: updateResult("beta", "path_missing", {
        note: "来源中已找不到记录的 skillPath。",
      }),
    };
    expect(updateBadgeTitle(missing)).toContain("找不到");
  });

  it("formats epoch times deterministically and handles invalid values", () => {
    expect(formatEpochDateTime(1793163988000)).toBe("2026-10-28 13:06");
    expect(formatEpochDateTime(null)).toBe("—");
    expect(formatEpochDateTime(Number.NaN)).toBe("—");
  });

  it("marks only successfully updated skills current after an update", () => {
    const base = report([
      updateResult("alpha", "update"),
      updateResult("beta", "update"),
      updateResult("gamma", "current"),
    ]);
    const patched = markSkillsCurrent(base, ["alpha"])!;
    const alpha = patched.skills.find((entry) => entry.name === "alpha")!;
    expect(alpha.state).toBe("current");
    expect(alpha.lockHash).toBe(alpha.remoteHash);
    expect(alpha.error).toBeNull();
    expect(alpha.note).toBeNull();
    expect(
      patched.skills.find((entry) => entry.name === "beta")!.state,
    ).toBe("update");
    expect(markSkillsCurrent(undefined, ["alpha"])).toBeUndefined();
  });
});

describe("last check summaries", () => {
  it("maps a persisted state without warnings", () => {
    const state: UpdateCheckState = {
      version: 1,
      lastAttemptAt: 1793163990000,
      lastSuccessAt: null,
      lastOutcome: "failed",
      failedSourceCount: 2,
      cachedSourceCount: 0,
      skippedSourceCount: 1,
    };

    expect(summarizePersistedState(state)).toEqual({
      lastAttemptAt: 1793163990000,
      lastSuccessAt: null,
      outcome: "failed",
      failedSourceCount: 2,
      cachedSourceCount: 0,
      skippedSourceCount: 1,
      warnings: [],
    });
  });
});


describe("selectUpdateDialog", () => {
  it("preserves single/all eligibility and filters universal copy agents", () => {
    const skill = {
      ...makeUpdateViewSkill({
        name: "alpha",
        management: "managed",
        localModification: "clean",
        attachments: [
          attachment("claude-code", "copy"),
          attachment("gemini-cli", "copy"),
          attachment("codex", "copy"),
        ],
      }),
      update: "update" as const,
    };
    const summary = computeUpdateSummary([skill]);
    const agents = [claudeAgent, geminiAgent, codexAgent];

    const all = selectUpdateDialog({
      mode: "all",
      skills: [skill],
      agents,
      summary,
      singleName: null,
    });
    expect(all.initialNames).toEqual(["alpha"]);
    expect(all.copyCandidates.size).toBe(1);
    expect(all.singleCopyAgents).toEqual([]);

    const single = selectUpdateDialog({
      mode: "single",
      skills: [skill],
      agents,
      summary,
      singleName: "alpha",
    });
    expect(single.initialNames).toEqual(["alpha"]);
    expect(single.singleCopyAgents).toEqual(["Claude Code"]);
    expect(single.singleAffectedAgents).toEqual(["Claude Code"]);
  });

  it("skips modified candidates in all mode but allows single confirmation", () => {
    const modified = {
      ...makeUpdateViewSkill({
        name: "modified",
        management: "managed",
        localModification: "modified",
      }),
      update: "update" as const,
    };
    const summary = computeUpdateSummary([modified]);
    const agents = [claudeAgent];

    const all = selectUpdateDialog({
      mode: "all",
      skills: [modified],
      agents,
      summary,
      singleName: null,
    });
    expect(all.initialNames).toEqual([]);
    expect(all.grouped.get("local_modified")?.[0]?.name).toBe("modified");

    const single = selectUpdateDialog({
      mode: "single",
      skills: [modified],
      agents,
      summary,
      singleName: "modified",
    });
    expect(single.initialNames).toEqual(["modified"]);
    expect(single.singleNeedsConfirmation).toBe(true);
  });
});
