import { Channel, invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import type {
  AppDataKind,
  CancelAck,
  DetachResult,
  EnvReport,
  InstallRequest,
  InventorySnapshot,
  SkillDetail,
  TaskEvent,
  UpdateCheckReport,
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

/**
 * Start a serialized install mutation. Rust re-validates the source, skills,
 * and agent ids, rejects conflicts, snapshots the lock, and appends `-a
 * universal`; the frontend never supplies argv, paths, or env.
 */
export function startInstall(
  request: InstallRequest,
  onEvent: (event: TaskEvent) => void,
): Promise<string> {
  const channel = new Channel<TaskEvent>();
  channel.onmessage = onEvent;
  return invoke<string>("start_install", { request, onEvent: channel });
}

/**
 * Read-only update check. Rust reads the lock and GitHub trees directly; the
 * skills CLI is never spawned for checking. No automatic/background callers —
 * only explicit user actions may invoke this.
 */
export function checkUpdates(skillIds?: string[]): Promise<UpdateCheckReport> {
  return invoke<UpdateCheckReport>("check_updates", {
    skillIds: skillIds ?? null,
  });
}

/**
 * Start a serialized update task. The frontend only sends update-candidate
 * skill ids; Rust re-validates, backs up, rebuilds `skills add`, verifies and
 * refreshes the baseline. No `skills update` / `skills check` is ever spawned.
 */
export function startUpdate(
  skillIds: string[],
  onEvent: (event: TaskEvent) => void,
): Promise<string> {
  const channel = new Channel<TaskEvent>();
  channel.onmessage = onEvent;
  return invoke<string>("start_update", { skillIds, onEvent: channel });
}

/**
 * Detach one agent's attachment natively (no CLI). Rust re-validates the
 * managed/canonical condition, rejects universal/native/broken rows, scans
 * copy dependencies by realpath and backs up copy attachments before deletion.
 */
export function detachAgent(
  skillId: string,
  agentId: string,
): Promise<DetachResult> {
  return invoke<DetachResult>("detach_agent", { skillId, agentId });
}

/** Open a fixed app-data directory; the frontend never supplies a path. */
export function revealAppData(kind: AppDataKind): Promise<void> {
  return invoke<void>("reveal_app_data", { kind });
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
