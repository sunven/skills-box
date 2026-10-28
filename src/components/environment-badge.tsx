import { AlertCircle, CheckCircle2, Loader2 } from "lucide-react";
import { Badge } from "./ui/badge";
import { environmentSummary } from "../lib/environment-display";
import type { EnvReport } from "../lib/types";

interface EnvironmentBadgeProps {
  report?: EnvReport;
  isLoading: boolean;
  isError: boolean;
}

export function EnvironmentBadge({
  report,
  isLoading,
  isError,
}: EnvironmentBadgeProps) {
  if (isLoading) {
    return (
      <Badge variant="muted" className="h-6 px-2.5" data-testid="environment-badge">
        <Loader2 className="size-3 animate-spin" aria-hidden="true" />
        检测中
      </Badge>
    );
  }

  if (isError || !report) {
    return (
      <Badge
        variant="destructive"
        className="h-6 px-2.5"
        data-testid="environment-badge"
      >
        <AlertCircle className="size-3" aria-hidden="true" />
        环境异常
      </Badge>
    );
  }

  const summary = environmentSummary(report);
  return (
    <Badge
      variant={summary.tone}
      className="h-6 px-2.5"
      data-testid="environment-badge"
    >
      {summary.tone === "success" ? (
        <CheckCircle2 className="size-3" aria-hidden="true" />
      ) : (
        <AlertCircle className="size-3" aria-hidden="true" />
      )}
      {summary.label}
    </Badge>
  );
}
