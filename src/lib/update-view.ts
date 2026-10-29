//! Frontend projection of `UpdateCheckReport` onto inventory rows.
//!
//! Rust reports per-skill upstream states. This module keeps update state
//! orthogonal to management/local-modification state, forces copy-only and
//! unmanaged rows to `unknown`, and derives the "更新全部" candidate/skip view.

import type {
  AnomalyGroup,
  AnomalySummary,
  AnomalyView,
  InventoryAgent,
  InventorySkill,
  UpdateCheckOutcome,
  UpdateCheckReport,
  UpdateCheckState,
  UpdateViewSkill,
  UpdateState,
} from "./types";
import { UPDATE_LABELS } from "./inventory-view";

export const updateReportQueryKey = ["updateReport"] as const;

export type UpdateBadgeVariant = "success" | "warning" | "muted" | "destructive";

export type UpdateSkipReason =
  | "local_modified"
  | "local_unknown"
  | "unknown"
  | "not_checked"
  | "path_missing"
  | "error"
  | "copy_only";

export const UPDATE_SKIP_LABELS: Record<UpdateSkipReason, string> = {
  local_modified: "本地改动",
  local_unknown: "本地改动?",
  unknown: "未知",
  not_checked: "未检查",
  path_missing: "源路径缺失",
  error: "检查失败",
  copy_only: "仅副本",
};

export interface UpdateSkipItem {
  name: string;
  reason: UpdateSkipReason;
  label: string;
  detail: string;
}

export interface UpdateSummary {
  candidates: UpdateViewSkill[];
  skipped: UpdateSkipItem[];
}

export type UpdateMode = "single" | "all";

export const UPDATE_REASON_ORDER: UpdateSkipReason[] = [
  "local_modified",
  "local_unknown",
  "unknown",
  "not_checked",
  "path_missing",
  "error",
  "copy_only",
];

export interface UpdateDialogView {
  mode: UpdateMode;
  summary: UpdateSummary;
  grouped: Map<UpdateSkipReason, UpdateSkipItem[]>;
  initialNames: string[];
  singleSkill?: UpdateViewSkill;
  singleEligible: boolean;
  singleNeedsConfirmation: boolean;
  singleCopyAgents: string[];
  singleAffectedAgents: string[];
  copyCandidates: Set<string>;
  copyCandidateCount: number;
}

export function selectUpdateDialog(input: {
  mode: UpdateMode;
  skills: UpdateViewSkill[];
  agents: InventoryAgent[];
  summary: UpdateSummary;
  singleName: string | null;
}): UpdateDialogView {
  const universalIds = new Set(
    input.agents.filter((agent) => agent.universal).map((agent) => agent.id),
  );
  const singleSkill =
    input.mode === "single"
      ? input.skills.find((skill) => skill.name === input.singleName)
      : undefined;
  const singleEligible = Boolean(
    singleSkill &&
      singleSkill.management === "managed" &&
      singleSkill.update === "update",
  );
  const singleNeedsConfirmation = Boolean(
    singleSkill &&
      (singleSkill.localModification === "modified" ||
        singleSkill.localModification === "unknown"),
  );
  const singleCopyAgents = singleSkill
    ? copyAgentNames(singleSkill, universalIds)
    : [];
  const singleAffectedAgents = singleSkill
    ? affectedAgentNames(singleSkill, universalIds)
    : [];
  const initialNames =
    input.mode === "single"
      ? singleEligible && singleSkill
        ? [singleSkill.name]
        : []
      : input.summary.candidates.map((skill) => skill.name);
  const grouped = new Map<UpdateSkipReason, UpdateSkipItem[]>();
  for (const item of input.summary.skipped) {
    const list = grouped.get(item.reason) ?? [];
    list.push(item);
    grouped.set(item.reason, list);
  }
  const copyCandidates = new Set(
    input.summary.candidates
      .filter((skill) => copyAgentNames(skill, universalIds).length > 0)
      .map((skill) => skill.name),
  );
  const copyCandidateCount = copyCandidates.size;

  return {
    mode: input.mode,
    summary: input.summary,
    grouped,
    initialNames,
    singleSkill,
    singleEligible,
    singleNeedsConfirmation,
    singleCopyAgents,
    singleAffectedAgents,
    copyCandidates,
    copyCandidateCount,
  };
}

function copyAgentNames(
  skill: UpdateViewSkill,
  universalIds: Set<string>,
): string[] {
  return skill.attachments
    .filter(
      (attachment) =>
        attachment.mode === "copy" && !universalIds.has(attachment.agentId),
    )
    .map((attachment) => attachment.agentName);
}

function affectedAgentNames(
  skill: UpdateViewSkill,
  universalIds: Set<string>,
): string[] {
  return skill.attachments
    .filter(
      (attachment) =>
        (attachment.mode === "link" || attachment.mode === "copy") &&
        !universalIds.has(attachment.agentId),
    )
    .map((attachment) => attachment.agentName);
}

export function applyUpdateReport(
  skills: InventorySkill[],
  report?: UpdateCheckReport | null,
): UpdateViewSkill[] {
  const byName = new Map(
    (report?.skills ?? []).map((result) => [result.name, result]),
  );
  return skills.map((skill) => {
    if (skill.management !== "managed") {
      // Copy-only and unmanaged rows can never be update candidates.
      return { ...skill, update: "unknown" };
    }
    const result = byName.get(skill.name);
    return {
      ...skill,
      update: result?.state ?? "unknown",
      updateResult: result,
    };
  });
}

export function mergeUpdateReports(
  base: UpdateCheckReport | undefined,
  patch: UpdateCheckReport,
): UpdateCheckReport {
  if (!base) return patch;
  const skills = new Map(base.skills.map((skill) => [skill.name, skill]));
  for (const skill of patch.skills) skills.set(skill.name, skill);

  const sources = new Map(
    base.sources.map((source) => [sourceKey(source.source, source.ref), source]),
  );
  for (const source of patch.sources) {
    const key = sourceKey(source.source, source.ref);
    const previous = sources.get(key);
    sources.set(key, {
      ...source,
      skillCount: Math.max(previous?.skillCount ?? 0, source.skillCount),
    });
  }

  const mergedSkills = [...skills.values()].sort((left, right) =>
    left.name.localeCompare(right.name),
  );
  const mergedSources = [...sources.values()].sort(
    (left, right) =>
      left.source.localeCompare(right.source) ||
      (left.ref ?? "").localeCompare(right.ref ?? ""),
  );
  const lastSuccessAtEpochMs = maxNullable(
    base.lastSuccessAtEpochMs,
    patch.lastSuccessAtEpochMs,
  );
  return {
    skills: mergedSkills,
    sources: mergedSources,
    checkedAtEpochMs: patch.checkedAtEpochMs,
    lastSuccessAtEpochMs,
    skippedSourceCount: mergedSources.filter(
      (source) => source.status === "skipped",
    ).length,
    cachedSourceCount: mergedSources.filter(
      (source) => source.status === "cached",
    ).length,
    warnings: [...new Set([...base.warnings, ...patch.warnings])],
  };
}

function sourceKey(source: string, reference: string | null): string {
  return `${source.toLocaleLowerCase()}\n${reference ?? "HEAD"}`;
}

function maxNullable(
  left: number | null | undefined,
  right: number | null | undefined,
): number | null {
  if (left == null) return right ?? null;
  if (right == null) return left;
  return Math.max(left, right);
}

export interface LastCheckSummary {
  lastAttemptAt: number;
  lastSuccessAt: number | null;
  outcome: UpdateCheckOutcome;
  failedSourceCount: number;
  cachedSourceCount: number;
  skippedSourceCount: number;
  warnings: string[];
}

export function summarizeFullCheck(
  report: UpdateCheckReport,
): LastCheckSummary {
  const failedSourceCount = report.sources.filter(
    (source) => source.status === "error",
  ).length;
  const successfulSourceCount = report.sources.filter(
    (source) => source.status === "ok" || source.status === "cached",
  ).length;
  const outcome: UpdateCheckOutcome =
    failedSourceCount > 0 && successfulSourceCount === 0
      ? "failed"
      : failedSourceCount > 0 || report.skippedSourceCount > 0
        ? "partial"
        : "ok";
  return {
    lastAttemptAt: report.checkedAtEpochMs,
    lastSuccessAt: report.lastSuccessAtEpochMs,
    outcome,
    failedSourceCount,
    cachedSourceCount: report.cachedSourceCount,
    skippedSourceCount: report.skippedSourceCount,
    warnings: report.warnings,
  };
}

export function summarizePersistedState(
  state: UpdateCheckState,
): LastCheckSummary {
  return {
    lastAttemptAt: state.lastAttemptAt,
    lastSuccessAt: state.lastSuccessAt,
    outcome: state.lastOutcome,
    failedSourceCount: state.failedSourceCount,
    cachedSourceCount: state.cachedSourceCount,
    skippedSourceCount: state.skippedSourceCount,
    warnings: [],
  };
}

/**
 * After an update task succeeds, mark the patched skills as current without
 * an automatic network check. Failed/skipped rows keep their previous state so
 * a retry still sees them as candidates.
 */
export function markSkillsCurrent(
  report: UpdateCheckReport | undefined,
  updatedNames: string[],
): UpdateCheckReport | undefined {
  if (!report || updatedNames.length === 0) return report;
  const names = new Set(updatedNames);
  return {
    ...report,
    skills: report.skills.map((skill) =>
      names.has(skill.name)
        ? {
            ...skill,
            state: "current",
            lockHash: skill.remoteHash ?? skill.lockHash,
            previousState: null,
            error: null,
            note: null,
            cached: false,
          }
        : skill,
    ),
  };
}

export function updateBadgeVariant(state: UpdateState): UpdateBadgeVariant {
  switch (state) {
    case "current":
      return "success";
    case "update":
      return "warning";
    case "error":
    case "path_missing":
      return "destructive";
    default:
      return "muted";
  }
}

export function updateBadgeTitle(skill: UpdateViewSkill): string | undefined {
  const result = skill.updateResult;
  if (!result) return undefined;
  const parts: string[] = [];
  if (result.error) parts.push(result.error.message);
  if (result.note) parts.push(result.note);
  if (result.state === "path_missing" && !result.note) {
    parts.push("来源中已找不到该 skill，可能已被删除或移动。");
  }
  if (result.cached) parts.push("缓存结果");
  if (result.lastSuccessAtEpochMs) {
    parts.push(`上次成功检查：${formatEpochDateTime(result.lastSuccessAtEpochMs)}`);
  }
  if (result.previousState && result.previousState !== "current") {
    parts.push(`上次结果：${UPDATE_LABELS[result.previousState]}`);
  }
  return parts.length > 0 ? parts.join(" · ") : undefined;
}

export function withUpdateAnomalies(
  anomalies: AnomalySummary,
  skills: UpdateViewSkill[],
): AnomalyView {
  const pathMissing = anomalyGroup(
    skills
      .filter((skill) => skill.update === "path_missing")
      .map((skill) => `${skill.name} · 源路径缺失`),
  );
  const checkFailed = anomalyGroup(
    skills
      .filter((skill) => skill.update === "error")
      .map((skill) => `${skill.name} · 检查失败`),
  );
  return {
    ...anomalies,
    total: anomalies.total + pathMissing.count + checkFailed.count,
    pathMissing,
    checkFailed,
  };
}

export function computeUpdateSummary(skills: UpdateViewSkill[]): UpdateSummary {
  const candidates: UpdateViewSkill[] = [];
  const skipped: UpdateSkipItem[] = [];

  for (const skill of skills) {
    if (skill.management === "unmanaged") continue;
    if (skill.management === "copy_only") {
      skipped.push(skip(skill, "copy_only", "仅副本没有 canonical 目录，不可更新。"));
      continue;
    }
    if (skill.localModification === "modified") {
      skipped.push(
        skip(skill, "local_modified", "检测到本地改动，需确认后单独更新。"),
      );
      continue;
    }
    if (skill.localModification === "unknown") {
      skipped.push(
        skip(skill, "local_unknown", "无法确认本地改动，需确认后单独更新。"),
      );
      continue;
    }
    switch (skill.update) {
      case "update":
        candidates.push(skill);
        break;
      case "path_missing":
        skipped.push(
          skip(
            skill,
            "path_missing",
            skill.updateResult?.note ?? "来源中已找不到该 skill，更新已禁用。",
          ),
        );
        break;
      case "error":
        skipped.push(
          skip(
            skill,
            "error",
            skill.updateResult?.error?.message ?? "检查失败，保留上次结果。",
          ),
        );
        break;
      case "unknown": {
        const note = skill.updateResult?.note ?? "无法检查更新。";
        const reason = note.includes("上限") || note.includes("尚未检查")
          ? "not_checked"
          : "unknown";
        skipped.push(skip(skill, reason, note));
        break;
      }
      case "current":
        break;
    }
  }

  return { candidates, skipped };
}

function skip(
  skill: UpdateViewSkill,
  reason: UpdateSkipReason,
  detail: string,
): UpdateSkipItem {
  return { name: skill.name, reason, label: UPDATE_SKIP_LABELS[reason], detail };
}

export function formatEpochDateTime(value: number | null): string {
  if (value == null) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "—";
  const pad = (part: number) => String(part).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(
    date.getDate(),
  )} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

export function anomalyGroup(examples: string[]): AnomalyGroup {
  return { count: examples.length, examples: examples.slice(0, 3) };
}
