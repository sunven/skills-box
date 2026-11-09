import type { AppDataReport } from "../lib/types";

export function appDataFixture(): AppDataReport {
  const mib = 1024 * 1024;
  return {
    directory: "~/Library/Application Support/com.sunven.skills-box",
    preferences: { retainBackups: true, operationLogging: true },
    totalBytes: 161 * mib, cleanableBytes: 140 * mib,
    categories: [
      { category: "backups", bytes: 60*mib, cleanableBytes: 40*mib, count: 3 },
      { category: "logs", bytes: 10*mib, cleanableBytes: 10*mib, count: 1 },
      { category: "cache", bytes: 90*mib, cleanableBytes: 90*mib, count: 2 },
      { category: "preserved", bytes: mib, cleanableBytes: 0, count: 5 },
      { category: "other", bytes: 0, cleanableBytes: 0, count: 0 },
    ],
    backups: [
      { id: "backup-alpha", name: "agent-browser", action: "uninstall", scope: "global", createdAtEpochMs: 1794200400000, bytes: 24*mib, protectedReason: null },
      { id: "backup-beta", name: "design-taste-frontend", action: "update", scope: "global", createdAtEpochMs: 1794196800000, bytes: 16*mib, protectedReason: null },
      { id: "backup-protected", name: "research", action: "update", scope: "global", createdAtEpochMs: 1794193200000, bytes: 20*mib, protectedReason: "运行中或中断操作正在引用" },
    ],
    warnings: [], scannedAtEpochMs: 1794204000000,
  };
}
