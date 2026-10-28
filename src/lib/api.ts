import { invoke } from "@tauri-apps/api/core";
import type { EnvReport } from "./types";

/**
 * The single IPC gateway. Components never call `invoke` directly; keeping the
 * command surface here makes the frontend/Rust contract auditable.
 */
export function getEnvironment(refresh = false): Promise<EnvReport> {
  return invoke<EnvReport>("get_environment", { refresh });
}
