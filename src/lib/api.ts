import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  CancelAck,
  EnvReport,
  InventorySnapshot,
  TaskEvent,
} from "./types";

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

/**
 * Start a read-only preview task. Events are ordered per task through a
 * dedicated channel; invalid sources reject before a task exists.
 */
export function startPreview(
  source: string,
  onEvent: (event: TaskEvent) => void,
): Promise<string> {
  const channel = new Channel<TaskEvent>();
  channel.onmessage = onEvent;
  return invoke<string>("start_preview", { source, onEvent: channel });
}

export function cancelTask(taskId: string): Promise<CancelAck> {
  return invoke<CancelAck>("cancel_task", { taskId });
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
