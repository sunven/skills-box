export type PathSource =
  | "override"
  | "volta"
  | "fnm"
  | "nvm"
  | "homebrew"
  | "system"
  | "login_shell"
  | "none";

export type ToolState = "ok" | "missing" | "too_old" | "error" | "not_checked";

export type LockStatus = "ok" | "missing" | "corrupt" | "unsupported" | "unreadable";

export interface AppError {
  code: string;
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
  issues: AppError[];
  warnings: string[];
  checkedAtEpochMs: number;
  pinnedCliVersion: string;
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

export type TaskPhase =
  | "preparing"
  | "cloning"
  | "running_cli"
  | "verifying"
  | "completed"
  | "failed"
  | "cancelled";

export type TaskOverall = "success" | "partial" | "failed" | "cancelled";

export type TaskSkillStatus = "installed" | "skipped" | "failed";

export interface TaskSkillResult {
  name: string;
  status: TaskSkillStatus;
  error?: AppError;
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
  overall: TaskOverall;
  skills: TaskSkillResult[];
  warnings: AppError[];
  backupPath?: string;
  logPath?: string;
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

/** The frontend sends only semantic values; Rust validates and appends universal. */
export interface InstallRequest {
  source: string;
  skills: string[];
  agents: string[];
}

export interface AgentAttachment {
  agentId: string;
  agentName: string;
  path: string;
  mode: AttachmentMode;
}

export interface InventorySkill {
  name: string;
  description: string | null;
  source: string | null;
  management: ManagementState;
  update: UpdateState;
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
}

export interface InventorySnapshot {
  skills: InventorySkill[];
  agents: InventoryAgent[];
  anomalies: AnomalySummary;
  warnings: string[];
  generatedAtEpochMs: number;
}

export type SkillMdStatus = "ok" | "missing" | "too_large" | "unreadable";

export interface SkillMd {
  status: SkillMdStatus;
  path: string | null;
  content: string | null;
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
  update: UpdateState;
  localModification: LocalModification;
  localModificationDetail: string | null;
  attachments: AgentAttachment[];
  warnings: string[];
  skillMd: SkillMd;
}
