import type {
  AgentAttachment,
  DetectedAgent,
  EnvReport,
  InventoryAgent,
  InventorySkill,
  InventorySnapshot,
  PreviewSummary,
  SkillDetail,
  SkillDirEntry,
  SkillDirListing,
  SkillFileContent,
  UpdateCheckReport,
  UpdateSkillResult,
  UpdateState,
} from "../lib/types";

const DAY = 86_400_000;
const NOW = new Date("2026-10-28T10:24:00+08:00").getTime();
const daysAgo = (days: number, hours = 0) =>
  new Date(NOW - days * DAY - hours * 3_600_000).toISOString();

const agents: InventoryAgent[] = [
  {
    id: "claude-code",
    displayName: "Claude Code",
    universal: false,
    shared: false,
    globalSkillsDir: "~/.claude/skills",
  },
  {
    id: "codex",
    displayName: "Codex",
    universal: true,
    shared: true,
    globalSkillsDir: "~/.agents/skills",
  },
  {
    id: "gemini-cli",
    displayName: "Gemini CLI",
    universal: true,
    shared: false,
    globalSkillsDir: "~/.gemini/skills",
  },
  {
    id: "qoder",
    displayName: "Qoder",
    universal: false,
    shared: false,
    globalSkillsDir: "~/.qoder/skills",
  },
];

const agentNames: Record<string, string> = {
  "claude-code": "Claude Code",
  codex: "Codex",
  "gemini-cli": "Gemini CLI",
  qoder: "Qoder",
};

function attach(
  agentId: string,
  mode: AgentAttachment["mode"] = "link",
  detachable?: boolean,
): AgentAttachment {
  return {
    agentId,
    agentName: agentNames[agentId]!,
    path: `~/.${agentId}/skills/__SKILL__`,
    mode,
    detachable: detachable ?? (mode === "link" || mode === "copy"),
  };
}

interface Seed {
  name: string;
  description: string;
  source: string;
  update: UpdateState;
  updatedAt: string | null;
  management?: InventorySkill["management"];
  localModification?: InventorySkill["localModification"];
  attachments?: AgentAttachment[];
  warnings?: string[];
}

const seeds: Seed[] = [
  {
    name: "agent-browser",
    description: "Browser automation CLI for AI agents: navigate, fill forms, extract data, screenshot.",
    source: "vercel-labs/agent-browser",
    update: "update",
    updatedAt: daysAgo(0, 2),
    attachments: [
      attach("claude-code", "link"),
      attach("codex", "native"),
      attach("gemini-cli", "copy"),
    ],
  },
  {
    name: "find-skills",
    description: "Discover and install agent skills from public skill registries.",
    source: "vercel-labs/skills",
    update: "current",
    updatedAt: daysAgo(3, 5),
    attachments: [attach("claude-code", "link"), attach("codex", "native")],
  },
  {
    name: "code-review",
    description: "Review changes since a fixed point along standards and spec axes.",
    source: "mattpocock/skills",
    update: "current",
    updatedAt: daysAgo(1, 7),
    attachments: [attach("claude-code", "link"), attach("codex", "native"), attach("qoder", "link")],
  },
  {
    name: "tdd",
    description: "Test-driven development loop: red, green, refactor with integration tests.",
    source: "mattpocock/skills",
    update: "current",
    updatedAt: daysAgo(2, 2),
    attachments: [attach("claude-code", "link"), attach("codex", "native")],
  },
  {
    name: "handoff",
    description: "Write a handoff document so a fresh agent can pick up the work.",
    source: "mattpocock/skills",
    update: "current",
    localModification: "modified",
    updatedAt: daysAgo(4, 6),
    attachments: [attach("claude-code", "link"), attach("qoder", "copy")],
    warnings: ["本地文件与基线指纹不一致"],
  },
  {
    name: "grill-me",
    description: "Stress-test a plan with relentless questions before committing to it.",
    source: "mattpocock/skills",
    update: "current",
    updatedAt: daysAgo(5, 1),
    attachments: [attach("claude-code", "link")],
  },
  {
    name: "diagnosing-bugs",
    description: "Bisect, reproduce and isolate hard bugs and performance regressions.",
    source: "mattpocock/skills",
    update: "update",
    updatedAt: daysAgo(1, 4),
    attachments: [attach("claude-code", "link"), attach("codex", "native")],
  },
  {
    name: "prototype",
    description: "Build a throwaway prototype to answer a design question.",
    source: "mattpocock/skills",
    update: "current",
    updatedAt: daysAgo(6, 2),
    attachments: [attach("claude-code", "link"), attach("gemini-cli", "link")],
  },
  {
    name: "triage",
    description: "Sort an incoming queue of issues into the five canonical states.",
    source: "mattpocock/skills",
    update: "current",
    updatedAt: daysAgo(8, 8),
    attachments: [
      attach("claude-code", "link"),
      attach("gemini-cli", "broken", false),
    ],
    warnings: ["Claude Code 挂载的符号链接目标缺失"],
  },
  {
    name: "domain-modeling",
    description: "Sharpen a project's domain model and record decisions in an ADR.",
    source: "mattpocock/skills",
    update: "current",
    updatedAt: daysAgo(12, 3),
    attachments: [attach("claude-code", "link")],
  },
  {
    name: "codebase-design",
    description: "Design deep modules and find where a seam belongs.",
    source: "mattpocock/skills",
    update: "current",
    updatedAt: daysAgo(12, 5),
    attachments: [attach("claude-code", "link"), attach("qoder", "link")],
  },
  {
    name: "to-prd",
    description: "Turn a conversation into a PRD and publish it to the issue tracker.",
    source: "mattpocock/skills",
    update: "current",
    updatedAt: daysAgo(7, 1),
    attachments: [attach("claude-code", "link")],
  },
  {
    name: "to-issues",
    description: "Slice a plan into independently grabbable vertical-slice issues.",
    source: "mattpocock/skills",
    update: "current",
    updatedAt: daysAgo(7, 2),
    attachments: [attach("claude-code", "link")],
  },
  {
    name: "ubiquitous-language",
    description: "Maintain a glossary so the codebase and the team speak one language.",
    source: "mattpocock/skills",
    update: "current",
    updatedAt: daysAgo(16, 6),
    attachments: [attach("claude-code", "link"), attach("gemini-cli", "copy")],
  },
  {
    name: "write-a-skill",
    description: "Package reusable instructions into a well-formed skill folder.",
    source: "mattpocock/skills",
    update: "current",
    updatedAt: daysAgo(18, 4),
    attachments: [attach("claude-code", "link"), attach("codex", "native")],
  },
  {
    name: "zoom-out",
    description: "Step back from the diff and re-examine the design at a wider zoom.",
    source: "mattpocock/skills",
    update: "current",
    updatedAt: daysAgo(21, 2),
    attachments: [attach("claude-code", "link")],
  },
  {
    name: "obsidian-vault",
    description: "Read and write an Obsidian vault with wikilinks and daily notes.",
    source: "mattpocock/skills",
    update: "unknown",
    management: "copy_only",
    updatedAt: daysAgo(26, 5),
    attachments: [attach("claude-code", "copy")],
  },
  {
    name: "teach",
    description: "Explain a topic at the reader's level, with worked examples.",
    source: "mattpocock/skills",
    update: "unknown",
    management: "unmanaged",
    updatedAt: daysAgo(31, 7),
    attachments: [attach("gemini-cli", "copy")],
  },
  {
    name: "edit-article",
    description: "Tighten an article: cut claims without proof, sharpen the opening.",
    source: "mattpocock/skills",
    update: "unknown",
    management: "unmanaged",
    updatedAt: daysAgo(34, 1),
    attachments: [attach("claude-code", "copy")],
  },
  {
    name: "pdf",
    description: "Extract text, tables and figures from PDF documents.",
    source: "anthropics/skills",
    update: "error",
    updatedAt: daysAgo(5, 9),
    attachments: [attach("claude-code", "link"), attach("codex", "native")],
    warnings: ["检查更新失败：网络超时"],
  },
  {
    name: "xlsx",
    description: "Create and edit spreadsheets with formulas and number formats.",
    source: "anthropics/skills",
    update: "path_missing",
    updatedAt: daysAgo(11, 3),
    attachments: [attach("claude-code", "link")],
  },
  {
    name: "frontend-design",
    description: "Design and build production interfaces with a strong point of view.",
    source: "anthropics/skills",
    update: "update",
    updatedAt: daysAgo(2, 8),
    attachments: [attach("claude-code", "link"), attach("codex", "native")],
  },
  {
    name: "skill-creator",
    description: "Create, edit and evaluate skills with a guided authoring loop.",
    source: "anthropics/skills",
    update: "current",
    updatedAt: daysAgo(19, 2),
    attachments: [attach("claude-code", "link"), attach("qoder", "link")],
  },
  {
    name: "mcp-config",
    description: "Add, update or remove MCP servers in CLI configuration files.",
    source: "anthropics/skills",
    update: "current",
    updatedAt: daysAgo(23, 6),
    attachments: [attach("claude-code", "link")],
  },
  {
    name: "webapp-testing",
    description: "Drive a local web app with Playwright and verify what a user sees.",
    source: "anthropics/skills",
    update: "update",
    updatedAt: daysAgo(4, 4),
    attachments: [attach("claude-code", "link"), attach("codex", "native"), attach("qoder", "link")],
  },
  {
    name: "caveman",
    description: "Write terse, high-signal prose. Drop filler and ceremony.",
    source: "mattpocock/skills",
    update: "update",
    updatedAt: daysAgo(3, 1),
    attachments: [attach("claude-code", "link")],
  },
  {
    name: "request-refactor-plan",
    description: "Draft a safe, staged refactor plan with rollback points.",
    source: "mattpocock/skills",
    update: "current",
    updatedAt: daysAgo(27, 7),
    attachments: [attach("claude-code", "link"), attach("gemini-cli", "link")],
  },
  {
    name: "git-guardrails-claude-code",
    description: "Block destructive git commands before an agent runs them.",
    source: "mattpocock/skills",
    update: "current",
    updatedAt: daysAgo(29, 2),
    attachments: [attach("claude-code", "link")],
  },
  {
    name: "brainstorming",
    description: "Diverge on options before converging on a plan.",
    source: "obra/superpowers",
    update: "current",
    updatedAt: daysAgo(33, 3),
    attachments: [attach("claude-code", "link"), attach("codex", "native")],
  },
  {
    name: "systematic-debugging",
    description: "Form hypotheses, instrument the code, and verify the fix.",
    source: "obra/superpowers",
    update: "current",
    updatedAt: daysAgo(36, 5),
    attachments: [attach("claude-code", "link"), attach("gemini-cli", "copy")],
  },
];

const skills: InventorySkill[] = seeds.map((seed) => ({
  name: seed.name,
  description: seed.description,
  source: seed.source,
  management: seed.management ?? "managed",
  update: seed.update,
  updatedAt: seed.updatedAt,
  canonicalPath:
    seed.management === "copy_only"
      ? null
      : `~/.agents/skills/${seed.name}`,
  attachments: (seed.attachments ?? []).map((entry) => ({
    ...entry,
    path: entry.path.replace("__SKILL__", seed.name),
  })),
  localModification:
    seed.localModification ?? (seed.management === "unmanaged" ? "unknown" : "clean"),
  warnings: seed.warnings ?? [],
}));

export const snapshot: InventorySnapshot = {
  skills,
  agents,
  anomalies: {
    total: 5,
    stale: { count: 1, examples: ["ghost-skill"] },
    unmanaged: { count: 2, examples: ["teach", "edit-article"] },
    broken: { count: 1, examples: ["triage · Gemini CLI"] },
    modified: { count: 1, examples: ["handoff"] },
  },
  warnings: [],
  generatedAtEpochMs: NOW,
};

const detectedAgents: DetectedAgent[] = agents.map((agent) => ({
  id: agent.id,
  displayName: agent.displayName,
  globalSkillsDir: agent.globalSkillsDir ?? null,
  universal: agent.universal,
}));

export const environment: EnvReport = {
  ready: true,
  mutationsEnabled: true,
  pathSource: "nvm",
  pathSourceDetail: "nvm 默认别名 24（v24.15.0）",
  node: {
    found: true,
    path: "/Users/sunven/.nvm/versions/node/v24.15.0/bin/node",
    version: "v24.15.0",
    status: "ok",
    detail: null,
  },
  npx: {
    found: true,
    path: "/Users/sunven/.nvm/versions/node/v24.15.0/bin/npx",
    version: "11.12.1",
    status: "ok",
    detail: null,
  },
  git: {
    found: true,
    path: "/usr/bin/git",
    version: "2.54.0",
    status: "ok",
    detail: null,
  },
  cli: {
    available: true,
    version: "1.7.4",
    pinnedVersion: "1.7.4",
    status: "ok",
    detail: null,
  },
  agents: detectedAgents,
  lock: {
    path: "~/.agents/.skill-lock.json",
    exists: true,
    status: "ok",
    version: 3,
    skillCount: seeds.length,
    error: null,
  },
  issues: [],
  warnings: [],
  checkedAtEpochMs: NOW - 4 * 60_000,
  pinnedCliVersion: "1.7.4",
  interrupted: null,
};

function remoteHash(name: string): string {
  let hash = 0;
  for (const char of name) hash = (hash * 31 + char.charCodeAt(0)) | 0;
  return Math.abs(hash).toString(16).padStart(8, "0").repeat(5).slice(0, 40);
}

const updateResults: UpdateSkillResult[] = seeds
  .filter((seed) => (seed.management ?? "managed") === "managed")
  .map((seed) => {
    const isCandidate = seed.update === "update";
    return {
      name: seed.name,
      source: seed.source,
      ref: "main",
      lockHash: remoteHash(seed.name),
      remoteHash: isCandidate ? remoteHash(seed.name + "-next") : remoteHash(seed.name),
      state: seed.update,
      cached: false,
      lastSuccessAtEpochMs: NOW - 6 * 60_000,
      previousState: isCandidate ? "current" : seed.update,
      error:
        seed.update === "error"
          ? {
              code: "update.network",
              message: "读取仓库超时（30s）",
              detail: "GET https://api.github.com/repos/anthropics/skills/git/trees/main timed out",
              retryable: true,
            }
          : null,
      note:
        seed.update === "path_missing"
          ? "来源中已找不到该 skill（skills/xlsx/SKILL.md），可能已被删除或移动。"
          : null,
    };
  });

export const updateReport: UpdateCheckReport = {
  skills: updateResults,
  sources: [
    {
      source: "anthropics/skills",
      ref: "main",
      status: "ok",
      skillCount: 6,
      cached: false,
      truncated: false,
      lastSuccessAtEpochMs: NOW - 6 * 60_000,
      error: null,
      note: null,
    },
    {
      source: "mattpocock/skills",
      ref: "main",
      status: "ok",
      skillCount: 18,
      cached: false,
      truncated: false,
      lastSuccessAtEpochMs: NOW - 6 * 60_000,
      error: null,
      note: null,
    },
    {
      source: "obra/superpowers",
      ref: null,
      status: "ok",
      skillCount: 2,
      cached: false,
      truncated: false,
      lastSuccessAtEpochMs: NOW - 6 * 60_000,
      error: null,
      note: null,
    },
    {
      source: "vercel-labs/agent-browser",
      ref: "main",
      status: "ok",
      skillCount: 1,
      cached: false,
      truncated: false,
      lastSuccessAtEpochMs: NOW - 6 * 60_000,
      error: null,
      note: null,
    },
    {
      source: "vercel-labs/skills",
      ref: "main",
      status: "cached",
      skillCount: 1,
      cached: true,
      truncated: false,
      lastSuccessAtEpochMs: NOW - 6 * 60_000,
      error: null,
      note: "命中 6 小时缓存",
    },
  ],
  checkedAtEpochMs: NOW - 6 * 60_000,
  lastSuccessAtEpochMs: NOW - 6 * 60_000,
  skippedSourceCount: 0,
  cachedSourceCount: 1,
  warnings: [],
};

export const previewSummary: PreviewSummary = {
  source: "mattpocock/skills",
  parseFailed: false,
  noSkills: false,
  skills: [
    {
      name: "code-review",
      description: "Review changes since a fixed point along standards and spec axes.",
      group: "Engineering",
    },
    {
      name: "tdd",
      description: "Test-driven development loop: red, green, refactor with integration tests.",
      group: "Engineering",
    },
    {
      name: "diagnose",
      description: "Reproduce, bisect and isolate a hard bug before proposing a fix.",
      group: "Engineering",
    },
    {
      name: "prototype",
      description: "Build a throwaway prototype to answer a design question.",
      group: "Engineering",
    },
    {
      name: "grill-me",
      description: "Stress-test a plan with relentless questions before committing.",
      group: "Productivity",
    },
    {
      name: "handoff",
      description: "Write a handoff document so a fresh agent can continue the work.",
      group: "Productivity",
    },
    {
      name: "caveman",
      description: "Write terse, high-signal prose. Drop filler and ceremony.",
      group: "Productivity",
    },
    {
      name: "to-prd",
      description: "Turn a conversation into a PRD and publish it to the issue tracker.",
      group: "Planning",
    },
    {
      name: "to-issues",
      description: "Slice a plan into independently grabbable vertical-slice issues.",
      group: "Planning",
    },
    {
      name: "zoom-out",
      description: "Step back from the diff and re-examine the design at a wider zoom.",
      group: "Planning",
    },
    {
      name: "agent-browser",
      description: "Browser automation CLI for AI agents.",
      group: "Existing",
      conflict: { management: "managed", source: "vercel-labs/agent-browser" },
    },
  ],
};

const skillMd = `---
name: agent-browser
description: Browser automation CLI for AI agents.
---

# agent-browser

A fast browser automation CLI designed for agents. It exposes a small command
surface that maps to real browser actions.

## When to use it

- Navigating a local development server and taking screenshots.
- Filling forms and reading the resulting page state.
- Extracting structured data from a rendered page.

## Commands

\`\`\`bash
agent-browser open http://localhost:5173
agent-browser snapshot --interactive
agent-browser fill "#email" "reader@fieldnotes.dev"
agent-browser click "button[type=submit]"
agent-browser screenshot out.png
\`\`\`

## Notes

The snapshot command returns the accessibility tree, which is usually a better
input for a model than raw HTML. Prefer role-based selectors over CSS paths.

See [the API reference](references/api.md) for the full command surface, or
![the diagram](assets/logo.png).
`;

export function skillDetail(name: string): SkillDetail {
  const skill = skills.find((entry) => entry.name === name) ?? skills[0]!;
  return {
    id: skill.name,
    name: skill.name,
    description: skill.description,
    source: skill.source,
    sourceType: "github",
    ref: "main",
    skillPath: `skills/${skill.name}/SKILL.md`,
    installedAt: daysAgo(40, 2),
    updatedAt: skill.updatedAt,
    canonicalPath: skill.canonicalPath,
    directoryPath:
      skill.management === "copy_only"
        ? null
        : `/Users/sunven/.agents/skills/${skill.name}`,
    directoryMtimeEpochMs: NOW - 14 * DAY,
    hash: remoteHash(skill.name),
    management: skill.management,
    update: skill.update,
    localModification: skill.localModification,
    localModificationDetail:
      skill.localModification === "modified"
        ? "SKILL.md 与基线指纹不一致（+12 −3 行）"
        : null,
    attachments: skill.attachments,
    warnings: skill.warnings,
  };
}

const PNG_DATA_URL =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";

function contentEntry(
  name: string,
  kind: SkillDirEntry["kind"],
  base: string,
  overrides: Partial<SkillDirEntry> = {},
): SkillDirEntry {
  const path = base ? `${base}/${name}` : name;
  const directory = kind === "dir";
  return {
    name,
    path,
    kind,
    symlink: false,
    sizeBytes: directory ? null : 1842,
    markdown: !directory && name.toLowerCase().endsWith(".md"),
    expandable: directory,
    ...overrides,
  };
}

export function listSkillDir(name: string, path = ""): SkillDirListing {
  const entries =
    path === ""
      ? [
          contentEntry("references", "dir", ""),
          contentEntry("scripts", "dir", ""),
          contentEntry("assets", "dir", ""),
          contentEntry("SKILL.md", "file", ""),
          contentEntry(".gitignore", "file", ""),
        ]
      : path === "references"
        ? [
            contentEntry("api.md", "file", "references"),
            contentEntry("notes.txt", "file", "references", {
              sizeBytes: 248,
            }),
          ]
        : path === "scripts"
          ? [contentEntry("run.sh", "file", "scripts", { sizeBytes: 312 })]
          : path === "assets"
            ? [
                contentEntry("logo.png", "file", "assets", {
                  sizeBytes: 1024,
                }),
              ]
            : [];
  return {
    path,
    displayPath: `/Users/sunven/.agents/skills/${name}${path ? `/${path}` : ""}`,
    entries,
    truncated: false,
    maxEntries: 500,
    maxDepth: 12,
  };
}

const apiMarkdown = `# API reference

The CLI exposes a small command surface.

- \`open\` — navigate to a URL
- \`snapshot\` — read the accessibility tree
- \`screenshot\` — save a PNG

[Back to SKILL.md](../SKILL.md)
`;

export function readSkillFile(name: string, path: string): SkillFileContent {
  const base = {
    path,
    displayPath: `/Users/sunven/.agents/skills/${name}/${path}`,
    status: "ok" as const,
    sizeBytes: 1842,
    limitBytes: 1024 * 1024,
    content: null,
    dataUrl: null,
    error: null,
  };
  if (path === "SKILL.md") {
    return {
      ...base,
      kind: "markdown",
      content: skillMd.split("agent-browser").join(name),
    };
  }
  if (path === "references/api.md") {
    return { ...base, kind: "markdown", content: apiMarkdown };
  }
  if (path === "references/notes.txt") {
    return {
      ...base,
      path,
      displayPath: base.displayPath,
      kind: "text",
      content: "Notes\n\n- keep selectors role-based\n- prefer snapshot over HTML\n",
      sizeBytes: 62,
    };
  }
  if (path === "scripts/run.sh") {
    return {
      ...base,
      kind: "text",
      content: "#!/bin/sh\nset -euo pipefail\nagent-browser snapshot --interactive\n",
      sizeBytes: 64,
    };
  }
  if (path === "assets/logo.png") {
    return {
      ...base,
      kind: "image",
      dataUrl: PNG_DATA_URL,
      sizeBytes: 1024,
      limitBytes: 5 * 1024 * 1024,
    };
  }
  if (path === ".gitignore") {
    return { ...base, kind: "text", content: "dist/\nnode_modules/\n" };
  }
  return {
    ...base,
    status: "unreadable",
    kind: "text",
    error: "文件不存在或不可读取。",
    sizeBytes: null,
  };
}
