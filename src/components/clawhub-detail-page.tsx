import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ArrowLeft } from "lucide-react";
import {
  asAppError,
  fallbackError,
  getClawhubSkillDetail,
  listClawhubVersionFiles,
  readClawhubVersionFile,
} from "../lib/api";
import {
  clawhubAddress,
  clawhubInitialVersion,
  clawhubKey,
  clawhubMetricLabels,
  clawhubVersionLabel,
  type ClawHubAliasDetail,
  type ClawHubCoordinate,
  type ClawHubMetrics as ClawHubMetricsValue,
  type ClawHubNativeDetail,
  type ClawHubSkill,
} from "../lib/clawhub";
import type { InventorySnapshot, ScopeId, SkillFileContent } from "../lib/types";
import { fileName, formatBytes, parentDir } from "../lib/skill-content";
import { Button } from "./ui/button";
import { ClawHubInstallDialog } from "./clawhub-install-dialog";
import { MarketplaceFileTree } from "./marketplace-files-tab";
import { MutationError } from "./mutation/mutation-error";
import { OfficialLink } from "./official-link";
import { SkillMarkdown } from "./skill-markdown";

interface Props {
  coordinate: ClawHubCoordinate;
  summary: ClawHubSkill;
  active: boolean;
  scope: ScopeId;
  scopeLabel: string;
  snapshot?: InventorySnapshot;
  disabled: boolean;
  onBack: () => void;
  onSwitchSkillsSh: () => void;
  onView: (name: string) => void;
  onInstallOpenChange: (open: boolean) => void;
}

/**
 * Read-only ClawHub detail. Every version-scoped query is keyed by the exact
 * version and the version body is remounted on change, so the overview and
 * the file tree can never show another version's content while switching.
 */
export function ClawHubDetailPage({
  coordinate,
  summary,
  active,
  scope,
  scopeLabel,
  snapshot,
  disabled,
  onBack,
  onSwitchSkillsSh,
  onView,
  onInstallOpenChange,
}: Props) {
  const headingRef = useRef<HTMLHeadingElement>(null);
  const [installOpen, setInstallOpen] = useState(false);
  const detail = useQuery({
    queryKey: ["marketplace", "clawhub", "detail", clawhubKey(coordinate)],
    queryFn: () => getClawhubSkillDetail(coordinate),
    enabled: active,
    retry: false,
    staleTime: 60_000,
  });
  useEffect(() => {
    headingRef.current?.focus({ preventScroll: true });
  }, []);
  useEffect(() => () => onInstallOpenChange(false), [onInstallOpenChange]);

  const data = detail.data;
  const native = data?.kind === "native" ? data : null;
  const name = data?.name ?? summary.name ?? coordinate.slug;
  const description = native?.description ?? summary.description;
  const metrics = native?.metrics ?? summary.metrics;
  const topics = native?.topics ?? summary.topics;
  const canonicalUrl = data?.canonicalUrl ?? summary.officialUrl;
  const installable = Boolean(
    native &&
      ((native.versions.length > 0 && native.latestVersion !== null) ||
        native.resolvedReference?.contentHash),
  );
  const installed = snapshot?.skills.find(
    (entry) =>
      entry.sourceType === "clawhub" &&
      entry.source === clawhubAddress(coordinate),
  );

  return (
    <>
      <main
        className="marketplace-detail-page"
        aria-labelledby="clawhub-detail-title"
        inert={installOpen}
      >
        <div className="marketplace-detail-bar">
          <div className="marketplace-detail-bar-inner">
            <Button variant="ghost" size="sm" onClick={onBack}>
              <ArrowLeft aria-hidden="true" />
              返回列表
            </Button>
            <span className="marketplace-detail-bar-name" title={name}>
              {name}
            </span>
            <div className="marketplace-detail-action">
              <span className="marketplace-detail-target" aria-live="polite">
                安装到：{scopeLabel}
              </span>
              {installable ? (
                <Button
                  size="sm"
                  title={`当前作用域：${scopeLabel}`}
                  disabled={!snapshot || disabled}
                  onClick={() => {
                    if (installed) {
                      onView(installed.name);
                    } else {
                      setInstallOpen(true);
                      onInstallOpenChange(true);
                    }
                  }}
                >
                  {installed ? "查看已安装技能" : "安装"}
                </Button>
              ) : null}
            </div>
          </div>
        </div>
        <div className="marketplace-detail-content">
        <header className="marketplace-detail-header">
          <h1 id="clawhub-detail-title" ref={headingRef} tabIndex={-1}>
            {name}
          </h1>
          <p className="marketplace-detail-coordinate">{clawhubAddress(coordinate)}</p>
          <p className="marketplace-detail-metadata">
            <span>收录于 ClawHub</span>
            {data?.kind === "native" && data.ownerDisplayName ? (
              <span>发布者：{data.ownerDisplayName}</span>
            ) : null}
          </p>
          {description ? <p className="marketplace-detail-description">{description}</p> : null}
          {topics.length ? (
            <p className="flex flex-wrap gap-1" aria-label="ClawHub 分类标签">
              {topics.map((topic) => (
                <span key={topic} className="badge">
                  {topic}
                </span>
              ))}
            </p>
          ) : null}
          <div className="marketplace-detail-links">
            <ClawHubMetrics metrics={metrics} />
            <OfficialLink url={canonicalUrl} label="ClawHub 页面" />
          </div>
        </header>
        {detail.isPending ? <p role="status">正在读取 ClawHub 详情…</p> : null}
        {detail.isError ? (
          <div className="notice" role="alert">
            {data ? <p>详情刷新失败，保留上次成功加载的内容。</p> : <p>无法读取 ClawHub 详情。</p>}
            <MutationError
              alertRole={false}
              error={asAppError(detail.error) ?? fallbackError("ClawHub 暂未返回该条目。")}
            />
            <Button
              variant="outline"
              size="sm"
              disabled={detail.isFetching}
              onClick={() => void detail.refetch()}
            >
              重试详情
            </Button>
            <OfficialLink url={canonicalUrl} label="ClawHub 页面" />
          </div>
        ) : null}
        {data?.kind === "native" ? (
          <NativeBody detail={data} active={active} />
        ) : data?.kind === "alias" ? (
          <AliasBody detail={data} onSwitchSkillsSh={onSwitchSkillsSh} />
        ) : null}
      </div>
    </main>
    {installOpen && data && data.kind === "native" ? (
      <ClawHubInstallDialog
        coordinate={coordinate}
        detail={data}
        scope={scope}
        scopeLabel={scopeLabel}
        snapshot={snapshot}
        disabled={disabled}
        onClose={() => {
          setInstallOpen(false);
          onInstallOpenChange(false);
        }}
        onView={(installedName) => {
          setInstallOpen(false);
          onInstallOpenChange(false);
          onView(installedName);
        }}
      />
    ) : null}
    </>
  );
}

export function ClawHubMetrics({ metrics }: { metrics: ClawHubMetricsValue }) {
  return (
    <div className="marketplace-stats" role="group" aria-label="ClawHub 指标">
      {clawhubMetricLabels.map(({ key, label }) => {
        const count = metrics[key];
        return (
          <span
            key={key}
            role="group"
            aria-label={`${label}：${count === null ? "未提供" : count.toLocaleString()}`}
          >
            {label}：{count === null ? "未提供" : count.toLocaleString()}
          </span>
        );
      })}
    </div>
  );
}

function NativeBody({ detail, active }: { detail: ClawHubNativeDetail; active: boolean }) {
  const initial = clawhubInitialVersion(detail);
  const [selected, setSelected] = useState<string | null>(initial);
  const selectedVersion =
    selected && detail.versions.some((version) => version.version === selected)
      ? selected
      : initial;
  const changelog =
    detail.versions.find((version) => version.version === selectedVersion)?.changelog ?? null;

  return (
    <>
      {detail.versions.length === 0 ? (
        <div className="notice" role="status">
          <p>该条目由 GitHub 仓库提供，ClawHub 未提供可浏览的版本打包内容。</p>
          <p>安装会固定解析出的 commit 并校验文件夹内容哈希；解析引用不会创建 Git 安装身份。</p>
          {detail.resolvedReference ? (
            <>
              <p>
                解析引用：
                {`${detail.resolvedReference.repo}@${detail.resolvedReference.commit}:${detail.resolvedReference.path}`}
              </p>
            </>
          ) : (
            <p>解析引用：暂不可用（仅诊断）。</p>
          )}
          <OfficialLink url={detail.canonicalUrl} label="ClawHub 页面" />
        </div>
      ) : (
        <>
          <div className="marketplace-toolbar">
            <label className="marketplace-filter-select">
              版本
              <select
                aria-label="ClawHub 版本"
                value={selectedVersion ?? ""}
                onChange={(event) => setSelected(event.target.value)}
              >
                {detail.versions.map((version) => (
                  <option key={version.version} value={version.version}>
                    {clawhubVersionLabel(version.version, version.version === detail.latestVersion)}
                  </option>
                ))}
              </select>
            </label>
            {detail.versionsTruncated ? (
              <span className="text-xs text-muted-foreground">
                仅显示最近 {detail.versions.length} 个版本。
              </span>
            ) : null}
          </div>
          {selectedVersion ? (
            <ClawHubVersionBody
              key={selectedVersion}
              coordinate={detail.coordinate}
              version={selectedVersion}
              canonicalUrl={detail.canonicalUrl}
              changelog={changelog}
              active={active}
            />
          ) : null}
        </>
      )}
    </>
  );
}

function AliasBody({
  detail,
  onSwitchSkillsSh,
}: {
  detail: ClawHubAliasDetail;
  onSwitchSkillsSh: () => void;
}) {
  return (
    <div className="notice" role="status">
      <p>该条目是 ClawHub 索引的 skills.sh 别名，不属于 ClawHub 原生技能。</p>
      <p>
        内容浏览与安装请在 skills.sh 来源中进行；此页不提供 ClawHub 版本内容或安装入口。
        {detail.repository ? ` 对应仓库：${detail.repository}。` : ""}
      </p>
      <div className="marketplace-detail-links">
        {detail.sourceUrl ? <OfficialLink url={detail.sourceUrl} label="skills.sh 页面" /> : null}
        <Button variant="outline" size="sm" onClick={onSwitchSkillsSh}>
          切换到 skills.sh 来源
        </Button>
      </div>
    </div>
  );
}

function ClawHubVersionBody({
  coordinate,
  version,
  canonicalUrl,
  changelog,
  active,
}: {
  coordinate: ClawHubCoordinate;
  version: string;
  canonicalUrl: string;
  changelog: string | null;
  active: boolean;
}) {
  const [tab, setTab] = useState<"overview" | "files">("overview");
  const [raw, setRaw] = useState(false);
  const [path, setPath] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(new Set<string>());
  const tabsRef = useRef<HTMLDivElement>(null);
  const previewRef = useRef<HTMLDivElement>(null);
  const listing = useQuery({
    queryKey: ["marketplace", "clawhub", "files", clawhubKey(coordinate), version],
    queryFn: () => listClawhubVersionFiles(coordinate, version),
    enabled: active,
    retry: false,
    staleTime: Infinity,
    gcTime: 300_000,
  });
  const files = listing.data?.files ?? [];
  const hasSkillMd = files.some((file) => file.path === "SKILL.md");
  // Prefer the version response's changelog; the version list entry is the
  // same-version fallback while the response loads.
  const versionChangelog = listing.data?.changelog ?? changelog;
  const overview = useQuery({
    queryKey: ["marketplace", "clawhub", "file", clawhubKey(coordinate), version, "SKILL.md"],
    queryFn: () => readClawhubVersionFile(coordinate, version, "SKILL.md"),
    // Start SKILL.md in parallel with the file list. If the list later says
    // the version has no SKILL.md, the render path ignores this result.
    enabled: active && (hasSkillMd || listing.isPending),
    retry: false,
    staleTime: Infinity,
    gcTime: 300_000,
  });
  const selectedPath = path ?? (hasSkillMd ? "SKILL.md" : (files[0]?.path ?? ""));
  const selectedExists = files.some((file) => file.path === selectedPath);
  const navigate = useCallback((next: string) => {
    setPath(next);
    setTab("files");
    setRaw(false);
    setExpanded((previous) => {
      const result = new Set(previous);
      const parts = next.split("/");
      for (let index = 1; index < parts.length; index++) {
        result.add(parts.slice(0, index).join("/"));
      }
      return result;
    });
    previewRef.current?.focus({ preventScroll: true });
    previewRef.current?.scrollIntoView?.({ block: "nearest" });
  }, []);
  const fileLinks = useMemo(
    () => ({ baseDir: parentDir(selectedPath), onNavigate: navigate }),
    [selectedPath, navigate],
  );
  function refresh() {
    void listing.refetch();
    if (hasSkillMd) void overview.refetch();
  }
  function activate(next: "overview" | "files") {
    setTab(next);
    setRaw(false);
  }

  return (
    <section aria-label={`ClawHub 版本 v${version}`}>
      <div className="marketplace-snapshot-notice">
        <p>ClawHub 版本 v{version} 的打包内容</p>
        <p>浏览内容不保证与将来安装时重新解析的内容完全一致；安装以实际下载内容为准。</p>
        {/* License is read from this version's response, never from latest. */}
        {listing.data ? <p>许可证：{listing.data.license ?? "未提供"}</p> : null}
        <p>解析引用：v{version}（ClawHub archive 版本；安装按确认时的 latest 版本解析）</p>
        <Button variant="ghost" size="sm" disabled={listing.isFetching} onClick={refresh}>
          刷新版本内容
        </Button>
      </div>
      {versionChangelog ? (
        <section className="marketplace-detail-overview" aria-label="版本更新说明">
          <h2 className="text-sm font-semibold">更新说明</h2>
          <pre className="content-text" role="region" aria-label="版本更新说明原文" tabIndex={0}>
            {versionChangelog}
          </pre>
        </section>
      ) : null}
      <div
        ref={tabsRef}
        className="marketplace-detail-tabs"
        role="tablist"
        aria-label="ClawHub 版本内容"
        onKeyDown={(event) => {
          if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
          event.preventDefault();
          const next =
            event.key === "Home"
              ? "overview"
              : event.key === "End"
                ? "files"
                : tab === "overview"
                  ? "files"
                  : "overview";
          activate(next);
          tabsRef.current?.querySelector<HTMLButtonElement>(`#clawhub-version-tab-${next}`)?.focus();
        }}
      >
        {(["overview", "files"] as const).map((value) => (
          <button
            key={value}
            type="button"
            role="tab"
            id={`clawhub-version-tab-${value}`}
            aria-controls={`clawhub-version-panel-${value}`}
            aria-selected={tab === value}
            tabIndex={tab === value ? 0 : -1}
            onClick={() => activate(value)}
          >
            {value === "overview" ? "概述" : "文件"}
          </button>
        ))}
      </div>
      <div
        id="clawhub-version-panel-overview"
        role="tabpanel"
        aria-labelledby="clawhub-version-tab-overview"
        hidden={tab !== "overview"}
        tabIndex={0}
      >
        {tab === "overview" ? (
          <section className="marketplace-detail-overview" aria-label={`版本 v${version} 概述`}>
            {listing.isPending ? (
              <p role="status">正在读取版本文件…</p>
            ) : listing.isError && !listing.data ? (
              <VersionError
                error={listing.error}
                canonicalUrl={canonicalUrl}
                pending={listing.isFetching}
                retry={refresh}
              />
            ) : (
              <>
                {listing.isError ? (
                  <p className="marketplace-file-notice" role="status">
                    文件列表刷新失败，保留上次内容。
                  </p>
                ) : null}
                {!hasSkillMd ? (
                  <p role="status">
                    该版本未提供 SKILL.md，可在「文件」中查看该版本的其他文件。
                  </p>
                ) : overview.isPending ? (
                  <p role="status">正在读取 SKILL.md…</p>
                ) : overview.isError && !overview.data ? (
                  <VersionError
                    error={overview.error}
                    canonicalUrl={canonicalUrl}
                    pending={overview.isFetching}
                    retry={refresh}
                  />
                ) : overview.data ? (
                  <>
                    {overview.isError ? (
                      <p className="marketplace-file-notice" role="status">
                        SKILL.md 刷新失败，保留上次内容。
                      </p>
                    ) : null}
                    <SkillFileBody
                      file={overview.data}
                      raw={raw}
                      onToggleRaw={() => setRaw((value) => !value)}
                      fileLinks={fileLinks}
                    />
                  </>
                ) : null}
              </>
            )}
          </section>
        ) : null}
      </div>
      <div
        id="clawhub-version-panel-files"
        role="tabpanel"
        aria-labelledby="clawhub-version-tab-files"
        hidden={tab !== "files"}
        tabIndex={0}
      >
        {tab === "files" ? (
          listing.isPending ? (
            <p role="status">正在读取版本文件…</p>
          ) : listing.isError && !listing.data ? (
            <VersionError
              error={listing.error}
              canonicalUrl={canonicalUrl}
              pending={listing.isFetching}
              retry={refresh}
            />
          ) : (
            <div className="marketplace-files">
              <div className="marketplace-files-summary">
                <span>
                  版本 v{version}
                  {listing.data ? ` · ${files.length} 个文件` : ""}
                </span>
                <span>只读预览</span>
              </div>
              {listing.isError ? (
                <p className="marketplace-file-notice" role="status">
                  文件列表刷新失败，保留上次内容。
                </p>
              ) : null}
              {files.length === 0 ? (
                <p className="marketplace-file-notice">此版本没有可浏览的文件。</p>
              ) : (
                <div className="marketplace-files-body">
                  <nav className="marketplace-file-tree" aria-label="ClawHub 文件树">
                    <MarketplaceFileTree
                      files={files}
                      selectedPath={selectedPath}
                      expanded={expanded}
                      onSelectFile={navigate}
                      onToggleDir={(dir) =>
                        setExpanded((previous) => {
                          const next = new Set(previous);
                          if (next.has(dir)) next.delete(dir);
                          else next.add(dir);
                          return next;
                        })
                      }
                    />
                  </nav>
                  <div
                    ref={previewRef}
                    className="marketplace-file-preview"
                    role="region"
                    tabIndex={-1}
                    aria-label="文件预览"
                  >
                    <FilePreview
                      coordinate={coordinate}
                      version={version}
                      path={selectedPath}
                      exists={selectedExists}
                      canonicalUrl={canonicalUrl}
                      active={tab === "files"}
                      onNavigate={navigate}
                    />
                  </div>
                </div>
              )}
            </div>
          )
        ) : null}
      </div>
    </section>
  );
}

function FilePreview({
  coordinate,
  version,
  path,
  exists,
  canonicalUrl,
  active,
  onNavigate,
}: {
  coordinate: ClawHubCoordinate;
  version: string;
  path: string;
  exists: boolean;
  canonicalUrl: string;
  active: boolean;
  onNavigate: (path: string) => void;
}) {
  const [raw, setRaw] = useState(false);
  const query = useQuery({
    queryKey: ["marketplace", "clawhub", "file", clawhubKey(coordinate), version, path],
    queryFn: () => readClawhubVersionFile(coordinate, version, path),
    enabled: active && exists,
    retry: false,
    staleTime: Infinity,
    gcTime: 300_000,
  });
  const fileLinks = useMemo(
    () => ({ baseDir: parentDir(path), onNavigate }),
    [path, onNavigate],
  );
  const data = query.data;
  return (
    <>
      <div className="marketplace-file-head">
        <span className="mono" title={path}>
          {path}
        </span>
        <div>
          {data ? <span>{formatBytes(data.sizeBytes)}</span> : null}
          {data?.status === "ok" && data.kind === "markdown" ? (
            <Button variant="ghost" size="sm" aria-pressed={raw} onClick={() => setRaw((value) => !value)}>
              {raw ? "返回渲染" : "查看原文"}
            </Button>
          ) : null}
        </div>
      </div>
      <div className="marketplace-file-content">
        {!exists ? (
          <div className="notice">
            <p role="status">此版本中没有该文件，请从文件树选择其他文件。</p>
            <OfficialLink url={canonicalUrl} label="ClawHub 页面" />
          </div>
        ) : query.isPending ? (
          <p role="status">正在读取文件…</p>
        ) : query.isError && !data ? (
          <div className="marketplace-file-notice" role="alert">
            <p>无法读取该文件。</p>
            <MutationError
              alertRole={false}
              error={asAppError(query.error) ?? fallbackError("ClawHub 文件读取失败。")}
            />
            <Button
              variant="outline"
              size="sm"
              disabled={query.isFetching}
              onClick={() => void query.refetch()}
            >
              重试读取文件
            </Button>
            <OfficialLink url={canonicalUrl} label="ClawHub 页面" />
          </div>
        ) : data ? (
          <>
            {query.isError ? (
              <p className="marketplace-file-notice" role="status">
                文件刷新失败，保留上次内容。
              </p>
            ) : null}
            <SkillFileBody
              file={data}
              raw={raw}
              onToggleRaw={() => setRaw((value) => !value)}
              fileLinks={fileLinks}
              hideHead
            />
          </>
        ) : null}
      </div>
    </>
  );
}

function SkillFileBody({
  file,
  raw,
  onToggleRaw,
  fileLinks,
  hideHead = false,
}: {
  file: SkillFileContent;
  raw: boolean;
  onToggleRaw: () => void;
  fileLinks: { baseDir: string; onNavigate: (path: string) => void };
  hideHead?: boolean;
}) {
  if (file.status === "too_large") {
    return (
      <p>
        文件超过 {formatBytes(file.limitBytes)} 预览上限，当前大小 {formatBytes(file.sizeBytes)}
        ，仅显示元数据。
      </p>
    );
  }
  if (file.status === "binary") {
    return <p>此文件为二进制或非 UTF-8 文本，暂不支持预览。</p>;
  }
  if (file.status === "unreadable") {
    return <p role="alert">{file.error ?? "无法读取该文件。"}</p>;
  }
  return (
    <>
      {hideHead ? null : (
        <div className="marketplace-detail-reading-tools">
          <span className="mono">{file.path}</span>
          <div>
            <span>{formatBytes(file.sizeBytes)}</span>
            {file.kind === "markdown" ? (
              <Button variant="ghost" size="sm" aria-pressed={raw} onClick={onToggleRaw}>
                {raw ? "返回渲染" : "查看原文"}
              </Button>
            ) : null}
          </div>
        </div>
      )}
      {file.kind === "image" && file.dataUrl ? (
        <img
          src={file.dataUrl}
          alt={fileName(file.path)}
          className="content-image"
          draggable={false}
        />
      ) : file.kind === "markdown" && !raw ? (
        <SkillMarkdown content={file.content ?? ""} fileLinks={fileLinks} />
      ) : (
        <pre
          className="content-text"
          role="region"
          aria-label={`${file.path} 原文`}
          tabIndex={0}
        >
          {file.content}
        </pre>
      )}
    </>
  );
}

function VersionError({
  error,
  canonicalUrl,
  pending,
  retry,
}: {
  error: unknown;
  canonicalUrl: string;
  pending: boolean;
  retry: () => void;
}) {
  return (
    <div className="notice" role="alert">
      <p>无法读取该版本内容。</p>
      <MutationError
        alertRole={false}
        error={asAppError(error) ?? fallbackError("ClawHub 暂未返回该版本。")}
      />
      <Button variant="outline" size="sm" disabled={pending} onClick={retry}>
        重试版本内容
      </Button>
      <OfficialLink url={canonicalUrl} label="ClawHub 页面" />
    </div>
  );
}

