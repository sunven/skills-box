import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  AlertTriangle,
  FileText,
  FolderOpen,
  Loader2,
  MousePointerClick,
  RefreshCw,
} from "lucide-react";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { SkillMarkdown } from "./skill-markdown";
import { asAppError, openSkillFolder, openSkillMd } from "../lib/api";
import { agentColor, agentInitials } from "../lib/agent-style";
import type { SkillDetail, UpdateSkillResult } from "../lib/types";
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
  updateBadgeVariant,
  type UpdateViewSkill,
} from "../lib/update-view";
import { useSkillDetailQuery } from "../hooks/use-skill-detail";
import { cn } from "../lib/utils";

interface DetailPaneProps {
  skill?: UpdateViewSkill | null;
  updateResult?: UpdateSkillResult | null;
  checkingUpdates?: boolean;
  onCheckUpdates?: (skillIds: string[]) => void;
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
      <dt className="text-muted-foreground/70">{label}</dt>
      <dd className={cn("min-w-0 break-words", mono && "font-mono text-[11px]")}>
        {children}
      </dd>
    </>
  );
}

function HashValue({ hash }: { hash: string }) {
  const [expanded, setExpanded] = useState(false);
  useEffect(() => setExpanded(false), [hash]);
  return (
    <span className="inline-flex flex-wrap items-center gap-1.5">
      <code title={hash} className="break-all">
        {expanded ? hash : shortHash(hash)}
      </code>
      {hash.length > 12 ? (
        <button
          type="button"
          className="text-[10px] text-primary underline underline-offset-2"
          aria-label={expanded ? "收起完整 hash" : "查看完整 hash"}
          onClick={() => setExpanded((value) => !value)}
        >
          {expanded ? "收起" : "完整"}
        </button>
      ) : null}
    </span>
  );
}

function AgentAttachmentRow({
  attachment,
}: {
  attachment: SkillDetail["attachments"][number];
}) {
  const broken = attachment.mode === "broken";
  return (
    <li
      data-testid="detail-agent-row"
      className={cn(
        "flex items-start gap-2.5 rounded-lg border px-2.5 py-2",
        broken
          ? "border-destructive/25 bg-red-50/60"
          : "border-border bg-card/60",
      )}
    >
      <span
        className="mt-0.5 grid size-6 shrink-0 place-items-center rounded-full text-[8.5px] font-extrabold tracking-tight text-white"
        style={{ backgroundColor: agentColor(attachment.agentId) }}
        aria-hidden="true"
      >
        {agentInitials(attachment.agentName)}
      </span>
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-1.5">
          <span className="text-[12px] font-medium">{attachment.agentName}</span>
          <Badge
            variant={
              broken
                ? "destructive"
                : attachment.mode === "native"
                  ? "muted"
                  : "outline"
            }
          >
            {ATTACHMENT_LABELS[attachment.mode]}
          </Badge>
        </div>
        <p
          className="mt-0.5 break-all font-mono text-[10.5px] text-muted-foreground/80"
          title={attachment.path}
        >
          {attachment.path}
        </p>
        {broken ? (
          <p className="mt-0.5 text-[10.5px] text-destructive">
            链接目标缺失；只读提示，不自动修复。
          </p>
        ) : null}
        {attachment.mode === "native" ? (
          <p className="mt-0.5 text-[10.5px] text-muted-foreground/70">
            共享 canonical store，不产生独立挂载。
          </p>
        ) : null}
      </div>
    </li>
  );
}

export function DetailPane({
  skill,
  updateResult,
  checkingUpdates = false,
  onCheckUpdates,
}: DetailPaneProps) {
  const skillId = skill?.name ?? null;
  const query = useSkillDetailQuery(skillId);
  const detail = query.data ?? null;
  const [view, setView] = useState<DetailView>("rendered");
  const [pendingAction, setPendingAction] = useState<ActionKind | null>(null);
  const [actionError, setActionError] = useState<ReturnType<typeof asAppError>>(
    null,
  );
  const actionErrorRef = useRef<HTMLDivElement>(null);
  const detailError = asAppError(query.error);

  useEffect(() => {
    setView("rendered");
    setActionError(null);
    setPendingAction(null);
  }, [skillId]);

  useEffect(() => {
    if (actionError) actionErrorRef.current?.focus();
  }, [actionError]);

  if (!skill) {
    return (
      <aside
        aria-label="详情"
        className="flex min-h-0 flex-col border-l border-border bg-card/40"
      >
        <div className="border-b border-border px-4 py-3">
          <h2 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
            详情
          </h2>
        </div>
        <div className="flex flex-1 items-center justify-center p-6">
          <div className="max-w-[15rem] text-center">
            <div className="mx-auto flex size-11 items-center justify-center rounded-xl border border-border bg-card text-muted-foreground/70 shadow-sm">
              <MousePointerClick className="size-5" aria-hidden="true" />
            </div>
            <h3 className="mt-3 text-sm font-medium text-foreground">
              未选择 skill
            </h3>
            <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
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
  const unmounted = attachments.length === 0 && management !== "unmanaged";
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
      className="flex min-h-0 flex-col border-l border-border bg-card/40"
    >
      <div className="border-b border-border px-4 py-3">
        <h2 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
          详情
        </h2>
      </div>
      <div className="min-h-0 flex-1 overflow-auto p-4" data-testid="detail-content">
        <div className="flex flex-wrap items-center gap-1.5">
          <h3 className="text-lg font-semibold tracking-tight">
            {detail?.name ?? skill.name}
          </h3>
          <Badge
            data-testid="detail-update-badge"
            variant={updateBadgeVariant(update)}
            title={updateBadgeTitle({ ...skill, updateResult: updateInfo ?? undefined })}
          >
            {UPDATE_LABELS[update]}
          </Badge>
          {management === "copy_only" ? (
            <Badge variant="outline">仅副本</Badge>
          ) : null}
          {management === "unmanaged" ? (
            <Badge variant="warning">未受管</Badge>
          ) : null}
          {localWarning ? (
            <Badge
              variant={localModification === "modified" ? "warning" : "muted"}
              title={
                localModification === "modified"
                  ? "当前文件与基线指纹不一致；更新或卸载前请确认并备份。"
                  : "首次扫描没有可比较的基线或 lock hash；更新或卸载前请确认。"
              }
            >
              {LOCAL_MODIFICATION_LABELS[localModification]}
            </Badge>
          ) : null}
          {unmounted ? <Badge variant="muted">未挂载</Badge> : null}
          {broken ? <Badge variant="destructive">断链</Badge> : null}
          {warnings.length > 0 ? (
            <Badge variant="warning" title={warnings.join("\n")}>
              警告
            </Badge>
          ) : null}
        </div>
        <p className="mt-1.5 text-xs leading-relaxed text-muted-foreground">
          {detail?.description ?? skill.description ?? "—"}
        </p>

        <div className="mt-3 flex flex-wrap gap-1.5">
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
              variant="outline"
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
        </div>

        {actionError ? (
          <div
            ref={actionErrorRef}
            role="alert"
            tabIndex={-1}
            className="mt-2 rounded-md border border-destructive/20 bg-red-50 px-2.5 py-1.5 text-[10.5px] leading-relaxed text-destructive"
          >
            {actionError.message}
            {actionError.detail ? ` · ${actionError.detail}` : ""}
            {actionError.hint ? ` ${actionError.hint}` : ""}
          </div>
        ) : null}

        {management === "unmanaged" ? (
          <div className="mt-2 rounded-md border border-warning/25 bg-warning-soft px-2.5 py-1.5 text-[10.5px] leading-relaxed text-warning">
            未受管：该目录没有 lock 条目，v1 只读展示，不提供更新或卸载。
          </div>
        ) : null}
        {management === "copy_only" ? (
          <div className="mt-2 rounded-md border border-border bg-muted/50 px-2.5 py-1.5 text-[10.5px] leading-relaxed text-muted-foreground">
            仅副本：没有 canonical 目录；不可更新或断开。卸载入口将在后续版本接入。
          </div>
        ) : null}
        {localModification === "modified" ? (
          <div className="mt-2 rounded-md border border-warning/30 bg-warning-soft px-2.5 py-1.5 text-[10.5px] leading-relaxed text-warning">
            检测到本地改动：共享副本与基线记录不一致。更新会覆盖修改，请先备份。
            {detail?.localModificationDetail
              ? ` ${detail.localModificationDetail}`
              : ""}
          </div>
        ) : null}
        {localModification === "unknown" && management !== "unmanaged" ? (
          <div className="mt-2 rounded-md border border-border bg-muted/50 px-2.5 py-1.5 text-[10.5px] leading-relaxed text-muted-foreground">
            无法确认本地改动：首次扫描没有可比较的基线，lock hash
            也无法比对。更新或卸载前需要你确认。
            {detail?.localModificationDetail
              ? ` ${detail.localModificationDetail}`
              : ""}
          </div>
        ) : null}

        {updateInfo?.error ? (
          <div
            role="alert"
            data-testid="update-check-error"
            className="mt-2 rounded-md border border-destructive/20 bg-red-50 px-2.5 py-2 text-[10.5px] leading-relaxed text-destructive"
          >
            <p className="font-medium">检查失败：{updateInfo.error.message}</p>
            {updateInfo.error.detail ? (
              <p className="mt-0.5 break-all text-destructive/80">
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
        ) : null}

        {update === "path_missing" ? (
          <div
            data-testid="update-path-missing"
            className="mt-2 rounded-md border border-destructive/20 bg-red-50 px-2.5 py-1.5 text-[10.5px] leading-relaxed text-destructive"
          >
            {updateInfo?.note ??
              "来源中已找不到该 skill，可能已被删除或移动；更新已禁用。"}
          </div>
        ) : null}

        {updateInfo && !updateInfo.error && update !== "path_missing" && updateInfo.note ? (
          <div className="mt-2 rounded-md border border-border bg-muted/50 px-2.5 py-1.5 text-[10.5px] leading-relaxed text-muted-foreground">
            {updateInfo.note}
          </div>
        ) : null}

        {query.isPending ? (
          <div
            role="status"
            className="mt-3 flex items-center gap-1.5 text-[11px] text-muted-foreground"
          >
            <Loader2 className="size-3 animate-spin" aria-hidden="true" />
            正在读取详情…
          </div>
        ) : null}

        {detailError ? (
          <div
            role="alert"
            className="mt-3 rounded-md border border-destructive/20 bg-red-50 px-2.5 py-2 text-[11px] leading-relaxed text-destructive"
          >
            <p className="font-medium">详情读取失败</p>
            <p className="mt-0.5">{detailError.message}</p>
            {detailError.hint ? (
              <p className="mt-0.5 text-destructive/80">{detailError.hint}</p>
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
        ) : null}

        {detail ? (
          <>
            {warnings.length > 0 ? (
              <section
                aria-label="详情警告"
                data-testid="detail-warnings"
                className="mt-3 rounded-md border border-warning/25 bg-warning-soft px-2.5 py-2 text-[10.5px] leading-relaxed text-warning"
              >
                <p className="font-medium">警告</p>
                <ul className="mt-0.5 list-disc pl-4">
                  {warnings.map((warning) => (
                    <li key={warning}>{warning}</li>
                  ))}
                </ul>
              </section>
            ) : null}

            <section className="mt-4">
              <h4 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground/80">
                概览
              </h4>
              <dl className="mt-1.5 grid grid-cols-[82px_minmax(0,1fr)] gap-x-3 gap-y-1.5 text-[11.5px]">
                <MetaRow label="来源">
                  <span>{sourceLabel ?? "来源未知"}</span>
                  {sourceTypeLabel ? <span>{sourceTypeLabel}</span> : null}
                </MetaRow>
                <MetaRow label="ref">{detail.ref ?? "—"}</MetaRow>
                <MetaRow label="skillPath">{detail.skillPath ?? "—"}</MetaRow>
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
                  {detail.directoryPath ?? "—"}
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

            <section className="mt-4">
              <h4 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground/80">
                Agent 挂载{" "}
                <span className="font-normal normal-case text-muted-foreground/70">
                  {attachments.length} 个
                </span>
              </h4>
              {attachments.length === 0 ? (
                <p className="mt-1.5 rounded-md border border-border bg-muted/40 px-2.5 py-2 text-[10.5px] leading-relaxed text-muted-foreground">
                  未挂载任何 agent。共享副本仍保留，可在安装向导中重新挂载。
                </p>
              ) : (
                <ul className="mt-1.5 space-y-1.5">
                  {attachments.map((attachment) => (
                    <AgentAttachmentRow
                      key={`${attachment.agentId}:${attachment.path}`}
                      attachment={attachment}
                    />
                  ))}
                </ul>
              )}
            </section>

            <section className="mt-4" aria-label="SKILL.md">
              <div className="flex items-center justify-between gap-2">
                <h4 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground/80">
                  SKILL.md
                </h4>
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
                      className="h-6 px-2 text-[10.5px]"
                      onClick={() => setView("rendered")}
                    >
                      渲染
                    </Button>
                    <Button
                      size="sm"
                      variant={view === "raw" ? "default" : "ghost"}
                      aria-pressed={view === "raw"}
                      className="h-6 px-2 text-[10.5px]"
                      onClick={() => setView("raw")}
                    >
                      原文
                    </Button>
                  </div>
                ) : null}
              </div>

              {skillMd ? (
                <p
                  className="mt-1 break-all font-mono text-[10px] text-muted-foreground/60"
                  title={skillMd.path ?? undefined}
                >
                  {skillMd.path ?? "—"}
                </p>
              ) : null}

              {!skillMd ? (
                <div
                  role="status"
                  className="mt-1.5 flex items-center gap-1.5 text-[11px] text-muted-foreground"
                >
                  <Loader2 className="size-3 animate-spin" aria-hidden="true" />
                  读取 SKILL.md…
                </div>
              ) : skillMd.status === "missing" ? (
                <div className="mt-1.5 rounded-md border border-warning/25 bg-warning-soft px-2.5 py-2 text-[10.5px] leading-relaxed text-warning">
                  未找到 SKILL.md。该 skill 目录可能不完整；不会渲染内容。
                </div>
              ) : skillMd.status === "too_large" ? (
                <div className="mt-1.5 rounded-md border border-warning/25 bg-warning-soft px-2.5 py-2 text-[10.5px] leading-relaxed text-warning">
                  SKILL.md 超过 1 MiB（当前{" "}
                  {skillMd.sizeBytes ?? "未知"} 字节），已停止加载。
                </div>
              ) : skillMd.status === "unreadable" ? (
                <div className="mt-1.5 rounded-md border border-warning/25 bg-warning-soft px-2.5 py-2 text-[10.5px] leading-relaxed text-warning">
                  无法读取 SKILL.md：{skillMd.error ?? "未知错误"}
                </div>
              ) : skillMd.content !== null ? (
                <>
                  {frontmatterWarning ? (
                    <div className="mt-1.5 rounded-md border border-warning/25 bg-warning-soft px-2.5 py-1.5 text-[10.5px] leading-relaxed text-warning">
                      渲染视图可能无法完整隐藏 frontmatter：解析失败，原文可查看完整内容。
                    </div>
                  ) : null}
                  {view === "raw" ? (
                    <pre
                      data-testid="skill-md-raw"
                      className="mt-1.5 max-h-[26rem] overflow-auto whitespace-pre-wrap break-words rounded-md border border-border bg-muted/40 p-2.5 font-mono text-[10.5px] leading-relaxed"
                    >
                      {skillMd.content}
                    </pre>
                  ) : (
                    <div className="mt-1.5 max-h-[26rem] overflow-auto rounded-md border border-border bg-card p-2.5">
                      <SkillMarkdown content={skillMd.content} />
                    </div>
                  )}
                </>
              ) : (
                <div className="mt-1.5 flex items-center gap-1.5 text-[11px] text-muted-foreground">
                  <AlertTriangle className="size-3" aria-hidden="true" />
                  没有可显示的内容。
                </div>
              )}
            </section>
          </>
        ) : null}
      </div>
    </aside>
  );
}
