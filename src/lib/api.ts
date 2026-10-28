import { invoke } from "@tauri-apps/api/core";
import type { EnvReport, InventorySnapshot } from "./types";

/**
 * The single IPC gateway. Components never call `invoke` directly; keeping the
 * command surface here makes the frontend/Rust contract auditable.
 */
export function getEnvironment(refresh = false): Promise<EnvReport> {
  return invoke<EnvReport>("get_environment", { refresh });
}

export function getInventory(): Promise<InventorySnapshot> {
  return invoke<InventorySnapshot>("get_inventory");
}

/** Normalize a rejected Tauri command into an `AppError`-shaped object. */
export function asAppError(error: unknown): {
  code?: string;
  message: string;
  detail?: string;
  hint?: string;
} | null {
  if (!error) return null;
  if (error instanceof Error) {
    return { message: error.message };
  }
  if (typeof error === "string") {
    return { message: error };
  }
  if (typeof error === "object") {
    const candidate = error as {
      code?: unknown;
      message?: unknown;
      detail?: unknown;
      hint?: unknown;
    };
    if (typeof candidate.message === "string") {
      return {
        code: typeof candidate.code === "string" ? candidate.code : undefined,
        message: candidate.message,
        detail:
          typeof candidate.detail === "string" ? candidate.detail : undefined,
        hint: typeof candidate.hint === "string" ? candidate.hint : undefined,
      };
    }
  }
  return { message: "未知错误" };
}
