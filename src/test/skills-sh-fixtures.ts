import type { SkillsShSkill, SkillsShSnapshot } from "../lib/skills-sh";
import type { PreviewSummary } from "../lib/types";

export const skillsShSkill: SkillsShSkill = {
  coordinate: { source: "example/skills", skillId: "project-notes" },
  name: "project-notes", installs: 12500, gitSource: "example/skills",
  officialUrl: "https://www.skills.sh/example/skills/project-notes",
};
export const skillsShWebsiteSkill: SkillsShSkill = {
  coordinate: { source: "docs.example", skillId: "writing" },
  name: "writing", installs: 24, gitSource: null,
  officialUrl: "https://www.skills.sh/site/docs.example/writing",
};
export const skillsShSnapshot: SkillsShSnapshot = {
  hash: "snapshot-fixture",
  files: [
    { path: "SKILL.md", contents: "---\nname: project-notes\ndescription: 整理项目笔记\n---\n# Project Notes\n\n记录项目决策与后续事项。\n\n[阅读指南](references/guide.md)", sizeBytes: 190 },
    { path: "references/guide.md", contents: "# 笔记指南\n\n记录日期、背景和相关文件。\n\n[返回技能说明](../SKILL.md)", sizeBytes: 100 },
    { path: "scripts/notes.py", contents: "print('notes')\n", sizeBytes: 15 },
  ],
};
export const skillsShPreview: PreviewSummary = {
  source: "example/skills", parseFailed: false, noSkills: false,
  skills: ["project-notes", "other-skill"].map((name) => ({ name, description: `${name} description` })),
};
