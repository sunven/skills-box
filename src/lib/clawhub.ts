import type { CliCommandPreview } from "./cli-commands";
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
/**
 * Keyword search has no cursor. `returned` is what this source displays after
 * excluding skills.sh aliases; `upstreamReturned` is the raw upstream count;
 * `limit` is the requested upstream cap.
 */
export interface ClawHubSearch {
  skills: ClawHubSkill[];
  returned: number;
  upstreamReturned: number;
  limit: number;
  filteredAliases: number;
  filteredOthers: number;
}

/** One version in the detail's version selector. Order is upstream order. */
export interface ClawHubVersion {
  version: string;
  createdAt: number | null;
  changelog: string | null;
}
/** ClawHub-native entry. `canonicalUrl` is always the documented listing. */
export interface ClawHubNativeDetail {
  kind: "native";
  coordinate: ClawHubCoordinate;
  name: string | null;
  description: string | null;
  topics: string[];
  metrics: ClawHubMetrics;
  ownerDisplayName: string | null;
  latestVersion: string | null;
  canonicalUrl: string;
  versions: ClawHubVersion[];
  versionsTruncated: boolean;
  /**
   * GitHub resolver result for entries without ClawHub-hosted versions.
   * Diagnostic only: installation is paused and this is never a Git identity.
   */
  resolvedReference: ClawHubResolvedReference | null;
}
/** `repo@commit:path` as resolved by ClawHub for GitHub-backed entries. */
export interface ClawHubResolvedReference {
  repo: string;
  path: string;
  commit: string;
  contentHash?: string | null;
}
/**
 * A ClawHub index entry backed by skills.sh. It has no ClawHub-hosted content
 * and must be browsed/installed from the skills.sh source instead.
 */
export interface ClawHubAliasDetail {
  kind: "alias";
  coordinate: ClawHubCoordinate;
  name: string | null;
  reference: string | null;
  sourceUrl: string | null;
  repository: string | null;
  canonicalUrl: string;
}
export type ClawHubDetail = ClawHubNativeDetail | ClawHubAliasDetail;
export interface ClawHubFileEntry {
  path: string;
  sizeBytes: number;
}
/** Files and metadata of one explicit version, all from the same response. */
export interface ClawHubVersionFiles {
  coordinate: ClawHubCoordinate;
  version: string;
  license: string | null;
  changelog: string | null;
  files: ClawHubFileEntry[];
}

/** Native archive installation preview. No CLI command is executed. */
export interface ClawHubInstallPreview {
  coordinate: ClawHubCoordinate;
  name: string;
  version: string;
  referenceLabel: string;
  canonicalUrl: string;
  commands: CliCommandPreview;
}

export const CLAWHUB_OFFICIAL_URL = "https://clawhub.ai/skills";
export const clawhubIdentity = (coordinate: ClawHubCoordinate | MarketplaceCoordinate) =>
  `${coordinate.namespace}/${coordinate.slug}`;
export const clawhubKey = (coordinate: ClawHubCoordinate) => `clawhub:${clawhubIdentity(coordinate)}`;
export const clawhubAddress = (coordinate: ClawHubCoordinate) =>
  `@${coordinate.namespace}/${coordinate.slug}`;
/** The version the detail opens on: upstream latest, else the first listed. */
export const clawhubInitialVersion = (detail: ClawHubNativeDetail) =>
  detail.latestVersion ?? detail.versions[0]?.version ?? null;
export const clawhubVersionLabel = (version: string, latest: boolean) =>
  latest ? `v${version}（latest）` : `v${version}`;

export const clawhubMetricLabels = [
  { key: "downloads", label: "下载量" },
  { key: "installs", label: "安装量" },
  { key: "stars", label: "星标" },
] as const;
