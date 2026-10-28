import { describe, expect, it } from "vitest";
import {
  UNKNOWN_SOURCE,
  computeAgentOptions,
  computeSourceOptions,
  computeStatusCounts,
  filterSkills,
  formatEpochMs,
  formatUpdatedAt,
  hasLocalModificationWarning,
  isAnomalyRow,
  selectVisibleSkills,
  shortHash,
  sortSkills,
} from "./inventory-view";
import {
  attachment,
  makeSkill,
  makeSnapshot,
  sampleSkills,
} from "../test/inventory-fixtures";

const defaults = {
  query: "",
  source: "all",
  agent: "all",
  status: "all" as const,
};

describe("inventory view model", () => {
  it("searches name, description and source case-insensitively but not paths", () => {
    const skills = sampleSkills();
    expect(
      filterSkills(skills, { ...defaults, query: "alpha" }).map((s) => s.name),
    ).toEqual(["alpha"]);
    expect(
      filterSkills(skills, { ...defaults, query: "单行描述" }).map((s) => s.name),
    ).toEqual(["alpha", "beta"]);
    expect(
      filterSkills(skills, { ...defaults, query: "OTHER/REPO" }).map(
        (s) => s.name,
      ),
    ).toEqual(["delta"]);
    expect(filterSkills(skills, { ...defaults, query: "~/.agents" })).toEqual([]);
  });

  it("filters by source, agent attachment and status", () => {
    const skills = sampleSkills();
    expect(
      filterSkills(skills, { ...defaults, source: "acme/skills" }).map(
        (s) => s.name,
      ),
    ).toEqual(["alpha", "beta"]);
    expect(
      filterSkills(skills, { ...defaults, source: UNKNOWN_SOURCE }).map(
        (s) => s.name,
      ),
    ).toEqual(["gamma"]);
    expect(
      filterSkills(skills, { ...defaults, agent: "gemini-cli" }).map(
        (s) => s.name,
      ),
    ).toEqual(["beta"]);
    expect(
      filterSkills(skills, { ...defaults, status: "unmanaged" }).map(
        (s) => s.name,
      ),
    ).toEqual(["gamma"]);
    expect(
      filterSkills(skills, { ...defaults, status: "anomaly" }).map(
        (s) => s.name,
      ),
    ).toEqual(["gamma"]);
    expect(
      filterSkills(skills, { ...defaults, status: "unknown" }).map(
        (s) => s.name,
      ),
    ).toEqual(["alpha", "beta", "gamma", "delta"]);
  });

  it("counts are derived from the full inventory regardless of other filters", () => {
    const skills = sampleSkills();
    const counts = computeStatusCounts(skills);
    expect(counts).toEqual({
      all: 4,
      update: 0,
      current: 0,
      unknown: 4,
      anomaly: 1,
      unmanaged: 1,
    });

    const sources = computeSourceOptions(skills);
    expect(sources).toEqual([
      { value: "acme/skills", label: "acme/skills", count: 2 },
      { value: "other/repo", label: "other/repo", count: 1 },
      { value: UNKNOWN_SOURCE, label: "来源未知", count: 1 },
    ]);

    const agents = computeAgentOptions(skills, makeSnapshot().agents);
    expect(agents.map((agent) => [agent.id, agent.count])).toEqual([
      ["claude-code", 4],
      ["gemini-cli", 1],
      ["codex", 1],
    ]);
  });

  it("treats unmanaged skills and broken attachments as anomalies", () => {
    const broken = makeSkill({
      name: "broken",
      attachments: [attachment("claude-code", "broken")],
    });
    expect(isAnomalyRow(broken)).toBe(true);
    expect(isAnomalyRow(makeSkill({ name: "ok" }))).toBe(false);
  });

  it("treats managed local modifications and unknowns as anomalies, but not unmanaged ones", () => {
    const modified = makeSkill({
      name: "modified",
      localModification: "modified",
    });
    const unknown = makeSkill({
      name: "unknown",
      localModification: "unknown",
    });
    const unmanaged = makeSkill({
      name: "unmanaged",
      management: "unmanaged",
      localModification: "unknown",
    });
    expect(hasLocalModificationWarning(modified)).toBe(true);
    expect(hasLocalModificationWarning(unknown)).toBe(true);
    expect(hasLocalModificationWarning(unmanaged)).toBe(false);
    expect(isAnomalyRow(modified)).toBe(true);
    expect(isAnomalyRow(unknown)).toBe(true);

    const skills = [modified, unknown, unmanaged, makeSkill({ name: "ok" })];
    expect(
      filterSkills(skills, { ...defaults, status: "anomaly" }).map(
        (skill) => skill.name,
      ),
    ).toEqual(["modified", "unknown", "unmanaged"]);
    expect(computeStatusCounts(skills).anomaly).toBe(3);
  });

  it("sorts by updatedAt desc by default with name as tie-breaker", () => {
    const skills = sampleSkills();
    expect(sortSkills(skills, "recent").map((s) => s.name)).toEqual([
      "beta",
      "alpha",
      "delta",
      "gamma",
    ]);
    expect(sortSkills(skills, "name").map((s) => s.name)).toEqual([
      "alpha",
      "beta",
      "delta",
      "gamma",
    ]);
    expect(sortSkills(skills, "source").map((s) => s.name)).toEqual([
      "alpha",
      "beta",
      "delta",
      "gamma",
    ]);
  });

  it("keeps null sources last and uses the name as a stable tie-breaker", () => {
    const skills = [
      makeSkill({ name: "zed", source: null }),
      makeSkill({ name: "mu", source: "b/repo" }),
      makeSkill({ name: "nu", source: "b/repo" }),
      makeSkill({ name: "alpha", source: "a/repo" }),
    ];
    expect(sortSkills(skills, "source").map((s) => s.name)).toEqual([
      "alpha",
      "mu",
      "nu",
      "zed",
    ]);
  });

  it("combines filter and sort without mutating the source array", () => {
    const skills = sampleSkills();
    const before = skills.map((s) => s.name);
    const visible = selectVisibleSkills(skills, {
      ...defaults,
      agent: "claude-code",
    });
    expect(visible.map((s) => s.name)).toEqual(["beta", "alpha", "delta", "gamma"]);
    expect(skills.map((s) => s.name)).toEqual(before);
  });

  it("formats the updated time and uses — when absent", () => {
    expect(formatUpdatedAt("2026-10-05T08:00:00.000Z")).toBe("2026-10-05");
    expect(formatUpdatedAt(null)).toBe("—");
  });

  it("formats directory mtimes and short hashes for the detail overview", () => {
    expect(formatEpochMs(1793163988000)).toBe("2026-10-28");
    expect(formatEpochMs(null)).toBe("—");
    expect(shortHash("0123456789abcdef0123456789abcdef01234567")).toBe(
      "0123456789ab…",
    );
    expect(shortHash("abc")).toBe("abc");
  });
});
