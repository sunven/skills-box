import { Badge } from "./ui/badge";
import {
  agentsLabel,
  cliLabel,
  gitLabel,
  lockLabel,
  lockTone,
  nodeLabel,
  npxLabel,
  pathSourceLabel,
  toolTone,
} from "../lib/environment-display";
import type { EnvReport } from "../lib/types";

interface EnvironmentMetricsProps {
  report?: EnvReport;
}

export function EnvironmentMetrics({ report }: EnvironmentMetricsProps) {
  const node = report?.node;
  const npx = report?.npx;
  const git = report?.git;
  const lock = report?.lock;
  const agents = report?.agents ?? [];

  const chipTitle = (parts: Array<string | null | undefined>) =>
    parts.filter(Boolean).join("\n") || undefined;

  return (
    <div
      aria-label="环境状态"
      className="flex flex-wrap items-center gap-1.5 border-b border-border bg-card/70 px-5 py-2"
    >
      <span className="mr-1 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
        环境
      </span>
      <Badge
        variant="outline"
        title={chipTitle([report?.pathSourceDetail])}
        className="font-normal"
      >
        {pathSourceLabel(report)}
      </Badge>
      <Badge
        variant={toolTone(node)}
        title={chipTitle([node?.path, node?.detail])}
        className="font-normal"
      >
        {nodeLabel(node)}
      </Badge>
      <Badge
        variant={toolTone(npx)}
        title={chipTitle([npx?.path, npx?.detail])}
        className="font-normal"
      >
        {npxLabel(npx)}
      </Badge>
      <Badge
        variant={toolTone(git)}
        title={chipTitle([git?.path, git?.detail])}
        className="font-normal"
      >
        {gitLabel(git)}
      </Badge>
      <Badge
        variant={toolTone(report?.cli)}
        title={chipTitle([report?.cli.detail])}
        className="font-normal"
      >
        {cliLabel(report)}
      </Badge>
      <Badge
        variant="outline"
        title={agents.map((agent) => agent.displayName).join("、") || undefined}
        className="font-normal"
      >
        {agentsLabel(report)}
      </Badge>
      <Badge
        variant={lockTone(lock)}
        title={lock?.path}
        className="font-normal"
      >
        {lockLabel(lock)}
      </Badge>
    </div>
  );
}
