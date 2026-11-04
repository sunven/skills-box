import {
  Activity,
  lazy,
  memo,
  Suspense,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import {
  ArrowUpCircle,
  Link2,
  Loader2,
  RefreshCw,
  Trash2,
  Unlink,
  X,
} from "lucide-react";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { AttachDialog } from "./attach-dialog";
import { DetachDialog } from "./detach-dialog";
import { asAppError, detachAgent } from "../lib/api";
import { agentInitials } from "../lib/agent-style";
import { agentLogoUrl } from "../lib/agent-logos";
import { promiseDriver, useMutation } from "../lib/mutation";
import type {
  AgentAttachment,
  DetachResult,
  InventoryAgent,
  ScopeId,
  SkillDetail,
} from "../lib/types";
import {
  ATTACHMENT_LABELS,
  LOCAL_MODIFICATION_LABELS,
  UPDATE_LABELS,
  formatEpochMs,
  formatUpdatedAt,
  shortHash,
} from "../lib/inventory-view";
import { formatEpochDateTime } from "../lib/update-view";
import type { UpdateViewSkill } from "../lib/types";
import { UpdateStatus } from "./update-status";
import { useSkillDetailQuery } from "../hooks/use-skill-detail";
import { selectDetailView } from "../lib/detail-view";
import { cn } from "../lib/utils";

// Load the Markdown stack when the content tab is first opened.
const SkillContentTab = lazy(() =>
  import("./skill-content-tab").then((module) => ({
    default: module.SkillContentTab,
  })),
);

const NO_AGENTS: InventoryAgent[] = [];

interface DetailPaneProps {
  scope: ScopeId;
  skill: UpdateViewSkill;
  entry?: DetailEntry | null;
  agents?: InventoryAgent[];
  /** Unavailable/error scope: every attachment mutation is disabled. */
  actionsDisabled?: boolean;
  checkingUpdates?: boolean;
  onCheckUpdates?: (skillIds: string[]) => void;
  onUpdate?: (skillName: string) => void;
  onUninstall?: (skillName: string) => void;
  onClose: () => void;
}

type DetailTab = "content" | "actions";

export interface DetailEntry {
  destination: "actions" | "content" | "attachments";
}

function MetaRow({
  label,
  mono,
  children,
}: {
  label: string;
  mono?: boolean;
  children: ReactNode;
}) {
  return (
    <>
      <dt>{label}</dt>
      <dd className={cn(mono && "mono")}>{children}</dd>
    </>
  );
}

function HashValue({ hash }: { hash: string }) {
  const [expandedHash, setExpandedHash] = useState<string | null>(null);
  const expanded = expandedHash === hash;
  return (
    <span className="inline-flex flex-wrap items-center gap-1.5">
      <code className="mono break-all">
        {expanded ? hash : shortHash(hash)}
      </code>
      {hash.length > 12 ? (
        <button
          type="button"
          className="text-[12px] text-muted-foreground underline underline-offset-2 hover:text-foreground"
          aria-label={expanded ? "收起完整 hash" : "查看完整 hash"}
          onClick={() => setExpandedHash(expanded ? null : hash)}
        >
          {expanded ? "收起" : "完整"}
        </button>
      ) : null}
    </span>
  );
}

const DESCRIPTION_COLLAPSE_LENGTH = 200;

function ClampedDescription({ text }: { text: string }) {
  const [expandedText, setExpandedText] = useState<string | null>(null);
  const [overflowing, setOverflowing] = useState(false);
  const textRef = useRef<HTMLParagraphElement>(null);
  const measureRef = useRef<HTMLSpanElement>(null);
  const expanded = expandedText === text;

  useEffect(() => {
    const element = textRef.current;
    const measure = measureRef.current;
    if (!element || !measure) return;
    const update = () => {
      const clampedHeight = element.clientHeight;
      const fullHeight = measure.clientHeight;
      const measured =
        clampedHeight > 0 && fullHeight > 0
          ? fullHeight > clampedHeight + 1
          : text.length > DESCRIPTION_COLLAPSE_LENGTH;
      setOverflowing(measured);
    };
    update();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(update);
    observer.observe(element);
    observer.observe(measure);
    return () => observer.disconnect();
  }, [expanded, text]);

  return (
    <div className="detail-desc-wrap">
      <p
        ref={textRef}
        className="detail-desc"
        data-testid="detail-description"
        data-expanded={expanded ? "true" : "false"}
      >
        {text}
      </p>
      <span
        ref={measureRef}
        className="detail-desc-measure"
        aria-hidden="true"
      >
        {text}
      </span>
      {overflowing || expanded ? (
        <button
          type="button"
          className="detail-desc-toggle"
          aria-expanded={expanded}
          onClick={() => setExpandedText(expanded ? null : text)}
        >
          {expanded ? "收起" : "展开"}
        </button>
      ) : null}
    </div>
  );
}

function AgentAttachmentRow({
  attachment,
  onDetach,
  disabled = false,
}: {
  attachment: SkillDetail["attachments"][number];
  onDetach: (attachment: AgentAttachment) => void;
  disabled?: boolean;
}) {
  const broken = attachment.mode === "broken";
  const logo = agentLogoUrl(attachment.agentId);
  return (
    <li
      data-testid="detail-agent-row"
      className="attach-row"
      data-broken={broken ? "true" : "false"}
    >
      <span className="agent-token" aria-hidden="true">
        {logo ? (
          <img
            src={logo}
            alt=""
            draggable={false}
            className="agent-token-logo"
          />
        ) : (
          agentInitials(attachment.agentName)
        )}
      </span>
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
          <span className="attach-name">{attachment.agentName}</span>
          <span className="attach-mode">
            {ATTACHMENT_LABELS[attachment.mode]}
          </span>
        </div>
        <p className="attach-path" title={attachment.path}>
          {attachment.path}
        </p>
        {broken ? (
          <p className="mt-1 text-[12px] text-destructive">
            链接目标缺失；只读提示，不自动修复。
          </p>
        ) : null}
        {attachment.mode === "native" ? (
          <p className="mt-1 text-[12px] text-muted-foreground">
            共享 canonical store，不产生独立挂载。
          </p>
        ) : null}
      </div>
      {attachment.detachable ? (
        <Button
          variant="ghost"
          size="sm"
          className="ml-auto h-7 shrink-0 px-2 text-[12px]"
          aria-label={`断开 ${attachment.agentName}`}
          disabled={disabled}
          onClick={() => onDetach(attachment)}
        >
          <Unlink aria-hidden="true" />
          断开
        </Button>
      ) : null}
    </li>
  );
}

export const DetailPane = memo(function DetailPane({
  scope,
  skill,
  entry = null,
  agents = NO_AGENTS,
  actionsDisabled = false,
  checkingUpdates = false,
  onCheckUpdates,
  onUpdate,
  onUninstall,
  onClose,
}: DetailPaneProps) {
  const skillId = skill.id;
  const query = useSkillDetailQuery(scope, skillId);
  const detail = query.data ?? null;
  const [tab, setTab] = useState<DetailTab>(entry?.destination === "content" ? "content" : "actions");
  const [contentVisited, setContentVisited] = useState(entry?.destination === "content");
  const [detachTarget, setDetachTarget] = useState<AgentAttachment | null>(null);
  const [detachNotice, setDetachNotice] = useState<DetachResult | null>(null);
  const [attachOpen, setAttachOpen] = useState(false);
  const closeRef = useRef<HTMLButtonElement>(null);
  const attachmentsRef = useRef<HTMLElement>(null);
  const detailError = asAppError(query.error);
  const detachMutation = useMutation({
    action: "detach",
    scope,
    skillId,
    fallbackFailure: "断开失败，请刷新后重试。",
    fallbackStart: "断开失败，请刷新后重试。",
  });

  // Selecting another skill restarts the drawer UI. Adjusting state during
  // rendering avoids an extra commit that would flash the previous skill's
  // tab/dialog state.
  const [activeSkillId, setActiveSkillId] = useState(skillId);
  const [activeEntry, setActiveEntry] = useState(entry);
  if (activeSkillId !== skillId) {
    setActiveSkillId(skillId);
    setTab("actions");
    setContentVisited(false);
    setDetachTarget(null);
    setDetachNotice(null);
    setAttachOpen(false);
  }
  if (activeEntry !== entry) {
    setActiveEntry(entry);
    setTab(entry?.destination === "content" ? "content" : "actions");
    if (entry?.destination === "content") setContentVisited(true);
  }

  useEffect(() => {
    detachMutation.reset();
  }, [skillId, detachMutation.reset]);

  useEffect(() => {
    const previousFocus = document.activeElement as HTMLElement | null;
    closeRef.current?.focus({ preventScroll: true });
    return () => previousFocus?.focus({ preventScroll: true });
  }, []);

  useEffect(() => {
    if (entry?.destination === "attachments") {
      attachmentsRef.current?.scrollIntoView?.({ block: "start" });
      attachmentsRef.current?.focus({ preventScroll: true });
    }
  }, [entry]);

  useEffect(() => {
    const handleKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      // A stacked confirmation dialog owns Escape while it is open.
      if (document.querySelector(".modal-overlay")) return;
      event.preventDefault();
      onClose();
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [onClose]);

  const detailView = useMemo(
    () => selectDetailView(skill, detail, agents),
    [skill, detail, agents],
  );
  const {
    management,
    update,
    updateResult: updateInfo,
    localModification,
    attachments,
    warnings,
    broken,
    unattached,
    attachableAgents,
    localWarning,
    frontmatterWarning,
  } = detailView;
  const directoryLabel =
    management === "copy_only" ? "agent 路径" : "canonical 路径";

  const openDetach = (attachment: AgentAttachment) => {
    detachMutation.reset();
    setDetachTarget(attachment);
  };

  const confirmDetach = async () => {
    const target = detachTarget;
    if (!target || actionsDisabled) return;
    await detachMutation.start(
      promiseDriver({
        total: 0,
        message: `正在断开 ${target.agentName}…`,
        indeterminate: true,
        run: async () => {
          const detached = await detachAgent(
            skill.id,
            target.agentId,
            scope,
          );
          setDetachNotice(detached);
          setDetachTarget(null);
          return {
            rows: [
              {
                name: detached.agentName,
                status: "success" as const,
                statusLabel: "已断开",
              },
            ],
            backupPath: detached.backupPath,
          };
        },
      }),
    );
  };

  const selectTab = (next: DetailTab) => {
    setTab(next);
    if (next === "content") setContentVisited(true);
  };

  const handleTabKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const next: DetailTab = tab === "content" ? "actions" : "content";
    selectTab(next);
    document.getElementById(`detail-tab-${next}`)?.focus();
  };

  const sourceLabel = detail?.source ?? skill.source;
  const sourceTypeLabel =
    detail?.sourceType === "github"
      ? " · 公开 GitHub"
      : detail?.sourceType
        ? ` · ${detail.sourceType}`
        : "";
  const hasAlerts =
    query.isPending || Boolean(detailError) || detailView.hasAlerts;

  return (
    <>
      <div
        className="detail-scrim"
        data-testid="detail-scrim"
        aria-hidden="true"
        onClick={onClose}
      />
      <aside
        aria-label="详情"
        aria-busy={query.isPending}
        data-testid="detail-drawer"
        className="detail-pane"
      >
        <div className="detail-head">
          <h2 className="sr-only">详情</h2>
          <Button
            ref={closeRef}
            variant="ghost"
            size="icon"
            className="detail-close size-7"
            aria-label="关闭详情"
            onClick={onClose}
          >
            <X className="size-4" aria-hidden="true" />
          </Button>
          <div className="detail-heading">
            <div className="detail-title-line">
              <h2 className="detail-title">{detailView.name}</h2>
              <UpdateStatus
                skill={{ ...skill, updateResult: updateInfo ?? undefined }}
                testId="detail-update-badge"
              />
              {management === "copy_only" ? (
                <span className="skill-note">仅副本</span>
              ) : null}
              {management === "unmanaged" ? (
                <span className="skill-note" data-tone="warning">
                  未受管
                </span>
              ) : null}
              {localWarning ? (
                <span
                  className="skill-note"
                  data-tone={
                    localModification === "modified" ? "warning" : undefined
                  }
                  title={
                    localModification === "modified"
                      ? "当前文件与基线指纹不一致；更新或卸载前请确认并备份。"
                      : "首次扫描没有可比较的基线或 lock hash；更新或卸载前请确认。"
                  }
                >
                  {LOCAL_MODIFICATION_LABELS[localModification]}
                </span>
              ) : null}
              {unattached ? <span className="skill-note">未挂载</span> : null}
              {broken ? (
                <span className="skill-note" data-tone="negative">
                  断链
                </span>
              ) : null}
              {warnings.length > 0 ? (
                <Badge variant="warning" title={warnings.join("\n")}>
                  警告
                </Badge>
              ) : null}
            </div>
            <div
              className="detail-update-summary"
              data-testid="detail-update-summary"
            >
              {management === "managed" ? (
                <div className="detail-update-actions">
                  {onCheckUpdates ? (
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={checkingUpdates}
                      aria-busy={checkingUpdates}
                      onClick={() => onCheckUpdates([skill.name])}
                      title="只读检查该 skill 的来源；不会运行 skills check"
                    >
                      <RefreshCw
                        className={
                          checkingUpdates
                            ? "size-3.5 animate-spin"
                            : "size-3.5"
                        }
                        aria-hidden="true"
                      />
                      检查更新
                    </Button>
                  ) : null}
                  {update === "update" && onUpdate ? (
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => onUpdate(skill.name)}
                      title="更新前备份；使用当前物理挂载的 agent 重建 skills add"
                    >
                      <ArrowUpCircle className="size-3.5" aria-hidden="true" />
                      更新
                    </Button>
                  ) : null}
                </div>
              ) : (
                <p className="text-[12px] leading-relaxed text-muted-foreground">
                  {management === "copy_only"
                    ? "仅副本 skill 不提供更新检查。"
                    : "未受管 skill 不提供更新检查。"}
                </p>
              )}
              <dl className="detail-update-meta">
                <MetaRow label="上次检查">
                  <span
                    data-testid="detail-last-update-check"
                    title={
                      updateInfo?.error?.message ??
                      updateInfo?.note ??
                      undefined
                    }
                  >
                    {updateInfo?.lastSuccessAtEpochMs
                      ? `${formatEpochDateTime(updateInfo.lastSuccessAtEpochMs)}${
                          updateInfo.cached ? "（缓存）" : ""
                        }`
                      : "尚未检查"}
                  </span>
                </MetaRow>
              </dl>
              {management !== "unmanaged" && onUninstall ? (
                <Button
                  variant="outline"
                  size="sm"
                  className="shrink-0 border-destructive/35 text-destructive hover:bg-destructive-soft hover:text-destructive"
                  onClick={() => onUninstall(skill.name)}
                  title="删除 canonical、所有 agent 挂载、lock 条目与基线；先备份，不自动回滚"
                >
                  <Trash2 className="size-3.5" aria-hidden="true" />
                  彻底卸载
                </Button>
              ) : null}
            </div>
          </div>
          <ClampedDescription
            text={detail?.description ?? skill.description ?? "—"}
          />
        </div>

        {hasAlerts ? (
          <div className="detail-alerts" data-testid="detail-alerts">
            {query.isPending ? (
              <div
                role="status"
                className="flex items-center gap-1.5 text-[12px] text-muted-foreground"
              >
                <Loader2 className="size-3 animate-spin" aria-hidden="true" />
                正在读取详情…
              </div>
            ) : null}

            {detailError ? (
              <div role="alert" className="notice" data-tone="negative">
                <div>
                  <p className="font-medium">详情读取失败</p>
                  <p className="mt-0.5">{detailError.message}</p>
                  {detailError.hint ? (
                    <p className="mt-0.5">{detailError.hint}</p>
                  ) : null}
                  <Button
                    variant="outline"
                    size="sm"
                    className="mt-2"
                    onClick={() => void query.refetch()}
                    disabled={query.isFetching}
                  >
                    重试读取
                  </Button>
                </div>
              </div>
            ) : null}

            {management === "unmanaged" ? (
              <div className="notice" data-tone="warning">
                未受管：该目录没有 lock 条目，v1 只读展示，不提供更新或卸载。
              </div>
            ) : null}
            {management === "copy_only" ? (
              <div className="notice">
                仅副本：没有 canonical 目录；不可更新或断开，可彻底卸载。
              </div>
            ) : null}
            {localModification === "modified" ? (
              <div className="notice" data-tone="warning">
                <div>
                  检测到本地改动：共享副本与基线记录不一致。更新会覆盖修改，请先备份。
                  {detail?.localModificationDetail
                    ? ` ${detail.localModificationDetail}`
                    : ""}
                </div>
              </div>
            ) : null}
            {localModification === "unknown" && management !== "unmanaged" ? (
              <div className="notice">
                <div>
                  无法确认本地改动：首次扫描没有可比较的基线，lock hash
                  也无法比对。更新或卸载前需要你确认。
                  {detail?.localModificationDetail
                    ? ` ${detail.localModificationDetail}`
                    : ""}
                </div>
              </div>
            ) : null}

            {updateInfo?.error ? (
              <div
                role="alert"
                data-testid="update-check-error"
                className="notice"
                data-tone="negative"
              >
                <div>
                  <p className="font-medium">
                    检查失败：{updateInfo.error.message}
                  </p>
                  {updateInfo.error.detail ? (
                    <p className="mono mt-0.5 break-all">
                      {updateInfo.error.detail}
                    </p>
                  ) : null}
                  {updateInfo.error.hint ? (
                    <p className="mt-0.5">{updateInfo.error.hint}</p>
                  ) : null}
                  <p className="mt-0.5">
                    {updateInfo.lastSuccessAtEpochMs
                      ? `上次成功检查：${formatEpochDateTime(updateInfo.lastSuccessAtEpochMs)}${
                          updateInfo.previousState &&
                          updateInfo.previousState !== "current"
                            ? ` · 上次结果：${UPDATE_LABELS[updateInfo.previousState]}`
                            : ""
                        }`
                      : "尚未成功检查过该来源；不会显示为已是最新。"}
                  </p>
                </div>
              </div>
            ) : null}

            {update === "path_missing" ? (
              <div
                data-testid="update-path-missing"
                className="notice"
                data-tone="negative"
              >
                {updateInfo?.note ??
                  "来源中已找不到该 skill，可能已被删除或移动；更新已禁用。"}
              </div>
            ) : null}

            {updateInfo &&
            !updateInfo.error &&
            update !== "path_missing" &&
            updateInfo.note ? (
              <div className="notice">{updateInfo.note}</div>
            ) : null}

            {warnings.length > 0 ? (
              <section
                aria-label="详情警告"
                data-testid="detail-warnings"
                className="notice"
                data-tone="warning"
              >
                <div>
                  <p className="font-medium">警告</p>
                  <ul className="mt-0.5 list-disc pl-4">
                    {warnings.map((warning) => (
                      <li key={warning}>{warning}</li>
                    ))}
                  </ul>
                </div>
              </section>
            ) : null}
          </div>
        ) : null}

        <div
          className="detail-tabs"
          role="tablist"
          aria-label="详情分区"
          onKeyDown={handleTabKeyDown}
        >
          <button
            type="button"
            role="tab"
            id="detail-tab-actions"
            aria-controls="detail-panel-actions"
            aria-selected={tab === "actions"}
            tabIndex={tab === "actions" ? 0 : -1}
            className="detail-tab"
            data-selected={tab === "actions" ? "true" : "false"}
            onClick={() => selectTab("actions")}
          >
            操作
          </button>
          <button
            type="button"
            role="tab"
            id="detail-tab-content"
            aria-controls="detail-panel-content"
            aria-selected={tab === "content"}
            tabIndex={tab === "content" ? 0 : -1}
            className="detail-tab"
            data-selected={tab === "content" ? "true" : "false"}
            onClick={() => selectTab("content")}
          >
            内容
          </button>
        </div>

        <div className="detail-panels">
          <section
            role="tabpanel"
            id="detail-panel-content"
            aria-labelledby="detail-tab-content"
            hidden={tab !== "content"}
            className="detail-panel detail-panel-content"
            data-testid="detail-content-panel"
          >
            {contentVisited ? (
              <Activity mode={tab === "content" ? "visible" : "hidden"}>
                <Suspense
                  fallback={
                    <div className="content-empty">
                      <Loader2
                        className="size-4 animate-spin"
                        aria-hidden="true"
                      />
                      <p>正在加载内容视图…</p>
                    </div>
                  }
                >
                  <SkillContentTab
                    key={skill.id}
                    scope={scope}
                    skillId={skill.id}
                    frontmatterWarning={frontmatterWarning}
                  />
                </Suspense>
              </Activity>
            ) : null}
          </section>

          <section
            role="tabpanel"
            id="detail-panel-actions"
            aria-labelledby="detail-tab-actions"
            hidden={tab !== "actions"}
            className="detail-panel detail-panel-actions detail-scroll"
            data-testid="detail-actions-panel"
          >
            {detachNotice ? (
              <div
                role="status"
                data-testid="detach-result"
                className="notice"
                data-tone="positive"
              >
                <div>
                  <p className="font-medium">
                    已从 {detachNotice.agentName} 断开；canonical 与 lock 已保留。
                  </p>
                  {detachNotice.unattached ? (
                    <p className="mt-0.5">已无 agent 挂载，显示「未挂载」。</p>
                  ) : null}
                  {detachNotice.backupPath ? (
                    <p className="mono mt-0.5 break-all">
                      备份：{detachNotice.backupPath}
                    </p>
                  ) : null}
                </div>
              </div>
            ) : null}

            <section ref={attachmentsRef} tabIndex={-1} aria-label="Agent 挂载" className="detail-section">
              <div className="flex items-center justify-between gap-2">
                <h4 className="detail-section-title">
                  Agent 挂载{" "}
                  <span className="count">{attachments.length} 个</span>
                </h4>
                {management === "managed" ? (
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 shrink-0 px-2 text-[12px]"
                    data-testid="detail-attach"
                    disabled={actionsDisabled || attachableAgents.length === 0}
                    title={
                      actionsDisabled
                        ? "当前作用域不可用，挂载已禁用"
                        : attachableAgents.length === 0
                          ? "没有可挂载的 agent（需已检测、未挂载且不共享 canonical）"
                          : "为这个 skill 追加一个 agent 挂载"
                    }
                    onClick={() => setAttachOpen(true)}
                  >
                    <Link2 aria-hidden="true" />
                    挂载到…
                  </Button>
                ) : null}
              </div>
              {attachments.length === 0 ? (
                <p className="notice mt-2.5">
                  未挂载任何 agent。共享副本仍保留，可在安装向导中重新挂载。
                </p>
              ) : (
                <ul className="mt-2.5 space-y-1.5">
                  {attachments.map((attachment) => (
                    <AgentAttachmentRow
                      key={`${attachment.agentId}:${attachment.path}`}
                      attachment={attachment}
                      disabled={actionsDisabled}
                      onDetach={openDetach}
                    />
                  ))}
                </ul>
              )}
            </section>

            {detail ? (
              <section className="detail-section">
                <h4 className="detail-section-title">概览</h4>
                <dl className="meta-list">
                  <MetaRow label="来源">
                    <span>{sourceLabel ?? "来源未知"}</span>
                    {sourceTypeLabel ? (
                      <span className="text-muted-foreground">
                        {sourceTypeLabel}
                      </span>
                    ) : null}
                  </MetaRow>
                  <MetaRow label="ref">{detail.ref ?? "—"}</MetaRow>
                  <MetaRow label="skillPath" mono>
                    <span
                      className="path-value"
                      title={detail.skillPath ?? undefined}
                    >
                      {detail.skillPath ?? "—"}
                    </span>
                  </MetaRow>
                  <MetaRow label="安装时间">
                    <span title={detail.installedAt ?? undefined}>
                      {formatUpdatedAt(detail.installedAt)}
                    </span>
                  </MetaRow>
                  <MetaRow label="更新时间">
                    <span title={detail.updatedAt ?? undefined}>
                      {formatUpdatedAt(detail.updatedAt)}
                    </span>
                  </MetaRow>
                  <MetaRow label={directoryLabel} mono>
                    <span
                      className="path-value"
                      title={detail.directoryPath ?? undefined}
                    >
                      {detail.directoryPath ?? "—"}
                    </span>
                  </MetaRow>
                  <MetaRow label="目录修改时间">
                    {formatEpochMs(detail.directoryMtimeEpochMs)}
                  </MetaRow>
                  <MetaRow label="版本指纹" mono>
                    {detail.hash ? <HashValue hash={detail.hash} /> : "—"}
                  </MetaRow>
                  <MetaRow label="本地改动">
                    {management === "unmanaged"
                      ? "—"
                      : localModification === "clean"
                        ? "未检测到改动"
                        : LOCAL_MODIFICATION_LABELS[localModification]}
                  </MetaRow>
                  <MetaRow label="更新状态">{UPDATE_LABELS[update]}</MetaRow>
                  <MetaRow label="管理状态">
                    {management === "managed"
                      ? "受管"
                      : management === "copy_only"
                        ? "仅副本"
                        : "未受管"}
                  </MetaRow>
                </dl>
              </section>
            ) : null}
          </section>
        </div>

        <DetachDialog
          open={detachTarget !== null}
          skillName={detailView.name}
          canonicalPath={detailView.canonicalPath}
          target={detachTarget}
          otherAttachments={attachments.filter(
            (attachment) => attachment.agentId !== detachTarget?.agentId,
          )}
          pending={detachMutation.state.running}
          progressMessage={detachMutation.state.message}
          error={
            detachMutation.state.startError ?? detachMutation.state.error
          }
          disabled={actionsDisabled}
          onClose={() => {
            if (detachMutation.state.running) return;
            setDetachTarget(null);
            detachMutation.reset();
          }}
          onConfirm={() => void confirmDetach()}
        />
        <AttachDialog
          open={attachOpen}
          scope={scope}
          skillId={skill.id}
          skillName={skill.name}
          agents={attachableAgents}
          disabled={actionsDisabled}
          onClose={() => setAttachOpen(false)}
        />
      </aside>
    </>
  );
});
