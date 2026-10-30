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
  update_extra_agent: {
    args: { agentId: string; projectSkillsDir: string | null };
    result: ExtraAgent;
  };
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
    setSnapshot(
      { ...snapshot, skills: [...snapshot.skills, ...added] },
      scope,
    );
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
    const stripAttachments = (snapshot: InventorySnapshot): InventorySnapshot => ({
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
      !projectList.projects.some((project) => projectScope(project.id) === scope)
    ) {
      return null;
    }
    const checked = checkedStates.get(scope);
    const scoped = data.updateCheckStates?.[scope];
    const state = checked ?? (
      scoped !== undefined
        ? factoryValue(scoped)
        : scope === GLOBAL_SCOPE ? factoryValue(data.updateCheckState) : null
    );
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
    check_updates: ({ scope, skillIds, checkRevision }) => {
      const revision = scope === GLOBAL_SCOPE ? undefined : revisionFor(scope);
      if (
        scope !== GLOBAL_SCOPE &&
        (!projectList.projects.some((project) => projectScope(project.id) === scope) ||
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
            skills: report.skills.filter((skill) =>
              skillIds.includes(skill.name) || skillIds.includes(`${scope}/${skill.name}`)),
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
    update_extra_agent: ({ agentId, projectSkillsDir }) =>
      updateExtra(agentId, projectSkillsDir),
    remove_extra_agent: ({ agentId }) => removeExtra(agentId),
    start_uninstall: ({ skillId, scope, onEvent }) => {
      // Rust resolves the name with `ScopeId::parse_skill_id`, so a project
      // uninstall must carry the scope-qualified id or it is rejected.
      if (scope !== GLOBAL_SCOPE && !skillId.startsWith(`${scope}/`)) {
        throw {
          code: "path.invalid",
          message: `skill id 不属于当前项目 scope：${skillId}`,
          retryable: false,
        } satisfies AppError;
      }
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
