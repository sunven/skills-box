import type { PackDetail, PackSnapshot } from "../lib/expert-packs";
import { marketplaceSkill } from "./marketplace-fixtures";
export const expertPack: PackDetail = {
  slug: "research-workflow",
  displayName: "研究助理",
  summary: "从资料检索到笔记整理，串联完整研究流程。",
  memberCount: 1,
  workflow:
    "---\nscene: research\n---\n# 研究流程\n\n1. 使用 ima-skills 收集资料。\n2. 核对来源并整理结论。\n",
  members: [
    {
      slug: marketplaceSkill.coordinate.slug,
      namespace: marketplaceSkill.coordinate.namespace,
      coordinate: marketplaceSkill.coordinate,
      error: null,
    },
  ],
  installError: null,
};
export function packSnapshot(agents: string[]): PackSnapshot {
  return {
    slug: expertPack.slug,
    displayName: expertPack.displayName,
    summary: expertPack.summary,
    originalWorkflow: expertPack.workflow,
    workflowName: expertPack.slug,
    installedWorkflow: `---\nname: ${expertPack.slug}\ndescription: 研究助理\nscene: research\n---\n# 研究流程\n\n1. 使用 ima-skills 收集资料。\n2. 核对来源并整理结论。\n\n## 本地技能名称对照\n\n| 官方成员引用 | 完整来源 | 本地技能名 |\n| --- | --- | --- |\n| ${marketplaceSkill.coordinate.slug} | @${marketplaceSkill.coordinate.namespace}/${marketplaceSkill.coordinate.slug} | ${marketplaceSkill.originalName} |\n`,
    members: [
      {
        reference: marketplaceSkill.coordinate.slug,
        coordinate: marketplaceSkill.coordinate,
        name: marketplaceSkill.originalName!,
        version: marketplaceSkill.version,
        markdown: marketplaceSkill.markdown,
      },
    ],
    agents,
    installedAt: Date.now(),
  };
}
