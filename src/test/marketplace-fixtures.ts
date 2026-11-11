import type { MarketplaceCli, MarketplaceDetail, MarketplaceCategory, MarketplaceFileEntry } from "../lib/marketplace";
import type { SkillFileContent } from "../lib/types";
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

const fileTexts: Record<string, string> = {
  "SKILL.md": marketplaceSkill.markdown,
  "references/guide.md": "# 笔记指南\n\n将项目决策写入独立笔记，保留日期与关联文件。\n\n[查看示例脚本](../scripts/notes.py)\n\n[返回技能说明](../SKILL.md)",
  "scripts/notes.py": 'from pathlib import Path\n\nfor note in Path("notes").glob("*.md"):\n    print(note.name)\n',
  "LICENSE": "MIT License\n\nCopyright (c) Example\n",
};
const previewImage = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aXioAAAAASUVORK5CYII=";
export const marketplaceFiles: MarketplaceFileEntry[] = [
  ...Object.entries(fileTexts).map(([path, text]) => ({ path, sizeBytes: new TextEncoder().encode(text).length })),
  { path: "assets/preview.png", sizeBytes: 68 },
  { path: "assets/data.bin", sizeBytes: 128 },
  { path: "references/large.txt", sizeBytes: 2 * 1024 * 1024 },
];
export function marketplaceFileContent(path: string): SkillFileContent {
  const entry = marketplaceFiles.find((file) => file.path === path);
  if (!entry) throw new Error("此版本中没有该文件");
  const image = path.endsWith(".png");
  return {
    path, displayPath: path, sizeBytes: entry.sizeBytes,
    limitBytes: image ? 5 * 1024 * 1024 : 1024 * 1024,
    kind: image ? "image" : path.endsWith(".md") ? "markdown" : "text",
    status: path.endsWith(".bin") ? "binary" : path.endsWith("large.txt") ? "too_large" : "ok",
    content: fileTexts[path] ?? null,
    dataUrl: image ? previewImage : null,
    error: null,
  };
}

export const marketplaceCategories: MarketplaceCategory[] = [
  { key: "pay-skill", name: "Pay Skill" },
  { key: "office-efficiency", name: "办公效率" },
  { key: "content-creation", name: "内容创作" },
  { key: "dev-programming", name: "开发编程" },
  { key: "data-analysis", name: "数据分析" },
  { key: "design-media", name: "设计多媒体" },
  { key: "ai-agent", name: "AI Agent" },
  { key: "knowledge-management", name: "知识管理" },
  { key: "business-ops", name: "商业运营" },
  { key: "education", name: "教育学习" },
  { key: "professional", name: "行业专业" },
  { key: "it-ops-security", name: "IT 运维与安全" },
  { key: "life-service", name: "生活服务" },
];
