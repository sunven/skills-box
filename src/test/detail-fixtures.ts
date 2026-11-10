import type {
  InventorySkill,
  SkillDetail,
  SkillDirEntry,
  SkillDirListing,
  SkillFileContent,
} from "../lib/types";

export function makeDetail(
  overrides: Partial<SkillDetail> & { name: string },
): SkillDetail {
  const { name, ...rest } = overrides;
  const directory = rest.directoryPath ?? `~/.agents/skills/${name}`;
  return {
    id: name,
    name,
    description: null,
    source: null,
    sourceType: null,
    ref: null,
    skillPath: null,
    installedAt: null,
    updatedAt: null,
    canonicalPath: `~/.agents/skills/${name}`,
    directoryPath: directory,
    directoryMtimeEpochMs: 1793163988000,
    hash: null,
    management: "managed",
    localModification: "clean",
    localModificationDetail: null,
    attachments: [],
    warnings: [],
    ...rest,
  };
}

export function detailForSkill(skill: InventorySkill): SkillDetail {
  const directory =
    skill.canonicalPath ?? skill.attachments[0]?.path ?? null;
  return makeDetail({
    name: skill.name,
    description: skill.description,
    source: skill.source,
    sourceType: skill.sourceType ?? "github",
    ref: skill.sourceVersion ?? null,
    management: skill.management,
    updatedAt: skill.updatedAt,
    canonicalPath: skill.canonicalPath,
    directoryPath: directory,
    localModification: skill.localModification,
    attachments: skill.attachments,
    warnings: skill.warnings,
  });
}

export const IMAGE_DATA_URL =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";

function entry(
  name: string,
  kind: SkillDirEntry["kind"],
  overrides: Partial<SkillDirEntry> = {},
): SkillDirEntry {
  const directory = kind === "dir";
  return {
    name,
    path: overrides.path ?? name,
    kind,
    symlink: false,
    sizeBytes: directory ? null : 80,
    markdown: !directory && name.toLowerCase().endsWith(".md"),
    expandable: directory,
    ...overrides,
  };
}

const rootEntries: SkillDirEntry[] = [
  entry("assets", "dir"),
  entry("references", "dir"),
  entry("scripts", "dir"),
  entry("SKILL.md", "file"),
];

const referenceEntries: SkillDirEntry[] = [
  entry("api.md", "file", { path: "references/api.md" }),
  entry("notes.txt", "file", { path: "references/notes.txt", sizeBytes: 24 }),
];

const scriptEntries: SkillDirEntry[] = [
  entry("run.sh", "file", { path: "scripts/run.sh", sizeBytes: 18 }),
];

const assetEntries: SkillDirEntry[] = [
  entry("logo.png", "file", { path: "assets/logo.png", sizeBytes: 1024 }),
];

export function makeDirListing(
  skillId: string,
  path = "",
  overrides: Partial<SkillDirListing> = {},
): SkillDirListing {
  const entries =
    path === ""
      ? rootEntries
      : path === "references"
        ? referenceEntries
        : path === "scripts"
          ? scriptEntries
          : path === "assets"
            ? assetEntries
            : [];
  return {
    path,
    displayPath: `~/.agents/skills/${skillId}${path ? `/${path}` : ""}`,
    entries,
    truncated: false,
    maxEntries: 500,
    maxDepth: 12,
    ...overrides,
  };
}

const defaultSkillMd = (skillId: string) =>
  `---\nname: ${skillId}\ndescription: ${skillId} 的描述\n---\n# ${skillId} 标题\n\n正文。\n`;

export function makeFileContent(
  skillId: string,
  path: string,
  overrides: Partial<SkillFileContent> = {},
): SkillFileContent {
  const base = {
    path,
    displayPath: `~/.agents/skills/${skillId}/${path}`,
    status: "ok" as const,
    sizeBytes: 80,
    limitBytes: 1024 * 1024,
    content: null,
    dataUrl: null,
    error: null,
  };
  if (path === "SKILL.md") {
    return {
      ...base,
      kind: "markdown",
      content: defaultSkillMd(skillId),
      ...overrides,
    };
  }
  if (path.toLowerCase().endsWith(".md")) {
    return {
      ...base,
      kind: "markdown",
      content: "# API\n\n参考文档。\n",
      ...overrides,
    };
  }
  if (/\.(png|jpe?g|gif|webp|svg)$/i.test(path)) {
    return {
      ...base,
      kind: "image",
      dataUrl: IMAGE_DATA_URL,
      limitBytes: 5 * 1024 * 1024,
      ...overrides,
    };
  }
  return {
    ...base,
    kind: "text",
    content: "#!/bin/sh\necho ok\n",
    ...overrides,
  };
}
