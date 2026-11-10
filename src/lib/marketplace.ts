import type { AppError } from "./types";
import type { CliCommandPreview } from "./cli-commands";

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
