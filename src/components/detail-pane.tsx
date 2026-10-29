import { useEffect, useRef, useState, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  ArrowLeft,
  ArrowUpCircle,
  FileText,
  FolderOpen,
  Loader2,
  RefreshCw,
  Trash2,
  Unlink,
} from "lucide-react";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { DetachDialog, type DetachDialogError } from "./detach-dialog";
import { SkillMarkdown } from "./skill-markdown";
import { asAppError, detachAgent, openSkillFolder, openSkillMd } from "../lib/api";
import { agentInitials } from "../lib/agent-style";
import { agentLogoUrl } from "../lib/agent-logos";
import type {
  AgentAttachment,
  DetachResult,
  SkillDetail,
  UpdateSkillResult,
} from "../lib/types";
import {
  ATTACHMENT_LABELS,
  LOCAL_MODIFICATION_LABELS,
  UPDATE_LABELS,
  formatEpochMs,
  formatUpdatedAt,
  hasBrokenAttachment,
  hasLocalModificationWarning,
  shortHash,
} from "../lib/inventory-view";
import {
  formatEpochDateTime,
  updateBadgeTitle,
  type UpdateViewSkill,
} from "../lib/update-view";
import { useSkillDetailQuery } from "../hooks/use-skill-detail";
import { cn } from "../lib/utils";

interface DetailPaneProps {
  skill?: UpdateViewSkill | null;
  updateResult?: UpdateSkillResult | null;
  checkingUpdates?: boolean;
  onCheckUpdates?: (skillIds: string[]) => void;
  onUpdate?: (skillName: string) => void;
  onUninstall?: (skillName: string) => void;
  onBack?: () => void;
  titleIsPrimary?: boolean;
}

type DetailView = "rendered" | "raw";
type ActionKind = "folder" | "md";

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
  const [expanded, setExpanded] = useState(false);
  useEffect(() => setExpanded(false), [hash]);
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
          onClick={() => setExpanded((value) => !value)}
        >
          {expanded ? "收起" : "完整"}
        </button>
      ) : null}
    </span>
  );
}

function UpdateStatus({
  skill,
}: {
  skill: UpdateViewSkill;
}) {
  const tone =
    skill.update === "update"
      ? "accent"
      : skill.update === "error" || skill.update === "path_missing"
        ? "negative"
        : "muted";
  return (
    <span
      data-testid="detail-update-badge"
      className="status-chip"
      data-tone={tone}
      title={updateBadgeTitle(skill)}
    >
      {UPDATE_LABELS[skill.update]}
    </span>
  );
}

function AgentAttachmentRow({
  attachment,
  onDetach,
}: {
  attachment: SkillDetail["attachments"][number];
  onDetach: (attachment: AgentAttachment) => void;
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
          onClick={() => onDetach(attachment)}
        >
          <Unlink aria-hidden="true" />
          断开
        </Button>
      ) : null}
    </li>
  );
}

export function DetailPane({
  skill,
  updateResult,
  checkingUpdates = false,
  onCheckUpdates,
  onUpdate,
  onUninstall,
  onBack,
  titleIsPrimary = false,
}: DetailPaneProps) {
  const skillId = skill?.name ?? null;
  const query = useSkillDetailQuery(skillId);
  const queryClient = useQueryClient();
  const detail = query.data ?? null;
  const [view, setView] = useState<DetailView>("rendered");
  const [pendingAction, setPendingAction] = useState<ActionKind | null>(null);
  const [actionError, setActionError] = useState<ReturnType<typeof asAppError>>(
    null,
  );
  const [detachTarget, setDetachTarget] = useState<AgentAttachment | null>(null);
  const [detachPending, setDetachPending] = useState(false);
  const [detachError, setDetachError] = useState<DetachDialogError | null>(null);
  const [detachNotice, setDetachNotice] = useState<DetachResult | null>(null);
  const actionErrorRef = useRef<HTMLDivElement>(null);
  const detailError = asAppError(query.error);

  useEffect(() => {
    setView("rendered");
    setActionError(null);
    setPendingAction(null);
    setDetachTarget(null);
    setDetachPending(false);
    setDetachError(null);
    setDetachNotice(null);
  }, [skillId]);

  useEffect(() => {
    if (actionError) actionErrorRef.current?.focus();
  }, [actionError]);

  if (!skill) {
    return (
      <aside aria-label="详情" className="detail-pane">
        <div className="detail-head">
          <h2 className="sr-only">详情</h2>
          {onBack ? (
            <button type="button" className="detail-back" onClick={onBack}>
              <ArrowLeft className="size-3.5" aria-hidden="true" />
              返回列表
            </button>
          ) : null}
        </div>
        <div className="flex flex-1 items-center justify-center p-6">
          <div className="max-w-[16rem] text-center">
            <h3 className="text-sm font-medium text-foreground">未选择 skill</h3>
            <p className="mt-1.5 text-[12.5px] leading-relaxed text-muted-foreground">
              从列表中选择一个 skill 查看来源、挂载与本地改动。
            </p>
          </div>
        </div>
      </aside>
    );
  }

  const current = detail ?? skill;
  const management = current.management;
  const update = skill.update;
  const updateInfo = updateResult ?? skill.updateResult ?? null;
  const localModification = current.localModification;
  const attachments = current.attachments;
  const warnings = detail?.warnings ?? [];
  const broken = hasBrokenAttachment({
    ...skill,
    attachments,
  });
  const unattached = attachments.length === 0 && management !== "unmanaged";
  const localWarning = hasLocalModificationWarning({
    ...skill,
    management,
    localModification,
  });
  // Managed skills only: unmanaged and copy-only rows are Finder-only in v1.
  const canOpenEditor = management === "managed";
  const directoryLabel = management === "copy_only" ? "agent 路径" : "canonical 路径";

  const runAction = async (kind: ActionKind) => {
    setActionError(null);
    setPendingAction(kind);
    try {
      if (kind === "folder") {
        await openSkillFolder(skill.name);
      } else {
        await openSkillMd(skill.name);
      }
    } catch (error) {
      setActionError(asAppError(error) ?? { message: "未知错误" });
    } finally {
      setPendingAction(null);
    }
  };

  const openDetach = (attachment: AgentAttachment) => {
    setDetachError(null);
    setDetachTarget(attachment);
  };

  const confirmDetach = async () => {
    if (!detachTarget || !skill) return;
    setDetachPending(true);
    setDetachError(null);
    try {
      const result = await detachAgent(skill.name, detachTarget.agentId);
      setDetachNotice(result);
      setDetachTarget(null);
    } catch (error) {
      setDetachError(
        asAppError(error) ?? { message: "断开失败，请刷新后重试。" },
      );
    } finally {
      setDetachPending(false);
      // A failed attempt may still have changed the disk; always re-read the
      // detail and inventory so the UI never shows an assumed state.
      void queryClient.invalidateQueries({ queryKey: ["skill", skill.name] });
      void queryClient.invalidateQueries({ queryKey: ["inventory"] });
    }
  };

  const sourceLabel = detail?.source ?? skill.source;
  const sourceTypeLabel =
    detail?.sourceType === "github"
      ? " · 公开 GitHub"
      : detail?.sourceType
        ? ` · ${detail.sourceType}`
        : "";
  const skillMd = detail?.skillMd;
  const frontmatterWarning = warnings.some((warning) =>
    warning.includes("frontmatter"),
  );

  return (
    <aside
      aria-label="详情"
      aria-busy={query.isPending}
      className="detail-pane"
    >
      <div className="detail-head">
        <h2 className="sr-only">详情</h2>
        {onBack ? (
          <button type="button" className="detail-back" onClick={onBack}>
            <ArrowLeft className="size-3.5" aria-hidden="true" />
            返回列表
          </button>
        ) : null}
        <div className="detail-title-line">
          {titleIsPrimary ? (
            <h1 className="detail-title">{detail?.name ?? skill.name}</h1>
          ) : (
            <h2 className="detail-title">{detail?.name ?? skill.name}</h2>
          )}
          <UpdateStatus skill={{ ...skill, updateResult: updateInfo ?? undefined }} />
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
        <p className="detail-desc">
          {detail?.description ?? skill.description ?? "—"}
        </p>

        <div className="detail-actions">
          <Button
            variant="outline"
            size="sm"
            disabled={pendingAction !== null}
            onClick={() => void runAction("folder")}
          >
            {pendingAction === "folder" ? (
              <Loader2 className="animate-spin" aria-hidden="true" />
            ) : (
              <FolderOpen aria-hidden="true" />
            )}
            在 Finder 中查看
          </Button>
          {canOpenEditor ? (
            <Button
              variant="outline"
              size="sm"
              disabled={pendingAction !== null}
              onClick={() => void runAction("md")}
            >
              {pendingAction === "md" ? (
                <Loader2 className="animate-spin" aria-hidden="true" />
              ) : (
                <FileText aria-hidden="true" />
              )}
              默认编辑器打开
            </Button>
          ) : null}
          {management === "managed" && onCheckUpdates ? (
            <Button
              variant="ghost"
              size="sm"
              disabled={checkingUpdates}
              aria-busy={checkingUpdates}
              onClick={() => onCheckUpdates([skill.name])}
              title="只读检查该 skill 的来源；不会运行 skills check"
            >
              <RefreshCw
                className={checkingUpdates ? "size-3.5 animate-spin" : "size-3.5"}
                aria-hidden="true"
              />
              检查更新
            </Button>
          ) : null}
          {management === "managed" && update === "update" && onUpdate ? (
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
      </div>

      <div className="detail-scroll" data-testid="detail-content">
        <div className="flex flex-col gap-2 pt-3">
          {actionError ? (
            <div
              ref={actionErrorRef}
              role="alert"
              tabIndex={-1}
              className="notice"
              data-tone="negative"
            >
              <div>
                {actionError.message}
                {actionError.detail ? ` · ${actionError.detail}` : ""}
                {actionError.hint ? ` ${actionError.hint}` : ""}
              </div>
            </div>
          ) : null}

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
        </div>

        {detail ? (
          <>
            {warnings.length > 0 ? (
              <section
                aria-label="详情警告"
                data-testid="detail-warnings"
                className="notice mt-3"
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
                  <span className="path-value" title={detail.skillPath ?? undefined}>
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
                <MetaRow label="远端 tree SHA" mono>
                  {updateInfo?.remoteHash ? (
                    <span title={updateInfo.remoteHash}>
                      {shortHash(updateInfo.remoteHash)}
                    </span>
                  ) : (
                    "—"
                  )}
                </MetaRow>
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

            <section className="detail-section">
              <h4 className="detail-section-title">
                Agent 挂载{" "}
                <span className="count">{attachments.length} 个</span>
              </h4>
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
                      onDetach={openDetach}
                    />
                  ))}
                </ul>
              )}
            </section>

            <section className="detail-section" aria-label="SKILL.md">
              <div className="flex items-center justify-between gap-2">
                <h4 className="detail-section-title">SKILL.md</h4>
                {skillMd?.status === "ok" && skillMd.content !== null ? (
                  <div
                    role="group"
                    aria-label="SKILL.md 显示方式"
                    className="flex items-center gap-1"
                  >
                    <Button
                      size="sm"
                      variant={view === "rendered" ? "default" : "ghost"}
                      aria-pressed={view === "rendered"}
                      className="h-7 px-2 text-[12px]"
                      onClick={() => setView("rendered")}
                    >
                      渲染
                    </Button>
                    <Button
                      size="sm"
                      variant={view === "raw" ? "default" : "ghost"}
                      aria-pressed={view === "raw"}
                      className="h-7 px-2 text-[12px]"
                      onClick={() => setView("raw")}
                    >
                      原文
                    </Button>
                  </div>
                ) : null}
              </div>

              {skillMd ? (
                <p
                  className="mono mt-1.5 truncate text-muted-foreground"
                  title={skillMd.path ?? undefined}
                >
                  {skillMd.path ?? "—"}
                </p>
              ) : null}

              {!skillMd ? (
                <div
                  role="status"
                  className="mt-2 flex items-center gap-1.5 text-[12px] text-muted-foreground"
                >
                  <Loader2 className="size-3 animate-spin" aria-hidden="true" />
                  读取 SKILL.md…
                </div>
              ) : skillMd.status === "missing" ? (
                <div className="notice mt-2" data-tone="warning">
                  未找到 SKILL.md。该 skill 目录可能不完整；不会渲染内容。
                </div>
              ) : skillMd.status === "too_large" ? (
                <div className="notice mt-2" data-tone="warning">
                  SKILL.md 超过 1 MiB（当前 {skillMd.sizeBytes ?? "未知"}{" "}
                  字节），已停止加载。
                </div>
              ) : skillMd.status === "unreadable" ? (
                <div className="notice mt-2" data-tone="warning">
                  无法读取 SKILL.md：{skillMd.error ?? "未知错误"}
                </div>
              ) : skillMd.content !== null ? (
                <>
                  {frontmatterWarning ? (
                    <div className="notice mt-2" data-tone="warning">
                      渲染视图可能无法完整隐藏 frontmatter：解析失败，原文可查看完整内容。
                    </div>
                  ) : null}
                  {view === "raw" ? (
                    <pre data-testid="skill-md-raw" className="md-raw">
                      {skillMd.content}
                    </pre>
                  ) : (
                    <div className="md-well">
                      <SkillMarkdown content={skillMd.content} />
                    </div>
                  )}
                </>
              ) : (
                <div className="mt-2 flex items-center gap-1.5 text-[12px] text-muted-foreground">
                  <AlertTriangle className="size-3" aria-hidden="true" />
                  没有可显示的内容。
                </div>
              )}
            </section>

            {management !== "unmanaged" && onUninstall ? (
              <section className="detail-section">
                <h4 className="detail-section-title">危险操作</h4>
                <p className="mt-2 text-[12px] leading-relaxed text-muted-foreground">
                  彻底卸载会删除 canonical、所有 agent 挂载、lock
                  条目与本地基线；先备份，不自动回滚。
                </p>
                <Button
                  variant="outline"
                  size="sm"
                  className="mt-2 border-destructive/35 text-destructive hover:bg-destructive-soft hover:text-destructive"
                  onClick={() => onUninstall(skill.name)}
                  title="删除 canonical、所有 agent 挂载、lock 条目与基线；先备份，不自动回滚"
                >
                  <Trash2 className="size-3.5" aria-hidden="true" />
                  彻底卸载
                </Button>
              </section>
            ) : null}
          </>
        ) : null}
      </div>
      <DetachDialog
        open={detachTarget !== null}
        skillName={detail?.name ?? skill.name}
        canonicalPath={detail?.canonicalPath ?? null}
        target={detachTarget}
        otherAttachments={attachments.filter(
          (attachment) => attachment.agentId !== detachTarget?.agentId,
        )}
        pending={detachPending}
        error={detachError}
        onClose={() => {
          if (detachPending) return;
          setDetachTarget(null);
          setDetachError(null);
        }}
        onConfirm={() => void confirmDetach()}
      />
    </aside>
  );
}
