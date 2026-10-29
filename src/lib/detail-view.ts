//! Pure merge of one inventory row with its on-demand SkillDetail.
//!
//! DetailPane still owns the query, tabs and dialog state; this selector owns
//! the row/detail merge and the warning/alert derivations that used to live in
//! the component.

import { hasBrokenAttachment, hasLocalModificationWarning } from "./inventory-view";
import type {
  AgentAttachment,
  InventoryAgent,
  LocalModification,
  ManagementState,
  SkillDetail,
  UpdateSkillResult,
  UpdateState,
  UpdateViewSkill,
} from "./types";

export interface DetailView {
  name: string;
  description: string | null;
  canonicalPath: string | null;
  management: ManagementState;
  localModification: LocalModification;
  attachments: AgentAttachment[];
  warnings: string[];
  update: UpdateState;
  updateResult: UpdateSkillResult | null;
  broken: boolean;
  unattached: boolean;
  localWarning: boolean;
  frontmatterWarning: boolean;
  hasAlerts: boolean;
  attachableAgents: InventoryAgent[];
}

export function selectDetailView(
  skill: UpdateViewSkill,
  detail: SkillDetail | null | undefined,
  agents: InventoryAgent[],
): DetailView {
  const management = (detail ?? skill).management;
  const localModification = (detail ?? skill).localModification;
  const attachments = (detail ?? skill).attachments;
  const warnings = detail?.warnings ?? [];
  const broken = hasBrokenAttachment({ ...skill, attachments });
  const unattached = attachments.length === 0 && management !== "unmanaged";
  const localWarning = hasLocalModificationWarning({
    ...skill,
    management,
    localModification,
  });
  const frontmatterWarning = warnings.some((warning) =>
    warning.includes("frontmatter"),
  );

  return {
    name: detail?.name ?? skill.name,
    description: detail?.description ?? skill.description,
    canonicalPath: detail?.canonicalPath ?? null,
    management,
    localModification,
    attachments,
    warnings,
    update: skill.update,
    updateResult: skill.updateResult ?? null,
    broken,
    unattached,
    localWarning,
    frontmatterWarning,
    hasAlerts:
      management === "unmanaged" ||
      management === "copy_only" ||
      localModification === "modified" ||
      localModification === "unknown" ||
      warnings.length > 0,
    attachableAgents: agents.filter(
      (agent) =>
        agent.detected &&
        !agent.universal &&
        !agent.shared &&
        !attachments.some((attachment) => attachment.agentId === agent.id),
    ),
  };
}
