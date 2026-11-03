import { invoke } from "@tauri-apps/api/core";
import { API_COMMANDS } from "./api";
import type { AppError } from "./types";

export type AppUpdatePhase = "idle" | "checking" | "current" | "available" | "downloading" | "verifying" | "ready" | "installing" | "installed" | "restarting" | "error";
export interface AppUpdateSnapshot {
  revision: number;
  enabled: boolean;
  currentVersion: string;
  phase: AppUpdatePhase;
  candidate: { version: string; notes: string | null } | null;
  downloadedBytes: number;
  totalBytes: number | null;
  hasVerifiedDownload: boolean;
  lastAttemptAt: number | null;
  error: AppError | null;
}
export const appUpdateQueryKey = ["app-update"] as const;
export const APP_UPDATE_INTERVAL = 6 * 60 * 60 * 1000;
export const APP_RELEASE_URL = "https://github.com/sunven/skills-box/releases/latest";
export const appUpdateBusy = (phase: AppUpdatePhase) => ["checking", "downloading", "verifying", "installing", "restarting"].includes(phase);
export const appUpdateCritical = (phase: AppUpdatePhase) => ["installing", "installed", "restarting"].includes(phase);
export const getAppUpdateState = () => invoke<AppUpdateSnapshot>(API_COMMANDS.getAppUpdateState);
export const checkAppUpdate = () => invoke<AppUpdateSnapshot>(API_COMMANDS.checkAppUpdate);
export const startAppUpdate = () => invoke<AppUpdateSnapshot>(API_COMMANDS.startAppUpdate);
export function initialAppUpdate(overrides: Partial<AppUpdateSnapshot> = {}): AppUpdateSnapshot {
  return { revision: 0, enabled: false, currentVersion: "0.1.0", phase: "idle", candidate: null,
    downloadedBytes: 0, totalBytes: null, hasVerifiedDownload: false, lastAttemptAt: null, error: null, ...overrides };
}
