import { Loader2 } from "lucide-react";
import { Button } from "../ui/button";
import { TASK_PHASE_LABELS } from "../../lib/task-labels";
import type { TaskPhase } from "../../lib/types";

export interface MutationProgressProps {
  phase: TaskPhase | null;
  message: string;
  completed: number;
  total: number;
  indeterminate?: boolean;
  cancelling: boolean;
  canCancel?: boolean;
  cancelLabel?: string;
  onCancel?: () => void;
  /** Shown instead of "准备中" for native actions without a task phase. */
  label?: string;
}

export function MutationProgress({
  phase,
  message,
  completed,
  total,
  indeterminate = false,
  cancelling,
  canCancel = false,
  cancelLabel,
  onCancel,
  label,
}: MutationProgressProps) {
  const safeTotal = total > 0 ? total : 1;
  const percent = Math.min(100, Math.round((completed / safeTotal) * 100));
  return (
    <div className="mt-3 panel-well" data-testid="task-progress">
      <div className="flex items-center gap-2 text-[12.5px]">
        <Loader2
          className="size-3.5 animate-spin text-muted-foreground"
          aria-hidden="true"
        />
        <span className="font-medium">
          {phase
            ? TASK_PHASE_LABELS[phase]
            : (label ?? TASK_PHASE_LABELS.preparing)}
        </span>
        {indeterminate ? null : (
          <span className="num text-[12px] text-muted-foreground">
            {completed}/{safeTotal}
          </span>
        )}
        {canCancel && onCancel ? (
          <Button
            variant="ghost"
            size="sm"
            className="ml-auto h-7"
            onClick={onCancel}
            disabled={cancelling}
          >
            {cancelLabel ?? "取消"}
          </Button>
        ) : null}
      </div>
      <div className="mt-2 h-1 overflow-hidden rounded-full bg-border">
        <div
          className={
            indeterminate
              ? "h-full w-2/5 animate-pulse rounded-full bg-foreground"
              : "h-full rounded-full bg-foreground transition-[width]"
          }
          style={indeterminate ? undefined : { width: `${percent}%` }}
        />
      </div>
      {message ? (
        <p className="mt-1.5 text-[12px] text-muted-foreground">{message}</p>
      ) : null}
    </div>
  );
}
