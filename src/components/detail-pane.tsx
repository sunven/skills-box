import { MousePointerClick } from "lucide-react";

export function DetailPane() {
  return (
    <aside className="flex min-h-0 flex-col border-l border-border bg-card/40">
      <div className="border-b border-border px-4 py-3">
        <h2 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
          详情
        </h2>
      </div>
      <div className="flex flex-1 items-center justify-center p-6">
        <div className="max-w-[15rem] text-center">
          <div className="mx-auto flex size-11 items-center justify-center rounded-xl border border-border bg-card text-muted-foreground/70 shadow-sm">
            <MousePointerClick className="size-5" aria-hidden="true" />
          </div>
          <h3 className="mt-3 text-sm font-medium text-foreground">未选择 skill</h3>
          <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
            从列表中选择一个 skill 查看来源、挂载与本地改动。
          </p>
        </div>
      </div>
    </aside>
  );
}
