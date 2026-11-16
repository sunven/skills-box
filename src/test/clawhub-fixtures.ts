import type { ClawHubListing, ClawHubSkill } from "../lib/clawhub";

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
