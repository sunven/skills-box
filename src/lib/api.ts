import type { BatchRequest, BatchPreview, BatchConfirmation } from "./batch";
import type { CliCommandRequest, CliCommandPreview, CliCommandRecord } from "./cli-commands";
import type {
  DiscoveryId,
  DiscoveryState,
  DiscoveryProgress,
} from "./discovery";
import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  ActiveMutationInfo,
  AppDataKind,
  AttachResult,
  CancelAck,
  DetachResult,
  EnvReport,
  GitHubSettingsStatus,
  GitHubAuthTestResult,
  ExtraAgent,
  ExtraAgentInput,
  InstallRequest,
  InventorySnapshot,
  ProjectEntry,
  RelocateProjectResult,
  ForgetProjectResult,
  ProjectList,
  RemoveExtraAgentResult,
  ScopeId,
  SkillDetail,
  SkillDirListing,
  SkillFileContent,
  TaskEvent,
  UpdateCheckResponse,
  UpdateCheckState,
  AppError,
} from "./types";
import { GLOBAL_SCOPE } from "./types";

/** Every IPC command the frontend may invoke. */
export const API_COMMANDS = {
  getAppUpdateState: "get_app_update_state",
  checkAppUpdate: "check_app_update",
  startAppUpdate: "start_app_update",
  getDiscoveryState: "get_discovery_state",
  startDiscoveryScan: "start_discovery_scan",
  acknowledgeDiscoveries: "acknowledge_discoveries",
  setDiscoveryIgnored: "set_discovery_ignored",
  invalidateDiscoverySource: "invalidate_discovery_source",
  getEnvironment: "get_environment",
  getCliCommands: "get_cli_commands",
  previewCliCommands: "preview_cli_commands",
  getGitHubSettings: "get_github_settings",
  saveGitHubToken: "save_github_token",
  clearGitHubToken: "clear_github_token",
  testGitHubAuth: "test_github_auth",
  listProjects: "list_projects",
  addProject: "add_project",
  relocateProject: "relocate_project",
  forgetProject: "forget_project",
  setActiveScope: "set_active_scope",
  getInventory: "get_inventory",
  getSkillDetail: "get_skill_detail",
  openSkillFolder: "open_skill_folder",
  listSkillDir: "list_skill_dir",
  readSkillFile: "read_skill_file",
  openSkillFile: "open_skill_file",
  openExternalUrl: "open_external_url",
  startPreview: "start_preview",
  cancelTask: "cancel_task",
  getActiveMutation: "get_active_mutation",
  dismissInterruptedMutation: "dismiss_interrupted_mutation",
  quitApp: "quit_app",
  startInstall: "start_install",
  checkUpdates: "check_updates",
  getUpdateCheckState: "get_update_check_state",
  getUpdateCheckStates: "get_update_check_states",
  startUpdate: "start_update",
  previewBatch: "preview_batch",
  refreshBatchPreview: "refresh_batch_preview",
  startBatch: "start_batch",
  detachAgent: "detach_agent",
  attachAgent: "attach_agent",
  addExtraAgent: "add_extra_agent",
  updateExtraAgent: "update_extra_agent",
  removeExtraAgent: "remove_extra_agent",
  startUninstall: "start_uninstall",
  revealAppData: "reveal_app_data",
} as const;

export type ApiCommand = (typeof API_COMMANDS)[keyof typeof API_COMMANDS];

/**
 * The single IPC gateway. Components never call `invoke` directly; keeping the
 * command surface here makes the frontend/Rust contract auditable.
 */
export function getEnvironment(refresh = false): Promise<EnvReport> {
  return invoke<EnvReport>(API_COMMANDS.getEnvironment, { refresh });
}

export function getCliCommands(): Promise<CliCommandRecord[]> {
  return invoke<CliCommandRecord[]>(API_COMMANDS.getCliCommands);
}
export function previewCliCommands(scope: ScopeId, request: CliCommandRequest): Promise<CliCommandPreview> {
  return invoke<CliCommandPreview>(API_COMMANDS.previewCliCommands, { scope, request });
}

export function getGitHubSettings(): Promise<GitHubSettingsStatus> {
  return invoke<GitHubSettingsStatus>(API_COMMANDS.getGitHubSettings);
}

export function saveGitHubToken(token: string): Promise<GitHubSettingsStatus> {
  return invoke<GitHubSettingsStatus>(API_COMMANDS.saveGitHubToken, { token });
}

export function clearGitHubToken(): Promise<GitHubSettingsStatus> {
  return invoke<GitHubSettingsStatus>(API_COMMANDS.clearGitHubToken);
}

export function testGitHubAuth(): Promise<GitHubAuthTestResult> {
  return invoke<GitHubAuthTestResult>(API_COMMANDS.testGitHubAuth);
}

/** App-registered projects plus the restored active scope. */
export function listProjects(): Promise<ProjectList> {
  return invoke<ProjectList>(API_COMMANDS.listProjects);
}

/** Register a project root and activate it. Nothing is created on disk. */
export function addProject(root: string): Promise<ProjectEntry> {
  return invoke<ProjectEntry>(API_COMMANDS.addProject, { root });
}

/** Point an existing project at a new root, keeping its id. */
export function relocateProject(
  id: string,
  root: string,
): Promise<RelocateProjectResult> {
  return invoke<RelocateProjectResult>(API_COMMANDS.relocateProject, {
    id,
    root,
  });
}

/** Remove the registry entry and its check state; never deletes files. */
export function forgetProject(id: string): Promise<ForgetProjectResult> {
  return invoke<ForgetProjectResult>(API_COMMANDS.forgetProject, { id });
}

/** Persist the active scope (and refresh the project's lastOpenedAt). */
export function setActiveScope(scope: ScopeId): Promise<void> {
  return invoke<void>(API_COMMANDS.setActiveScope, { scope });
}

export function getInventory(
  scope: ScopeId = GLOBAL_SCOPE,
): Promise<InventorySnapshot> {
  return invoke<InventorySnapshot>(API_COMMANDS.getInventory, { scope });
}

/** Detail is fetched on demand; the frontend only sends the inventory id. */
export function getSkillDetail(
  skillId: string,
  scope: ScopeId = GLOBAL_SCOPE,
): Promise<SkillDetail> {
  return invoke<SkillDetail>(API_COMMANDS.getSkillDetail, { skillId, scope });
}

/** Rust resolves and guards the folder; no path crosses the IPC boundary. */
export function openSkillFolder(
  skillId: string,
  scope: ScopeId = GLOBAL_SCOPE,
): Promise<void> {
  return invoke<void>(API_COMMANDS.openSkillFolder, { skillId, scope });
}

/** Lazy directory listing; `path` is relative to the server-resolved skill root. */
export function listSkillDir(
  skillId: string,
  path = "",
  scope: ScopeId = GLOBAL_SCOPE,
): Promise<SkillDirListing> {
  return invoke<SkillDirListing>(API_COMMANDS.listSkillDir, {
    skillId,
    path,
    scope,
  });
}

/** Read one skill file; oversized and binary files are values, not errors. */
export function readSkillFile(
  skillId: string,
  path: string,
  scope: ScopeId = GLOBAL_SCOPE,
): Promise<SkillFileContent> {
  return invoke<SkillFileContent>(API_COMMANDS.readSkillFile, {
    skillId,
    path,
    scope,
  });
}

/** Open one file inside the skill with the system default application. */
export function openSkillFile(
  skillId: string,
  path: string,
  scope: ScopeId = GLOBAL_SCOPE,
): Promise<void> {
  return invoke<void>(API_COMMANDS.openSkillFile, { skillId, path, scope });
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
  return invoke<void>(API_COMMANDS.openExternalUrl, { url });
}

/**
 * Start a read-only preview task. Events are ordered per task through a
 * dedicated channel; invalid sources reject before a task exists.
 */
export function startPreview(
  source: string,
  onEvent: (event: TaskEvent) => void,
  scope: ScopeId = GLOBAL_SCOPE,
): Promise<string> {
  const channel = new Channel<TaskEvent>();
  channel.onmessage = onEvent;
  return invoke<string>(API_COMMANDS.startPreview, {
    source,
    scope,
    onEvent: channel,
  });
}

export function cancelTask(taskId: string): Promise<CancelAck> {
  return invoke<CancelAck>(API_COMMANDS.cancelTask, { taskId });
}

/** The single in-flight mutation; `null` means closing the window is safe. */
export function getActiveMutation(): Promise<ActiveMutationInfo | null> {
  return invoke<ActiveMutationInfo | null>(API_COMMANDS.getActiveMutation);
}

/** Acknowledge the startup interruption warning; snapshots are retained. */
export function dismissInterruptedMutation(): Promise<void> {
  return invoke<void>(API_COMMANDS.dismissInterruptedMutation);
}

/**
 * Confirmed exit: Rust keeps the interruption marker, kills the CLI process
 * group even in the sealed write phase, waits briefly, then exits.
 */
export function quitApp(): Promise<void> {
  return invoke<void>(API_COMMANDS.quitApp);
}

/**
 * Start a serialized install mutation. Rust re-validates the source, skills,
 * and agent ids, rejects conflicts, snapshots the lock, and appends `-a
 * universal`; the frontend never supplies argv, paths, or env.
 */
export function startInstall(
  request: InstallRequest,
  onEvent: (event: TaskEvent) => void,
  scope: ScopeId,
  commandPreviewId?: string,
): Promise<string> {
  const channel = new Channel<TaskEvent>();
  channel.onmessage = onEvent;
  return invoke<string>(API_COMMANDS.startInstall, {
    request,
    commandPreviewId,
    scope,
    onEvent: channel,
  });
}

/**
 * Read-only update check. Rust reads the lock and GitHub trees directly; the
 * skills CLI is never spawned for checking. Runs automatically at launch and
 * every six hours while the app is open, and by explicit user actions.
 */
export function checkUpdates(
  skillIds?: string[],
  scope: ScopeId = GLOBAL_SCOPE,
  checkRevision?: string,
): Promise<UpdateCheckResponse> {
  return invoke<UpdateCheckResponse>(API_COMMANDS.checkUpdates, {
    skillIds: skillIds ?? null,
    scope,
    ...(checkRevision === undefined ? {} : { checkRevision }),
  });
}

/** Persisted summary of the last full check, for the pre-check header state. */
export function getUpdateCheckState(
  scope: ScopeId = GLOBAL_SCOPE,
): Promise<UpdateCheckState | null> {
  return invoke<UpdateCheckState | null>(API_COMMANDS.getUpdateCheckState, {
    scope,
  });
}

/** Every scope's persisted summary, in one file read. */
export function getUpdateCheckStates(): Promise<
  Record<string, UpdateCheckState>
> {
  return invoke<Record<string, UpdateCheckState>>(
    API_COMMANDS.getUpdateCheckStates,
  );
}

/**
 * Start a serialized update task. The frontend only sends update-candidate
 * skill ids; Rust re-validates, backs up, rebuilds `skills add`, verifies and
 * refreshes the baseline. No `skills update` / `skills check` is ever spawned.
 */
export function startUpdate(
  skillIds: string[],
  onEvent: (event: TaskEvent) => void,
  scope: ScopeId,
  commandPreviewId?: string,
): Promise<string> {
  const channel = new Channel<TaskEvent>();
  channel.onmessage = onEvent;
  return invoke<string>(API_COMMANDS.startUpdate, {
    skillIds,
    commandPreviewId,
    scope,
    onEvent: channel,
  });
}

/**
 * Detach one agent's attachment natively (no CLI). Rust re-validates the
 * managed/canonical condition, rejects universal/native/broken rows, scans
 * copy dependencies by realpath and backs up copy attachments before deletion.
 */
export function detachAgent(
  skillId: string,
  agentId: string,
  scope: ScopeId,
): Promise<DetachResult> {
  return invoke<DetachResult>(API_COMMANDS.detachAgent, {
    skillId,
    agentId,
    scope,
  });
}

/**
 * Attach one more agent to an already-installed managed skill. Rust re-checks
 * the managed/canonical condition, rejects universal/broken/occupied targets
 * and creates a symlink into the agent's skills directory. No CLI is spawned.
 */
export function attachAgent(
  skillId: string,
  agentId: string,
  scope: ScopeId,
): Promise<AttachResult> {
  return invoke<AttachResult>(API_COMMANDS.attachAgent, {
    skillId,
    agentId,
    scope,
  });
}

/** Persist a user-defined extra agent in the app-data `agents.json`. */
export function addExtraAgent(agent: ExtraAgentInput): Promise<ExtraAgent> {
  return invoke<ExtraAgent>(API_COMMANDS.addExtraAgent, { agent });
}

/**
 * Change or clear one extra agent's project skills directory. The backend
 * rejects the change while registered projects may still hold mounts.
 */
export function updateExtraAgent(
  agentId: string,
  projectSkillsDir: string | null,
): Promise<ExtraAgent> {
  return invoke<ExtraAgent>(API_COMMANDS.updateExtraAgent, {
    agentId,
    projectSkillsDir,
  });
}

/** Detach its skills first, then remove one user-defined extra agent. */
export function removeExtraAgent(
  agentId: string,
): Promise<RemoveExtraAgentResult> {
  return invoke<RemoveExtraAgentResult>(API_COMMANDS.removeExtraAgent, {
    agentId,
  });
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
  scope: ScopeId,
  commandPreviewId?: string,
): Promise<string> {
  const channel = new Channel<TaskEvent>();
  channel.onmessage = onEvent;
  return invoke<string>(API_COMMANDS.startUninstall, {
    skillId,
    commandPreviewId,
    scope,
    onEvent: channel,
  });
}

/** Open a fixed app-data directory; the frontend never supplies a path. */
export function revealAppData(kind: AppDataKind): Promise<void> {
  return invoke<void>(API_COMMANDS.revealAppData, { kind });
}

/** Normalize a rejected Tauri command into the shared `AppError` shape. */
export function asAppError(error: unknown): AppError | null {
  if (!error) return null;
  if (error instanceof Error) {
    return { message: error.message, retryable: false };
  }
  if (typeof error === "string") {
    return { message: error, retryable: false };
  }
  if (typeof error === "object") {
    const candidate = error as {
      code?: unknown;
      message?: unknown;
      detail?: unknown;
      hint?: unknown;
      retryable?: unknown;
    };
    if (typeof candidate.message === "string") {
      return {
        code: typeof candidate.code === "string" ? candidate.code : undefined,
        message: candidate.message,
        detail:
          typeof candidate.detail === "string" ? candidate.detail : undefined,
        hint: typeof candidate.hint === "string" ? candidate.hint : undefined,
        retryable:
          typeof candidate.retryable === "boolean"
            ? candidate.retryable
            : false,
      };
    }
  }
  return { message: "未知错误", retryable: false };
}

/** A client-side fallback with no invented backend code. */
export function fallbackError(message: string): AppError {
  return { message, retryable: false };
}

/** The lock-family errors drive the full-screen lock recovery path. */
export function isLockError(error: AppError | null | undefined): boolean {
  return error?.code?.startsWith("lock.") ?? false;
}

export function getDiscoveryState(): Promise<DiscoveryState> {
  return invoke<DiscoveryState>(API_COMMANDS.getDiscoveryState);
}
export function startDiscoveryScan(manual = false): Promise<DiscoveryProgress> {
  return invoke<DiscoveryProgress>(API_COMMANDS.startDiscoveryScan, { manual });
}
export function acknowledgeDiscoveries(ids?: DiscoveryId[]): Promise<void> {
  return invoke<void>(API_COMMANDS.acknowledgeDiscoveries, {
    ids: ids ?? null,
  });
}
export function setDiscoveryIgnored(
  id: DiscoveryId,
  ignored: boolean,
): Promise<void> {
  return invoke<void>(API_COMMANDS.setDiscoveryIgnored, { id, ignored });
}
export function invalidateDiscoverySource(source: string): Promise<void> {
  return invoke<void>(API_COMMANDS.invalidateDiscoverySource, { source });
}

export function previewBatch(
  scope: ScopeId,
  request: BatchRequest,
): Promise<BatchPreview> {
  return invoke<BatchPreview>(API_COMMANDS.previewBatch, { scope, request });
}

export function refreshBatchPreview(previewId: string): Promise<BatchPreview> {
  return invoke<BatchPreview>(API_COMMANDS.refreshBatchPreview, { previewId });
}

export function startBatch(
  scope: ScopeId,
  confirmation: BatchConfirmation,
  onEvent: (event: TaskEvent) => void,
): Promise<string> {
  const channel = new Channel<TaskEvent>();
  channel.onmessage = onEvent;
  return invoke<string>(API_COMMANDS.startBatch, {
    scope,
    confirmation,
    onEvent: channel,
  });
}
