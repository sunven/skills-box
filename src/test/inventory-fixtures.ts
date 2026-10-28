import type {
  AgentAttachment,
  AnomalyGroup,
  AnomalySummary,
  AttachmentMode,
  InventoryAgent,
  InventorySkill,
  InventorySnapshot,
} from "../lib/types";

export const claudeAgent: InventoryAgent = {
  id: "claude-code",
  displayName: "Claude Code",
  universal: false,
  shared: false,
  globalSkillsDir: "~/.claude/skills",
};

export const geminiAgent: InventoryAgent = {
  id: "gemini-cli",
  displayName: "Gemini CLI",
  universal: true,
  shared: false,
  globalSkillsDir: "~/.gemini/skills",
};

export const codexAgent: InventoryAgent = {
  id: "codex",
  displayName: "Codex",
  universal: true,
  shared: true,
  globalSkillsDir: "~/.agents/skills",
};

export const defaultAgents: InventoryAgent[] = [
  claudeAgent,
  geminiAgent,
  codexAgent,
];

const agentNames: Record<string, string> = {
  "claude-code": "Claude Code",
  "gemini-cli": "Gemini CLI",
  codex: "Codex",
};

export function attachment(
  agentId: string,
  mode: AttachmentMode = "link",
  path?: string,
): AgentAttachment {
  return {
    agentId,
    agentName: agentNames[agentId] ?? agentId,
    path: path ?? `~/.${agentId}/skills/alpha`,
    mode,
  };
}

export function makeSkill(
  overrides: Partial<InventorySkill> & { name: string },
): InventorySkill {
  return {
    description: null,
    source: null,
    management: "managed",
    update: "unknown",
    updatedAt: null,
    canonicalPath: "~/.agents/skills/alpha",
    attachments: [],
    warnings: [],
    ...overrides,
  };
}

export function group(
  count: number,
  examples: string[] = [],
): AnomalyGroup {
  return { count, examples };
}

export function makeAnomalies(
  overrides: Partial<AnomalySummary> = {},
): AnomalySummary {
  return {
    total: 0,
    stale: group(0),
    unmanaged: group(0),
    broken: group(0),
    ...overrides,
  };
}

export function makeSnapshot(
  overrides: Partial<InventorySnapshot> = {},
): InventorySnapshot {
  return {
    skills: [],
    agents: defaultAgents,
    anomalies: makeAnomalies(),
    warnings: [],
    generatedAtEpochMs: 1793163988000,
    ...overrides,
  };
}

export function emptySnapshot(): InventorySnapshot {
  return makeSnapshot();
}

export function sampleSkills(): InventorySkill[] {
  return [
    makeSkill({
      name: "alpha",
      description: "Alpha 的单行描述",
      source: "acme/skills",
      updatedAt: "2026-10-01T08:00:00.000Z",
      attachments: [attachment("claude-code"), attachment("codex", "native")],
    }),
    makeSkill({
      name: "beta",
      description: "Beta 的单行描述",
      source: "acme/skills",
      updatedAt: "2026-10-05T08:00:00.000Z",
      attachments: [
        attachment("gemini-cli"),
        attachment("claude-code", "copy"),
      ],
    }),
    makeSkill({
      name: "gamma",
      description: null,
      source: null,
      management: "unmanaged",
      updatedAt: null,
      canonicalPath: null,
      attachments: [attachment("claude-code", "copy")],
    }),
    makeSkill({
      name: "delta",
      description: "仅存在于 agent 的副本",
      source: "other/repo",
      management: "copy_only",
      updatedAt: "2026-09-01T08:00:00.000Z",
      canonicalPath: null,
      attachments: [attachment("claude-code", "copy")],
    }),
  ];
}

export function sampleSnapshot(): InventorySnapshot {
  return makeSnapshot({
    skills: sampleSkills(),
    anomalies: makeAnomalies({
      total: 3,
      stale: group(1, ["ghost"]),
      unmanaged: group(1, ["gamma"]),
      broken: group(1, ["delta · Claude Code"]),
    }),
  });
}
