import { SelectionCheckbox } from "./selection-checkbox";
import { memo } from "react";
import {
  ArrowUpCircle,
  FolderOpen,
  Loader2,
  MoreHorizontal,
} from "lucide-react";
import { Badge } from "./ui/badge";
import { agentInitials } from "../lib/agent-style";
import { agentLogoUrl } from "../lib/agent-logos";
import type { InventorySkill } from "../lib/types";
import {
  ATTACHMENT_LABELS,
  LOCAL_MODIFICATION_LABELS,
  formatUpdatedAt,
  hasBrokenAttachment,
  hasLocalModificationWarning,
  uniqueAttachments,
} from "../lib/inventory-view";
import type { UpdateViewSkill } from "../lib/types";
import { UpdateStatus } from "./update-status";

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
  checked: boolean;
  selectionDisabled: boolean;
  onToggle: (id: string) => void;
  onSelect: (name: string) => void;
  actionsDisabled: boolean;
  openingFolder: boolean;
  checking: boolean;
  menuOpen: boolean;
  onOpenFolder: (skill: UpdateViewSkill) => void;
  onOpenMenu: (skill: UpdateViewSkill, anchor: HTMLButtonElement) => void;
  onUpdate: (name: string) => void;
}

export const SkillRow = memo(function SkillRow({
  skill,
  selected,
  checked,
  selectionDisabled,
  onToggle,
  onSelect,
  actionsDisabled,
  openingFolder,
  checking,
  menuOpen,
  onOpenFolder,
  onOpenMenu,
  onUpdate,
}: SkillRowProps) {
  const broken = hasBrokenAttachment(skill);
  const unattached =
    skill.attachments.length === 0 && skill.management !== "unmanaged";

  return (
    <div
      className="skill-row skill-grid"
      data-selected={selected || undefined}
      data-checked={checked || undefined}
    >
      <SelectionCheckbox
        checked={checked}
        disabled={selectionDisabled}
        label={`选择技能 ${skill.name}`}
        onChange={() => onToggle(skill.id)}
      />
      <button
        type="button"
        data-testid="skill-row"
        data-skill-name={skill.name}
        aria-pressed={selected}
        onClick={() => onSelect(skill.name)}
        className="skill-row-select"
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
          <div className="skill-updated">
            {formatUpdatedAt(skill.updatedAt)}
          </div>
        </div>
        <div className="skill-col-mounts">
          <AgentAvatars skill={skill} />
        </div>
        <div className="skill-status">
          <UpdateStatus skill={skill} testId="update-badge" />
        </div>
      </button>
      <div
        className="skill-row-actions"
        aria-label={`${skill.name} 的常用操作`}
      >
        <button
          type="button"
          className="skill-row-action"
          aria-label={`打开目录 ${skill.name}`}
          title={
            openingFolder
              ? "正在打开目录…"
              : !skill.canonicalPath && skill.attachments.length === 0
                ? "技能目录不可用"
                : "打开目录"
          }
          disabled={
            openingFolder ||
            (!skill.canonicalPath && skill.attachments.length === 0)
          }
          aria-busy={openingFolder}
          onClick={() => onOpenFolder(skill)}
        >
          {openingFolder ? (
            <Loader2 className="animate-spin" aria-hidden="true" />
          ) : (
            <FolderOpen aria-hidden="true" />
          )}
        </button>
        {skill.management === "managed" && skill.update === "update" ? (
          <button
            type="button"
            className="skill-row-action"
            data-tone="accent"
            aria-label={`更新 ${skill.name}`}
            title={actionsDisabled ? "当前无法更新" : "更新"}
            disabled={actionsDisabled}
            onClick={() => onUpdate(skill.name)}
          >
            <ArrowUpCircle aria-hidden="true" />
          </button>
        ) : null}
        <button
          type="button"
          className="skill-row-action"
          aria-label={`更多操作 ${skill.name}`}
          title={checking ? "正在检查更新…" : "更多操作"}
          aria-haspopup="menu"
          aria-expanded={menuOpen}
          aria-busy={checking}
          aria-controls={menuOpen ? "skill-row-menu" : undefined}
          onClick={(event) => onOpenMenu(skill, event.currentTarget)}
          onKeyDown={(event) => {
            if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
            event.preventDefault();
            onOpenMenu(skill, event.currentTarget);
          }}
        >
          {checking ? (
            <Loader2 className="animate-spin" aria-hidden="true" />
          ) : (
            <MoreHorizontal aria-hidden="true" />
          )}
        </button>
      </div>
    </div>
  );
});
