import { useRef } from "react";
import { AlertTriangle, X } from "lucide-react";
import { Button } from "./ui/button";
import { useModalFocus } from "../hooks/use-modal-focus";
import type { AnomalyGroup, AnomalyView } from "../lib/types";

interface AnomalyBannerProps {
  anomalies: AnomalyView;
  onOpen: () => void;
}

export function AnomalyBanner({ anomalies, onOpen }: AnomalyBannerProps) {
  if (anomalies.total === 0) return null;
  return (
    <button
      type="button"
      onClick={onOpen}
      className="anomaly-strip"
      aria-label={`异常提示：异常 ${anomalies.total}`}
    >
      <AlertTriangle
        className="size-3.5 shrink-0 self-start"
        aria-hidden="true"
      />
      <span className="min-w-0 flex-1">
        <strong className="font-medium">异常 {anomalies.total}</strong>
        <span className="text-warning/90">
          {" "}
          · {anomalies.stale.count} 条 stale lock · {anomalies.unmanaged.count}{" "}
          个未受管 · {anomalies.broken.count} 个断链 ·{" "}
          {anomalies.modified.count} 个本地改动 · {anomalies.escape.count}{" "}
          个路径逃逸 · {anomalies.pathMissing.count} 个源路径缺失 ·{" "}
          {anomalies.checkFailed.count} 个检查失败
        </span>
      </span>
      <span className="link">查看异常提示</span>
    </button>
  );
}

interface AnomalyDialogProps {
  anomalies: AnomalyView;
  open: boolean;
  onClose: () => void;
  onProcessStale?: () => void;
  disabled?: boolean;
}

const CATEGORIES: Array<{
  key: keyof Pick<
    AnomalyView,
    | "stale"
    | "unmanaged"
    | "broken"
    | "modified"
    | "escape"
    | "pathMissing"
    | "checkFailed"
  >;
  label: string;
  description: string;
}> = [
  {
    key: "stale",
    label: "失效 lock 条目",
    description: "目录已不存在；不进入主列表，也不会出现在更新候选中。",
  },
  {
    key: "unmanaged",
    label: "未受管",
    description: "磁盘上有目录但没有安装记录；可以挂载或卸载，不能检查更新。",
  },
  {
    key: "broken",
    label: "断链",
    description: "agent 挂载的符号链接目标缺失；只读提示，不自动修复。",
  },
  {
    key: "modified",
    label: "本地改动",
    description:
      "当前文件与基线或 lock hash 不一致；「本地改动?」表示首次扫描无法确认，更新前需确认并备份。",
  },
  {
    key: "escape",
    label: "路径逃逸",
    description:
      "项目 canonical 或 agent 项目目录解析后位于项目根之外；只读提示，禁止读取内容。",
  },
  {
    key: "pathMissing",
    label: "源路径缺失",
    description:
      "上游仓库仍存在，但 lock 记录的 skillPath 已找不到；不计为可更新，更新禁用。",
  },
  {
    key: "checkFailed",
    label: "检查失败",
    description:
      "网络、限流或仓库不可达；保留上次成功结果，绝不显示为已是最新。",
  },
];

function CategoryCard({
  label,
  description,
  group,
}: {
  label: string;
  description: string;
  group: AnomalyGroup;
}) {
  return (
    <section className="panel-well">
      <div className="flex items-baseline gap-2">
        <strong className="num text-xl font-semibold leading-none">
          {group.count}
        </strong>
        <span className="text-[12.5px] font-medium">{label}</span>
      </div>
      <p className="mt-1.5 text-[12px] leading-relaxed text-muted-foreground">
        {description}
      </p>
      {group.examples.length > 0 ? (
        <ul className="mt-1.5 space-y-0.5 text-[12px] text-muted-foreground">
          {group.examples.map((example) => (
            <li key={example} className="mono truncate">
              {example}
            </li>
          ))}
        </ul>
      ) : (
        <p className="mt-1.5 text-[12px] text-muted-foreground">—</p>
      )}
    </section>
  );
}

export function AnomalyDialog({ anomalies, open, onClose, onProcessStale, disabled }: AnomalyDialogProps) {
  const closeRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useModalFocus({
    open,
    panelRef,
    initialFocusRef: closeRef,
    onClose,
  });

  if (!open) return null;

  return (
    <div
      className="modal-overlay"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onCloseRef.current();
      }}
    >
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="anomaly-title"
        className="modal-panel max-w-xl"
      >
        <div className="modal-head">
          <div>
            <h2 id="anomaly-title" className="modal-title">
              <AlertTriangle
                className="size-4 text-warning"
                aria-hidden="true"
              />
              异常提示
            </h2>
            <p className="modal-sub">
              失效安装记录可清理或恢复；其他异常展示各自原因。
            </p>
          </div>
          <Button
            ref={closeRef}
            variant="ghost"
            size="icon"
            aria-label="关闭异常提示"
            onClick={onClose}
            className="size-7 shrink-0"
          >
            <X className="size-4" aria-hidden="true" />
          </Button>
        </div>

        <div className="modal-body">
          <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
            {CATEGORIES.map((category) => (
              <div key={category.key}>
              <CategoryCard
                key={category.key}
                label={category.label}
                description={category.description}
                group={anomalies[category.key]}
              />
              {category.key === "stale" && anomalies.stale.count > 0 && onProcessStale ? <Button variant="outline" size="sm" className="mt-2" disabled={disabled} onClick={onProcessStale}>处理</Button> : null}
              </div>
            ))}
          </div>

          <p className="mt-3 rounded-md bg-raised px-3 py-2 text-[12px] text-muted-foreground">
            清理或恢复先预览再确认，执行前自动备份；已完成的操作保留。
          </p>
        </div>
      </div>
    </div>
  );
}
