import type {
  ClawHubAliasDetail,
  ClawHubListing,
  ClawHubNativeDetail,
  ClawHubSearch,
  ClawHubSkill,
  ClawHubVersionFiles,
} from "../lib/clawhub";
import type { SkillFileContent } from "../lib/types";

export const clawhubSkill: ClawHubSkill = {
  coordinate: { source: "clawhub", namespace: "steipete", slug: "weather" },
  name: "Weather",
  description: "天气预报与本地化出行建议。",
  metrics: { downloads: 1200, installs: 34, stars: 5 },
  topics: ["weather", "api"],
  official: true,
  latestVersion: "1.2.0",
  officialUrl: "https://clawhub.ai/steipete/skills/weather",
};

/** GitHub-backed shape: no latestVersion, no metrics, no official marker. */
export const clawhubSparseSkill: ClawHubSkill = {
  coordinate: { source: "clawhub", namespace: "aws", slug: "agents-build" },
  name: null,
  description: null,
  metrics: { downloads: null, installs: null, stars: null },
  topics: [],
  official: null,
  latestVersion: null,
  officialUrl: "https://clawhub.ai/aws/skills/agents-build",
};

export const clawhubSecondPageSkill: ClawHubSkill = {
  coordinate: { source: "clawhub", namespace: "openclaw", slug: "browser" },
  name: "Browser",
  description: null,
  metrics: { downloads: 9, installs: null, stars: 0 },
  topics: ["browser"],
  official: false,
  latestVersion: "2.0.0",
  officialUrl: "https://clawhub.ai/openclaw/skills/browser",
};

export const clawhubPageOne: ClawHubListing = {
  skills: [clawhubSkill, clawhubSparseSkill],
  nextCursor: "cursor-2",
};

/** Repeats a page-one entry to prove "load more" appends without duplicates. */
export const clawhubPageTwo: ClawHubListing = {
  skills: [clawhubSkill, clawhubSecondPageSkill],
  nextCursor: null,
};

/**
 * Search results arrive already filtered by the backend: the one skills.sh
 * alias is counted but has no card in this source.
 */
export const clawhubSearch: ClawHubSearch = {
  skills: [clawhubSkill, clawhubSparseSkill],
  returned: 2,
  upstreamReturned: 3,
  limit: 50,
  filteredAliases: 1,
  filteredOthers: 0,
};

/** A ClawHub card that is actually indexed from skills.sh. */
export const clawhubAliasSkill: ClawHubSkill = {
  coordinate: { source: "clawhub", namespace: "anthropics", slug: "debug" },
  name: "debug",
  description: null,
  metrics: { downloads: null, installs: null, stars: null },
  topics: [],
  official: null,
  latestVersion: null,
  officialUrl: "https://clawhub.ai/anthropics/skills/debug",
};

export const clawhubAliasDetail: ClawHubAliasDetail = {
  kind: "alias",
  coordinate: clawhubAliasSkill.coordinate,
  name: "debug",
  reference: "skills-sh:anthropics/knowledge-work-plugins/debug",
  sourceUrl: "https://www.skills.sh/anthropics/knowledge-work-plugins/debug",
  repository: "anthropics/knowledge-work-plugins",
  canonicalUrl: "https://clawhub.ai/anthropics/skills/debug",
};

export const clawhubVersionOne = "1.2.0";
export const clawhubVersionOld = "1.0.0";

export const clawhubDetail: ClawHubNativeDetail = {
  kind: "native",
  coordinate: clawhubSkill.coordinate,
  name: "Weather",
  description: "天气预报与本地化出行建议。",
  topics: ["weather", "api"],
  metrics: { downloads: 1200, installs: 34, stars: 5 },
  ownerDisplayName: "Peter Steinberger",
  latestVersion: clawhubVersionOne,
  canonicalUrl: "https://clawhub.ai/steipete/skills/weather",
  versions: [
    { version: clawhubVersionOne, createdAt: 1767632598365, changelog: "1.2.0：新增本地化出行建议。" },
    { version: clawhubVersionOld, createdAt: 1767545394459, changelog: "1.0.0：首个公开版本。" },
  ],
  versionsTruncated: false,
  resolvedReference: null,
};

const versionTexts: Record<string, Record<string, string>> = {
  [clawhubVersionOne]: {
    "SKILL.md":
      "---\nname: weather\ndescription: 天气预报\n---\n# Weather 1.2.0\n\n查看天气与本地化出行建议。\n\n[阅读指南](references/guide.md)\n\n[缺失文件](references/missing.md)\n",
    "references/guide.md":
      "# 天气指南 1.2.0\n\n优先查询当前天气，再读取未来预报。\n\n[查看脚本](../scripts/forecast.py)\n",
    "scripts/forecast.py": "def forecast(city: str) -> str:\n    return f\"{city}: sunny\"\n",
  },
  [clawhubVersionOld]: {
    "SKILL.md":
      "---\nname: weather\ndescription: 天气预报\n---\n# Weather 1.0.0\n\n旧版天气查询说明。\n\n[指南](references/guide.md)\n",
    "references/guide.md": "# 天气指南 1.0.0\n\n旧版指南仅支持当前天气。\n",
    "scripts/forecast.py": "def forecast(city: str) -> str:\n    return city\n",
  },
};

const previewImage =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aXioAAAAASUVORK5CYII=";

export function clawhubVersionFilesFixture(version: string): ClawHubVersionFiles {
  const texts = versionTexts[version] ?? versionTexts[clawhubVersionOne];
  const files = [
    ...Object.entries(texts).map(([path, text]) => ({
      path,
      sizeBytes: new TextEncoder().encode(text).length,
    })),
    { path: "assets/preview.png", sizeBytes: 68 },
    { path: "assets/data.bin", sizeBytes: 128 },
    { path: "references/large.txt", sizeBytes: 2 * 1024 * 1024 },
  ];
  return {
    coordinate: clawhubSkill.coordinate,
    version,
    license: version === clawhubVersionOne ? "MIT-0" : "MIT",
    changelog: version === clawhubVersionOne ? "1.2.0：新增本地化出行建议。" : "1.0.0：首个公开版本。",
    files,
  };
}

export function clawhubFileContent(version: string, path: string): SkillFileContent {
  const texts = versionTexts[version] ?? {};
  const sizeBytes =
    clawhubVersionFilesFixture(version).files.find((file) => file.path === path)?.sizeBytes ?? 0;
  const image = path.endsWith(".png");
  const large = path.endsWith("large.txt");
  const binary = path.endsWith(".bin");
  return {
    path,
    displayPath: path,
    sizeBytes,
    limitBytes: image ? 5 * 1024 * 1024 : 1024 * 1024,
    kind: image ? "image" : path.endsWith(".md") ? "markdown" : "text",
    status: binary ? "binary" : large ? "too_large" : "ok",
    content: binary || large ? null : (texts[path] ?? null),
    dataUrl: image ? previewImage : null,
    error: null,
  };
}
