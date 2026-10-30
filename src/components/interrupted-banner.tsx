import { AlertTriangle, ArchiveRestore } from "lucide-react";
import { Button } from "./ui/button";
import type { InterruptedReport, LockHealth, MutationKind, ScopeId } from "../lib/types";
import { GLOBAL_SCOPE } from "../lib/types";

const KIND_LABELS: Record<MutationKind, string> = {
  install: "安装",
  update: "更新",
  uninstall: "卸载",
};

const LOCK_LABELS: Record<LockHealth["status"], string> = {
  ok: "lock v3 正常",
  missing: "lock 不存在（首次安装前中断）",
  corrupt: "lock 损坏",
  unsupported: "lock 版本不受支持",
  unreadable: "lock 无法读取",
};

interface InterruptedBannerProps {
  report: InterruptedReport | null | undefined;
  /** Only the scope that owns the interrupted mutation sees the warning. */
  scope: ScopeId;
  onDismiss: () => void;
  onRevealBackups: () => void;
}

/**
 * Startup warning when a mutation marker survived (crash or confirmed exit
 * during a write). It reports the lock health and which snapshots still exist;
 * it never mutates the skill store.
 */
export function InterruptedBanner({
  report,
  scope,
  onDismiss,
  onRevealBackups,
}: InterruptedBannerProps) {
  if (!report) return null;
  // Markers written before scopes existed belong to the global scope.
  if ((report.scope ?? GLOBAL_SCOPE) !== scope) return null;

  const kind = report.kind ? KIND_LABELS[report.kind] : "变更";
  const missing = report.snapshotsMissing.length;
  return (
    <div
      role="alert"
      data-testid="interrupted-banner"
      className="border-b border-warning/30 bg-warning-soft px-6 py-3 text-warning"
    >
      <div className="flex items-start gap-3">
        <AlertTriangle className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
        <div className="min-w-0 flex-1">
          <p className="text-[13px] font-semibold">检测到上次变更被中断</p>
          {report.markerCorrupt ? (
            <p className="mt-0.5 text-[12.5px] leading-relaxed">
              中断记录无法解析（{report.markerError ?? "未知错误"}）。请手动检查
              lock、canonical 与 agent 挂载。
            </p>
          ) : (
            <p className="mt-0.5 text-[12.5px] leading-relaxed">
              {kind} · {report.skills.join("、") || "—"} · 已保留快照{" "}
              {report.snapshotsPresent.length} 个
              {missing > 0 ? `，缺失 ${missing} 个` : ""}
            </p>
          )}
          <p className="mt-0.5 text-[12.5px] leading-relaxed">
            {LOCK_LABELS[report.lock.status]}
            {report.lock.version ? `（v${report.lock.version}）` : ""}
            {missing > 0
              ? " · 记录的备份路径已不存在，请检查 app 数据目录。"
              : " · 应用不会自动回滚，请按需从备份恢复。"}
          </p>
          <div className="mt-2 flex flex-wrap gap-2">
            <Button variant="outline" size="sm" onClick={onRevealBackups}>
              <ArchiveRestore aria-hidden="true" />
              打开备份目录
            </Button>
            <Button variant="ghost" size="sm" onClick={onDismiss}>
              知道了
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
