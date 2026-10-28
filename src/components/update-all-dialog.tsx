import { useEffect, useRef } from "react";
import { ArrowUpCircle, X } from "lucide-react";
import { Button } from "./ui/button";
import type { UpdateSkipItem, UpdateSkipReason, UpdateViewSkill } from "../lib/update-view";
import { computeUpdateSummary } from "../lib/update-view";
import { shortHash } from "../lib/inventory-view";

interface UpdateAllDialogProps {
  open: boolean;
  skills: UpdateViewSkill[];
  onClose: () => void;
}

const REASON_ORDER: UpdateSkipReason[] = [
  "local_modified",
  "local_unknown",
  "unknown",
  "not_checked",
  "path_missing",
  "error",
  "copy_only",
];

export function UpdateAllDialog({
  open,
  skills,
  onClose,
}: UpdateAllDialogProps) {
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

  const summary = computeUpdateSummary(skills);
  const grouped = new Map<UpdateSkipReason, UpdateSkipItem[]>();
  for (const item of summary.skipped) {
    const list = grouped.get(item.reason) ?? [];
    list.push(item);
    grouped.set(item.reason, list);
  }

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
        aria-labelledby="update-all-title"
        className="flex max-h-[85vh] w-full max-w-2xl flex-col rounded-xl border border-border bg-card p-4 shadow-xl"
      >
        <div className="flex items-start justify-between gap-3">
          <div>
            <h2
              id="update-all-title"
              className="flex items-center gap-2 text-sm font-semibold"
            >
              <ArrowUpCircle className="size-4 text-primary" aria-hidden="true" />
              更新全部
            </h2>
            <p className="mt-1 text-[11px] text-muted-foreground">
              只读汇总，不会执行任何变更；更新执行将在后续接入。
            </p>
          </div>
          <Button
            ref={closeRef}
            variant="ghost"
            size="sm"
            aria-label="关闭更新全部"
            onClick={onClose}
            className="size-7 shrink-0 p-0"
          >
            <X className="size-4" aria-hidden="true" />
          </Button>
        </div>

        <div className="mt-3 grid min-h-0 flex-1 grid-cols-2 gap-2 overflow-auto">
          <section
            data-testid="update-candidates"
            className="rounded-lg border border-border bg-muted/40 p-2.5"
          >
            <div className="flex items-baseline gap-2">
              <strong className="text-lg leading-none text-success">
                {summary.candidates.length}
              </strong>
              <span className="text-[11.5px] font-semibold">更新候选</span>
            </div>
            <p className="mt-1 text-[10.5px] leading-relaxed text-muted-foreground">
              仅包含受管、本地干净且来源可更新的 skill。
            </p>
            {summary.candidates.length === 0 ? (
              <p className="mt-2 text-[10.5px] text-muted-foreground/70">
                没有可直接更新的 skill。
              </p>
            ) : (
              <ul className="mt-2 space-y-1 text-[10.5px]">
                {summary.candidates.map((skill) => (
                  <li key={skill.name} className="rounded-md bg-card px-2 py-1">
                    <span className="font-medium">{skill.name}</span>
                    <span className="ml-1 text-muted-foreground">
                      {skill.source ?? "来源未知"}
                    </span>
                    {skill.updateResult?.remoteHash ? (
                      <span
                        className="ml-1 font-mono text-[9.5px] text-muted-foreground/70"
                        title={skill.updateResult.remoteHash}
                      >
                        {shortHash(skill.updateResult.remoteHash, 8)}
                      </span>
                    ) : null}
                  </li>
                ))}
              </ul>
            )}
          </section>

          <section
            data-testid="update-skipped"
            className="rounded-lg border border-border bg-muted/40 p-2.5"
          >
            <div className="flex items-baseline gap-2">
              <strong className="text-lg leading-none text-warning">
                {summary.skipped.length}
              </strong>
              <span className="text-[11.5px] font-semibold">跳过项</span>
            </div>
            <p className="mt-1 text-[10.5px] leading-relaxed text-muted-foreground">
              本地改动、未知、源路径缺失、检查失败与仅副本不会进入更新执行。
            </p>
            {summary.skipped.length === 0 ? (
              <p className="mt-2 text-[10.5px] text-muted-foreground/70">
                没有被跳过的 skill。
              </p>
            ) : (
              <div className="mt-2 space-y-1.5">
                {REASON_ORDER.filter((reason) => grouped.has(reason)).map(
                  (reason) => {
                    const items = grouped.get(reason)!;
                    return (
                      <div key={reason}>
                        <p className="text-[10px] font-semibold text-warning">
                          {items[0]!.label} · {items.length}
                        </p>
                        <ul className="mt-0.5 space-y-0.5 text-[10.5px] text-muted-foreground">
                          {items.map((item) => (
                            <li key={item.name} title={item.detail}>
                              <span className="font-medium text-foreground">
                                {item.name}
                              </span>
                              <span className="ml-1">{item.detail}</span>
                            </li>
                          ))}
                        </ul>
                      </div>
                    );
                  },
                )}
              </div>
            )}
          </section>
        </div>

        <p className="mt-3 rounded-md border border-border bg-muted/50 px-2.5 py-1.5 text-[10.5px] text-muted-foreground">
          更新执行（ticket 07）会逐项跳过上述项目；本页不会调用 skills update 或 skills check。
        </p>
      </div>
    </div>
  );
}
