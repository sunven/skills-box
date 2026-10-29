import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  ActiveMutationInfo,
  AppDataKind,
  CancelAck,
  DetachResult,
  EnvReport,
  InstallRequest,
  InventorySnapshot,
  SkillDetail,
  SkillDirListing,
  SkillFileContent,
  TaskEvent,
  UpdateCheckReport,
  UpdateCheckState,
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

/** Lazy directory listing; `path` is relative to the server-resolved skill root. */
export function listSkillDir(
  skillId: string,
  path = "",
): Promise<SkillDirListing> {
  return invoke<SkillDirListing>("list_skill_dir", { skillId, path });
}

/** Read one skill file; oversized and binary files are values, not errors. */
export function readSkillFile(
  skillId: string,
  path: string,
): Promise<SkillFileContent> {
  return invoke<SkillFileContent>("read_skill_file", { skillId, path });
}

/** Open one file inside the skill with the system default application. */
export function openSkillFile(
  skillId: string,
  path: string,
): Promise<void> {
  return invoke<void>("open_skill_file", { skillId, path });
}

/**
 * External links are https-only. Rust re-validates the scheme and host, and no
 * opener permission is exposed to the frontend: the only URL path is this
 * typed command.
 */
export function openExternalUrl(url: string): Promise<void> {
  if (!/^https:\/\//i.test(url)) {
    return Promise.reject(new Error("只允许通过 opener 打开 https 链接。"));
  }
  return invoke<void>("open_external_url", { url });
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

/** The single in-flight mutation; `null` means closing the window is safe. */
export function getActiveMutation(): Promise<ActiveMutationInfo | null> {
  return invoke<ActiveMutationInfo | null>("get_active_mutation");
}

/** Acknowledge the startup interruption warning; snapshots are retained. */
export function dismissInterruptedMutation(): Promise<void> {
  return invoke<void>("dismiss_interrupted_mutation");
}

/**
 * Confirmed exit: Rust keeps the interruption marker, kills the CLI process
 * group even in the sealed write phase, waits briefly, then exits.
 */
export function quitApp(): Promise<void> {
  return invoke<void>("quit_app");
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
 * skills CLI is never spawned for checking. Runs automatically at launch and
 * every six hours while the app is open, and by explicit user actions.
 */
export function checkUpdates(skillIds?: string[]): Promise<UpdateCheckReport> {
  return invoke<UpdateCheckReport>("check_updates", {
    skillIds: skillIds ?? null,
  });
}

/** Persisted summary of the last full check, for the pre-check header state. */
export function getUpdateCheckState(): Promise<UpdateCheckState | null> {
  return invoke<UpdateCheckState | null>("get_update_check_state");
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

/**
 * Start a serialized uninstall. Rust re-validates that the skill is managed or
 * copy-only, rejects cross-agent copy dependencies, backs up first, runs
 * `skills remove -g -s <name> -y` (never `-a` / `--all`) and verifies that the
 * canonical folder, every attachment and the lock key are gone.
 */
export function startUninstall(
  skillId: string,
  onEvent: (event: TaskEvent) => void,
): Promise<string> {
  const channel = new Channel<TaskEvent>();
  channel.onmessage = onEvent;
  return invoke<string>("start_uninstall", { skillId, onEvent: channel });
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
