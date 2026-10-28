import type { InventorySkill, SkillDetail } from "../lib/types";

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
    update: "unknown",
    localModification: "clean",
    localModificationDetail: null,
    attachments: [],
    warnings: [],
    skillMd: {
      status: "ok",
      path: `${directory}/SKILL.md`,
      content: `---\nname: ${name}\ndescription: ${name} 的描述\n---\n# ${name}\n\n正文。\n`,
      sizeBytes: 80,
      limitBytes: 1024 * 1024,
      error: null,
    },
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
    management: skill.management,
    update: skill.update,
    updatedAt: skill.updatedAt,
    canonicalPath: skill.canonicalPath,
    directoryPath: directory,
    localModification: skill.localModification,
    attachments: skill.attachments,
    warnings: skill.warnings,
    skillMd: {
      status: "ok",
      path: directory ? `${directory}/SKILL.md` : null,
      content: `---\nname: ${skill.name}\ndescription: ${
        skill.description ?? "—"
      }\n---\n# ${skill.name}\n\n正文。\n`,
      sizeBytes: 80,
      limitBytes: 1024 * 1024,
      error: null,
    },
  });
}
