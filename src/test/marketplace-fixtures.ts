import type { MarketplaceCli, MarketplaceDetail } from "../lib/marketplace";
export const marketplaceSkill: MarketplaceDetail = {
  coordinate: {
    source: "skillhub",
    namespace: "example",
    slug: "workspace-notes",
  },
  name: "Workspace Notes",
  publisher: "Example",
  description: "整理项目笔记、检索知识并记录决策。",
  version: "1.2.0",
  officialUrl: "https://skillhub.cn/skills/example/workspace-notes",
  stars: 109,
  downloads: 11_384_000,
  originalName: "workspace-notes",
  markdown:
    "---\nname: workspace-notes\ndescription: 项目笔记\n---\n# Workspace Notes\n\n整理项目笔记和记录决策。\n\n## 运行要求\n使用技能前，请选择笔记目录。",
  installError: null,
  requirements: [],
};
export const marketplaceCli: MarketplaceCli = {
  available: true,
  path: "/Users/demo/.local/bin/skillhub",
  version: "skillhub 2026.8.5",
  error: null,
  installCommand:
    "curl -fsSL https://skillhub-1388575217.cos.ap-guangzhou.myqcloud.com/install/install.sh | bash -s -- --cli-only",
  instructionsUrl: "https://skillhub.cn/install/skillhub.md",
};
