import type {
  AppError,
  CliStatus,
  DetectedAgent,
  EnvReport,
  ToolStatus,
} from "../lib/types";

const okNode: ToolStatus = {
  found: true,
  path: "/Users/tester/.nvm/versions/node/v24.15.0/bin/node",
  version: "v24.15.0",
  status: "ok",
  detail: null,
};

const okNpx: ToolStatus = {
  found: true,
  path: "/Users/tester/.nvm/versions/node/v24.15.0/bin/npx",
  version: "11.12.1",
  status: "ok",
  detail: null,
};

const okGit: ToolStatus = {
  found: true,
  path: "/usr/bin/git",
  version: "2.54.0",
  status: "ok",
  detail: null,
};

const okCli: CliStatus = {
  available: true,
  version: "1.7.4",
  pinnedVersion: "latest",
  status: "ok",
  detail: null,
};

const detectedAgents: DetectedAgent[] = [
  {
    id: "claude-code",
    displayName: "Claude Code",
    globalSkillsDir: "~/.claude/skills",
    universal: false,
  },
  {
    id: "codex",
    displayName: "Codex",
    globalSkillsDir: "~/.agents/skills",
    universal: true,
  },
];

export const messages = {
  nodeMissing: "未找到 Node.js。Skills Box 需要 Node 22.20 或更高版本。",
  nodeTooOld: "Node 版本过低（当前 v20.11.0），需要 22.20 或更高版本。",
  npxMissing: "未找到 npx。",
  gitMissing: "未找到 git。",
  cliMissing: "skills CLI 不可用（npx skills@latest）。",
};

export function healthyReport(overrides: Partial<EnvReport> = {}): EnvReport {
  return {
    ready: true,
    mutationsEnabled: true,
    pathSource: "nvm",
    pathSourceDetail: "nvm 默认别名 24（v24.15.0）",
    node: okNode,
    npx: okNpx,
    git: okGit,
    cli: okCli,
    agents: detectedAgents,
    lock: {
      path: "~/.agents/.skill-lock.json",
      exists: true,
      status: "ok",
      version: 3,
      skillCount: 61,
      error: null,
    },
    issues: [],
    warnings: [],
    checkedAtEpochMs: 1793163988000,
    pinnedCliVersion: "latest",
    ...overrides,
  };
}

function issue(
  code: string,
  message: string,
  hint?: string,
  retryable = true,
): AppError {
  return hint ? { code, message, hint, retryable } : { code, message, retryable };
}

export function nodeMissingReport(): EnvReport {
  return healthyReport({
    ready: false,
    mutationsEnabled: false,
    pathSource: "none",
    pathSourceDetail: null,
    node: {
      found: false,
      path: null,
      version: null,
      status: "missing",
      detail: "已尝试：用户覆盖、Volta、fnm、nvm、Homebrew、系统路径与登录 shell。",
    },
    npx: {
      found: false,
      path: null,
      version: null,
      status: "not_checked",
      detail: "Node 未就绪，未检测 npx",
    },
    cli: {
      available: false,
      version: null,
      pinnedVersion: "latest",
      status: "not_checked",
      detail: "Node 未就绪，未检测 skills CLI",
    },
    issues: [
      issue(
        "env.node_missing",
        messages.nodeMissing,
        "请安装 Node.js 22.20 或更高版本，然后重新检测。",
      ),
    ],
  });
}

export function nodeTooOldReport(): EnvReport {
  return healthyReport({
    ready: false,
    mutationsEnabled: false,
    pathSource: "nvm",
    pathSourceDetail: "nvm 默认别名 24（v20.11.0）",
    node: {
      found: true,
      path: "~/.nvm/versions/node/v20.11.0/bin/node",
      version: "v20.11.0",
      status: "too_old",
      detail: "~/.nvm/versions/node/v20.11.0/bin/node（v20.11.0）",
    },
    npx: { ...okNpx, found: false, status: "not_checked", detail: null },
    cli: {
      available: false,
      version: null,
      pinnedVersion: "latest",
      status: "not_checked",
      detail: "Node 未就绪，未检测 skills CLI",
    },
    issues: [
      issue(
        "env.node_too_old",
        messages.nodeTooOld,
        "请升级 Node.js 到 22.20 或更高版本，然后重新检测。",
      ),
    ],
  });
}

export function npxMissingReport(): EnvReport {
  return healthyReport({
    ready: false,
    mutationsEnabled: false,
    npx: {
      found: false,
      path: "~/.nvm/versions/node/v24.15.0/bin/npx",
      version: null,
      status: "missing",
      detail: "~/.nvm/versions/node/v24.15.0/bin/npx 不存在",
    },
    cli: {
      available: false,
      version: null,
      pinnedVersion: "latest",
      status: "not_checked",
      detail: "npx 未就绪，未检测 skills CLI",
    },
    issues: [
      issue(
        "env.npx_missing",
        messages.npxMissing,
        "npx 随 Node.js 一起提供，请检查 Node.js 安装是否完整。",
      ),
    ],
  });
}

export function gitMissingReport(): EnvReport {
  return healthyReport({
    ready: false,
    mutationsEnabled: false,
    git: {
      found: false,
      path: null,
      version: null,
      status: "missing",
      detail: "已检查 /opt/homebrew/bin、/usr/local/bin、/usr/bin 与登录 shell PATH。",
    },
    issues: [
      issue(
        "env.git_missing",
        messages.gitMissing,
        "请安装 Xcode Command Line Tools（xcode-select --install）或 git。",
      ),
    ],
  });
}

export function cliMissingReport(): EnvReport {
  return healthyReport({
    ready: false,
    mutationsEnabled: false,
    cli: {
      available: false,
      version: null,
      pinnedVersion: "latest",
      status: "error",
      detail: "network down",
    },
    issues: [
      issue(
        "env.cli_missing",
        messages.cliMissing,
        "首次运行需要联网下载 skills@latest，请检查网络后重新检测。",
      ),
    ],
  });
}
