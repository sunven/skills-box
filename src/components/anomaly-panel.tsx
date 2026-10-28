import { useEffect, useRef } from "react";
import { AlertTriangle, X } from "lucide-react";
import { Button } from "./ui/button";
import type { AnomalyGroup, AnomalySummary } from "../lib/types";

interface AnomalyBannerProps {
  anomalies: AnomalySummary;
  onOpen: () => void;
}

export function AnomalyBanner({ anomalies, onOpen }: AnomalyBannerProps) {
  if (anomalies.total === 0) return null;
  return (
    <button
      type="button"
      onClick={onOpen}
      className="mx-1 mt-1 flex items-start gap-2 rounded-lg border border-warning/30 bg-warning-soft px-2.5 py-2 text-left text-[11px] leading-snug text-warning transition-colors hover:border-warning/50"
    >
      <span className="mt-0.5 grid size-4 shrink-0 place-items-center rounded-full border border-warning/40 text-[10px] font-bold">
        !
      </span>
      <span className="min-w-0 flex-1">
        <strong className="block text-xs">异常 {anomalies.total}</strong>
        <span className="text-warning/90">
          {anomalies.stale.count} 条 stale lock · {anomalies.unmanaged.count}{" "}
          个未受管 · {anomalies.broken.count} 个断链
        </span>
      </span>
      <span className="shrink-0 pt-3 text-[10.5px] underline underline-offset-2">
        查看只读提示
      </span>
    </button>
  );
}

interface AnomalyDialogProps {
  anomalies: AnomalySummary;
  open: boolean;
  onClose: () => void;
}

const CATEGORIES: Array<{
  key: keyof Pick<AnomalySummary, "stale" | "unmanaged" | "broken">;
  label: string;
  description: string;
}> = [
  {
    key: "stale",
    label: "失效 lock 条目",
    description: "目录已不存在；不进入主列表，也不会出现在更新候选中。",
  },
  {
    key: "unmanaged",
    label: "未受管",
    description: "磁盘上有目录但没有 lock 条目；v1 只读展示，不提供更新或卸载。",
  },
  {
    key: "broken",
    label: "断链",
    description: "agent 挂载的符号链接目标缺失；只读提示，不自动修复。",
  },
];

function CategoryCard({
  label,
  description,
  group,
}: {
  label: string;
  description: string;
  group: AnomalyGroup;
}) {
  return (
    <section className="rounded-lg border border-border bg-muted/40 p-2.5">
      <div className="flex items-baseline gap-2">
        <strong className="text-lg leading-none text-warning">
          {group.count}
        </strong>
        <span className="text-[11.5px] font-semibold">{label}</span>
      </div>
      <p className="mt-1 text-[10.5px] leading-relaxed text-muted-foreground">
        {description}
      </p>
      {group.examples.length > 0 ? (
        <ul className="mt-1.5 space-y-0.5 text-[10.5px] text-muted-foreground">
          {group.examples.map((example) => (
            <li key={example} className="truncate font-mono">
              {example}
            </li>
          ))}
        </ul>
      ) : (
        <p className="mt-1.5 text-[10.5px] text-muted-foreground/60">—</p>
      )}
    </section>
  );
}

export function AnomalyDialog({ anomalies, open, onClose }: AnomalyDialogProps) {
  const closeRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useEffect(() => {
    if (!open) return;
    const previousFocus = document.activeElement as HTMLElement | null;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onCloseRef.current();
        return;
      }
      if (event.key !== "Tab") return;
      const focusables = panelRef.current?.querySelectorAll<HTMLElement>(
        'button:not([disabled]), [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
      );
      if (!focusables || focusables.length === 0) return;
      const first = focusables[0]!;
      const last = focusables[focusables.length - 1]!;
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", handleKeyDown);
    closeRef.current?.focus();
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
      previousFocus?.focus();
    };
  }, [open]);

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/30 p-6"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onCloseRef.current();
      }}
    >
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="anomaly-title"
        className="w-full max-w-lg rounded-xl border border-border bg-card p-4 shadow-xl"
      >
        <div className="flex items-start justify-between gap-3">
          <div>
            <h2
              id="anomaly-title"
              className="flex items-center gap-2 text-sm font-semibold"
            >
              <AlertTriangle
                className="size-4 text-warning"
                aria-hidden="true"
              />
              异常与只读提示
            </h2>
            <p className="mt-1 text-[11px] text-muted-foreground">
              v1 只展示异常，不提供一键修复。所有操作都不会使用 remove
              --all。
            </p>
          </div>
          <Button
            ref={closeRef}
            variant="ghost"
            size="sm"
            aria-label="关闭异常提示"
            onClick={onClose}
            className="size-7 shrink-0 p-0"
          >
            <X className="size-4" aria-hidden="true" />
          </Button>
        </div>

        <div className="mt-3 grid grid-cols-2 gap-2">
          {CATEGORIES.map((category) => (
            <CategoryCard
              key={category.key}
              label={category.label}
              description={category.description}
              group={anomalies[category.key]}
            />
          ))}
        </div>

        <p className="mt-3 rounded-md border border-border bg-muted/50 px-2.5 py-1.5 text-[10.5px] text-muted-foreground">
          建议：操作前备份 ~/.agents/.skill-lock.json；逐名操作，不使用批量删除。
        </p>
      </div>
    </div>
  );
}
