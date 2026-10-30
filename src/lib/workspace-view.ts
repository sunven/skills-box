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
  InventoryAgent,
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
import {
  applyUpdateReport,
  computeUpdateSummary,
  formatEpochDateTime,
  selectUpdateDialog,
  summarizeFullCheck,
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
  checkFailed: boolean;
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
  lastCheckAt: number | null;
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
    switch (error.code) {
      case "project.lock_corrupt":
        return { label: "锁损坏", tone: "negative" };
      case "project.lock_unsupported":
        return { label: "锁版本不支持", tone: "negative" };
      case "lock.read_failed":
        return { label: "锁读取失败", tone: "negative" };
      case "project.root_unavailable":
        return { label: "路径不可用", tone: "negative" };
      default:
        return { label: "读取失败", tone: "negative" };
    }
  }
  if (loading) return { label: "读取中", tone: "muted" };
  return { label: "正常", tone: "ok" };
}

export function buildWorkspaceView(input: WorkspaceViewInput): WorkspaceView {
  const snapshotSkills = input.snapshot?.skills ?? [];
  const agents: InventoryAgent[] = input.snapshot?.agents ?? [];
  const skills = applyUpdateReport(snapshotSkills, input.updateReport);
  const anomalies = withUpdateAnomalies(
    input.snapshot?.anomalies ?? EMPTY_ANOMALIES,
    skills,
  );
  const updateSummary = computeUpdateSummary(skills);
  const visibleSkills = selectVisibleSkills(skills, input.filters, input.sort);
  const selectedSkill =
    visibleSkills.find((skill) => skill.name === input.selectedName) ?? null;
  const statusCounts = computeStatusCounts(skills);
  const sourceOptions = computeSourceOptions(skills);
  const agentOptions = computeAgentOptions(skills, agents);
  const lastCheck = input.lastFullCheck
    ? summarizeFullCheck(input.lastFullCheck)
    : input.updateCheckState
      ? summarizePersistedState(input.updateCheckState)
      : null;
  const hasManagedSkills = snapshotSkills.some(
    (skill) => skill.management === "managed",
  );
  const activeProject = projectForScope(input.projects, input.activeScope);
  const activeProjectUnavailable = Boolean(
    activeProject && !activeProject.available,
  );
  const scopeError =
    input.activeScope !== "global" && !activeProjectUnavailable
      ? (input.scopeError ?? null)
      : null;
  const candidateCount = updateSummary.candidates.length;
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
        updateCount: active
          ? hasLiveReport
            ? candidateCount
            : (state?.candidateCount ?? 0)
          : (state?.candidateCount ?? 0),
        checkFailed: state?.lastOutcome === "failed",
        error: active && scopeError !== null,
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
          lastCheckAt: lastCheck?.lastAttemptAt ?? null,
        };
      })()
    : null;
  const updateDialog = input.updateMode
    ? selectUpdateDialog({
        mode: input.updateMode,
        skills,
        agents,
        summary: updateSummary,
        singleName: input.updateSkillName,
      })
    : null;

  return {
    scopeProjects,
    activeProject,
    activeProjectUnavailable,
    projectHeader,
    scopeError,
    skills,
    visibleSkills,
    selectedSkill,
    statusCounts,
    sourceOptions,
    agentOptions,
    anomalies,
    updateSummary,
    updateDialog,
    totalSkills: skills.length,
    sourceCount: sourceOptions.length,
    candidateCount,
    lastCheck,
    hasManagedSkills,
  };
}
