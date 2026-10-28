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
      <span className="env-badge" data-testid="environment-badge">
        <span className="env-dot" data-tone="muted" />
        检测中
      </span>
    );
  }

  if (isError || !report) {
    return (
      <span className="env-badge" data-testid="environment-badge">
        <span className="env-dot" data-tone="destructive" />
        环境异常
      </span>
    );
  }

  const summary = environmentSummary(report);
  return (
    <span className="env-badge" data-testid="environment-badge">
      <span className="env-dot" data-tone={summary.tone} />
      {summary.label}
    </span>
  );
}
