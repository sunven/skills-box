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
  InventoryAgent,
  InventorySnapshot,
  UpdateCheckReport,
  UpdateCheckState,
  UpdateViewSkill,
} from "./types";
import {
  applyUpdateReport,
  computeUpdateSummary,
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
};

export interface WorkspaceViewInput {
  snapshot?: InventorySnapshot;
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
    candidateCount: updateSummary.candidates.length,
    lastCheck,
    hasManagedSkills,
  };
}
