import { Channel, invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import type {
  CancelAck,
  EnvReport,
  InventorySnapshot,
  SkillDetail,
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

/** Detail is fetched on demand; the frontend only sends the inventory id. */
export function getSkillDetail(skillId: string): Promise<SkillDetail> {
  return invoke<SkillDetail>("get_skill_detail", { skillId });
}

/** Rust resolves and guards the folder; no path crosses the IPC boundary. */
export function openSkillFolder(skillId: string): Promise<void> {
  return invoke<void>("open_skill_folder", { skillId });
}

/** Rust resolves and guards SKILL.md; opens with the default editor. */
export function openSkillMd(skillId: string): Promise<void> {
  return invoke<void>("open_skill_md", { skillId });
}

/** External links are https-only and leave the app through the opener. */
export function openExternalUrl(url: string): Promise<void> {
  if (!/^https:\/\//i.test(url)) {
    return Promise.reject(new Error("只允许通过 opener 打开 https 链接。"));
  }
  return openUrl(url);
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
