import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { TitlebarBack } from "./app-titlebar";
import {
  asAppError,
  fallbackError,
  getClawhubSkillDetail,
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
import type { InventorySnapshot, ScopeId } from "../lib/types";
import { Button } from "./ui/button";
import { ClawHubInstallDialog } from "./clawhub-install-dialog";
import { MarketplaceContentBrowser } from "./marketplace-content-browser";
import { MutationError } from "./mutation/mutation-error";
import { OfficialLink } from "./official-link";

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
 * version and the Content browser resets on change, so the overview and
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
      <TitlebarBack active={active} label="返回列表" onBack={onBack} />
      <main
        className="marketplace-detail-page"
        aria-labelledby="clawhub-detail-title"
        inert={installOpen}
      >
        <div className="marketplace-detail-bar">
          <div className="marketplace-detail-bar-inner">
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
            <MarketplaceContentBrowser
              source={{ kind: "clawhub", coordinate: detail.coordinate, version: selectedVersion,
                canonicalUrl: detail.canonicalUrl, changelog }}
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

