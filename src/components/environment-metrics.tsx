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
import { ActualCliCommands } from "./cli-command-display";

interface EnvironmentMetricsProps {
  report?: EnvReport;
}

type Tone = "success" | "warning" | "destructive" | "muted" | "outline";

function dotTone(tone: Tone): string {
  if (tone === "success") return "success";
  if (tone === "warning") return "warning";
  if (tone === "destructive") return "destructive";
  return "muted";
}

export function EnvironmentMetrics({ report }: EnvironmentMetricsProps) {
  const node = report?.node;
  const npx = report?.npx;
  const git = report?.git;
  const lock = report?.lock;
  const agents = report?.agents ?? [];

  const rows = [
    {
      key: "path",
      tone: "muted" as Tone,
      label: pathSourceLabel(report),
      title: report?.pathSourceDetail ?? undefined,
    },
    {
      key: "node",
      tone: toolTone(node),
      label: nodeLabel(node),
      title: [node?.path, node?.detail].filter(Boolean).join("\n") || undefined,
    },
    {
      key: "npx",
      tone: toolTone(npx),
      label: npxLabel(npx),
      title: [npx?.path, npx?.detail].filter(Boolean).join("\n") || undefined,
    },
    {
      key: "git",
      tone: toolTone(git),
      label: gitLabel(git),
      title: [git?.path, git?.detail].filter(Boolean).join("\n") || undefined,
    },
    {
      key: "cli",
      tone: toolTone(report?.cli),
      label: cliLabel(report),
      title: report?.cli.detail ?? undefined,
    },
    {
      key: "agents",
      tone: "outline" as Tone,
      label: agentsLabel(report),
      title: agents.map((agent) => agent.displayName).join("、") || undefined,
    },
    {
      key: "lock",
      tone: lockTone(lock),
      label: lockLabel(lock),
      title: lock?.path,
    },
  ];

  return (
    <div aria-label="环境状态" className="env-rows">
      {rows.map((row) => (
        <div key={row.key} className="env-row" title={row.title}>
          <span className="env-dot" data-tone={dotTone(row.tone)} />
          <span>{row.label}</span>
        </div>
      ))}
      <details className="env-cli-command">
        <summary>CLI 调用记录</summary>
        <ActualCliCommands origin="environment" />
      </details>
    </div>
  );
}
