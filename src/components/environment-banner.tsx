import { AlertTriangle, Loader2, RotateCcw } from "lucide-react";
import { Button } from "./ui/button";
import { isLockError } from "../lib/api";
import type { AppError, EnvReport } from "../lib/types";

interface EnvironmentBannerProps {
  report?: EnvReport;
  isLoading: boolean;
  error: unknown;
  rechecking: boolean;
  onRecheck: () => void;
}

function errorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string" && error.trim()) return error;
  if (error && typeof error === "object") {
    const candidate = error as { message?: unknown; detail?: unknown };
    if (typeof candidate.message === "string" && candidate.message.trim()) {
      return typeof candidate.detail === "string" && candidate.detail.trim()
        ? `${candidate.message} · ${candidate.detail}`
        : candidate.message;
    }
    try {
      return JSON.stringify(error);
    } catch {
      return "未知错误";
    }
  }
  return "未知错误";
}

export function EnvironmentBanner({
  report,
  isLoading,
  error,
  rechecking,
  onRecheck,
}: EnvironmentBannerProps) {
  if (isLoading && !report && !error) {
    return (
      <div
        role="status"
        className="flex items-center gap-2 border-b border-border bg-background px-6 py-2 text-[12.5px] text-muted-foreground"
      >
        <Loader2 className="size-3.5 animate-spin" aria-hidden="true" />
        正在检测运行环境…
      </div>
    );
  }

  if (error) {
    return (
      <div
        role="alert"
        className="border-b border-destructive/25 bg-destructive-soft px-6 py-3"
      >
        <div className="flex items-start gap-3">
          <AlertTriangle
            className="mt-0.5 size-4 shrink-0 text-destructive"
            aria-hidden="true"
          />
          <div className="min-w-0 flex-1">
            <p className="text-[13px] font-medium text-destructive">
              环境检测失败
            </p>
            <p className="mt-0.5 break-words text-[12.5px] text-destructive/90">
              {errorText(error)}
            </p>
            <p className="mt-1 text-[12.5px] text-destructive/80">
              只读界面仍可使用。请确认后端命令可用，然后重新检测。
            </p>
          </div>
          <Button
            variant="outline"
            size="sm"
            onClick={onRecheck}
            disabled={rechecking}
          >
            {rechecking ? (
              <Loader2 className="size-3.5 animate-spin" aria-hidden="true" />
            ) : (
              <RotateCcw className="size-3.5" aria-hidden="true" />
            )}
            重新检测
          </Button>
        </div>
      </div>
    );
  }

  const issues: AppError[] = report?.issues ?? [];
  if (issues.length === 0) return null;

  const onlyLockIssues = issues.every((issue) => isLockError(issue));
  return (
    <div
      role="alert"
      className="border-b border-destructive/25 bg-destructive-soft px-6 py-3"
    >
      <div className="flex items-start gap-3">
        <AlertTriangle
          className="mt-0.5 size-4 shrink-0 text-destructive"
          aria-hidden="true"
        />
        <div className="min-w-0 flex-1">
          <p className="text-[13px] font-medium text-destructive">
            {onlyLockIssues
              ? "Skill lock 异常，变更入口已禁用"
              : "运行环境不完整，变更入口已禁用"}
          </p>
          <ul className="mt-1 space-y-1">
            {issues.map((issue, index) => (
              <li
                key={`${issue.code ?? issue.message}-${index}`}
                className="text-[12.5px] text-destructive/90"
              >
                <span className="font-medium">{issue.message}</span>
                {issue.detail ? (
                  <span className="break-words text-destructive/80">
                    {" "}
                    · {issue.detail}
                  </span>
                ) : null}
                {issue.hint ? (
                  <span className="block text-destructive/80">{issue.hint}</span>
                ) : null}
              </li>
            ))}
          </ul>
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={onRecheck}
          disabled={rechecking}
        >
          {rechecking ? (
            <Loader2 className="size-3.5 animate-spin" aria-hidden="true" />
          ) : (
            <RotateCcw className="size-3.5" aria-hidden="true" />
          )}
          重新检测
        </Button>
      </div>
    </div>
  );
}
