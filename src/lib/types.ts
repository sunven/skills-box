export type PathSource =
  | "override"
  | "volta"
  | "fnm"
  | "nvm"
  | "homebrew"
  | "system"
  | "login_shell"
  | "none";

/** The only two scope shapes the IPC contract accepts. */
export type ScopeId = "global" | `project:${string}`;

export const GLOBAL_SCOPE: ScopeId = "global";

export interface ProjectEntry {
  /** Stable UUID assigned at registration; never changes on relocate. */
  id: string;
  /** Canonical absolute path recorded at registration. */
  root: string;
  /** Always the root directory basename; projects cannot be renamed. */
  displayName: string;
  addedAt: number;
  lastOpenedAt: number;
  /** Opaque persisted lifetime of Project update-check results. */
  checkRevision?: string;
  /** False when the root is missing, not a directory or unreadable. */
  available: boolean;
  unavailableReason?: string;
  /** Project lock parse/read failure; present even when the scope is inactive. */
  lockError?: string;
}

export interface ProjectList {
  /** Ordered by lastOpenedAt, newest first. */
  projects: ProjectEntry[];
  /** Restored scope, falling back to global when missing or unavailable. */
  lastActiveScope: ScopeId;
  /** App-level warning, e.g. a corrupt registry preserved on disk. */
  warnings: string[];
}

export type ToolState = "ok" | "missing" | "too_old" | "error" | "not_checked";

export type LockStatus = "ok" | "missing" | "corrupt" | "unsupported" | "unreadable";

export interface AppError {
  /** Omitted only for client-side fallbacks; backend errors always carry one. */
  code?: string;
  message: string;
  detail?: string;
  hint?: string;
  retryable: boolean;
}

export interface ToolStatus {
  found: boolean;
  path: string | null;
  version: string | null;
  status: ToolState;
  detail: string | null;
}

export interface CliStatus {
  available: boolean;
  version: string | null;
  pinnedVersion: string;
  status: ToolState;
  detail: string | null;
}

export interface DetectedAgent {
  id: string;
  displayName: string;
  globalSkillsDir: string | null;
  universal: boolean;
}

export interface LockHealth {
  path: string;
  exists: boolean;
  status: LockStatus;
  version: number | null;
  skillCount: number | null;
  error: AppError | null;
}

export interface EnvReport {
  ready: boolean;
  mutationsEnabled: boolean;
  pathSource: PathSource;
  pathSourceDetail: string | null;
  node: ToolStatus;
  npx: ToolStatus;
  git: ToolStatus;
  cli: CliStatus;
  agents: DetectedAgent[];
  lock: LockHealth;
  interrupted?: InterruptedReport | null;
  issues: AppError[];
  warnings: string[];
  checkedAtEpochMs: number;
  pinnedCliVersion: string;
}

export type MutationKind = "install" | "update" | "uninstall";

/** Startup verification for a mutation whose process ended before finishing. */
export interface InterruptedReport {
  markerPath: string;
  /** Scope that owns the interrupted mutation; legacy markers are global. */
  scope?: ScopeId;
  markerCorrupt: boolean;
  markerError?: string;
  taskId?: string;
  kind?: MutationKind;
  skills: string[];
  startedAtEpochMs?: number;
  snapshotPaths: string[];
  snapshotsPresent: string[];
  snapshotsMissing: string[];
  lock: LockHealth;
  detectedAtEpochMs: number;
}

export interface ActiveMutationInfo {
  taskId: string;
  scope?: ScopeId;
  kind: MutationKind;
  skills: string[];
  startedAtEpochMs: number;
  snapshotPaths: string[];
}

export type ManagementState = "managed" | "copy_only" | "unmanaged";
export type UpdateState =
  | "current"
  | "update"
  | "unknown"
  | "error"
  | "path_missing";
export type AttachmentMode = "link" | "copy" | "native" | "broken";
export type LocalModification = "clean" | "modified" | "unknown";
export type AgentOrigin = "catalog" | "builtin" | "user";

export type TaskPhase =
  | "preparing"
  | "cloning"
  | "backup"
  | "running_cli"
  | "verifying"
  | "completed"
  | "failed"
  | "cancelled";

export type TaskAction = "preview" | "install" | "update" | "uninstall";

export type TaskOverall = "success" | "partial" | "failed" | "cancelled";

export type TaskSkillStatus =
  | "installed"
  | "updated"
  | "removed"
  | "skipped"
  | "failed";

export interface TaskSkillResult {
  name: string;
  status: TaskSkillStatus;
  error?: AppError;
  backupPath?: string;
}

export interface SkillConflict {
  management: ManagementState;
  source: string | null;
}

export interface PreviewSkill {
  name: string;
  description: string;
  group?: string;
  conflict?: SkillConflict;
}

export interface PreviewSummary {
  source: string;
  skills: PreviewSkill[];
  rawOutput?: string;
  parseFailed: boolean;
  noSkills: boolean;
}

export interface TaskResult {
  action: TaskAction;
  /** Scope that owns this result; results never cross scopes. */
  scope?: ScopeId;
  overall: TaskOverall;
  skills: TaskSkillResult[];
  warnings: AppError[];
  backupPath?: string;
  logPath?: string;
  startedAt: number;
  finishedAtEpochMs: number;
  preview?: PreviewSummary;
}

export interface TaskEvent {
  taskId: string;
  phase: TaskPhase;
  message: string;
  skill?: string;
  completed?: number;
  total?: number;
  result?: TaskResult;
  error?: AppError;
}

export interface CancelAck {
  taskId: string;
  accepted: boolean;
  message?: string;
}

export type AppDataKind = "backups" | "logs" | "cache" | "baselines";

/** The frontend sends only semantic values; Rust validates and appends universal. */
export interface InstallRequest {
  source: string;
  skills: string[];
  agents: string[];
  /**
   * Explicit retry of failed items from a previous partial install. Rust only
   * allows overwriting a managed/stale lock entry from the same source.
   */
  retry?: boolean;
}

export interface AgentAttachment {
  agentId: string;
  agentName: string;
  path: string;
  mode: AttachmentMode;
  /** Backend-computed detach eligibility (managed + canonical + link/copy + non-universal). */
  detachable: boolean;
}

/** Result of a native detach from one agent. */
export interface DetachResult {
  skillId: string;
  agentId: string;
  agentName: string;
  mode: AttachmentMode;
  attachmentPath: string;
  canonicalPath: string;
  backupPath?: string;
  logPath: string;
  remainingAgents: string[];
  /** True when no agent attachment remains; the skill is still managed. */
  unattached: boolean;
}

export interface InventorySkill {
  /**
   * Scope-composite identity. Global ids are the plain name; project ids are
   * `project:<uuid>/<name>` and are opaque to the UI.
   */
  id: string;
  name: string;
  description: string | null;
  source: string | null;
  management: ManagementState;
  updatedAt: string | null;
  canonicalPath: string | null;
  attachments: AgentAttachment[];
  localModification: LocalModification;
  warnings: string[];
}

export interface InventoryAgent {
  id: string;
  displayName: string;
  universal: boolean;
  shared: boolean;
  globalSkillsDir: string | null;
  /** `catalog` agents come from the vendored CLI data; the rest are extras. */
  origin: AgentOrigin;
  /** False only for extra agents whose detection path does not exist. */
  detected: boolean;
  /** Configured project-relative skills directory (extras only). */
  projectSkillsDir?: string | null;
}

/** Result of a native attach to one agent. */
export interface AttachResult {
  skillId: string;
  agentId: string;
  agentName: string;
  attachmentPath: string;
  canonicalPath: string;
  logPath: string;
}

export interface ExtraAgentInput {
  id: string;
  displayName: string;
  skillsDir: string;
  /** Optional project-relative skills directory (validated by the backend). */
  projectSkillsDir?: string | null;
}

export interface ExtraAgent extends ExtraAgentInput {
  builtin: boolean;
}

export interface ExtraProjectDetachment {
  projectId: string;
  projectName: string;
  /** Scope-composite skill ids detached from this project. */
  detached: string[];
}

export interface RemoveExtraAgentResult {
  id: string;
  displayName: string;
  /** Global skill ids detached before the definition was removed. */
  detached: string[];
  /** Project detachments performed before the definition was removed. */
  projects: ExtraProjectDetachment[];
}

export interface AnomalyGroup {
  count: number;
  examples: string[];
}

export interface AnomalySummary {
  total: number;
  stale: AnomalyGroup;
  unmanaged: AnomalyGroup;
  broken: AnomalyGroup;
  modified: AnomalyGroup;
  /** Project paths whose resolved location escapes the registered root. */
  escape: AnomalyGroup;
}

export interface InventorySnapshot {
  skills: InventorySkill[];
  agents: InventoryAgent[];
  /**
   * Scope install/attach target set (spec §3.5). Project scope includes
   * globally detected catalog agents and configured extras whose project
   * directory does not exist yet.
   */
  installTargets: InventoryAgent[];
  anomalies: AnomalySummary;
  warnings: string[];
  generatedAtEpochMs: number;
}

export type UpdateSourceStatus = "ok" | "cached" | "error" | "skipped";

export interface UpdateSkillResult {
  name: string;
  source: string | null;
  ref: string | null;
  lockHash: string | null;
  remoteHash: string | null;
  state: UpdateState;
  unknownReason?: "not_checked" | "uncheckable";
  cached: boolean;
  lastSuccessAtEpochMs: number | null;
  previousState: UpdateState | null;
  error: AppError | null;
  note: string | null;
}

/** Inventory row overlaid with the latest update report. */
export interface UpdateViewSkill extends InventorySkill {
  update: UpdateState;
  updateResult?: UpdateSkillResult;
}

export interface UpdateSourceResult {
  source: string;
  ref: string | null;
  status: UpdateSourceStatus;
  skillCount: number;
  cached: boolean;
  truncated: boolean;
  lastSuccessAtEpochMs: number | null;
  error: AppError | null;
  note: string | null;
}

export interface UpdateCheckReport {
  skills: UpdateSkillResult[];
  sources: UpdateSourceResult[];
  checkedAtEpochMs: number;
  lastSuccessAtEpochMs: number | null;
  skippedSourceCount: number;
  cachedSourceCount: number;
  warnings: string[];
}

/** One check_updates response; full checks also return the persisted summary. */
export type UpdateCheckResponse =
  | {
      invalidated?: false;
      scope?: ScopeId;
      checkRevision?: string;
      report: UpdateCheckReport;
      state?: UpdateCheckState | null;
    }
  | {
      invalidated: true;
      scope: ScopeId;
      checkRevision: string;
    };

export interface RelocateProjectResult {
  project: ProjectEntry;
  warnings: AppError[];
}
export interface ForgetProjectResult {
  scope: ScopeId;
  warnings: AppError[];
}

export type UpdateCheckOutcome = "ok" | "partial" | "failed";

/** Persisted summary of the last full check; scoped checks never write it. */
export interface UpdateCheckState {
  version: number;
  checkRevision?: string;
  lastAttemptAt: number;
  lastSuccessAt: number | null;
  lastOutcome: UpdateCheckOutcome;
  failedSourceCount: number;
  cachedSourceCount: number;
  skippedSourceCount: number;
  /** Candidate count from the last check, per scope. */
  candidateCount?: number;
}

/** Anomaly summary extended with read-only update anomalies. */
export interface AnomalyView extends AnomalySummary {
  pathMissing: AnomalyGroup;
  checkFailed: AnomalyGroup;
}

export type SkillEntryKind = "file" | "dir" | "broken" | "outside";

export interface SkillDirEntry {
  name: string;
  /** Relative to the skill root, using `/` separators. */
  path: string;
  kind: SkillEntryKind;
  symlink: boolean;
  sizeBytes: number | null;
  markdown: boolean;
  expandable: boolean;
}

export interface SkillDirListing {
  path: string;
  displayPath: string;
  entries: SkillDirEntry[];
  truncated: boolean;
  maxEntries: number;
  maxDepth: number;
}

export type SkillFileStatus = "ok" | "too_large" | "binary" | "unreadable";
export type SkillFileKind = "markdown" | "text" | "image";

export interface SkillFileContent {
  path: string;
  displayPath: string;
  status: SkillFileStatus;
  kind: SkillFileKind;
  content: string | null;
  dataUrl: string | null;
  sizeBytes: number | null;
  limitBytes: number;
  error: string | null;
}

export interface SkillDetail {
  id: string;
  name: string;
  description: string | null;
  source: string | null;
  sourceType: string | null;
  ref: string | null;
  skillPath: string | null;
  installedAt: string | null;
  updatedAt: string | null;
  canonicalPath: string | null;
  directoryPath: string | null;
  directoryMtimeEpochMs: number | null;
  hash: string | null;
  management: ManagementState;
  localModification: LocalModification;
  localModificationDetail: string | null;
  attachments: AgentAttachment[];
  warnings: string[];
}
