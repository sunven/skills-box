import { MousePointerClick } from "lucide-react";
import { Badge } from "./ui/badge";
import type { InventorySkill } from "../lib/types";
import { UPDATE_LABELS, formatUpdatedAt } from "../lib/inventory-view";

interface DetailPaneProps {
  skill?: InventorySkill | null;
}

export function DetailPane({ skill }: DetailPaneProps) {
  return (
    <aside
      aria-label="详情"
      className="flex min-h-0 flex-col border-l border-border bg-card/40"
    >
      <div className="border-b border-border px-4 py-3">
        <h2 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
          详情
        </h2>
      </div>
      {skill ? (
        <div className="flex-1 overflow-auto p-4" data-testid="detail-content">
          <div className="flex flex-wrap items-center gap-1.5">
            <h3 className="text-lg font-semibold tracking-tight">
              {skill.name}
            </h3>
            <Badge variant="muted">{UPDATE_LABELS[skill.update]}</Badge>
            {skill.management === "copy_only" ? (
              <Badge variant="outline">仅副本</Badge>
            ) : null}
            {skill.management === "unmanaged" ? (
              <Badge variant="warning">未受管</Badge>
            ) : null}
          </div>
          <p className="mt-1.5 text-xs leading-relaxed text-muted-foreground">
            {skill.description ?? "—"}
          </p>
          <dl className="mt-4 grid grid-cols-[76px_minmax(0,1fr)] gap-x-3 gap-y-1.5 text-[11.5px]">
            <dt className="text-muted-foreground/70">来源</dt>
            <dd className="break-words">{skill.source ?? "来源未知"}</dd>
            <dt className="text-muted-foreground/70">更新时间</dt>
            <dd>{formatUpdatedAt(skill.updatedAt)}</dd>
            <dt className="text-muted-foreground/70">管理状态</dt>
            <dd>
              {skill.management === "managed"
                ? "受管"
                : skill.management === "copy_only"
                  ? "仅副本"
                  : "未受管"}
            </dd>
          </dl>
          <p className="mt-5 rounded-md border border-border bg-muted/50 px-3 py-2 text-[11px] leading-relaxed text-muted-foreground">
            详情面板将在后续票据中完善：挂载列表、SKILL.md、本地改动与动作入口。
          </p>
        </div>
      ) : (
        <div className="flex flex-1 items-center justify-center p-6">
          <div className="max-w-[15rem] text-center">
            <div className="mx-auto flex size-11 items-center justify-center rounded-xl border border-border bg-card text-muted-foreground/70 shadow-sm">
              <MousePointerClick className="size-5" aria-hidden="true" />
            </div>
            <h3 className="mt-3 text-sm font-medium text-foreground">
              未选择 skill
            </h3>
            <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
              从列表中选择一个 skill 查看来源、挂载与本地改动。
            </p>
          </div>
        </div>
      )}
    </aside>
  );
}
