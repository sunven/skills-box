import { describe, expect, it } from "vitest";
import { selectDetailView } from "./detail-view";
import {
  attachment,
  claudeAgent,
  codexAgent,
  defaultAgents,
  geminiAgent,
  makeUpdateViewSkill,
} from "../test/inventory-fixtures";
import { makeDetail } from "../test/detail-fixtures";
import type { InventoryAgent, UpdateSkillResult } from "./types";

const qoderAgent: InventoryAgent = {
  id: "qoder",
  displayName: "Qoder",
  universal: false,
  shared: false,
  globalSkillsDir: "~/.qoder/skills",
  origin: "catalog",
  detected: true,
};

const updateResult: UpdateSkillResult = {
  name: "alpha",
  source: "acme/skills",
  ref: "main",
  lockHash: "a".repeat(40),
  remoteHash: "b".repeat(40),
  state: "update",
  cached: false,
  lastSuccessAtEpochMs: 900,
  previousState: null,
  error: null,
  note: null,
};

describe("detail view selector", () => {
  it("uses the detail merge for warnings and derived flags", () => {
    const skill = {
      ...makeUpdateViewSkill({
        name: "alpha",
        description: "row desc",
        management: "managed",
        localModification: "clean",
        attachments: [attachment("claude-code", "link")],
        warnings: ["row warning"],
      }),
      update: "update" as const,
      updateResult,
    };
    const detail = makeDetail({
      name: "alpha",
      description: "detail desc",
      management: "managed",
      localModification: "modified",
      attachments: [
        attachment("claude-code", "broken"),
        attachment("gemini-cli", "copy"),
      ],
      warnings: ["SKILL.md frontmatter 解析失败"],
    });
    const view = selectDetailView(skill, detail, [...defaultAgents, qoderAgent]);

    expect(view.name).toBe("alpha");
    expect(view.description).toBe("detail desc");
    expect(view.canonicalPath).toBe("~/.agents/skills/alpha");
    expect(view.management).toBe("managed");
    expect(view.localModification).toBe("modified");
    expect(view.attachments).toHaveLength(2);
    expect(view.warnings).toEqual(["SKILL.md frontmatter 解析失败"]);
    expect(view.update).toBe("update");
    expect(view.updateResult).toBe(updateResult);
    expect(view.broken).toBe(true);
    expect(view.unattached).toBe(false);
    expect(view.localWarning).toBe(true);
    expect(view.frontmatterWarning).toBe(true);
    expect(view.hasAlerts).toBe(true);
    expect(view.attachableAgents).toEqual([qoderAgent]);
  });

  it("falls back to the row when detail has not loaded", () => {
    const skill = {
      ...makeUpdateViewSkill({
        name: "alpha",
        description: "row desc",
        management: "managed",
        localModification: "unknown",
        warnings: ["row warning"],
      }),
      updateResult,
    };
    const view = selectDetailView(skill, null, [claudeAgent, geminiAgent, codexAgent]);

    expect(view.name).toBe("alpha");
    expect(view.description).toBe("row desc");
    expect(view.canonicalPath).toBeNull();
    expect(view.management).toBe("managed");
    expect(view.localModification).toBe("unknown");
    expect(view.warnings).toEqual([]);
    expect(view.updateResult).toBe(updateResult);
    expect(view.localWarning).toBe(true);
    expect(view.hasAlerts).toBe(true);
  });
});
