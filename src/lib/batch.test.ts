import { describe, expect, it } from "vitest";
import { completedSkillIds, mergeBatchRows, type BatchRow } from "./batch";
const row = (
  agentId: string,
  status: BatchRow["status"],
  eligible = true,
): BatchRow => ({
  key: agentId,
  skillId: "project:one/alpha",
  name: "alpha",
  agentId,
  agentName: agentId,
  status,
  eligible,
  reason: null,
  error: null,
  backupPath: null,
  localModification: "clean",
  deletesCopy: false,
  paths: [],
});
describe("batch result selection", () => {
  it("retains skills with partial Agent failures and clears only fully completed eligible targets", () => {
    expect(
      completedSkillIds([row("claude", "success"), row("qoder", "failed")])
        .size,
    ).toBe(0);
    expect(
      completedSkillIds([
        row("claude", "success"),
        row("codex", "skipped", false),
      ]).has("project:one/alpha"),
    ).toBe(true);
    expect(completedSkillIds([row("codex", "skipped", false)]).size).toBe(0);
  });
  it("retains original successes and does not clear a skill whose retry became ineligible", () => {
    const merged = mergeBatchRows(
      [row("claude", "success"), row("qoder", "failed")],
      [row("qoder", "skipped", false)],
    );
    expect(merged[0]!.status).toBe("success");
    expect(completedSkillIds(merged).size).toBe(0);
  });
});
