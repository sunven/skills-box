import { PackageOpen, Search } from "lucide-react";
import { Input } from "./ui/input";

export function SkillListPane() {
  return (
    <section className="flex min-h-0 min-w-0 flex-col bg-background">
      <div className="flex items-center gap-2 border-b border-border bg-card px-4 py-2.5">
        <div className="relative min-w-0 flex-1">
          <Search
            className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground"
            aria-hidden="true"
          />
          <Input
            disabled
            aria-label="搜索 skill、来源或描述"
            placeholder="搜索 skill、来源或描述"
            className="h-8 pl-8 text-xs"
          />
        </div>
        <select
          disabled
          aria-label="排序"
          className="h-8 rounded-md border border-border-strong bg-card px-2 text-xs text-muted-foreground shadow-sm disabled:opacity-60"
          defaultValue="recent"
        >
          <option value="recent">最近更新</option>
          <option value="name">名称</option>
          <option value="source">来源</option>
        </select>
        <span className="w-12 shrink-0 text-right text-[11px] tabular-nums text-muted-foreground">
          —
        </span>
      </div>

      <div className="flex flex-1 items-center justify-center p-8">
        <div className="max-w-xs text-center">
          <div className="mx-auto flex size-12 items-center justify-center rounded-xl border border-border bg-card text-muted-foreground/70 shadow-sm">
            <PackageOpen className="size-5" aria-hidden="true" />
          </div>
          <h2 className="mt-3 text-sm font-medium text-foreground">暂无 skill</h2>
          <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
            安装或读取全局 skill 后，会显示在这里。
          </p>
        </div>
      </div>
    </section>
  );
}
