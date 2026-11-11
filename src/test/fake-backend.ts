import type { PackListing, PackDetail, PackPreview, LocalPack, PackSnapshot } from "../lib/expert-packs";
import { expertPack, packSnapshot } from "./expert-pack-fixtures";
import type { MarketplaceCoordinate, MarketplaceListing, MarketplaceDetail, MarketplaceCli, MarketplacePreview, MarketplaceSort, MarketplaceCategory, MarketplaceApiKeyFilter, MarketplaceFiles } from "../lib/marketplace";
import { marketplaceAddress } from "../lib/marketplace";
import { marketplaceSkill, marketplaceCli, marketplaceCategories, marketplaceFiles, marketplaceFileContent } from "./marketplace-fixtures";
import type {
  StaleRecord,
  BatchRequest,
  BatchPreview,
  BatchConfirmation,
  BatchRow,
  BatchTarget,
} from "../lib/batch";
import type {
  DiscoveryId,
  DiscoveryState,
  DiscoveryProgress,
} from "../lib/discovery";
import { initialAppUpdate, type AppUpdateSnapshot } from "../lib/app-update";
import { mergeCommandRecord, type CliCommandRecord, type CliCommandRequest, type CliCommandPreview } from "../lib/cli-commands";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { Channel } from "@tauri-apps/api/core";
import type { ApiCommand } from "../lib/api";
import type {
  ActiveMutationInfo,
  AgentAttachment,
  AppDataKind,
  AppError,
  AttachResult,
  CancelAck,
  DetachResult,
  EnvReport,
  GitHubSettingsStatus,
  GitHubAuthTestResult,
  ExtraAgent,
  ExtraAgentInput,
  InstallRequest,
  InventoryAgent,
  InventorySkill,
  InventorySnapshot,
  PreviewSummary,
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
  TaskResult,
  UpdateCheckReport,
  UpdateCheckResponse,
  UpdateCheckState,
  AppDataReport,
  DataPreferences,
  AppDataSelection,
  AppDataPreview,
  AppDataCleanupResult,
} from "../lib/types";
import { healthyReport } from "./fixtures";
import {
  detailForSkill,
  makeDirListing,
  makeFileContent,
} from "./detail-fixtures";
import { emptySnapshot } from "./inventory-fixtures";
import { GLOBAL_SCOPE } from "../lib/types";

type Factory<T> = T | (() => T);
type MaybePromise<T> = T | Promise<T>;

export interface FakeCommandContract {
  list_expert_packs: { args: { keyword: string; page: number }; result: PackListing };
  get_expert_pack: { args: { slug: string }; result: PackDetail };
  list_local_packs: { args: { scope: ScopeId }; result: LocalPack[] };
  prepare_pack_install: { args: { scope: ScopeId; slug: string; agents: string[] }; result: PackPreview };
  start_pack_install: { args: { scope: ScopeId; previewId: string; onEvent: Channel<TaskEvent> }; result: string };
  forget_expert_pack: { args: { scope: ScopeId; slug: string }; result: void };
  list_marketplace_categories: { args: Record<string, never>; result: MarketplaceCategory[] };
  list_marketplace_skills: { args: { keyword: string; page: number; sort: MarketplaceSort; category: string; apiKey: MarketplaceApiKeyFilter }; result: MarketplaceListing };
  get_marketplace_skill: { args: { coordinate: MarketplaceCoordinate }; result: MarketplaceDetail };
  list_marketplace_files: { args: { coordinate: MarketplaceCoordinate; version: string }; result: MarketplaceFiles };
  read_marketplace_file: { args: { coordinate: MarketplaceCoordinate; version: string; path: string }; result: SkillFileContent };
  get_marketplace_cli: { args: Record<string, never>; result: MarketplaceCli };
  prepare_marketplace_install: { args: { scope: ScopeId; coordinate: MarketplaceCoordinate; agents: string[] }; result: MarketplacePreview };
  start_marketplace_install: { args: { scope: ScopeId; previewId: string; onEvent: Channel<TaskEvent> }; result: string };
  list_stale_records: { args: { scope: ScopeId }; result: StaleRecord[] };
  get_cli_commands: { args: Record<string, never>; result: CliCommandRecord[] };
  preview_cli_commands: { args: { scope: ScopeId; request: CliCommandRequest }; result: CliCommandPreview };
  preview_batch: {
    args: { scope: ScopeId; request: BatchRequest };
    result: BatchPreview;
  };
  refresh_batch_preview: { args: { previewId: string }; result: BatchPreview };
  start_batch: {
    args: {
      scope: ScopeId;
      confirmation: BatchConfirmation;
      onEvent: Channel<TaskEvent>;
    };
    result: string;
  };
  get_app_update_state: {
    args: Record<string, never>;
    result: AppUpdateSnapshot;
  };
  check_app_update: { args: Record<string, never>; result: AppUpdateSnapshot };
  start_app_update: { args: Record<string, never>; result: AppUpdateSnapshot };
  get_discovery_state: { args: Record<string, never>; result: DiscoveryState };
  start_discovery_scan: {
    args: { manual?: boolean };
    result: DiscoveryProgress;
  };
  acknowledge_discoveries: {
    args: { ids: DiscoveryId[] | null };
    result: void;
  };
  set_discovery_ignored: {
    args: { id: DiscoveryId; ignored: boolean };
    result: void;
  };
  invalidate_discovery_source: { args: { source: string }; result: void };
  get_github_settings: {
    args: Record<string, never>;
    result: GitHubSettingsStatus;
  };
  save_github_token: { args: { token: string }; result: GitHubSettingsStatus };
  clear_github_token: {
    args: Record<string, never>;
    result: GitHubSettingsStatus;
  };
  test_github_auth: {
    args: Record<string, never>;
    result: GitHubAuthTestResult;
  };
  get_environment: { args: { refresh?: boolean }; result: EnvReport };
  list_projects: { args: Record<string, never>; result: ProjectList };
  add_project: { args: { root: string }; result: ProjectEntry };
  relocate_project: {
    args: { id: string; root: string };
    result: RelocateProjectResult;
  };
  forget_project: { args: { id: string }; result: ForgetProjectResult };
  set_active_scope: { args: { scope: ScopeId }; result: void };
  get_inventory: {
    args: { scope: ScopeId };
    result: InventorySnapshot;
  };
  get_skill_detail: {
    args: { scope: ScopeId; skillId: string };
    result: SkillDetail;
  };
  open_skill_folder: {
    args: { scope: ScopeId; skillId: string };
    result: void;
  };
  list_skill_dir: {
    args: { scope: ScopeId; skillId: string; path?: string };
    result: SkillDirListing;
  };
  read_skill_file: {
    args: { scope: ScopeId; skillId: string; path: string };
    result: SkillFileContent;
  };
  open_skill_file: {
    args: { scope: ScopeId; skillId: string; path: string };
    result: void;
  };
  open_external_url: { args: { url: string }; result: void };
  start_preview: {
    args: { source: string; scope: ScopeId; onEvent: Channel<TaskEvent> };
    result: string;
  };
  cancel_task: { args: { taskId: string }; result: CancelAck };
  get_active_mutation: {
    args: Record<string, never>;
    result: ActiveMutationInfo | null;
  };
  dismiss_interrupted_mutation: { args: Record<string, never>; result: void };
  quit_app: { args: Record<string, never>; result: void };
  start_install: {
    args: {
      request: InstallRequest;
      commandPreviewId?: string;
      scope: ScopeId;
      onEvent: Channel<TaskEvent>;
    };
    result: string;
  };
  check_updates: {
    args: { skillIds: string[] | null; scope: ScopeId; checkRevision?: string };
    result: UpdateCheckResponse;
  };
  get_update_check_state: {
    args: { scope: ScopeId };
    result: UpdateCheckState | null;
  };
  get_update_check_states: {
    args: Record<string, never>;
    result: Record<string, UpdateCheckState>;
  };
  start_update: {
    args: { skillIds: string[]; scope: ScopeId; onEvent: Channel<TaskEvent>; commandPreviewId?: string };
    result: string;
  };
  detach_agent: {
    args: { skillId: string; agentId: string; scope: ScopeId };
    result: DetachResult;
  };
  attach_agent: {
    args: { skillId: string; agentId: string; scope: ScopeId };
    result: AttachResult;
  };
  add_extra_agent: { args: { agent: ExtraAgentInput }; result: ExtraAgent };
  update_extra_agent: {
    args: { agentId: string; projectSkillsDir: string | null };
    result: ExtraAgent;
  };
  remove_extra_agent: {
    args: { agentId: string };
    result: RemoveExtraAgentResult;
  };
  start_uninstall: {
    args: { skillId: string; scope: ScopeId; onEvent: Channel<TaskEvent>; commandPreviewId?: string };
    result: string;
  };
  reveal_app_data: { args: { kind: AppDataKind }; result: void };
  get_app_data: { args: Record<string, never>; result: AppDataReport };
  save_data_preferences: { args: { preferences: DataPreferences }; result: DataPreferences };
  preview_app_data_cleanup: { args: { selection: AppDataSelection }; result: AppDataPreview };
  clean_app_data: { args: { previewId: string }; result: AppDataCleanupResult };
  reveal_backup: { args: { id: string }; result: void };
}

export type FakeHandler<K extends ApiCommand> = (
  args: FakeCommandContract[K]["args"],
  backend: FakeBackend,
) => MaybePromise<FakeCommandContract[K]["result"]>;

export type FakeHandlers = { [K in ApiCommand]: FakeHandler<K> };

export type FakeCall = {
  [K in ApiCommand]: {
    command: K;
    payload: FakeCommandContract[K]["args"];
  };
}[ApiCommand];

export interface FakeBackendData {
  staleRecords?: Partial<Record<ScopeId, StaleRecord[]>>;
  appUpdate: AppUpdateSnapshot;
  discovery: DiscoveryState;
  githubSettings: GitHubSettingsStatus;
  appData: AppDataReport;
  environment: Factory<EnvReport>;
  /** Initial in-memory registry; list/add/relocate/forget mutate their own copy. */
  projects: Factory<ProjectList>;
  /** Global snapshot, also the fallback for scopes without an entry. */
  snapshot: InventorySnapshot;
  /** Per-scope inventories; projects default to an empty snapshot. */
  snapshots?: Partial<Record<ScopeId, Factory<InventorySnapshot>>>;
  detail: (
    skillId: string,
    snapshot: InventorySnapshot,
    scope?: ScopeId,
  ) => SkillDetail;
  listDir: (skillId: string, path: string) => SkillDirListing;
  readFile: (skillId: string, path: string) => SkillFileContent;
  updateReport: Factory<UpdateCheckReport>;
  /** Per-scope check reports; projects default to an empty report. */
  updateReports?: Partial<Record<ScopeId, Factory<UpdateCheckReport>>>;
  updateCheckState: Factory<UpdateCheckState | null>;
  /** Per-scope persisted check state; projects default to `null`. */
  updateCheckStates?: Partial<
    Record<ScopeId, Factory<UpdateCheckState | null>>
  >;
  activeMutation: Factory<ActiveMutationInfo | null>;
  preview: PreviewSummary;
  /** Per-scope preview summaries; projects default to the global preview. */
  previews?: Partial<Record<ScopeId, Factory<PreviewSummary>>>;
}

export interface FakeBackendOptions {
  data?: Partial<FakeBackendData>;
  commands?: Partial<FakeHandlers>;
  /** Preview: automatically drive start_* to completion. Tests default off. */
  autoTasks?: boolean | { delayMs?: number };
}

export interface FakeTask {
  taskId: string;
  command: ApiCommand;
  channel: Channel<TaskEvent>;
  emit(event: TaskEvent): void;
}

export interface FakeBackend {
  readonly calls: readonly FakeCall[];
  readonly tasks: readonly FakeTask[];
  readonly lastTask: FakeTask | null;
  count(command: ApiCommand): number;
  callsOf<K extends ApiCommand>(
    command: K,
  ): Array<{ command: K; payload: FakeCommandContract[K]["args"] }>;
  update(data: Partial<FakeBackendData>): void;
  /** Run the default handler, for overrides that fall through after an error. */
  dispatch<K extends ApiCommand>(
    command: K,
    args: FakeCommandContract[K]["args"],
  ): MaybePromise<FakeCommandContract[K]["result"]>;
  install(options?: { shouldMockEvents?: boolean }): FakeBackend;
  reset(): void;
  supports(command: ApiCommand): boolean;
}

const installedBackends = new Set<FakeBackend>();
export function resetFakeBackends() {
  for (const backend of installedBackends) backend.reset();
  installedBackends.clear();
}

export const FAKE_LOG_PATH =
  "~/Library/Application Support/com.sunven.skills-box/logs/operations.jsonl";
const FAKE_BACKUP = "/tmp/backups/fake";

export function emptyUpdateReport(
  overrides: Partial<UpdateCheckReport> = {},
): UpdateCheckReport {
  return {
    skills: [],
    sources: [],
    checkedAtEpochMs: 0,
    lastSuccessAtEpochMs: null,
    skippedSourceCount: 0,
    cachedSourceCount: 0,
    warnings: [],
    ...overrides,
  };
}

function factoryValue<T>(source: Factory<T>): T {
  return typeof source === "function" ? (source as () => T)() : source;
}

function notFound(kind: "skill" | "agent", id: string): AppError {
  return {
    code: kind === "skill" ? "skill.not_found" : "agent.not_found",
    message: `${kind === "skill" ? "skill" : "agent"}「${id}」不存在。`,
    retryable: false,
  };
}

function taskResult(
  kind: "preview" | "install" | "update" | "uninstall",
  names: string[],
  preview: PreviewSummary,
  scope: ScopeId = GLOBAL_SCOPE,
): TaskResult {
  const status =
    kind === "install"
      ? "installed"
      : kind === "update"
        ? "updated"
        : "removed";
  return {
    action: kind,
    scope,
    overall: "success",
    skills: kind === "preview" ? [] : names.map((name) => ({ name, status })),
    warnings: [],
    backupPath: FAKE_BACKUP,
    logPath: FAKE_LOG_PATH,
    startedAt: Date.now() - 4_200,
    finishedAtEpochMs: Date.now(),
    preview: kind === "preview" ? preview : undefined,
  };
}

export function createFakeBackend(
  options: FakeBackendOptions = {},
): FakeBackend {
  const defaultData: FakeBackendData = {
    appData: {
      directory: "~/Library/Application Support/com.sunven.skills-box",
      preferences: { retainBackups: true, operationLogging: true },
      totalBytes: 0, cleanableBytes: 0, backups: [], warnings: [], scannedAtEpochMs: Date.now(),
      categories: (["backups", "logs", "cache", "preserved", "other"] as const).map((category) => ({ category, bytes: 0, cleanableBytes: 0, count: 0 })),
    },
    appUpdate: initialAppUpdate(),
    discovery: {
      sources: [],
      skills: [],
      unreadCount: 0,
      progress: { runId: 0, running: false, total: 0, completed: 0 },
      warnings: [],
      error: null,
    },
    githubSettings: {
      hasSavedToken: false,
      hasEnvironmentToken: false,
      effectiveSource: "anonymous",
    },
    environment: healthyReport(),
    projects: {
      projects: [],
      lastActiveScope: GLOBAL_SCOPE,
      warnings: [],
    },
    snapshot: emptySnapshot(),
    detail: (skillId, snapshot) => {
      const skill = snapshot.skills.find((entry) => entry.name === skillId);
      if (!skill) throw notFound("skill", skillId);
      return detailForSkill(skill);
    },
    listDir: makeDirListing,
    readFile: makeFileContent,
    updateReport: emptyUpdateReport(),
    updateCheckState: null,
    activeMutation: null,
    preview: {
      source: "acme/skills",
      skills: [],
      parseFailed: false,
      noSkills: false,
    },
  };
  const initialData: FakeBackendData = { ...defaultData, ...options.data };
  let data: FakeBackendData = { ...initialData };
  const marketPreviews = new Map<string, { scope: ScopeId; preview: MarketplacePreview; agents: string[] }>();
  const marketTasks = new Map<string, { scope: ScopeId; preview: MarketplacePreview; agents: string[] }>();
  const dataPreviews = new Map<string, AppDataPreview>();
  let projectList: ProjectList = factoryValue(initialData.projects);
  const initialProjects = projectList;

  const calls: FakeCall[] = [];
  const tasks: FakeTask[] = [];
  const channelIndexes = new Map<number, number>();
  const timers = new Set<number>();
  const autoDelay =
    typeof options.autoTasks === "object"
      ? (options.autoTasks.delayMs ?? 80)
      : 80;
  const autoTasks = Boolean(options.autoTasks);
  let taskCounter = 0;
  let cliRecords: CliCommandRecord[] = [];
  const savedPacks = new Map<ScopeId, Map<string, PackSnapshot>>();
  const packPreviews = new Map<string, { scope: ScopeId; preview: PackPreview }>();
  const packTasks = new Map<string, { scope: ScopeId; preview: PackPreview }>();
  function localPacks(scope: ScopeId): LocalPack[] {
    const inventory = snapshotFor(scope);
    return [...(savedPacks.get(scope)?.values() ?? [])].map((saved) => {
      const skills = [ ...saved.members.map((m) => m.name), saved.workflowName ].map((name) => {
        const skill = inventory.skills.find((s) => s.name === name);
        const attachedAgents = skill?.attachments.map((a) => a.agentId) ?? [];
        const missingAgents = saved.agents.filter((id) => !attachedAgents.includes(id));
        const original = saved.members.find((m) => m.name === name);
        return { name, present: !!skill, missingAgents, attachedAgents, version: skill?.sourceVersion ?? null, versionChanged: !!original && !!skill?.sourceVersion && skill.sourceVersion !== original.version, modified: skill?.localModification === "modified", conflict: false };
      });
      return { snapshot: saved, complete: skills.every((s) => s.present && !s.missingAgents.length), skills };
    });
  }
  const taskCommands = new Map<string, { scope: ScopeId; commands: string[] }>();
  const publishCommand = (record: CliCommandRecord) => {
    cliRecords = mergeCommandRecord(cliRecords, record);
    window.dispatchEvent(new CustomEvent("cli-command", { detail: record }));
  };
  const quoteCommand = (value: string) => /^[a-zA-Z0-9_./:@%+=,-]+$/.test(value) ? value : `'${value.replace(/'/g, "'\\''")}'`;
  const commandFor = (scope: ScopeId, args: string[]) => {
    const root = projectList.projects.find((project) => projectScope(project.id) === scope)?.root ?? "/Users/sunven/Library/Application Support/com.sunven.skills-box/cli-cwd";
    const npx = factoryValue(data.environment).npx.path ?? "/usr/local/bin/npx";
    return `cd ${quoteCommand(root)} && ${[npx, "-y", "skills@latest", ...args].map(quoteCommand).join(" ")}`;
  };
  const cliPreview = (scope: ScopeId, request: CliCommandRequest): CliCommandPreview => {
    const snapshot = snapshotFor(scope);
    const globalArgs = scope === GLOBAL_SCOPE ? ["-g"] : [];
    const installArgs = (source: string, names: string[], agents: string[]) => ["add", source, ...globalArgs, ...names.flatMap((name) => ["-s", name]), ...agents.flatMap((id) => ["-a", id]), "-y", "--json"];
    let entries: CliCommandPreview["entries"];
    const marketplaceVersions: Record<string, string> = {};
    if (request.action === "preview") {
      const source = request.source.trim();
      entries = [{ name: source, command: commandFor(scope, ["add", source, "-l", ...globalArgs]), reason: null }];
    } else if (request.action === "install") {
      const agents = request.request.agents.filter((id) => { const agent = snapshot.agents.find((agent) => agent.id === id); return id !== "universal" && id !== "codex" && (agent === undefined || agent.origin === "catalog") && !agent?.universal; });
      entries = [{ name: request.request.skills.join("、"), command: commandFor(scope, installArgs(request.request.source, request.request.skills, [...agents, "universal"])), reason: null }];
    } else if (request.action === "update") {
      entries = request.skills.map((name) => {
        const skill = snapshot.skills.find((skill) => skill.id === name || skill.name === name);
        const update = reportFor(scope).skills.find((skill) => skill.name === name);
        if (skill?.sourceType === "skillhub") {
          const [namespace, slug] = (skill.source ?? "").replace(/^@/, "").split("/");
          const version = update?.remoteHash ?? marketplaceSkill.version;
          marketplaceVersions[skill.name] = version;
          return { name: skill.name, command: `skillhub --skip-self-upgrade install ${slug}@${version} --namespace ${namespace} --dir /tmp/skills-box-market-update-${skill.name} --json`, reason: null };
        }
        const source = `${update?.source ?? skill?.source ?? "acme/skills"}${update?.ref ? `#${update.ref}` : ""}`;
        const agents = [...new Set((skill?.attachments ?? []).filter((a) => { const agent = snapshot.agents.find((agent) => agent.id === a.agentId); return !agent?.universal && (agent === undefined || agent.origin === "catalog") && (a.mode === "link" || a.mode === "copy"); }).map((a) => a.agentId))].sort();
        return { name, command: commandFor(scope, installArgs(source, [skill?.name ?? name], [...agents, "universal"])), reason: null };
      });
    } else {
      const skill = snapshot.skills.find((skill) => skill.id === request.skill || skill.name === request.skill);
      const name = skill?.name ?? request.skill.split("/").slice(-1)[0]!;
      entries = [{ name, command: skill?.management === "unmanaged" || ["skillhub", "skillhub-pack"].includes(skill?.sourceType ?? "") ? null : commandFor(scope, ["remove", ...globalArgs, "-s", name, "-y"]), reason: null }];
    }
    return { id: JSON.stringify([scope, request.action, entries]), entries, ...(Object.keys(marketplaceVersions).length ? { marketplaceVersions } : {}) };
  };
  const prepareTaskCommands = (scope: ScopeId, request: CliCommandRequest, expected?: string) => {
    const preview = cliPreview(scope, request);
    if (expected !== undefined && expected !== preview.id) throw { code: "cli.command_changed", message: "执行命令已变化，请查看刷新后的命令并再次执行。", retryable: true } satisfies AppError;
    return { scope, commands: preview.entries.flatMap((entry) => entry.command ? [entry.command] : []) };
  };
  const installSources = new Map<string, string>();
  const refreshedDiscoverySources = new Set<string>();
  let batchPreviewCounter = 0;
  const pendingCopyProjects = new Map<string, ProjectEntry>();
  const batchPreviews = new Map<
    string,
    { scope: ScopeId; request: BatchRequest; preview: BatchPreview }
  >();
  const batchRuns = new Map<
    string,
    { stop: boolean; preview: BatchPreview; startedAt: number }
  >();

  const runCallback = (id: number, payload: unknown) => {
    const internals = (
      window as unknown as {
        __TAURI_INTERNALS__: {
          runCallback: (id: number, data: unknown) => void;
        };
      }
    ).__TAURI_INTERNALS__;
    internals.runCallback(id, payload);
  };

  const makeTask = (
    command: ApiCommand,
    channel: Channel<TaskEvent>,
  ): FakeTask => {
    const task: FakeTask = {
      taskId: `task-${++taskCounter}`,
      command,
      channel,
      emit(event) {
        if (!tasks.includes(task)) return;
        const invocation = taskCommands.get(task.taskId);
        if (invocation && (event.phase === "running_cli" || event.phase === "cloning")) {
          invocation.commands.forEach((command, index) => {
            if (index !== (event.completed ?? 0)) return;
            const id = `${task.taskId}:${index}`;
            if (!cliRecords.some((record) => record.id === id)) publishCommand({ id, command, context: { origin: "task", scope: invocation.scope, taskId: task.taskId, source: null }, status: "running", startedAt: Date.now() });
          });
        }
        if (["completed", "failed", "cancelled"].includes(event.phase)) {
          cliRecords.filter((record) => record.context.taskId === task.taskId && record.status === "running").forEach((record) => publishCommand({ ...record, status: event.phase === "completed" ? "succeeded" : event.phase === "cancelled" ? "cancelled" : "failed" }));
        }
        if (
          event.phase === "completed" ||
          event.phase === "failed" ||
          event.phase === "cancelled"
        )
          batchRuns.delete(task.taskId);
        const pack = packTasks.get(task.taskId);
        if (pack && event.result) {
          const saved = pack.preview.snapshot;
          const installed = event.result.skills.filter((s) => s.status === "installed").map((s) => s.name);
          installInto(pack.scope, installed);
          const inventory = snapshotFor(pack.scope);
          setSnapshot({ ...inventory, skills: inventory.skills.map((s) => {
            if (!installed.includes(s.name)) return s;
            const member = saved.members.find((m) => m.name === s.name);
            return { ...s, sourceType: member ? "skillhub" : "skillhub-pack", source: member ? marketplaceAddress(member.coordinate) : saved.slug, sourceVersion: member?.version ?? null,
              attachments: saved.agents.map((id) => ({ agentId: id, agentName: inventory.installTargets.find((a) => a.id === id)?.displayName ?? id, mode: "link" as const, path: `~/.${id}/skills/${s.name}`, detachable: true })) };
          }) }, pack.scope);
        }
        const market = marketTasks.get(task.taskId);
        if (market && event.result?.overall === "success") {
          const detail = market.preview.detail;
          installInto(market.scope, [detail.originalName!]);
          const snapshot = snapshotFor(market.scope);
          setSnapshot({ ...snapshot, skills: snapshot.skills.map((s) => s.name === detail.originalName ? { ...s, source: marketplaceAddress(detail.coordinate), sourceType: "skillhub", sourceVersion: detail.version, description: detail.description, attachments: market.agents.map((id) => ({ agentId: id, agentName: snapshot.installTargets.find((a) => a.id === id)?.displayName ?? id, mode: "link" as const, path: `~/.${id}/skills/${s.name}`, detachable: true })) } : s) }, market.scope);
        }
        if (event.result?.action === "install") {
          const source = installSources.get(task.taskId);
          const installed = new Set(
            event.result.skills
              .filter((s) => s.status === "installed")
              .map((s) => s.name.toLowerCase()),
          );
          const resultScope = event.result.scope ?? GLOBAL_SCOPE;
          updateDiscovery((s) =>
            s.id.source === source && installed.has(s.id.name)
              ? {
                  ...s,
                  unread: false,
                  installations: s.installations.map((i) =>
                    i.scope === resultScope ? { ...i, status: "installed" } : i,
                  ),
                }
              : s,
          );
        }
        const index = channelIndexes.get(channel.id) ?? 0;
        channelIndexes.set(channel.id, index + 1);
        runCallback(channel.id, { index, message: event });
      },
    };
    tasks.push(task);
    return task;
  };

  const scheduleAutoTask = (
    task: FakeTask,
    kind: "preview" | "install" | "update" | "uninstall",
    names: string[],
    scope: ScopeId = GLOBAL_SCOPE,
  ) => {
    if (!autoTasks) return;
    const total = kind === "preview" ? 1 : Math.max(1, names.length);
    timers.add(
      window.setTimeout(() => {
        task.emit({
          taskId: task.taskId,
          phase: kind === "preview" ? "cloning" : "running_cli",
          message: kind === "preview" ? "正在读取仓库…" : "正在准备写入…",
          completed: 0,
          total,
        });
      }, autoDelay),
    );
    timers.add(
      window.setTimeout(() => {
        if (kind === "update") {
          names.slice(1).forEach((name, index) => task.emit({ taskId: task.taskId, phase: "running_cli", message: `正在更新 ${name}`, skill: name, completed: index + 1, total }));
        }
        if (kind === "install") installInto(scope, names);
        if (kind === "update") updateInto(scope, names);
        if (kind === "uninstall") uninstallFrom(scope, names);
        const result = taskResult(kind, names, previewFor(scope), scope);
        const message =
          kind === "preview"
            ? `找到 ${previewFor(scope).skills.length} 个 skill`
            : kind === "uninstall"
              ? `已卸载 ${names.length} 个 skill`
              : `${names.length} 个 skill 已完成`;
        task.emit({
          taskId: task.taskId,
          phase: "completed",
          message,
          completed: total,
          total,
          result,
        });
      }, autoDelay * 4),
    );
  };

  const snapshotFor = (scope: ScopeId = GLOBAL_SCOPE): InventorySnapshot => {
    const scoped = data.snapshots?.[scope];
    if (scoped) return factoryValue(scoped);
    if (scope === GLOBAL_SCOPE) return data.snapshot;
    return emptySnapshot();
  };

  const setSnapshot = (
    snapshot: InventorySnapshot,
    scope: ScopeId = GLOBAL_SCOPE,
  ) => {
    if (scope === GLOBAL_SCOPE) {
      data = { ...data, snapshot };
      return;
    }
    data = {
      ...data,
      snapshots: { ...data.snapshots, [scope]: snapshot },
    };
  };

  const previewFor = (scope: ScopeId = GLOBAL_SCOPE): PreviewSummary => {
    const scoped = data.previews?.[scope];
    if (scoped) return factoryValue(scoped);
    return factoryValue(data.preview);
  };

  const canonicalRootFor = (scope: ScopeId = GLOBAL_SCOPE): string => {
    if (scope === GLOBAL_SCOPE) return "~/.agents/skills";
    const id = scope.slice("project:".length);
    const root = projectList.projects.find((entry) => entry.id === id)?.root;
    return root
      ? `${root.replace(/\/+$/, "")}/.agents/skills`
      : "~/.agents/skills";
  };

  /** Apply a completed install to the scope's own inventory snapshot. */
  const installInto = (scope: ScopeId, names: string[]) => {
    if (names.length === 0) return;
    const snapshot = snapshotFor(scope);
    const source = previewFor(scope).source;
    const canonicalRoot = canonicalRootFor(scope);
    const existing = new Set(snapshot.skills.map((skill) => skill.name));
    const added: InventorySkill[] = names
      .filter((name) => !existing.has(name))
      .map((name) => ({
        id: scope === GLOBAL_SCOPE ? name : `${scope}/${name}`,
        name,
        description: null,
        source,
        management: "managed",
        updatedAt: null,
        canonicalPath: `${canonicalRoot}/${name}`,
        attachments: [],
        localModification: "clean",
        warnings: [],
      }));
    if (added.length === 0) return;
    setSnapshot({ ...snapshot, skills: [...snapshot.skills, ...added] }, scope);
  };

  /** Apply a completed update to the scope's own inventory snapshot. */
  const updateInto = (scope: ScopeId, names: string[]) => {
    if (names.length === 0) return;
    const snapshot = snapshotFor(scope);
    const selected = new Set(names);
    const updatedAt = new Date().toISOString();
    const skills = snapshot.skills.map((skill) =>
      selected.has(skill.name) || selected.has(skill.id)
        ? {
            ...skill,
            management: "managed" as const,
            localModification: "clean" as const,
            sourceVersion: skill.sourceType === "skillhub" ? reportFor(scope).skills.find((row) => row.name === skill.name)?.remoteHash ?? skill.sourceVersion : skill.sourceVersion,
            updatedAt,
          }
        : skill,
    );
    setSnapshot({ ...snapshot, skills }, scope);
  };

  /** Remove uninstalled rows from the scope's snapshot only. */
  const uninstallFrom = (scope: ScopeId, ids: string[]) => {
    if (ids.length === 0) return;
    const snapshot = snapshotFor(scope);
    const selected = new Set(ids);
    const skills = snapshot.skills.filter(
      (skill) => !selected.has(skill.id) && !selected.has(skill.name),
    );
    if (skills.length === snapshot.skills.length) return;
    setSnapshot({ ...snapshot, skills }, scope);
  };

  const findSkill = (
    skillId: string,
    scope: ScopeId = GLOBAL_SCOPE,
  ): InventorySkill => {
    const snapshot = snapshotFor(scope);
    const skill = snapshot.skills.find(
      (entry) => entry.id === skillId || entry.name === skillId,
    );
    if (!skill) throw notFound("skill", skillId);
    return skill;
  };

  const findAgent = (
    agentId: string,
    scope: ScopeId = GLOBAL_SCOPE,
  ): InventoryAgent | undefined =>
    snapshotFor(scope).agents.find((agent) => agent.id === agentId);

  const nativeAttach = (
    skillId: string,
    agentId: string,
    scope: ScopeId = GLOBAL_SCOPE,
  ): AttachResult => {
    const snapshot = snapshotFor(scope);
    const skill = findSkill(skillId, scope);
    const agent = findAgent(agentId, scope);
    const agentName = agent?.displayName ?? agentId;
    const skillsDir = agent?.globalSkillsDir ?? `~/.${agentId}/skills`;
    const attachment: AgentAttachment = {
      agentId,
      agentName,
      path: `${skillsDir}/${skill.name}`,
      mode: "link",
      detachable: true,
    };
    setSnapshot(
      {
        ...snapshot,
        skills: snapshot.skills.map((entry) =>
          entry.id === skill.id
            ? {
                ...entry,
                attachments: entry.attachments.some(
                  (current) => current.agentId === agentId,
                )
                  ? entry.attachments
                  : [...entry.attachments, attachment],
              }
            : entry,
        ),
      },
      scope,
    );
    return {
      skillId,
      agentId,
      agentName,
      attachmentPath: attachment.path,
      canonicalPath: skill.canonicalPath ?? `~/.agents/skills/${skill.name}`,
      logPath: FAKE_LOG_PATH,
    };
  };

  const nativeDetach = (
    skillId: string,
    agentId: string,
    scope: ScopeId = GLOBAL_SCOPE,
  ): DetachResult => {
    const snapshot = snapshotFor(scope);
    const skill = findSkill(skillId, scope);
    const attachment = skill.attachments.find(
      (entry) => entry.agentId === agentId,
    );
    if (!attachment) throw notFound("agent", agentId);
    const remaining = skill.attachments.filter(
      (entry) => entry.agentId !== agentId,
    );
    setSnapshot(
      {
        ...snapshot,
        skills: snapshot.skills.map((entry) =>
          entry.id === skill.id ? { ...entry, attachments: remaining } : entry,
        ),
      },
      scope,
    );
    return {
      skillId,
      agentId,
      agentName: attachment.agentName,
      mode: attachment.mode,
      attachmentPath: attachment.path,
      canonicalPath: skill.canonicalPath ?? `~/.agents/skills/${skill.name}`,
      backupPath:
        attachment.mode === "copy"
          ? `${FAKE_BACKUP}/1-detach-${skill.name}`
          : undefined,
      logPath: FAKE_LOG_PATH,
      remainingAgents: remaining.map((entry) => entry.agentName),
      unattached: remaining.length === 0,
    };
  };

  const addExtra = (input: ExtraAgentInput): ExtraAgent => {
    if (findAgent(input.id)) {
      throw {
        code: "agent.exists",
        message: `agent「${input.id}」已存在。`,
        retryable: false,
      } satisfies AppError;
    }
    const projectSkillsDir = input.projectSkillsDir?.trim() || null;
    const agent: InventoryAgent = {
      id: input.id,
      displayName: input.displayName,
      universal: false,
      shared: false,
      globalSkillsDir: input.skillsDir,
      origin: "user",
      detected: true,
      projectSkillsDir,
    };
    setSnapshot({
      ...data.snapshot,
      agents: [...data.snapshot.agents, agent],
    });
    return { ...input, projectSkillsDir, builtin: false };
  };

  const projectScopeOf = (id: string): ScopeId => `project:${id}`;

  const extraProjectBlockers = (agentId: string): string[] => {
    const blockers: string[] = [];
    for (const entry of projectList.projects) {
      if (!entry.available) {
        blockers.push(
          `${entry.displayName}（${entry.root}）：项目不可用，无法确认挂载`,
        );
        continue;
      }
      const count = snapshotFor(projectScopeOf(entry.id)).skills.filter(
        (skill) =>
          skill.attachments.some(
            (attachment) => attachment.agentId === agentId,
          ),
      ).length;
      if (count > 0) {
        blockers.push(`${entry.displayName}：仍有 ${count} 个挂载`);
      }
    }
    return blockers;
  };

  const updateExtra = (
    agentId: string,
    projectSkillsDir: string | null,
  ): ExtraAgent => {
    const agent = findAgent(agentId);
    if (!agent) throw notFound("agent", agentId);
    if (agent.origin === "builtin") {
      throw {
        code: "extra.builtin",
        message: `内置额外 agent「${agent.displayName}」不可修改。`,
        retryable: false,
      } satisfies AppError;
    }
    const next = projectSkillsDir?.trim() || null;
    if ((agent.projectSkillsDir ?? null) !== next) {
      const blockers = extraProjectBlockers(agentId);
      if (blockers.length > 0) {
        throw {
          code: "extra.project_blocked",
          message: "无法修改额外 agent 的项目目录：项目可能仍保留挂载。",
          detail: blockers.join("\n"),
          retryable: false,
        } satisfies AppError;
      }
    }
    const apply = (snapshot: InventorySnapshot): InventorySnapshot => ({
      ...snapshot,
      agents: snapshot.agents.map((entry) =>
        entry.id === agentId ? { ...entry, projectSkillsDir: next } : entry,
      ),
      installTargets: snapshot.installTargets.map((entry) =>
        entry.id === agentId ? { ...entry, projectSkillsDir: next } : entry,
      ),
    });
    setSnapshot(apply(snapshotFor(GLOBAL_SCOPE)));
    for (const entry of projectList.projects) {
      const scope = projectScopeOf(entry.id);
      setSnapshot(apply(snapshotFor(scope)), scope);
    }
    return {
      id: agent.id,
      displayName: agent.displayName,
      skillsDir: agent.globalSkillsDir ?? "",
      projectSkillsDir: next,
      builtin: false,
    };
  };

  const removeExtra = (agentId: string): RemoveExtraAgentResult => {
    const agent = findAgent(agentId);
    if (!agent) throw notFound("agent", agentId);
    const unavailable = projectList.projects
      .filter((entry) => !entry.available)
      .map(
        (entry) =>
          `${entry.displayName}（${entry.root}）：${
            entry.unavailableReason ?? "项目不可用"
          }`,
      );
    if (unavailable.length > 0) {
      throw {
        code: "extra.project_blocked",
        message: "无法删除额外 agent：仍有项目不可用。",
        detail: unavailable.join("\n"),
        retryable: false,
      } satisfies AppError;
    }
    const detached = data.snapshot.skills
      .filter((entry) =>
        entry.attachments.some((attachment) => attachment.agentId === agentId),
      )
      .map((entry) => entry.name);
    const stripAttachments = (
      snapshot: InventorySnapshot,
    ): InventorySnapshot => ({
      ...snapshot,
      agents: snapshot.agents.filter((entry) => entry.id !== agentId),
      installTargets: snapshot.installTargets.filter(
        (entry) => entry.id !== agentId,
      ),
      skills: snapshot.skills.map((entry) => ({
        ...entry,
        attachments: entry.attachments.filter(
          (attachment) => attachment.agentId !== agentId,
        ),
      })),
    });
    setSnapshot(stripAttachments(data.snapshot));
    const projects = projectList.projects.map((entry) => {
      const scope = projectScopeOf(entry.id);
      const snapshot = snapshotFor(scope);
      const detachedIds = snapshot.skills
        .filter((skill) =>
          skill.attachments.some(
            (attachment) => attachment.agentId === agentId,
          ),
        )
        .map((skill) => skill.id);
      setSnapshot(stripAttachments(snapshot), scope);
      return {
        projectId: entry.id,
        projectName: entry.displayName,
        detached: detachedIds,
      };
    });
    return {
      id: agent.id,
      displayName: agent.displayName,
      detached,
      projects,
    };
  };

  let projectCounter = 0;
  let revisionCounter = 0;
  const fakeProject = (root: string, id?: string): ProjectEntry => {
    const now = Date.now();
    return {
      id: id ?? `project-${++projectCounter}`,
      root,
      displayName: root.replace(/\/+$/, "").split("/").pop() || root,
      addedAt: now,
      lastOpenedAt: now,
      checkRevision: `revision-${++revisionCounter}`,
      available: true,
    };
  };

  const projectScope = (id: string): ScopeId => `project:${id}`;

  const sortProjects = (projects: ProjectEntry[]): ProjectEntry[] =>
    [...projects].sort(
      (left, right) =>
        right.lastOpenedAt - left.lastOpenedAt ||
        right.addedAt - left.addedAt ||
        left.id.localeCompare(right.id),
    );

  const resolveActive = (scope: ScopeId): ScopeId => {
    if (scope === GLOBAL_SCOPE) return GLOBAL_SCOPE;
    const id = scope.slice("project:".length);
    return projectList.projects.some(
      (entry) => entry.id === id && entry.available,
    )
      ? scope
      : GLOBAL_SCOPE;
  };

  const updateProjectList = (next: Partial<ProjectList>) => {
    projectList = {
      ...projectList,
      ...next,
      projects: sortProjects(next.projects ?? projectList.projects),
    };
  };

  const addProject = (root: string): ProjectEntry => {
    const trimmed = root.replace(/\/+$/, "") || root;
    if (projectList.projects.some((entry) => entry.root === trimmed)) {
      throw {
        code: "project.duplicate",
        message: `项目目录「${trimmed}」已注册。`,
        retryable: false,
      } satisfies AppError;
    }
    const entry = fakeProject(trimmed);
    updateProjectList({
      projects: [entry, ...projectList.projects],
      lastActiveScope: projectScope(entry.id),
    });
    return entry;
  };

  const relocateProject = (id: string, root: string): RelocateProjectResult => {
    const existing = projectList.projects.find((entry) => entry.id === id);
    if (!existing) {
      throw {
        code: "project.not_found",
        message: `project「${id}」不存在。`,
        retryable: false,
      } satisfies AppError;
    }
    const trimmed = root.replace(/\/+$/, "") || root;
    if (
      projectList.projects.some(
        (entry) => entry.root === trimmed && entry.id !== id,
      )
    ) {
      throw {
        code: "project.duplicate",
        message: `项目目录「${trimmed}」已注册。`,
        retryable: false,
      } satisfies AppError;
    }
    const relocated: ProjectEntry = {
      ...existing,
      checkRevision: `revision-${++revisionCounter}`,
      root: trimmed,
      displayName: trimmed.split("/").pop() || trimmed,
      lastOpenedAt: Date.now(),
      available: true,
      unavailableReason: undefined,
    };
    updateProjectList({
      projects: projectList.projects.map((entry) =>
        entry.id === id ? relocated : entry,
      ),
      lastActiveScope: projectScope(id),
    });
    checkedStates.delete(projectScope(id));
    return { project: relocated, warnings: [] };
  };

  const forgetProject = (id: string): ForgetProjectResult => {
    const scope = projectScope(id);
    if (!projectList.projects.some((entry) => entry.id === id)) {
      throw {
        code: "project.not_found",
        message: `project「${id}」不存在。`,
        retryable: false,
      } satisfies AppError;
    }
    updateProjectList({
      projects: projectList.projects.filter((entry) => entry.id !== id),
      lastActiveScope:
        projectList.lastActiveScope === scope
          ? GLOBAL_SCOPE
          : projectList.lastActiveScope,
    });
    checkedStates.delete(scope);
    return { scope, warnings: [] };
  };

  const setActiveScope = (scope: ScopeId): void => {
    if (scope !== GLOBAL_SCOPE) {
      const id = scope.slice("project:".length);
      const existing = projectList.projects.find((entry) => entry.id === id);
      if (!existing) {
        throw {
          code: "project.not_found",
          message: `project「${id}」不存在。`,
          retryable: false,
        } satisfies AppError;
      }
      updateProjectList({
        projects: projectList.projects.map((entry) =>
          entry.id === id ? { ...entry, lastOpenedAt: Date.now() } : entry,
        ),
        lastActiveScope: scope,
      });
      return;
    }
    updateProjectList({ lastActiveScope: GLOBAL_SCOPE });
  };

  // A check made during a test replaces the persisted state for that scope
  // only, so per-scope badges and last-check labels behave like Rust.
  const checkedStates = new Map<ScopeId, UpdateCheckState>();

  const reportFor = (scope: ScopeId): UpdateCheckReport => {
    const scoped = data.updateReports?.[scope];
    if (scoped) return factoryValue(scoped);
    if (scope === GLOBAL_SCOPE) return factoryValue(data.updateReport);
    return emptyUpdateReport({ checkedAtEpochMs: Date.now() });
  };

  const revisionFor = (scope: ScopeId) =>
    projectList.projects.find((project) => projectScope(project.id) === scope)
      ?.checkRevision ?? "initial";
  const checkStateFor = (scope: ScopeId): UpdateCheckState | null => {
    if (
      scope !== GLOBAL_SCOPE &&
      !projectList.projects.some(
        (project) => projectScope(project.id) === scope,
      )
    ) {
      return null;
    }
    const checked = checkedStates.get(scope);
    const scoped = data.updateCheckStates?.[scope];
    const state =
      checked ??
      (scoped !== undefined
        ? factoryValue(scoped)
        : scope === GLOBAL_SCOPE
          ? factoryValue(data.updateCheckState)
          : null);
    if (scope !== GLOBAL_SCOPE && state?.checkRevision !== revisionFor(scope)) {
      return null;
    }
    return state;
  };

  const allCheckStates = (): Record<string, UpdateCheckState> => {
    const states: Record<string, UpdateCheckState> = {};
    for (const scope of [
      GLOBAL_SCOPE,
      ...projectList.projects.map((project) => projectScope(project.id)),
    ]) {
      const state = checkStateFor(scope);
      if (state) states[scope] = state;
    }
    return states;
  };

  const stateFromReport = (report: UpdateCheckReport): UpdateCheckState => {
    const failedSourceCount = report.sources.filter(
      (source) => source.status === "error",
    ).length;
    const successfulSourceCount = report.sources.filter(
      (source) => source.status === "ok" || source.status === "cached",
    ).length;
    const lastOutcome =
      failedSourceCount > 0 && successfulSourceCount === 0
        ? "failed"
        : failedSourceCount > 0 || report.skippedSourceCount > 0
          ? "partial"
          : "ok";
    return {
      version: 1,
      lastAttemptAt: report.checkedAtEpochMs,
      lastSuccessAt: report.lastSuccessAtEpochMs,
      lastOutcome,
      failedSourceCount,
      cachedSourceCount: report.cachedSourceCount,
      skippedSourceCount: report.skippedSourceCount,
      candidateCount: report.skills.filter((skill) => skill.state === "update")
        .length,
    };
  };

  const updateDiscovery = (
    transform: (
      skill: DiscoveryState["skills"][number],
    ) => DiscoveryState["skills"][number],
  ) => {
    const skills = data.discovery.skills.map(transform);
    data = {
      ...data,
      discovery: {
        ...data.discovery,
        skills,
        unreadCount: skills.filter((s) => s.unread && !s.ignored).length,
      },
    };
  };
  const recordsFor = (scope: ScopeId): StaleRecord[] => {
    const configured = data.staleRecords?.[scope];
    if (configured) return configured;
    const group = snapshotFor(scope).anomalies.stale;
    return Array.from({ length: group.count }, (_, index) => {
      const name = group.examples[index] ?? `missing-skill-${index + 1}`;
      return { id: scope === GLOBAL_SCOPE ? name : `${scope}/${name}`, name, source: "acme/skills", reference: null, cleanReason: null, restoreReason: null };
    });
  };
  const removeRecord = (scope: ScopeId, id: string) => {
    const records = recordsFor(scope).filter((r) => r.id !== id);
    data = { ...data, staleRecords: { ...data.staleRecords, [scope]: records } };
    const snapshot = snapshotFor(scope);
    const prior = snapshot.anomalies.stale.count;
    setSnapshot({ ...snapshot, anomalies: { ...snapshot.anomalies,
      total: snapshot.anomalies.total - prior + records.length,
      stale: { count: records.length, examples: records.slice(0, 3).map((r) => r.name) },
    } }, scope);
  };
  const refreshRecordDiscovery = (scope: ScopeId, source: string, name: string, restored: boolean) => {
    refreshedDiscoverySources.add(source);
    updateDiscovery((skill) => skill.id.source === source && skill.id.name === name.toLowerCase()
      ? { ...skill, installations: skill.installations.map((installation) => installation.scope === scope
        ? { ...installation, status: restored ? "installed" : "notInstalled" } : installation) }
      : skill);
  };
  const discoveryState = (): DiscoveryState => {
    if (refreshedDiscoverySources.size === 0) return data.discovery;
    const scopes = [GLOBAL_SCOPE, ...projectList.projects.map((project) => projectScope(project.id))];
    const active = (source: string) => !refreshedDiscoverySources.has(source) || scopes.some((scope) =>
      snapshotFor(scope).skills.some((skill) => skill.management === "managed" && skill.source === source)
      || recordsFor(scope).some((record) => record.source === source));
    const skills = data.discovery.skills.filter((skill) => active(skill.id.source));
    // Keep dormant source catalogs and read/ignore decisions in data.discovery.
    return { ...data.discovery, skills, sources: data.discovery.sources.filter((source) => active(source.source)),
      unreadCount: skills.filter((skill) => skill.unread && !skill.ignored).length };
  };
  const buildBatchPreview = (
    id: string,
    scope: ScopeId,
    request: BatchRequest,
  ): BatchPreview => {
    const snapshot = snapshotFor(scope);
    if (request.action === "clean_records" || request.action === "restore") {
      if (scope !== GLOBAL_SCOPE && !projectList.projects.some((p) => projectScope(p.id) === scope && p.available)) throw { code: "project.root_unavailable", message: "项目不可用。", retryable: true } satisfies AppError;
      const targets = request.skillIds.map((skillId): BatchTarget => {
        const record = recordsFor(scope).find((r) => r.id === skillId);
        const name = record?.name ?? skillId.split("/").pop()!;
        const selected = request.agentIds.map((id) => snapshot.installTargets.find((a) => a.id === id));
        const reason = !record ? "安装记录已不存在。" : snapshot.skills.some((s) => s.name === name) ? "同名技能已有文件或挂载。"
          : request.action === "clean_records" ? record.cleanReason
          : record.restoreReason ?? (!factoryValue(data.environment).mutationsEnabled ? "恢复需要可用的 Node、npx 和 Git。" : selected.some((a) => !a?.detected || a.shared || a.universal) ? "所选 Agent 不可用。" : null);
        return { key: JSON.stringify([skillId, null]), skillId, name, agentId: null, agentName: null,
          eligible: !reason, reason, localModification: "unknown", deletesCopy: false, source: record?.source, reference: record?.reference,
          paths: request.action === "clean_records" ? [scope === GLOBAL_SCOPE ? "~/.agents/.skill-lock.json" : `${canonicalRootFor(scope)}/../../skills-lock.json`, `app-data/baselines/${scope}/${name}.json`]
            : [`${canonicalRootFor(scope)}/${name}`, ...selected.filter((a) => a).map((a) => `${a!.globalSkillsDir}/${name}`)],
          command: request.action === "restore" && record && !reason ? `npx skills@latest add ${record.source}${record.reference ? `#${record.reference}` : ""} --skill ${name} -a ${[...request.agentIds, "universal"].join(" ")} --json -y${scope === GLOBAL_SCOPE ? " -g" : ""}` : undefined,
        };
      }).filter((t) => !request.targetKeys || request.targetKeys.includes(t.key));
      return { id, scope, action: request.action, targets, revision: JSON.stringify({ targets, records: recordsFor(scope), agents: request.agentIds }) };
    }
    if (request.action === "copy_to_project") {
      if (scope !== GLOBAL_SCOPE || !request.projectRoot?.startsWith("/")) throw { code: "project.invalid_root", message: "请选择有效的项目目录。", retryable: false } satisfies AppError;
      const root = request.projectRoot.replace(/\/+$/, "");
      let project = projectList.projects.find((p) => p.root === root) ?? pendingCopyProjects.get(root);
      if (!project) {
        project = fakeProject(root);
        pendingCopyProjects.set(root, project);
      }
      if (!project.available) throw { code: "project.root_unavailable", message: "项目目录不可用。", retryable: false } satisfies AppError;
      const destination = snapshotFor(projectScope(project.id));
      const projectAgents = snapshot.installTargets.filter((a) => !a.universal && !a.shared && a.detected &&
        (a.origin === "catalog" || !!a.projectSkillsDir)).map((a) => ({
          ...a, globalSkillsDir: `${root}/${a.projectSkillsDir ?? (a.id === "claude-code" ? ".claude/skills" : `.${a.id}/skills`)}`,
        }));
      const targets: BatchTarget[] = [];
      for (const skillId of request.skillIds) {
        const skill = snapshot.skills.find((s) => s.id === skillId);
        const existing = destination.skills.find((s) => s.name === skill?.name);
        const conflict = existing && (existing.source !== skill?.source || existing.description !== skill?.description || existing.localModification === "modified");
        for (const agentId of [null, ...request.agentIds]) {
          const key = JSON.stringify([skillId, agentId]);
          if (request.targetKeys && !request.targetKeys.includes(key)) continue;
          const agent = projectAgents.find((a) => a.id === agentId);
          const reason = !skill ? "技能已不存在。"
            : ["skillhub", "skillhub-pack"].includes(skill.sourceType ?? "") ? "市场技能请从技能市场选择项目进行独立安装。"
            : skill.management !== "managed" || !skill.canonicalPath ? "仅受管且有共享目录的技能可复制。"
            : conflict ? "项目已有同名技能，内容不同，不覆盖。"
            : !agentId ? existing ? "项目已有相同内容，复用，不重复复制。" : null
            : existing?.management === "unmanaged" ? "项目已有未受管技能，无法新增独立挂载。"
            : !agent ? "Agent 不支持此项目。"
            : existing?.attachments.some((a) => a.agentId === agentId) ? "已有挂载，不覆盖。" : null;
          targets.push({
            key, skillId, name: skill?.name ?? skillId, agentId, agentName: agentId ? agent?.displayName ?? agentId : null,
            operation: agentId ? "attach" : "copy", eligible: reason === null, reason,
            localModification: skill?.localModification ?? "unknown", deletesCopy: false,
            paths: [agentId ? `${agent?.globalSkillsDir}/${skill?.name}` : `${root}/.agents/skills/${skill?.name}`],
          });
        }
      }
      return { id, scope, action: request.action, project, agents: projectAgents, targets,
        revision: JSON.stringify({ project, targets, source: request.skillIds.map((id) => snapshot.skills.find((s) => s.id === id)), destination: destination.skills }) };
    }
    const targets: BatchTarget[] = [];
    const keys = request.targetKeys ? new Set(request.targetKeys) : null;
    for (const skillId of request.skillIds) {
      const skill = snapshot.skills.find((skill) => skill.id === skillId);
      const agents =
        request.action === "attach" || request.action === "detach" || request.action === "uninstall_agents"
          ? request.agentIds
          : [null];
      for (const agentId of agents) {
        const agent = (request.action === "detach" || request.action === "uninstall_agents" ? snapshot.agents : snapshot.installTargets).find(
          (agent) => agent.id === agentId,
        );
        const mounted = skill?.attachments.find(
          (attachment) => attachment.agentId === agentId,
        );
        const target: BatchTarget = {
          key: JSON.stringify([skillId, agentId]),
          skillId,
          name: skill?.name ?? skillId.split("/").slice(-1)[0]!,
          agentId,
          agentName: agentId ? (agent?.displayName ?? agentId) : null,
          eligible: false,
          reason: null,
          localModification: skill?.localModification ?? "unknown",
          deletesCopy: false,
          paths: [],
        };
        if (!skill) target.reason = "技能已不存在。";
        else if (skill.management === "unmanaged" && request.action !== "uninstall" && request.action !== "uninstall_agents")
          target.reason = "未受管技能不可执行此操作。";
        else if (request.action === "update") {
          const result = reportFor(scope).skills.find(
            (result) => result.name === skill.name,
          );
          target.reason =
            skill.sourceType === "skillhub-pack" ? "专家包工作流不支持上游更新。" :
            skill.sourceType === "skillhub" ? "市场技能请单独更新，或使用更新全部。" :
            skill.management === "copy_only"
              ? "仅副本不可更新。"
              : skill.localModification !== "clean"
                ? "本地有改动或无法确认本地改动，请单独确认更新。"
                : result?.state !== "update"
                  ? "没有可用更新。"
                  : null;
          if (target.reason === null && result) {
            target.command = cliPreview(scope, { action: "update", skills: [skill.name] }).entries[0]?.command ?? undefined;
          }
        } else if (request.action === "uninstall") {
          target.command = cliPreview(scope, { action: "uninstall", skill: skill.id }).entries[0]?.command ?? undefined;
          target.paths = [
            skill.canonicalPath,
            ...skill.attachments.map((attachment) => attachment.path),
          ].filter((path): path is string => !!path);
        } else if (request.action === "uninstall_agents") {
          target.reason = !agent?.detected ? "Agent 未检测到。"
            : agent.shared || mounted?.mode === "native" ? "共享目录，无法单独移除。"
            : !mounted ? "没有对应的 Agent 挂载。" : null;
          target.deletesCopy = mounted?.mode === "copy";
          target.paths = mounted ? [mounted.path] : [];
        } else {
          target.reason =
            skill.management === "copy_only" || !skill.canonicalPath
              ? "没有共享技能目录。"
              : !agent?.detected
                ? "Agent 未检测到。"
                : agent.shared || agent.universal
                  ? "共享 Agent 无法独立操作。"
                  : request.action === "attach" && mounted
                    ? "已有挂载，不覆盖。"
                    : request.action === "detach" && !mounted?.detachable
                      ? "没有可解除的挂载。"
                      : null;
          target.deletesCopy =
            request.action === "detach" && mounted?.mode === "copy";
          target.paths = mounted ? [mounted.path] : [];
        }
        target.eligible = target.reason === null;
        if (!keys || keys.has(target.key)) targets.push(target);
      }
    }
    if (request.action === "uninstall_agents") {
      for (const skill of snapshot.skills) {
        const copies = targets.filter((t) => t.skillId === skill.id && t.eligible && t.deletesCopy);
        if (!skill.canonicalPath && copies.length > 0 && skill.attachments.every((a) =>
          a.mode === "broken" || copies.some((t) => t.agentId === a.agentId))) {
          copies.forEach((target) => { target.removesLastCopy = true; });
        }
      }
      targets.sort((a, b) => Number(a.deletesCopy) - Number(b.deletesCopy));
    }
    return {
      id,
      scope,
      action: request.action,
      targets,
      revision: JSON.stringify({
        targets,
        root: projectList.projects.find(
          (project) => projectScope(project.id) === scope,
        )?.root,
      }),
    };
  };

  const scheduleBatch = (task: FakeTask, preview: BatchPreview) => {
    if (!autoTasks) return;
    const rows: BatchRow[] = [];
    let index = 0;
    let processed = 0;
    const total = preview.targets.filter((target) => target.eligible).length;
    const failedCopies = new Set<string>();
    const step = () => {
      const run = batchRuns.get(task.taskId);
      if (!run) return;
      while (index < preview.targets.length) {
        const target = preview.targets[index++]!;
        if (!target.eligible || run.stop || (target.operation === "attach" && failedCopies.has(target.name))) {
          rows.push({
            ...target,
            status: target.eligible ? "not_executed" : "skipped",
            error: null,
            backupPath: null,
          });
          continue;
        }
        const invocation: CliCommandRecord | null = target.command ? {
          id: `${task.taskId}:${index}`, command: target.command,
          context: { origin: "task", scope: preview.scope, taskId: task.taskId, source: null },
          status: "running", startedAt: Date.now(),
        } : null;
        if (invocation) publishCommand(invocation);
        let failure: AppError | null = null;
        try {
          if (preview.action === "clean_records" || preview.action === "restore") {
            if (!recordsFor(preview.scope).some((r) => r.id === target.skillId) || snapshotFor(preview.scope).skills.some((s) => s.name === target.name)) throw { code: "batch.target_changed", message: "记录或目录已变化，请重新预览。", retryable: true } satisfies AppError;
            if (preview.action === "restore") {
              installInto(preview.scope, [target.name]);
              const installed = snapshotFor(preview.scope);
              setSnapshot({ ...installed, skills: installed.skills.map((s) => s.name === target.name ? { ...s, source: target.source ?? null } : s) }, preview.scope);
              for (const agent of installed.installTargets) if (!agent.shared && !agent.universal && target.paths.includes(`${agent.globalSkillsDir}/${target.name}`)) nativeAttach(target.skillId, agent.id, preview.scope);
            }
            removeRecord(preview.scope, target.skillId);
            refreshRecordDiscovery(preview.scope, target.source ?? "", target.name, preview.action === "restore");
          } else if (preview.action === "update")
            updateInto(preview.scope, [target.name]);
          else if (preview.action === "uninstall")
            uninstallFrom(preview.scope, [target.skillId]);
          else if (preview.action === "attach")
            nativeAttach(target.skillId, target.agentId!, preview.scope);
          else if (preview.action === "uninstall_agents") {
            nativeDetach(target.skillId, target.agentId!, preview.scope);
            const snapshot = snapshotFor(preview.scope);
            setSnapshot({ ...snapshot, skills: snapshot.skills.filter((skill) =>
              skill.id !== target.skillId || skill.canonicalPath || skill.attachments.some((a) => a.mode !== "broken")) }, preview.scope);
          }
          else if (preview.action === "copy_to_project" && preview.project) {
            const destinationScope = projectScope(preview.project.id);
            if (target.operation === "copy") {
              const source = findSkill(target.skillId, GLOBAL_SCOPE);
              const destination = snapshotFor(destinationScope);
              setSnapshot({ ...destination, skills: [...destination.skills, {
                ...source, id: `${destinationScope}/${source.name}`,
                canonicalPath: `${preview.project.root}/.agents/skills/${source.name}`,
                attachments: [], localModification: "clean",
              }] }, destinationScope);
            } else nativeAttach(`${destinationScope}/${target.name}`, target.agentId!, destinationScope);
          } else nativeDetach(target.skillId, target.agentId!, preview.scope);
        } catch (cause) {
          failure = cause as AppError;
          if (target.operation === "copy") failedCopies.add(target.name);
        }
        if (invocation) publishCommand({ ...invocation, status: failure ? "failed" : "succeeded" });
        rows.push({
          ...target,
          status: failure ? "failed" : "success",
          error: failure,
          backupPath: preview.action === "attach" ? null : FAKE_BACKUP,
        });
        processed += 1;
        task.emit({
          taskId: task.taskId,
          phase: "verifying",
          completed: processed,
          total,
          message: `已处理 ${processed} / ${total} 个目标`,
        });
        timers.add(window.setTimeout(step, autoDelay));
        return;
      }
      const success = rows.filter((row) => row.status === "success");
      task.emit({
        taskId: task.taskId,
        phase: run.stop ? "cancelled" : "completed",
        completed: processed,
        total,
        message: "批量操作已结束。",
        result: {
          action: "batch",
          scope: preview.project ? projectScope(preview.project.id) : preview.scope,
          overall: run.stop
            ? "cancelled"
            : success.length === rows.length
              ? "success"
              : success.length > 0
                ? "partial"
                : "failed",
          skills: rows
            .filter((row) => row.eligible && row.status !== "not_executed")
            .map((row) => ({
              name: row.name,
              status:
                row.status === "success"
                  ? preview.action === "update"
                    ? "updated"
                    : preview.action === "restore" || preview.action === "attach" || preview.action === "copy_to_project"
                      ? "installed"
                      : "removed"
                  : row.status === "skipped"
                    ? "skipped"
                    : "failed",
              error: row.error ?? undefined,
            })),
          warnings: [],
          startedAt: run.startedAt,
          finishedAtEpochMs: Date.now(),
          logPath: FAKE_LOG_PATH,
          backupPath: FAKE_BACKUP,
          batch: { action: preview.action, rows },
        },
      });
    };
    timers.add(window.setTimeout(step, autoDelay));
  };

  const defaultHandlers: FakeHandlers = {
    list_stale_records: ({ scope }) => recordsFor(scope),
    preview_batch: ({ scope, request }) => {
      const id = `batch-preview-${++batchPreviewCounter}`;
      const preview = buildBatchPreview(id, scope, request);
      batchPreviews.set(id, { scope, request, preview });
      return preview;
    },
    refresh_batch_preview: ({ previewId }) => {
      const stored = batchPreviews.get(previewId);
      if (!stored)
        throw {
          code: "batch.preview_expired",
          message: "预览已失效，请重新预览。",
          retryable: true,
        };
      const preview = buildBatchPreview(
        previewId,
        stored.scope,
        stored.request,
      );
      batchPreviews.set(previewId, { ...stored, preview });
      return preview;
    },
    start_batch: ({ scope, confirmation, onEvent }) => {
      const stored = batchPreviews.get(confirmation.previewId);
      if (
        !stored ||
        stored.scope !== scope ||
        buildBatchPreview(stored.preview.id, scope, stored.request).revision !==
          confirmation.revision
      ) {
        throw {
          code: "batch.preview_expired",
          message: "预览已变化，请重新预览并确认。",
          retryable: true,
        };
      }
      const preview = stored.preview;
      if (!preview.targets.some((t) => t.eligible)) throw { code: "batch.confirmation_needed", message: "没有可执行目标。", retryable: false } satisfies AppError;
      if (
        ((preview.action === "uninstall" || preview.action === "uninstall_agents") && !confirmation.confirmedDelete) ||
        (preview.targets.some(
          (target) => target.eligible && target.deletesCopy,
        ) &&
          !confirmation.confirmedCopies)
      ) {
        throw {
          code: "batch.confirmation_needed",
          message: "请确认删除范围。",
          retryable: false,
        };
      }
      const task = makeTask("start_batch", onEvent);
      if (preview.project) {
        if (!projectList.projects.some((p) => p.id === preview.project!.id)) {
          updateProjectList({ projects: [...projectList.projects, { ...preview.project, available: true }] });
        }
        const targetScope = projectScope(preview.project.id);
        const current = snapshotFor(targetScope);
        setSnapshot({ ...current, agents: preview.agents ?? [], installTargets: preview.agents ?? [] }, targetScope);
      }
      batchRuns.set(task.taskId, {
        preview,
        stop: false,
        startedAt: Date.now(),
      });
      batchPreviews.delete(confirmation.previewId);
      scheduleBatch(task, preview);
      return task.taskId;
    },
    get_app_update_state: () => data.appUpdate,
    check_app_update: () => {
      data = {
        ...data,
        appUpdate: {
          ...data.appUpdate,
          revision: data.appUpdate.revision + 1,
          phase: data.appUpdate.candidate ? "available" : "current",
          lastAttemptAt: Date.now(),
          error: null,
        },
      };
      return data.appUpdate;
    },
    start_app_update: () => {
      if (!data.appUpdate.candidate)
        throw {
          code: "app_update.unavailable",
          message: "当前没有可安装的应用更新。",
          retryable: false,
        };
      data = {
        ...data,
        appUpdate: {
          ...data.appUpdate,
          revision: data.appUpdate.revision + 1,
          phase: "downloading",
          error: null,
        },
      };
      if (autoTasks) {
        for (const [index, phase] of (
          ["verifying", "installing", "restarting"] as const
        ).entries()) {
          timers.add(
            window.setTimeout(
              () => {
                data = {
                  ...data,
                  appUpdate: {
                    ...data.appUpdate,
                    revision: data.appUpdate.revision + 1,
                    phase,
                    totalBytes: 1048576,
                    downloadedBytes: 1048576,
                  },
                };
              },
              autoDelay * (index + 1) * 4,
            ),
          );
        }
      }
      return data.appUpdate;
    },
    get_discovery_state: () => discoveryState(),
    start_discovery_scan: () => {
      const now = Date.now();
      const activeSources = new Set(discoveryState().sources.map((source) => source.source));
      const sources = data.discovery.sources.map((source) => {
        if (!activeSources.has(source.source)) return source;
        if ((!source.needsRevalidation && source.lastSuccessAt !== null) || (source.pausedUntil !== null && source.pausedUntil > now)) return source;
        const preview = cliPreview(GLOBAL_SCOPE, { action: "preview", source: source.source });
        const record: CliCommandRecord = { id: `discovery:${source.source}:${now}`, command: preview.entries[0]!.command!,
          context: { origin: "discovery", scope: GLOBAL_SCOPE, taskId: null, source: source.source }, status: "running", startedAt: now };
        publishCommand(record);
        publishCommand({ ...record, status: source.error ? "failed" : "succeeded" });
        return { ...source, lastAttemptAt: now, lastSuccessAt: source.error ? source.lastSuccessAt : now, needsRevalidation: Boolean(source.error) };
      });
      data = { ...data, discovery: { ...data.discovery, sources } };
      return data.discovery.progress;
    },
    acknowledge_discoveries: ({ ids }) => {
      updateDiscovery((s) =>
        !ids ||
        ids.some((id) => id.source === s.id.source && id.name === s.id.name)
          ? { ...s, unread: false }
          : s,
      );
    },
    set_discovery_ignored: ({ id, ignored }) => {
      updateDiscovery((s) =>
        id.source === s.id.source && id.name === s.id.name
          ? { ...s, ignored, unread: false }
          : s,
      );
    },
    invalidate_discovery_source: ({ source }) => {
      data = {
        ...data,
        discovery: {
          ...data.discovery,
          sources: data.discovery.sources.map((s) =>
            s.source === source ? { ...s, needsRevalidation: true } : s,
          ),
        },
      };
    },
    get_github_settings: () => ({ ...data.githubSettings }),
    save_github_token: () => {
      data = {
        ...data,
        githubSettings: {
          ...data.githubSettings,
          hasSavedToken: true,
          effectiveSource: "settings",
        },
      };
      return { ...data.githubSettings };
    },
    clear_github_token: () => {
      data = {
        ...data,
        githubSettings: {
          ...data.githubSettings,
          hasSavedToken: false,
          effectiveSource: data.githubSettings.hasEnvironmentToken
            ? "environment"
            : "anonymous",
        },
      };
      return { ...data.githubSettings };
    },
    test_github_auth: () => ({
      source: data.githubSettings.effectiveSource,
      limit: data.githubSettings.effectiveSource === "anonymous" ? 60 : 5_000,
      remaining:
        data.githubSettings.effectiveSource === "anonymous" ? 59 : 4_999,
      resetAtEpochMs: Date.now() + 3_600_000,
    }),
    list_expert_packs: ({ keyword, page }) => ({ packs: page === 1 && `${expertPack.displayName} ${expertPack.summary}`.includes(keyword) ? [expertPack] : [], total: `${expertPack.displayName} ${expertPack.summary}`.includes(keyword) ? 1 : 0, page, pageSize: 24 }),
    get_expert_pack: ({ slug }) => ({ ...expertPack, slug }),
    list_local_packs: ({ scope }) => localPacks(scope),
    forget_expert_pack: ({ scope, slug }) => { savedPacks.get(scope)?.delete(slug); },
    prepare_pack_install: ({ scope, agents }) => {
      const saved = savedPacks.get(scope)?.get(expertPack.slug);
      const snapshot = saved ?? packSnapshot(agents);
      const id = `pack-${scope}-${packPreviews.size}`;
      const preview: PackPreview = { snapshot, supplement: !!saved, items: [...snapshot.members.map((m) => ({ name: m.name, action: snapshotFor(scope).skills.some((s) => s.name === m.name) ? "reuse" as const : "install" as const, version: m.version, snapshotVersion: m.version, missingAgents: snapshot.agents })), { name: snapshot.workflowName, action: "install", version: null, snapshotVersion: null, missingAgents: snapshot.agents }], commands: { id, entries: snapshot.members.map((m) => ({ name: m.name, command: `skillhub --skip-self-upgrade install ${m.coordinate.slug}@${m.version} --namespace ${m.coordinate.namespace} --dir /tmp/pack-${id} --json`, reason: "暂存并校验确认版本" })) } };
      packPreviews.set(id, { scope, preview }); return preview;
    },
    start_pack_install: ({ scope, previewId, onEvent }) => {
      const plan = packPreviews.get(previewId);
      if (!plan || plan.scope !== scope) throw { code: "cli.command_changed", message: "请重新预览", retryable: true } satisfies AppError;
      const task = makeTask("start_pack_install", onEvent);
      if (!savedPacks.has(scope)) savedPacks.set(scope, new Map());
      savedPacks.get(scope)!.set(plan.preview.snapshot.slug, plan.preview.snapshot);
      packTasks.set(task.taskId, plan);
      taskCommands.set(task.taskId, { scope, commands: plan.preview.commands.entries.flatMap((e) => e.command ? [e.command] : []) });
      scheduleAutoTask(task, "install", plan.preview.items.map((i) => i.name), scope);
      return task.taskId;
    },
    list_marketplace_categories: () => marketplaceCategories,
    list_marketplace_skills: ({ keyword, page, category, apiKey }) => {
      const matches = apiKey !== "required" && (!category || category === "knowledge-management") && `${marketplaceSkill.name} ${marketplaceSkill.description}`.toLowerCase().includes(keyword.toLowerCase());
      return { skills: page === 1 && matches ? [marketplaceSkill] : [], total: matches ? 1 : 0, page, pageSize: 24 };
    },
    get_marketplace_skill: ({ coordinate }) => ({ ...marketplaceSkill, coordinate }),
    list_marketplace_files: ({ coordinate, version }) => ({ coordinate, version, files: marketplaceFiles }),
    read_marketplace_file: ({ path }) => marketplaceFileContent(path),
    get_marketplace_cli: () => marketplaceCli,
    prepare_marketplace_install: ({ scope, coordinate, agents }) => {
      const existing = snapshotFor(scope).skills.find((s) => s.name === marketplaceSkill.originalName);
      if (existing) throw { code: "skill.conflict", message: "当前作用域已有同名技能。", retryable: false } satisfies AppError;
      const detail = { ...marketplaceSkill, coordinate };
      const id = `marketplace-${scope}-${marketPreviews.size}`;
      const preview: MarketplacePreview = { detail, commands: { id, entries: [{ name: detail.originalName!, command: `cd /tmp/skills-box-skillhub-preview && /Users/demo/.local/bin/skillhub --skip-self-upgrade install ${coordinate.slug}@${detail.version} --namespace ${coordinate.namespace} --dir /tmp/skills-box-skillhub-preview --json`, reason: null }] } };
      marketPreviews.set(id, { scope, preview, agents });
      return preview;
    },
    start_marketplace_install: ({ scope, previewId, onEvent }) => {
      const plan = marketPreviews.get(previewId);
      if (!plan || plan.scope !== scope) throw { code: "cli.command_changed", message: "请刷新预览。", retryable: true } satisfies AppError;
      const task = makeTask("start_marketplace_install", onEvent);
      marketTasks.set(task.taskId, plan);
      taskCommands.set(task.taskId, { scope, commands: plan.preview.commands.entries.flatMap((e) => e.command ? [e.command] : []) });
      scheduleAutoTask(task, "install", [plan.preview.detail.originalName!], scope);
      return task.taskId;
    },
    get_cli_commands: () => cliRecords,
    preview_cli_commands: ({ scope, request }) => cliPreview(scope, request),
    get_environment: ({ refresh }) => {
      const report = factoryValue(data.environment);
      if (report.cli.status !== "not_checked" && (refresh || !cliRecords.some((record) => record.context.origin === "environment"))) publishCommand({ id: `env-${Date.now()}`, command: commandFor(GLOBAL_SCOPE, ["--version"]), context: { origin: "environment", scope: GLOBAL_SCOPE, taskId: null, source: null }, status: report.cli.available ? "succeeded" : "failed", startedAt: Date.now() });
      return report;
    },
    list_projects: () => ({
      ...projectList,
      projects: sortProjects(projectList.projects),
      lastActiveScope: resolveActive(projectList.lastActiveScope),
    }),
    add_project: ({ root }) => addProject(root),
    relocate_project: ({ id, root }) => relocateProject(id, root),
    forget_project: ({ id }) => forgetProject(id),
    set_active_scope: ({ scope }) => setActiveScope(scope),
    get_inventory: ({ scope }) => snapshotFor(scope),
    get_skill_detail: ({ skillId, scope }) =>
      data.detail(skillId, snapshotFor(scope), scope),
    open_skill_folder: () => undefined,
    list_skill_dir: ({ skillId, path }) => data.listDir(skillId, path ?? ""),
    read_skill_file: ({ skillId, path }) => data.readFile(skillId, path),
    open_skill_file: () => undefined,
    open_external_url: () => undefined,
    start_preview: ({ source, scope, onEvent }) => {
      const commands = prepareTaskCommands(scope, { action: "preview", source });
      const task = makeTask("start_preview", onEvent);
      taskCommands.set(task.taskId, commands);
      scheduleAutoTask(task, "preview", [], scope);
      return task.taskId;
    },
    cancel_task: ({ taskId }) => {
      const batch = batchRuns.get(taskId);
      if (batch) batch.stop = true;
      return {
        taskId,
        accepted: true,
        message: "已请求取消",
      };
    },
    get_active_mutation: () => {
      const running = [...batchRuns.entries()][0];
      return running
        ? {
            taskId: running[0],
            scope: running[1].preview.scope,
            kind: "batch",
            skills: running[1].preview.targets.map((target) => target.name),
            startedAtEpochMs: running[1].startedAt,
            snapshotPaths: [],
          }
        : factoryValue(data.activeMutation);
    },
    dismiss_interrupted_mutation: () => undefined,
    quit_app: () => undefined,
    start_install: ({ request, scope, onEvent, commandPreviewId }) => {
      const commands = prepareTaskCommands(scope, { action: "install", request }, commandPreviewId);
      const task = makeTask("start_install", onEvent);
      taskCommands.set(task.taskId, commands);
      installSources.set(
        task.taskId,
        request.source.startsWith("https://")
          ? request.source
          : request.source.toLowerCase(),
      );
      scheduleAutoTask(task, "install", request.skills, scope);
      return task.taskId;
    },
    check_updates: ({ scope, skillIds, checkRevision }) => {
      const revision = scope === GLOBAL_SCOPE ? undefined : revisionFor(scope);
      if (
        scope !== GLOBAL_SCOPE &&
        (!projectList.projects.some(
          (project) => projectScope(project.id) === scope,
        ) ||
          (checkRevision !== undefined && checkRevision !== revision))
      ) {
        return {
          invalidated: true,
          scope,
          checkRevision: checkRevision ?? "initial",
        };
      }
      const report = reportFor(scope);
      if (skillIds !== null) {
        return {
          report: {
            ...report,
            skills: report.skills.filter(
              (skill) =>
                skillIds.includes(skill.name) ||
                skillIds.includes(`${scope}/${skill.name}`),
            ),
          },
          scope,
          checkRevision: revision,
        };
      }
      const state = { ...stateFromReport(report), checkRevision: revision };
      checkedStates.set(scope, state);
      return { report, state, scope, checkRevision: revision };
    },
    get_update_check_state: ({ scope }) => checkStateFor(scope),
    get_update_check_states: () => allCheckStates(),
    start_update: ({ skillIds, scope, onEvent, commandPreviewId }) => {
      const commands = prepareTaskCommands(scope, { action: "update", skills: skillIds }, commandPreviewId);
      const task = makeTask("start_update", onEvent);
      taskCommands.set(task.taskId, commands);
      scheduleAutoTask(task, "update", skillIds, scope);
      return task.taskId;
    },
    detach_agent: ({ skillId, agentId, scope }) =>
      nativeDetach(skillId, agentId, scope),
    attach_agent: ({ skillId, agentId, scope }) =>
      nativeAttach(skillId, agentId, scope),
    add_extra_agent: ({ agent }) => addExtra(agent),
    update_extra_agent: ({ agentId, projectSkillsDir }) =>
      updateExtra(agentId, projectSkillsDir),
    remove_extra_agent: ({ agentId }) => removeExtra(agentId),
    start_uninstall: ({ skillId, scope, onEvent, commandPreviewId }) => {
      // Rust resolves the name with `ScopeId::parse_skill_id`, so a project
      // uninstall must carry the scope-qualified id or it is rejected.
      if (scope !== GLOBAL_SCOPE && !skillId.startsWith(`${scope}/`)) {
        throw {
          code: "path.invalid",
          message: `skill id 不属于当前项目 scope：${skillId}`,
          retryable: false,
        } satisfies AppError;
      }
      const commands = prepareTaskCommands(scope, { action: "uninstall", skill: skillId }, commandPreviewId);
      const task = makeTask("start_uninstall", onEvent);
      taskCommands.set(task.taskId, commands);
      scheduleAutoTask(task, "uninstall", [skillId], scope);
      return task.taskId;
    },
    get_app_data: () => structuredClone(data.appData),
    save_data_preferences: ({ preferences }) => {
      data = { ...data, appData: { ...data.appData, preferences: { ...preferences } } };
      return { ...preferences };
    },
    preview_app_data_cleanup: ({ selection }) => {
      if (selection.backupIds.some((id) => !data.appData.backups.some((b) => b.id === id && !b.protectedReason))) throw { code: "app_data.failed", message: "备份不存在或受保护。", retryable: true };
      const items: AppDataPreview["items"] = data.appData.backups.filter((b) => !b.protectedReason && (selection.itemIds ? selection.itemIds.includes(b.id) : (selection.allBackups || selection.backupIds.includes(b.id)))).map((b) => ({ id: b.id, label: `backups/${b.name}`, category: "backups", bytes: b.bytes }));
      for (const category of ["logs", "cache"] as const) {
        const usage = data.appData.categories.find((c) => c.category === category);
        if ((selection.itemIds ? selection.itemIds.includes(category) : selection[category]) && usage?.cleanableBytes) items.push({ id: category, label: category === "logs" ? "logs/operations.jsonl" : "cache/archives", category, bytes: usage.cleanableBytes });
      }
      if (!items.length) throw { code: "app_data.failed", message: "所选类别没有可清理的文件。", retryable: true };
      const preview = { id: `data-preview-${dataPreviews.size + 1}`, items, totalBytes: items.reduce((n,i) => n + i.bytes,0) };
      dataPreviews.set(preview.id, preview);
      return preview;
    },
    clean_app_data: ({ previewId }) => {
      if (factoryValue(data.activeMutation)) throw { code: "task.busy", message: "有任务正在执行。", retryable: true };
      const preview = dataPreviews.get(previewId);
      if (!preview) throw { code: "app_data.failed", message: "清理预览已失效。", retryable: true };
      dataPreviews.delete(previewId);
      const ids = new Set(preview.items.map((i) => i.id));
      const categories = data.appData.categories.map((c) => {
        const amount = preview.items.filter((i) => i.category === c.category).reduce((n,i) => n + i.bytes,0);
        return { ...c, bytes: c.bytes - amount, cleanableBytes: c.cleanableBytes - amount, count: Math.max(0,c.count - preview.items.filter((i) => i.category === c.category).length) };
      });
      data = { ...data, appData: { ...data.appData, categories, totalBytes: data.appData.totalBytes - preview.totalBytes, cleanableBytes: data.appData.cleanableBytes - preview.totalBytes, backups: data.appData.backups.filter((b) => !ids.has(b.id)) } };
      return { freedBytes: preview.totalBytes, items: preview.items.map((i) => ({ id: i.id, label: i.label, freedBytes: i.bytes, error: null })) };
    },
    reveal_backup: ({ id }) => { if (!data.appData.backups.some((b) => b.id === id)) throw { code: "app_data.failed", message: "备份不存在。", retryable: true }; },
    reveal_app_data: () => undefined,
  };

  const overrides = options.commands ?? {};

  const handlerFor = (command: ApiCommand) =>
    (overrides[command] ?? defaultHandlers[command]) as FakeHandler<ApiCommand>;

  const backend: FakeBackend = {
    calls,
    tasks,
    get lastTask() {
      return tasks[tasks.length - 1] ?? null;
    },
    count(command) {
      return calls.filter((call) => call.command === command).length;
    },
    callsOf<K extends ApiCommand>(command: K) {
      return calls.filter((call) => call.command === command) as Array<{
        command: K;
        payload: FakeCommandContract[K]["args"];
      }>;
    },
    update(patch) {
      data = { ...data, ...patch };
      if (patch.projects) projectList = factoryValue(patch.projects);
    },
    dispatch<K extends ApiCommand>(
      command: K,
      args: FakeCommandContract[K]["args"],
    ) {
      return defaultHandlers[command](args as never, backend) as MaybePromise<
        FakeCommandContract[K]["result"]
      >;
    },
    install({ shouldMockEvents = false } = {}) {
      installedBackends.add(backend);
      mockIPC(
        (command, payload) => {
          const typed = command as ApiCommand;
          if (!backend.supports(typed)) {
            throw new Error(`fake backend: unmocked command ${command}`);
          }
          calls.push({
            command: typed,
            payload:
              typed === "save_github_token" ? { token: "[redacted]" } : payload,
          } as FakeCall);
          return handlerFor(typed)(payload as never, backend);
        },
        { shouldMockEvents },
      );
      return backend;
    },
    reset() {
      cliRecords = [];
      taskCommands.clear();
      installSources.clear();
      refreshedDiscoverySources.clear();
      batchPreviews.clear();
      pendingCopyProjects.clear();
      batchRuns.clear();
      for (const timer of timers) window.clearTimeout(timer);
      timers.clear();
      calls.length = 0;
      tasks.length = 0;
      taskCounter = 0;
      projectCounter = 0;
      revisionCounter = 0;
      checkedStates.clear();
      channelIndexes.clear();
      data = { ...initialData };
      projectList = initialProjects;
    },
    supports(command) {
      return Object.prototype.hasOwnProperty.call(defaultHandlers, command);
    },
  };

  return backend;
}
