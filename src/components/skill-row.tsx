import { Badge } from "./ui/badge";
import { cn } from "../lib/utils";
import { agentColor, agentInitials } from "../lib/agent-style";
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
  updateBadgeVariant,
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
    <div className="flex items-center pl-1" data-testid="agent-stack">
      {shown.map((attachment) => {
        const broken = attachment.mode === "broken";
        const label = ATTACHMENT_LABELS[attachment.mode];
        return (
          <span
            key={attachment.agentId}
            data-testid="agent-avatar"
            title={`${attachment.agentName} · ${label}${broken ? "（断链）" : ""}`}
            aria-label={`${attachment.agentName}（${label}${broken ? "，断链" : ""}）`}
            className={cn(
              "grid size-[18px] shrink-0 place-items-center rounded-full border-2 border-card text-[7.5px] font-extrabold tracking-tight text-white",
              "-ml-[5px] first:ml-0",
              broken && "ring-2 ring-destructive/60",
            )}
            style={{ backgroundColor: agentColor(attachment.agentId) }}
          >
            {agentInitials(attachment.agentName)}
          </span>
        );
      })}
      {remaining > 0 ? (
        <span
          data-testid="agent-more"
          aria-label={`另有 ${remaining} 个 agent`}
          className="ml-1 text-[9.5px] text-muted-foreground"
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

export function SkillRow({ skill, selected, onSelect }: SkillRowProps) {
  const broken = hasBrokenAttachment(skill);
  const unmounted =
    skill.attachments.length === 0 && skill.management !== "unmanaged";

  return (
    <button
      type="button"
      data-testid="skill-row"
      data-skill-name={skill.name}
      aria-pressed={selected}
      onClick={() => onSelect(skill.name)}
      className={cn(
        "grid w-full grid-cols-[minmax(0,1fr)_auto] items-start gap-x-3 border-b border-border/70 px-3 py-2.5 text-left transition-colors",
        selected
          ? "bg-primary/10 shadow-[inset_3px_0_0_var(--primary)]"
          : "hover:bg-muted/60",
      )}
    >
      <div className="min-w-0">
        <div className="flex min-w-0 flex-wrap items-center gap-1.5">
          <span className="truncate text-[12.5px] font-semibold">
            {skill.name}
          </span>
          <Badge
            data-testid="update-badge"
            variant={updateBadgeVariant(skill.update)}
            title={updateBadgeTitle(skill)}
          >
            {UPDATE_LABELS[skill.update]}
          </Badge>
          {skill.management === "copy_only" ? (
            <Badge variant="outline">仅副本</Badge>
          ) : null}
          {skill.management === "unmanaged" ? (
            <Badge variant="warning">未受管</Badge>
          ) : null}
          {unmounted ? <Badge variant="muted">未挂载</Badge> : null}
          {broken ? <Badge variant="destructive">断链</Badge> : null}
          {hasLocalModificationWarning(skill) ? (
            <Badge
              data-testid="local-modification-badge"
              variant={skill.localModification === "modified" ? "warning" : "muted"}
              title={
                skill.localModification === "modified"
                  ? "当前文件与基线指纹不一致；更新或卸载前请确认并备份。"
                  : "首次扫描没有可比较的基线或 lock hash；更新或卸载前请确认。"
              }
            >
              {LOCAL_MODIFICATION_LABELS[skill.localModification]}
            </Badge>
          ) : null}
          {skill.warnings.length > 0 ? (
            <span title={skill.warnings.join("\n")}>
              <Badge variant="warning">警告</Badge>
            </span>
          ) : null}
        </div>
        <p className="mt-0.5 truncate text-[11.5px] text-muted-foreground">
          {skill.description ?? "—"}
        </p>
        <div className="mt-0.5 flex min-w-0 items-center gap-1.5 text-[10.5px] text-muted-foreground/80">
          <span className="max-w-[45%] truncate" title={skill.source ?? undefined}>
            {skill.source ?? "来源未知"}
          </span>
          <span aria-hidden="true">·</span>
          <span>{formatUpdatedAt(skill.updatedAt)}</span>
        </div>
      </div>
      <AgentAvatars skill={skill} />
    </button>
  );
}
