import type { AppError } from "./types";
import type { CliCommandPreview } from "./cli-commands";

export const marketplaceSortOptions = [
  { value: "score", label: "全部" },
  { value: "trending", label: "近期飙升" },
  { value: "downloads", label: "下载量" },
  { value: "updated_at", label: "最近上新" },
] as const;
export type MarketplaceSort = (typeof marketplaceSortOptions)[number]["value"];

export const marketplaceApiKeyOptions = [
  { value: "all", label: "不限 API Key" },
  { value: "required", label: "需要 API Key" },
  { value: "none", label: "无需 API Key" },
] as const;
export type MarketplaceApiKeyFilter = (typeof marketplaceApiKeyOptions)[number]["value"];

export interface MarketplaceCategory {
  key: string;
  name: string;
}

export interface MarketplaceCoordinate {
  source: "skillhub";
  namespace: string;
  slug: string;
}
export interface MarketplaceSkill {
  coordinate: MarketplaceCoordinate;
  name: string;
  publisher: string;
  description: string;
  version: string;
  officialUrl: string;
  stars: number | null;
  downloads: number | null;
}
export interface MarketplaceListing {
  skills: MarketplaceSkill[];
  total: number;
  page: number;
  pageSize: number;
}
export interface MarketplaceDetail extends MarketplaceSkill {
  markdown: string;
  originalName: string | null;
  installError: AppError | null;
  requirements: string[];
}
export interface MarketplaceFileEntry {
  path: string;
  sizeBytes: number;
}
export interface MarketplaceFiles {
  coordinate: MarketplaceCoordinate;
  version: string;
  files: MarketplaceFileEntry[];
}
export interface MarketplaceCli {
  available: boolean;
  path: string | null;
  version: string | null;
  error: AppError | null;
  installCommand: string;
  instructionsUrl: string;
}
export interface MarketplacePreview {
  detail: MarketplaceDetail;
  commands: CliCommandPreview;
}
export const marketplaceKey = (coordinate: MarketplaceCoordinate) =>
  `${coordinate.source}:@${coordinate.namespace}/${coordinate.slug}`;
export const marketplaceAddress = (coordinate: MarketplaceCoordinate) =>
  `@${coordinate.namespace}/${coordinate.slug}`;
