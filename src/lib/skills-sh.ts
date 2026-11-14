import type { InventorySnapshot } from "./types";

export const skillsShViews = [
  { value: "all-time", label: "累计榜", metric: "累计安装量" },
  { value: "trending", label: "24 小时趋势榜", metric: "24 小时安装量" },
  { value: "hot", label: "热榜", metric: "最近一小时安装量" },
] as const;
export type SkillsShView = (typeof skillsShViews)[number]["value"];
export interface SkillsShCoordinate { source: string; skillId: string }
export interface SkillsShSkill {
  coordinate: SkillsShCoordinate;
  name: string;
  installs: number | null;
  gitSource: string | null;
  officialUrl: string;
}
export interface SkillsShListing { skills: SkillsShSkill[]; total: number; page: number; hasMore: boolean }
export interface SkillsShFile { path: string; contents: string | null; sizeBytes: number }
export interface SkillsShSnapshot { files: SkillsShFile[] | null; hash: string | null }
export interface MarketplaceInstallSeed { source: string; name: string; origin: "skills-sh"; officialUrl: string }
export const skillsShKey = (coordinate: SkillsShCoordinate) => `${coordinate.source}/${coordinate.skillId}`;

function gitIdentity(source: string) {
  return source.replace(/^https:\/\/(?:www\.)?github\.com\//i, "").replace(/\/+$/, "").replace(/\.git$/i, "").toLowerCase();
}
export function skillsShInstallation(skill: SkillsShSkill, snapshot?: InventorySnapshot) {
  const sameName = snapshot?.skills.find((entry) => entry.name.toLowerCase() === skill.name.toLowerCase());
  const installed = sameName && sameName.management !== "unmanaged" && skill.gitSource &&
    ["github", "git"].includes(sameName.sourceType ?? "") && sameName.source &&
    gitIdentity(sameName.source) === gitIdentity(skill.gitSource) ? sameName : undefined;
  return { installed, collision: installed ? undefined : sameName };
}

export type MarketplaceSourceId = "skillhub" | "skills-sh";
const SOURCE_KEY = "skills-box.marketplace-source";
export function readMarketplaceSource(): MarketplaceSourceId {
  try { return localStorage.getItem(SOURCE_KEY) === "skills-sh" ? "skills-sh" : "skillhub"; }
  catch { return "skillhub"; }
}
export function rememberMarketplaceSource(source: MarketplaceSourceId) {
  try { localStorage.setItem(SOURCE_KEY, source); } catch { /* Session selection still works. */ }
}
