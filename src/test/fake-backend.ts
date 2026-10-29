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
  ExtraAgent,
  ExtraAgentInput,
  InstallRequest,
  InventoryAgent,
  InventorySkill,
  InventorySnapshot,
  PreviewSummary,
  RemoveExtraAgentResult,
  SkillDetail,
  SkillDirListing,
  SkillFileContent,
  TaskEvent,
  TaskResult,
  UpdateCheckReport,
  UpdateCheckState,
} from "../lib/types";
import { healthyReport } from "./fixtures";
import { detailForSkill, makeDirListing, makeFileContent } from "./detail-fixtures";
import { emptySnapshot } from "./inventory-fixtures";

type Factory<T> = T | (() => T);
type MaybePromise<T> = T | Promise<T>;

export interface FakeCommandContract {
  get_environment: { args: { refresh?: boolean }; result: EnvReport };
  get_inventory: { args: Record<string, never>; result: InventorySnapshot };
  get_skill_detail: { args: { skillId: string }; result: SkillDetail };
  open_skill_folder: { args: { skillId: string }; result: void };
  list_skill_dir: {
    args: { skillId: string; path?: string };
    result: SkillDirListing;
  };
  read_skill_file: {
    args: { skillId: string; path: string };
    result: SkillFileContent;
  };
  open_skill_file: { args: { skillId: string; path: string }; result: void };
  open_external_url: { args: { url: string }; result: void };
  start_preview: {
    args: { source: string; onEvent: Channel<TaskEvent> };
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
    args: { request: InstallRequest; onEvent: Channel<TaskEvent> };
    result: string;
  };
  check_updates: {
    args: { skillIds: string[] | null };
    result: UpdateCheckReport;
  };
  get_update_check_state: {
    args: Record<string, never>;
    result: UpdateCheckState | null;
  };
  start_update: {
    args: { skillIds: string[]; onEvent: Channel<TaskEvent> };
    result: string;
  };
  detach_agent: {
    args: { skillId: string; agentId: string };
    result: DetachResult;
  };
  attach_agent: {
    args: { skillId: string; agentId: string };
    result: AttachResult;
  };
  add_extra_agent: { args: { agent: ExtraAgentInput }; result: ExtraAgent };
  remove_extra_agent: {
    args: { agentId: string };
    result: RemoveExtraAgentResult;
  };
  start_uninstall: {
    args: { skillId: string; onEvent: Channel<TaskEvent> };
    result: string;
  };
  reveal_app_data: { args: { kind: AppDataKind }; result: void };
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
  environment: Factory<EnvReport>;
  snapshot: InventorySnapshot;
  detail: (skillId: string, snapshot: InventorySnapshot) => SkillDetail;
  listDir: (skillId: string, path: string) => SkillDirListing;
  readFile: (skillId: string, path: string) => SkillFileContent;
  updateReport: Factory<UpdateCheckReport>;
  updateCheckState: Factory<UpdateCheckState | null>;
  activeMutation: Factory<ActiveMutationInfo | null>;
  preview: PreviewSummary;
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
): TaskResult {
  const status =
    kind === "install"
      ? "installed"
      : kind === "update"
        ? "updated"
        : "removed";
  return {
    action: kind,
    overall: "success",
    skills:
      kind === "preview"
        ? []
        : names.map((name) => ({ name, status })),
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
    environment: healthyReport(),
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
  ) => {
    if (!autoTasks) return;
    const total = kind === "preview" ? 1 : Math.max(1, names.length);
    timers.add(
      window.setTimeout(() => {
        task.emit({
          taskId: task.taskId,
          phase: kind === "preview" ? "cloning" : "preparing",
          message: kind === "preview" ? "正在读取仓库…" : "正在准备写入…",
          completed: 0,
          total,
        });
      }, autoDelay),
    );
    timers.add(
      window.setTimeout(() => {
        const result = taskResult(kind, names, data.preview);
        const message =
          kind === "preview"
            ? `找到 ${data.preview.skills.length} 个 skill`
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

  const setSnapshot = (snapshot: InventorySnapshot) => {
    data = { ...data, snapshot };
  };

  const findSkill = (skillId: string): InventorySkill => {
    const skill = data.snapshot.skills.find((entry) => entry.name === skillId);
    if (!skill) throw notFound("skill", skillId);
    return skill;
  };

  const findAgent = (agentId: string): InventoryAgent | undefined =>
    data.snapshot.agents.find((agent) => agent.id === agentId);

  const nativeAttach = (skillId: string, agentId: string): AttachResult => {
    const skill = findSkill(skillId);
    const agent = findAgent(agentId);
    const agentName = agent?.displayName ?? agentId;
    const skillsDir = agent?.globalSkillsDir ?? `~/.${agentId}/skills`;
    const attachment: AgentAttachment = {
      agentId,
      agentName,
      path: `${skillsDir}/${skillId}`,
      mode: "link",
      detachable: true,
    };
    setSnapshot({
      ...data.snapshot,
      skills: data.snapshot.skills.map((entry) =>
        entry.name === skillId
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
    });
    return {
      skillId,
      agentId,
      agentName,
      attachmentPath: attachment.path,
      canonicalPath:
        skill.canonicalPath ?? `~/.agents/skills/${skillId}`,
      logPath: FAKE_LOG_PATH,
    };
  };

  const nativeDetach = (skillId: string, agentId: string): DetachResult => {
    const skill = findSkill(skillId);
    const attachment = skill.attachments.find(
      (entry) => entry.agentId === agentId,
    );
    if (!attachment) throw notFound("agent", agentId);
    const remaining = skill.attachments.filter(
      (entry) => entry.agentId !== agentId,
    );
    setSnapshot({
      ...data.snapshot,
      skills: data.snapshot.skills.map((entry) =>
        entry.name === skillId
          ? { ...entry, attachments: remaining }
          : entry,
      ),
    });
    return {
      skillId,
      agentId,
      agentName: attachment.agentName,
      mode: attachment.mode,
      attachmentPath: attachment.path,
      canonicalPath:
        skill.canonicalPath ?? `~/.agents/skills/${skillId}`,
      backupPath:
        attachment.mode === "copy"
          ? `${FAKE_BACKUP}/1-detach-${skillId}`
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
    const agent: InventoryAgent = {
      id: input.id,
      displayName: input.displayName,
      universal: false,
      shared: false,
      globalSkillsDir: input.skillsDir,
      origin: "user",
      detected: true,
    };
    setSnapshot({
      ...data.snapshot,
      agents: [...data.snapshot.agents, agent],
    });
    return { ...input, builtin: false };
  };

  const removeExtra = (agentId: string): RemoveExtraAgentResult => {
    const agent = findAgent(agentId);
    if (!agent) throw notFound("agent", agentId);
    const detached = data.snapshot.skills
      .filter((entry) =>
        entry.attachments.some((attachment) => attachment.agentId === agentId),
      )
      .map((entry) => entry.name);
    setSnapshot({
      ...data.snapshot,
      agents: data.snapshot.agents.filter((entry) => entry.id !== agentId),
      skills: data.snapshot.skills.map((entry) => ({
        ...entry,
        attachments: entry.attachments.filter(
          (attachment) => attachment.agentId !== agentId,
        ),
      })),
    });
    return {
      id: agent.id,
      displayName: agent.displayName,
      detached,
    };
  };

  const defaultHandlers: FakeHandlers = {
    get_environment: () => factoryValue(data.environment),
    get_inventory: () => data.snapshot,
    get_skill_detail: ({ skillId }) => data.detail(skillId, data.snapshot),
    open_skill_folder: () => undefined,
    list_skill_dir: ({ skillId, path }) => data.listDir(skillId, path ?? ""),
    read_skill_file: ({ skillId, path }) => data.readFile(skillId, path),
    open_skill_file: () => undefined,
    open_external_url: () => undefined,
    start_preview: ({ onEvent }) => {
      const task = makeTask("start_preview", onEvent);
      scheduleAutoTask(task, "preview", []);
      return task.taskId;
    },
    cancel_task: ({ taskId }) => ({
      taskId,
      accepted: true,
      message: "已请求取消",
    }),
    get_active_mutation: () => factoryValue(data.activeMutation),
    dismiss_interrupted_mutation: () => undefined,
    quit_app: () => undefined,
    start_install: ({ request, onEvent }) => {
      const task = makeTask("start_install", onEvent);
      scheduleAutoTask(task, "install", request.skills);
      return task.taskId;
    },
    check_updates: () => factoryValue(data.updateReport),
    get_update_check_state: () => factoryValue(data.updateCheckState),
    start_update: ({ skillIds, onEvent }) => {
      const task = makeTask("start_update", onEvent);
      scheduleAutoTask(task, "update", skillIds);
      return task.taskId;
    },
    detach_agent: ({ skillId, agentId }) => nativeDetach(skillId, agentId),
    attach_agent: ({ skillId, agentId }) => nativeAttach(skillId, agentId),
    add_extra_agent: ({ agent }) => addExtra(agent),
    remove_extra_agent: ({ agentId }) => removeExtra(agentId),
    start_uninstall: ({ skillId, onEvent }) => {
      const task = makeTask("start_uninstall", onEvent);
      scheduleAutoTask(task, "uninstall", [skillId]);
      return task.taskId;
    },
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
    },
    dispatch<K extends ApiCommand>(
      command: K,
      args: FakeCommandContract[K]["args"],
    ) {
      return defaultHandlers[command](
        args as never,
        backend,
      ) as MaybePromise<FakeCommandContract[K]["result"]>;
    },
    install({ shouldMockEvents = false } = {}) {
      mockIPC(
        (command, payload) => {
          const typed = command as ApiCommand;
          if (!backend.supports(typed)) {
            throw new Error(`fake backend: unmocked command ${command}`);
          }
          calls.push({
            command: typed,
            payload,
          } as FakeCall);
          return handlerFor(typed)(payload as never, backend);
        },
        { shouldMockEvents },
      );
      return backend;
    },
    reset() {
      for (const timer of timers) window.clearTimeout(timer);
      timers.clear();
      calls.length = 0;
      tasks.length = 0;
      taskCounter = 0;
      channelIndexes.clear();
      data = { ...initialData };
    },
    supports(command) {
      return Object.prototype.hasOwnProperty.call(defaultHandlers, command);
    },
  };

  return backend;
}
