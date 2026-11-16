import type { MarketplaceCoordinate } from "./marketplace";

/** The only documented ClawHub catalog sorts exposed by the app. */
export const clawhubSorts = [
  { value: "recommended", label: "推荐" },
  { value: "updated", label: "最近更新" },
  { value: "downloads", label: "下载量" },
] as const;
export type ClawHubSort = (typeof clawhubSorts)[number]["value"];

export type ClawHubCoordinate = MarketplaceCoordinate & { source: "clawhub" };
export interface ClawHubMetrics {
  downloads: number | null;
  installs: number | null;
  stars: number | null;
}
export interface ClawHubSkill {
  coordinate: ClawHubCoordinate;
  name: string | null;
  description: string | null;
  metrics: ClawHubMetrics;
  topics: string[];
  official: boolean | null;
  latestVersion: string | null;
  officialUrl: string;
}
/** Cursor page. There is deliberately no total, page number or page size. */
export interface ClawHubListing {
  skills: ClawHubSkill[];
  nextCursor: string | null;
}

export const CLAWHUB_OFFICIAL_URL = "https://clawhub.ai/skills";
export const clawhubIdentity = (coordinate: ClawHubCoordinate | MarketplaceCoordinate) =>
  `${coordinate.namespace}/${coordinate.slug}`;
export const clawhubKey = (coordinate: ClawHubCoordinate) => `clawhub:${clawhubIdentity(coordinate)}`;
export const clawhubAddress = (coordinate: ClawHubCoordinate) =>
  `@${coordinate.namespace}/${coordinate.slug}`;

export const clawhubMetricLabels = [
  { key: "downloads", label: "下载量" },
  { key: "installs", label: "安装量" },
  { key: "stars", label: "星标" },
] as const;
