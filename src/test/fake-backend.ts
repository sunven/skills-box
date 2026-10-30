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
  ProjectEntry,
  ProjectList,
  RemoveExtraAgentResult,
  ScopeId,
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
import { GLOBAL_SCOPE } from "../lib/types";

type Factory<T> = T | (() => T);
type MaybePromise<T> = T | Promise<T>;

export interface FakeCommandContract {
  get_environment: { args: { refresh?: boolean }; result: EnvReport };
  list_projects: { args: Record<string, never>; result: ProjectList };
  add_project: { args: { root: string }; result: ProjectEntry };
  relocate_project: { args: { id: string; root: string }; result: ProjectEntry };
  forget_project: { args: { id: string }; result: void };
  set_active_scope: { args: { scope: ScopeId }; result: void };
  get_inventory: {
    args: { scope: ScopeId };
    result: InventorySnapshot;
  };
  get_skill_detail: {
    args: { scope: ScopeId; skillId: string };
    result: SkillDetail;
  };
  open_skill_folder: { args: { scope: ScopeId; skillId: string }; result: void };
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
      scope: ScopeId;
      onEvent: Channel<TaskEvent>;
    };
    result: string;
  };
  check_updates: {
    args: { skillIds: string[] | null; scope: ScopeId };
    result: UpdateCheckReport;
  };
  get_update_check_state: {
    args: { scope: ScopeId };
    result: UpdateCheckState | null;
  };
  start_update: {
    args: { skillIds: string[]; scope: ScopeId; onEvent: Channel<TaskEvent> };
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
  remove_extra_agent: {
    args: { agentId: string };
    result: RemoveExtraAgentResult;
  };
  start_uninstall: {
    args: { skillId: string; scope: ScopeId; onEvent: Channel<TaskEvent> };
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
    scope: ScopeId = GLOBAL_SCOPE,
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
        if (kind === "install") installInto(scope, names);
        const result = taskResult(kind, names, previewFor(scope));
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
    setSnapshot(
      { ...snapshot, skills: [...snapshot.skills, ...added] },
      scope,
    );
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
      canonicalPath:
        skill.canonicalPath ?? `~/.agents/skills/${skill.name}`,
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
      canonicalPath:
        skill.canonicalPath ?? `~/.agents/skills/${skill.name}`,
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

  let projectCounter = 0;
  const fakeProject = (root: string, id?: string): ProjectEntry => {
    const now = Date.now();
    return {
      id: id ?? `project-${++projectCounter}`,
      root,
      displayName: root.replace(/\/+$/, "").split("/").pop() || root,
      addedAt: now,
      lastOpenedAt: now,
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

  const relocateProject = (id: string, root: string): ProjectEntry => {
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
    return relocated;
  };

  const forgetProject = (id: string): void => {
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

  const reportFor = (scope: ScopeId): UpdateCheckReport => {
    const scoped = data.updateReports?.[scope];
    if (scoped) return factoryValue(scoped);
    if (scope === GLOBAL_SCOPE) return factoryValue(data.updateReport);
    return emptyUpdateReport({ checkedAtEpochMs: Date.now() });
  };

  const checkStateFor = (scope: ScopeId): UpdateCheckState | null => {
    const scoped = data.updateCheckStates?.[scope];
    if (scoped !== undefined) return factoryValue(scoped);
    if (scope === GLOBAL_SCOPE) return factoryValue(data.updateCheckState);
    return null;
  };

  const defaultHandlers: FakeHandlers = {
    get_environment: () => factoryValue(data.environment),
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
    start_preview: ({ scope, onEvent }) => {
      const task = makeTask("start_preview", onEvent);
      scheduleAutoTask(task, "preview", [], scope);
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
    start_install: ({ request, scope, onEvent }) => {
      const task = makeTask("start_install", onEvent);
      scheduleAutoTask(task, "install", request.skills, scope);
      return task.taskId;
    },
    check_updates: ({ scope }) => reportFor(scope),
    get_update_check_state: ({ scope }) => checkStateFor(scope),
    start_update: ({ skillIds, scope, onEvent }) => {
      const task = makeTask("start_update", onEvent);
      scheduleAutoTask(task, "update", skillIds, scope);
      return task.taskId;
    },
    detach_agent: ({ skillId, agentId, scope }) =>
      nativeDetach(skillId, agentId, scope),
    attach_agent: ({ skillId, agentId, scope }) =>
      nativeAttach(skillId, agentId, scope),
    add_extra_agent: ({ agent }) => addExtra(agent),
    remove_extra_agent: ({ agentId }) => removeExtra(agentId),
    start_uninstall: ({ skillId, scope, onEvent }) => {
      const task = makeTask("start_uninstall", onEvent);
      scheduleAutoTask(task, "uninstall", [skillId], scope);
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
      if (patch.projects) projectList = factoryValue(patch.projects);
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
      projectCounter = 0;
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
