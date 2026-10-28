import { AlertTriangle, RefreshCw } from "lucide-react";
import { Button } from "./ui/button";
import type { AppError } from "../lib/types";

interface LockErrorScreenProps {
  error: Partial<AppError> & { message: string };
  onRetry: () => void;
  retrying: boolean;
}

export function LockErrorScreen({
  error,
  onRetry,
  retrying,
}: LockErrorScreenProps) {
  return (
    <section
      role="alert"
      aria-label="Lock 错误"
      className="flex flex-1 items-center justify-center overflow-auto p-8"
    >
      <div className="max-w-xl text-center">
        <div className="mx-auto flex size-14 items-center justify-center rounded-2xl border border-destructive/20 bg-red-50 text-destructive shadow-sm">
          <AlertTriangle className="size-6" aria-hidden="true" />
        </div>
        <h2 className="mt-4 text-lg font-semibold tracking-tight text-destructive">
          Skill lock 无法读取
        </h2>
        <p className="mt-2 text-sm text-foreground">{error.message}</p>
        {error.detail ? (
          <p className="mt-1 break-words font-mono text-[11px] text-muted-foreground">
            {error.code ? `${error.code} · ` : ""}
            {error.detail}
          </p>
        ) : null}
        {error.hint ? (
          <p className="mt-2 text-xs text-muted-foreground">{error.hint}</p>
        ) : null}
        <p className="mx-auto mt-4 max-w-md rounded-lg border border-destructive/20 bg-red-50/60 px-3 py-2 text-xs leading-relaxed text-destructive">
          为避免数据丢失，变更操作已禁用；列表不会以空库存代替，也不会显示任何
          skill。修复 lock 后重新读取。
        </p>
        <div className="mt-4 flex items-center justify-center gap-2">
          <Button variant="outline" size="sm" onClick={onRetry} disabled={retrying}>
            <RefreshCw
              className={retrying ? "size-3.5 animate-spin" : "size-3.5"}
              aria-hidden="true"
            />
            重新检测并读取
          </Button>
        </div>
      </div>
    </section>
  );
}
