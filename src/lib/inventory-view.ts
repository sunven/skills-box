import type {
  AgentAttachment,
  AgentOrigin,
  InventoryAgent,
  InventorySkill,
  UpdateState,
  UpdateViewSkill,
} from "./types";

export type SortKey = "recent" | "name" | "source";
export type StatusFilter =
  | "all"
  | "update"
  | "current"
  | "unknown"
  | "anomaly"
  | "unmanaged";

/** Sentinel used by the source filter for skills without a lock source. */
export const UNKNOWN_SOURCE = "__unknown__";

export interface InventoryFilters {
  query: string;
  source: string;
  agent: string;
  status: StatusFilter;
}

export interface SourceOption {
  value: string;
  label: string;
  count: number;
}

export interface AgentOption {
  id: string;
  displayName: string;
  universal: boolean;
  shared: boolean;
  count: number;
  origin: AgentOrigin;
  detected: boolean;
}

export type StatusCounts = Record<StatusFilter, number>;

export const STATUS_FILTERS: Array<{ value: StatusFilter; label: string }> = [
  { value: "all", label: "全部" },
  { value: "update", label: "可更新" },
  { value: "current", label: "已是最新" },
  { value: "unknown", label: "未知" },
  { value: "anomaly", label: "异常" },
  { value: "unmanaged", label: "未受管" },
];

export const SORT_OPTIONS: Array<{ value: SortKey; label: string }> = [
  { value: "recent", label: "最近更新" },
  { value: "name", label: "名称" },
  { value: "source", label: "来源" },
];

export const UPDATE_LABELS: Record<UpdateState, string> = {
  current: "已是最新",
  update: "可更新",
  unknown: "未知",
  error: "检查失败",
  path_missing: "源路径缺失",
};

export const ATTACHMENT_LABELS: Record<AgentAttachment["mode"], string> = {
  link: "链接",
  copy: "副本",
  native: "共享",
  broken: "断链",
};

export const LOCAL_MODIFICATION_LABELS: Record<
  InventorySkill["localModification"],
  string
> = {
  clean: "干净",
  modified: "本地改动",
  unknown: "本地改动?",
};

export function defaultFilters(): InventoryFilters {
  return { query: "", source: "all", agent: "all", status: "all" };
}

export function hasBrokenAttachment(skill: InventorySkill): boolean {
  return skill.attachments.some((attachment) => attachment.mode === "broken");
}

/**
 * Unmanaged skills have no lock reference, so their local-modification value
 * is always unknown and must not count as a modified warning.
 */
export function hasLocalModificationWarning(skill: InventorySkill): boolean {
  return (
    skill.management !== "unmanaged" && skill.localModification !== "clean"
  );
}

export function isAnomalyRow(skill: UpdateViewSkill): boolean {
  return (
    skill.management === "unmanaged" ||
    // Copy-only rows have no canonical folder; spec §4.2 counts them as
    // anomalies even though they stay visible and uninstallable.
    skill.management === "copy_only" ||
    hasBrokenAttachment(skill) ||
    hasLocalModificationWarning(skill) ||
    skill.update === "path_missing" ||
    skill.update === "error"
  );
}

export function matchesStatus(
  skill: UpdateViewSkill,
  status: StatusFilter,
): boolean {
  switch (status) {
    case "all":
      return true;
    case "update":
      return skill.update === "update";
    case "current":
      return skill.update === "current";
    case "unknown":
      return skill.update === "unknown";
    case "anomaly":
      return isAnomalyRow(skill);
    case "unmanaged":
      return skill.management === "unmanaged";
  }
}

export function filterSkills<T extends UpdateViewSkill>(
  skills: T[],
  filters: InventoryFilters,
): T[] {
  const query = filters.query.trim().toLocaleLowerCase();
  return skills.filter((skill) => {
    if (
      filters.source !== "all" &&
      (skill.source ?? UNKNOWN_SOURCE) !== filters.source
    ) {
      return false;
    }
    if (
      filters.agent !== "all" &&
      !skill.attachments.some(
        (attachment) => attachment.agentId === filters.agent,
      )
    ) {
      return false;
    }
    if (!matchesStatus(skill, filters.status)) return false;
    if (!query) return true;
    const haystack =
      `${skill.name} ${skill.description ?? ""} ${skill.source ?? ""}`.toLocaleLowerCase();
    return haystack.includes(query);
  });
}

export function sortSkills<T extends InventorySkill>(
  skills: T[],
  sort: SortKey,
): T[] {
  return [...skills].sort((left, right) => {
    if (sort === "name") {
      return left.name.localeCompare(right.name);
    }
    if (sort === "source") {
      const leftSource = left.source ?? "";
      const rightSource = right.source ?? "";
      if (leftSource && !rightSource) return -1;
      if (!leftSource && rightSource) return 1;
      const bySource = leftSource.localeCompare(rightSource);
      return bySource !== 0 ? bySource : left.name.localeCompare(right.name);
    }
    if (left.updatedAt && right.updatedAt) {
      const byUpdated = right.updatedAt.localeCompare(left.updatedAt);
      return byUpdated !== 0 ? byUpdated : left.name.localeCompare(right.name);
    }
    if (left.updatedAt) return -1;
    if (right.updatedAt) return 1;
    return left.name.localeCompare(right.name);
  });
}

export function selectVisibleSkills<T extends UpdateViewSkill>(
  skills: T[],
  filters: InventoryFilters,
  sort: SortKey = "recent",
): T[] {
  return sortSkills(filterSkills(skills, filters), sort);
}

export function computeStatusCounts(skills: UpdateViewSkill[]): StatusCounts {
  const counts: StatusCounts = {
    all: skills.length,
    update: 0,
    current: 0,
    unknown: 0,
    anomaly: 0,
    unmanaged: 0,
  };
  for (const skill of skills) {
    if (skill.update === "update") counts.update += 1;
    else if (skill.update === "current") counts.current += 1;
    else if (skill.update === "unknown") counts.unknown += 1;
    if (isAnomalyRow(skill)) counts.anomaly += 1;
    if (skill.management === "unmanaged") counts.unmanaged += 1;
  }
  return counts;
}

export function computeSourceOptions(skills: InventorySkill[]): SourceOption[] {
  const counts = new Map<string, number>();
  for (const skill of skills) {
    const value = skill.source ?? UNKNOWN_SOURCE;
    counts.set(value, (counts.get(value) ?? 0) + 1);
  }
  return [...counts.entries()]
    .map(([value, count]) => ({
      value,
      label: value === UNKNOWN_SOURCE ? "来源未知" : value,
      count,
    }))
    .sort(
      (left, right) =>
        right.count - left.count || left.label.localeCompare(right.label),
    );
}

export function computeAgentOptions(
  skills: InventorySkill[],
  agents: InventoryAgent[],
): AgentOption[] {
  const counts = new Map<string, number>();
  for (const skill of skills) {
    for (const agentId of new Set(
      skill.attachments.map((attachment) => attachment.agentId),
    )) {
      counts.set(agentId, (counts.get(agentId) ?? 0) + 1);
    }
  }

  const options: AgentOption[] = agents.map((agent) => ({
    id: agent.id,
    displayName: agent.displayName,
    universal: agent.universal,
    shared: agent.shared,
    count: counts.get(agent.id) ?? 0,
    origin: agent.origin,
    detected: agent.detected,
  }));

  // Be defensive: an attachment whose agent is missing from the snapshot is
  // still selectable rather than silently unfilterable.
  const known = new Set(options.map((option) => option.id));
  for (const skill of skills) {
    for (const attachment of skill.attachments) {
      if (known.has(attachment.agentId)) continue;
      known.add(attachment.agentId);
      options.push({
        id: attachment.agentId,
        displayName: attachment.agentName,
        universal: false,
        shared: false,
        count: counts.get(attachment.agentId) ?? 0,
        origin: "catalog",
        detected: true,
      });
    }
  }
  return options;
}

export function uniqueAttachments(skill: InventorySkill): AgentAttachment[] {
  const seen = new Set<string>();
  return skill.attachments.filter((attachment) => {
    if (seen.has(attachment.agentId)) return false;
    seen.add(attachment.agentId);
    return true;
  });
}

export function formatUpdatedAt(value: string | null): string {
  if (!value) return "—";
  return value.slice(0, 10);
}

export function formatEpochMs(value: number | null): string {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "—";
  return date.toISOString().slice(0, 10);
}

export function shortHash(hash: string, length = 12): string {
  return hash.length > length ? `${hash.slice(0, length)}…` : hash;
}
