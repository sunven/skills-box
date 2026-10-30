//! Pure workspace projection over the inventory, update report and local UI
//! inputs. App owns the query data and UI state; this module owns every derived
//! value those surfaces display.

import {
  computeAgentOptions,
  computeSourceOptions,
  computeStatusCounts,
  selectVisibleSkills,
  type InventoryFilters,
  type SortKey,
} from "./inventory-view";
import type {
  AnomalySummary,
  AnomalyView,
  AppError,
  InventorySnapshot,
  ProjectEntry,
  ProjectList,
  ScopeId,
  UpdateCheckReport,
  UpdateCheckState,
  UpdateViewSkill,
} from "./types";
import {
  projectCanonicalPath,
  projectForScope,
  projectScope,
} from "./scope";
import { isLockError } from "./api";
import {
  applyUpdateReport,
  computeUpdateSummary,
  formatEpochDateTime,
  selectUpdateDialog,
  summarizePersistedState,
  withUpdateAnomalies,
  type LastCheckSummary,
  type UpdateDialogView,
  type UpdateMode,
  type UpdateSummary,
} from "./update-view";

const EMPTY_ANOMALIES: AnomalySummary = {
  total: 0,
  stale: { count: 0, examples: [] },
  unmanaged: { count: 0, examples: [] },
  broken: { count: 0, examples: [] },
  modified: { count: 0, examples: [] },
  escape: { count: 0, examples: [] },
};

export interface ScopeProjectView {
  project: ProjectEntry;
  scope: ScopeId;
  active: boolean;
  unavailable: boolean;
  unavailableReason: string | null;
  updateCount: number;
  error: boolean;
}

export type ProjectLockTone = "ok" | "warning" | "negative" | "muted";

export interface ProjectHeaderView {
  project: ProjectEntry;
  root: string;
  canonicalPath: string;
  lockLabel: string;
  lockTone: ProjectLockTone;
  /** Formatted attempt time, or null when the scope was never checked. */
  lastCheckLabel: string;
}

export interface WorkspaceViewInput {
  snapshot?: InventorySnapshot;
  projects?: ProjectList;
  activeScope: ScopeId;
  /** Active scope inventory failure; project lock errors are scope states. */
  scopeError?: AppError | null;
  scopeCheckStates?: ReadonlyMap<ScopeId, UpdateCheckState | null>;
  inventoryLoading?: boolean;
  updateReport?: UpdateCheckReport | null;
  lastFullCheck?: UpdateCheckReport | null;
  updateCheckState?: UpdateCheckState | null;
  filters: InventoryFilters;
  sort: SortKey;
  selectedName: string | null;
  updateMode: UpdateMode | null;
  updateSkillName: string | null;
}

export interface WorkspaceView {
  scopeProjects: ScopeProjectView[];
  activeProject: ProjectEntry | null;
  activeProjectUnavailable: boolean;
  projectHeader: ProjectHeaderView | null;
  scopeError: AppError | null;
  skills: UpdateViewSkill[];
  visibleSkills: UpdateViewSkill[];
  selectedSkill: UpdateViewSkill | null;
  statusCounts: ReturnType<typeof computeStatusCounts>;
  sourceOptions: ReturnType<typeof computeSourceOptions>;
  agentOptions: ReturnType<typeof computeAgentOptions>;
  anomalies: AnomalyView;
  updateSummary: UpdateSummary;
  updateDialog: UpdateDialogView | null;
  totalSkills: number;
  sourceCount: number;
  candidateCount: number;
  lastCheck: LastCheckSummary | null;
  hasManagedSkills: boolean;
}

function lockView(
  error: AppError | null,
  unavailable: boolean,
  loading: boolean | undefined,
): { label: string; tone: ProjectLockTone } {
  if (unavailable) return { label: "—", tone: "muted" };
  if (error) {
    if (error.code === "project.lock_unsupported") {
      return { label: "锁版本不支持", tone: "negative" };
    }
    if (isLockError(error)) {
      return error.code === "lock.corrupt"
        ? { label: "锁损坏", tone: "negative" }
        : { label: "锁读取失败", tone: "negative" };
    }
    if (error.code === "project.root_unavailable") {
      return { label: "路径不可用", tone: "negative" };
    }
    return { label: "读取失败", tone: "negative" };
  }
  if (loading) return { label: "读取中", tone: "muted" };
  return { label: "正常", tone: "ok" };
}

type InventoryProjection = Pick<
  WorkspaceView,
  | "skills"
  | "anomalies"
  | "updateSummary"
  | "statusCounts"
  | "sourceOptions"
  | "agentOptions"
  | "totalSkills"
  | "sourceCount"
  | "candidateCount"
  | "hasManagedSkills"
>;

function projectInventory(
  snapshot: InventorySnapshot | undefined,
  updateReport: UpdateCheckReport | null | undefined,
): InventoryProjection {
  const snapshotSkills = snapshot?.skills ?? [];
  const skills = applyUpdateReport(snapshotSkills, updateReport);
  const anomalies = withUpdateAnomalies(
    snapshot?.anomalies ?? EMPTY_ANOMALIES,
    skills,
  );
  const updateSummary = computeUpdateSummary(skills);
  const sourceOptions = computeSourceOptions(skills);
  return {
    skills,
    anomalies,
    updateSummary,
    statusCounts: computeStatusCounts(skills),
    sourceOptions,
    agentOptions: computeAgentOptions(skills, snapshot?.agents ?? []),
    totalSkills: skills.length,
    sourceCount: sourceOptions.length,
    candidateCount: updateSummary.candidates.length,
    hasManagedSkills: snapshotSkills.some(
      (skill) => skill.management === "managed",
    ),
  };
}

function projectWorkspace(
  input: WorkspaceViewInput,
  inventory: InventoryProjection,
): WorkspaceView {
  const { skills, updateSummary, candidateCount } = inventory;
  const visibleSkills = selectVisibleSkills(skills, input.filters, input.sort);
  const selectedSkill =
    visibleSkills.find((skill) => skill.name === input.selectedName) ?? null;
  const lastCheck = input.updateCheckState
    ? {
        ...summarizePersistedState(input.updateCheckState),
        warnings: input.lastFullCheck?.warnings ?? [],
      }
    : null;
  const activeProject = projectForScope(input.projects, input.activeScope);
  const activeProjectUnavailable = Boolean(
    activeProject && !activeProject.available,
  );
  const scopeError =
    input.activeScope !== "global" && !activeProjectUnavailable
      ? (input.scopeError ?? null)
      : null;
  const hasLiveReport = input.updateReport != null;
  const scopeState = input.scopeCheckStates?.get(input.activeScope) ?? null;
  const scopeProjects: ScopeProjectView[] = [
    ...(input.projects?.projects ?? []),
  ]
    .sort(
      (left, right) =>
        right.lastOpenedAt - left.lastOpenedAt ||
        right.addedAt - left.addedAt ||
        left.id.localeCompare(right.id),
    )
    .map((project) => {
      const scope = projectScope(project.id);
      const active = scope === input.activeScope;
      const state = input.scopeCheckStates?.get(scope) ?? null;
      return {
        project,
        scope,
        active,
        unavailable: !project.available,
        unavailableReason: project.unavailableReason ?? null,
        updateCount:
          active && hasLiveReport ? candidateCount : (state?.candidateCount ?? 0),
        error:
          project.lockError != null ||
          state?.lastOutcome === "failed" ||
          (active && scopeError !== null),
      };
    });
  const projectHeader: ProjectHeaderView | null = activeProject
    ? (() => {
        const lock = lockView(
          scopeError,
          activeProjectUnavailable,
          input.inventoryLoading,
        );
        const lastCheck = scopeState ?? input.updateCheckState ?? null;
        return {
          project: activeProject,
          root: activeProject.root,
          canonicalPath: projectCanonicalPath(activeProject.root),
          lockLabel: lock.label,
          lockTone: lock.tone,
          lastCheckLabel: lastCheck
            ? `上次检查：${formatEpochDateTime(lastCheck.lastAttemptAt)}`
            : "尚未检查",
        };
      })()
    : null;
  const updateDialog = input.updateMode
    ? selectUpdateDialog({
        mode: input.updateMode,
        skills,
        agents: input.snapshot?.agents ?? [],
        summary: updateSummary,
        singleName: input.updateSkillName,
      })
    : null;

  return {
    ...inventory,
    scopeProjects,
    activeProject,
    activeProjectUnavailable,
    projectHeader,
    scopeError,
    visibleSkills,
    selectedSkill,
    updateDialog,
    lastCheck,
  };
}

export function buildWorkspaceView(input: WorkspaceViewInput): WorkspaceView {
  return projectWorkspace(
    input,
    projectInventory(input.snapshot, input.updateReport),
  );
}

/** Each App owns one selector; UI changes reuse the inventory's row objects. */
export function createWorkspaceViewSelector() {
  let snapshot: InventorySnapshot | undefined;
  let updateReport: UpdateCheckReport | null = null;
  let inventory: InventoryProjection | undefined;

  return (input: WorkspaceViewInput): WorkspaceView => {
    const report = input.updateReport ?? null;
    if (!inventory || snapshot !== input.snapshot || updateReport !== report) {
      inventory = projectInventory(input.snapshot, report);
      snapshot = input.snapshot;
      updateReport = report;
    }
    return projectWorkspace(input, inventory);
  };
}
