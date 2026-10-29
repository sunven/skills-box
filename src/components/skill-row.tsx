import { Badge } from "./ui/badge";
import { agentInitials } from "../lib/agent-style";
import { agentLogoUrl } from "../lib/agent-logos";
import type { InventorySkill } from "../lib/types";
import {
  ATTACHMENT_LABELS,
  LOCAL_MODIFICATION_LABELS,
  UPDATE_LABELS,
  formatUpdatedAt,
  hasBrokenAttachment,
  hasLocalModificationWarning,
  uniqueAttachments,
} from "../lib/inventory-view";
import {
  updateBadgeTitle,
  type UpdateViewSkill,
} from "../lib/update-view";

interface AgentAvatarsProps {
  skill: InventorySkill;
}

function AgentAvatars({ skill }: AgentAvatarsProps) {
  const attachments = uniqueAttachments(skill);
  if (attachments.length === 0) return null;
  const shown = attachments.slice(0, 5);
  const remaining = attachments.length - shown.length;

  return (
    <div className="mount-row" data-testid="agent-stack">
      {shown.map((attachment) => {
        const broken = attachment.mode === "broken";
        const label = ATTACHMENT_LABELS[attachment.mode];
        const logo = agentLogoUrl(attachment.agentId);
        return (
          <span
            key={attachment.agentId}
            data-testid="agent-avatar"
            data-mode={attachment.mode}
            title={`${attachment.agentName} · ${label}${broken ? "（断链）" : ""}`}
            aria-label={`${attachment.agentName}（${label}${broken ? "，断链" : ""}）`}
            className="agent-token"
          >
            {logo ? (
              <img
                src={logo}
                alt=""
                aria-hidden="true"
                draggable={false}
                className="agent-token-logo"
              />
            ) : (
              agentInitials(attachment.agentName)
            )}
          </span>
        );
      })}
      {remaining > 0 ? (
        <span
          data-testid="agent-more"
          aria-label={`另有 ${remaining} 个 agent`}
          className="mount-more"
        >
          +{remaining}
        </span>
      ) : null}
    </div>
  );
}

interface SkillRowProps {
  skill: UpdateViewSkill;
  selected: boolean;
  onSelect: (name: string) => void;
}

function UpdateStatus({ skill }: { skill: UpdateViewSkill }) {
  const tone =
    skill.update === "update"
      ? "accent"
      : skill.update === "error" || skill.update === "path_missing"
        ? "negative"
        : "muted";
  return (
    <span
      data-testid="update-badge"
      className="status-chip"
      data-tone={tone}
      title={updateBadgeTitle(skill)}
    >
      {UPDATE_LABELS[skill.update]}
    </span>
  );
}

export function SkillRow({ skill, selected, onSelect }: SkillRowProps) {
  const broken = hasBrokenAttachment(skill);
  const unattached =
    skill.attachments.length === 0 && skill.management !== "unmanaged";

  return (
    <button
      type="button"
      data-testid="skill-row"
      data-skill-name={skill.name}
      aria-pressed={selected}
      onClick={() => onSelect(skill.name)}
      className="skill-row skill-grid"
    >
      <div className="min-w-0">
        <div className="skill-name-line">
          <span className="skill-name">{skill.name}</span>
          {skill.management === "copy_only" ? (
            <span className="skill-note">仅副本</span>
          ) : null}
          {skill.management === "unmanaged" ? (
            <span className="skill-note" data-tone="warning">
              未受管
            </span>
          ) : null}
          {unattached ? <span className="skill-note">未挂载</span> : null}
          {broken ? (
            <span className="skill-note" data-tone="negative">
              断链
            </span>
          ) : null}
          {hasLocalModificationWarning(skill) ? (
            <span
              data-testid="local-modification-badge"
              className="skill-note"
              data-tone={
                skill.localModification === "modified" ? "warning" : undefined
              }
              title={
                skill.localModification === "modified"
                  ? "当前文件与基线指纹不一致；更新或卸载前请确认并备份。"
                  : "首次扫描没有可比较的基线或 lock hash；更新或卸载前请确认。"
              }
            >
              {LOCAL_MODIFICATION_LABELS[skill.localModification]}
            </span>
          ) : null}
          {skill.warnings.length > 0 &&
          !hasLocalModificationWarning(skill) &&
          !broken ? (
            <Badge variant="warning" title={skill.warnings.join("\n")}>
              警告
            </Badge>
          ) : null}
        </div>
        <p className="skill-desc">{skill.description ?? "—"}</p>
      </div>
      <div className="skill-col-source min-w-0">
        <div className="skill-source" title={skill.source ?? undefined}>
          {skill.source ?? "来源未知"}
        </div>
        <div className="skill-updated">{formatUpdatedAt(skill.updatedAt)}</div>
      </div>
      <div className="skill-col-mounts">
        <AgentAvatars skill={skill} />
      </div>
      <div className="skill-status">
        <UpdateStatus skill={skill} />
      </div>
    </button>
  );
}
