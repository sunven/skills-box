import type { AppError } from "./types";
import type { MarketplaceCoordinate } from "./marketplace";
import type { CliCommandPreview } from "./cli-commands";
export interface PackSummary {
  slug: string;
  displayName: string;
  summary: string;
  memberCount: number;
}
export interface PackReference {
  slug: string;
  namespace: string | null;
  coordinate: MarketplaceCoordinate | null;
  error: AppError | null;
}
export interface PackDetail extends PackSummary {
  workflow: string;
  members: PackReference[];
  installError: AppError | null;
}
export interface PackListing {
  packs: PackSummary[];
  total: number;
  page: number;
  pageSize: number;
}
export interface PackMember {
  reference: string;
  coordinate: MarketplaceCoordinate;
  name: string;
  version: string;
  markdown: string;
}
export interface PackSnapshot {
  slug: string;
  displayName: string;
  summary: string;
  originalWorkflow: string;
  workflowName: string;
  installedWorkflow: string;
  members: PackMember[];
  agents: string[];
  installedAt: number;
}
export interface PackSkillState {
  name: string;
  present: boolean;
  missingAgents: string[];
  attachedAgents: string[];
  version: string | null;
  versionChanged: boolean;
  modified: boolean;
  conflict: boolean;
}
export interface LocalPack {
  snapshot: PackSnapshot;
  complete: boolean;
  skills: PackSkillState[];
}
export interface PackPreview {
  snapshot: PackSnapshot;
  supplement: boolean;
  items: {
    name: string;
    action: "install" | "reuse" | "attach";
    version: string | null;
    snapshotVersion: string | null;
    missingAgents: string[];
  }[];
  commands: CliCommandPreview;
}
