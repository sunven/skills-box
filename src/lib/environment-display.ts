import type { EnvReport, LockHealth, PathSource, ToolState, ToolStatus } from "./types";

export const pathSourceLabels: Record<PathSource, string> = {
  override: "用户指定",
  volta: "Volta",
  fnm: "fnm",
  nvm: "nvm",
  homebrew: "Homebrew",
  system: "系统路径",
  login_shell: "登录 shell",
  none: "未解析",
};

export type ChipTone = "success" | "warning" | "destructive" | "muted" | "outline";

export function pathSourceLabel(report: EnvReport | undefined): string {
  if (!report) return "—";
  return `PATH · ${pathSourceLabels[report.pathSource]}`;
}

export function nodeLabel(node: ToolStatus | undefined): string {
  if (!node) return "Node —";
  switch (node.status) {
    case "ok":
      return `Node ${node.version ?? "—"}`;
    case "too_old":
      return `Node ${node.version ?? ""}（版本过低）`;
    case "missing":
      return "Node 未找到";
    case "error":
      return "Node 探测失败";
    case "not_checked":
      return "Node 未检测";
  }
}

export function npxLabel(npx: ToolStatus | undefined): string {
  if (!npx) return "npx —";
  switch (npx.status) {
    case "ok":
      return `npx ${npx.version ?? ""}`;
    case "missing":
      return "npx 未找到";
    case "too_old":
      return "npx 版本过低";
    case "error":
      return "npx 不可用";
    case "not_checked":
      return "npx 未检测";
  }
}

export function gitLabel(git: ToolStatus | undefined): string {
  if (!git) return "git —";
  switch (git.status) {
    case "ok":
      return `git ${git.version ?? ""}`;
    case "missing":
      return "git 未找到";
    case "too_old":
      return "git 版本过低";
    case "error":
      return "git 不可用";
    case "not_checked":
      return "git 未检测";
  }
}

export function cliLabel(report: EnvReport | undefined): string {
  if (!report) return "skills —";
  if (report.cli.status === "ok" && report.cli.version) {
    return `skills ${report.cli.version}`;
  }
  if (report.cli.status === "not_checked") return "skills 未检测";
  return "skills CLI 不可用";
}

export function lockLabel(lock: LockHealth | undefined): string {
  if (!lock) return "Lock —";
  switch (lock.status) {
    case "ok":
      return `Lock v${lock.version ?? 3} · ${lock.skillCount ?? 0} 条`;
    case "missing":
      return "Lock 缺失";
    case "corrupt":
      return "Lock 损坏";
    case "unsupported":
      return `Lock v${lock.version ?? "?"}（不支持）`;
    case "unreadable":
      return "Lock 不可读";
  }
}

export function agentsLabel(report: EnvReport | undefined): string {
  if (!report) return "Agents —";
  if (report.agents.length === 0) return "未检测到 agent";
  return `Agents ${report.agents.length}`;
}

export function toolTone(tool: { status: ToolState } | undefined): ChipTone {
  if (!tool) return "muted";
  switch (tool.status) {
    case "ok":
      return "success";
    case "missing":
    case "too_old":
    case "error":
      return "destructive";
    case "not_checked":
      return "muted";
  }
}

export function lockTone(lock: LockHealth | undefined): ChipTone {
  if (!lock) return "muted";
  switch (lock.status) {
    case "ok":
      return "success";
    case "missing":
      return "muted";
    case "corrupt":
    case "unsupported":
    case "unreadable":
      return "destructive";
  }
}

export function environmentSummary(report: EnvReport | undefined): {
  label: string;
  tone: "success" | "warning" | "destructive" | "muted";
} {
  if (!report) return { label: "环境异常", tone: "destructive" };
  if (report.mutationsEnabled) return { label: "环境正常", tone: "success" };
  const onlyLockIssues =
    report.issues.length > 0 && report.issues.every((issue) => issue.code.startsWith("lock."));
  return onlyLockIssues
    ? { label: "环境受限", tone: "warning" }
    : { label: "环境异常", tone: "destructive" };
}
