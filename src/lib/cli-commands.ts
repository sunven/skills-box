import type { InstallRequest, ScopeId } from "./types";

export type CliCommandRequest =
  | { action: "preview"; source: string }
  | { action: "install"; request: InstallRequest }
  | { action: "update"; skills: string[] }
  | { action: "uninstall"; skill: string };
export interface CliCommandEntry { name: string; command: string | null; reason: string | null }
export interface CliCommandPreview { id: string; entries: CliCommandEntry[] }
export interface CliCommandRecord {
  id: string; command: string; status: "running" | "succeeded" | "failed" | "cancelled";
  startedAt: number;
  context: { origin: "task" | "environment" | "discovery"; scope: ScopeId; taskId: string | null; source: string | null };
}
export const cliCommandsQueryKey = ["cli-commands"] as const;
export function mergeCommandRecord(records: CliCommandRecord[], record: CliCommandRecord): CliCommandRecord[] {
  const previous = records.find((old) => old.id === record.id);
  // A snapshot or delayed event must never undo a terminal status.
  if (previous && previous.status !== "running" && record.status === "running") return records;
  if (record.context.origin !== "task" && records.some((old) =>
    JSON.stringify(old.context) === JSON.stringify(record.context) && old.startedAt > record.startedAt)) return records;
  return [...records.filter((old) => old.id !== record.id &&
    (record.context.origin === "task" || JSON.stringify(old.context) !== JSON.stringify(record.context))), record];
}
