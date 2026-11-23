import { useQuery } from "@tanstack/react-query";
import { asAppError, fallbackError, listClawhubVersionFiles, readClawhubVersionFile } from "../../lib/api";
import { clawhubKey, type ClawHubCoordinate } from "../../lib/clawhub";
import { Button } from "../ui/button";
import { MutationError } from "../mutation/mutation-error";
import { OfficialLink } from "../official-link";
import { BrowserFileLayout, ContentTabs, type Browser } from "./browser";
import { SkillFileBody, VersionedFilePreview } from "./preview";

export interface ClawHubContentSource {
  kind: "clawhub";
  coordinate: ClawHubCoordinate;
  version: string;
  canonicalUrl: string;
  changelog: string | null;
}

export function ClawHubContent({ source, active, browser }: {
  source: ClawHubContentSource; active: boolean; browser: Browser;
}) {
  const { coordinate, version, canonicalUrl, changelog } = source;
  const listing = useQuery({
    queryKey: ["marketplace", "clawhub", "files", clawhubKey(coordinate), version],
    queryFn: () => listClawhubVersionFiles(coordinate, version),
    enabled: active, retry: false, staleTime: Infinity, gcTime: 300_000,
  });
  const files = listing.data?.files ?? [];
  const hasSkillMd = files.some((file) => file.path === "SKILL.md");
  // The listing response wins; the detail-list fallback belongs to this version.
  const versionChangelog = listing.data?.changelog ?? changelog;
  const overview = useQuery({
    queryKey: ["marketplace", "clawhub", "file", clawhubKey(coordinate), version, "SKILL.md"],
    queryFn: () => readClawhubVersionFile(coordinate, version, "SKILL.md"),
    // Start in parallel, but ignore the result if the listing omits SKILL.md.
    enabled: active && (hasSkillMd || listing.isPending),
    retry: false, staleTime: Infinity, gcTime: 300_000,
  });
  const path = browser.path ?? (hasSkillMd ? "SKILL.md" : (files[0]?.path ?? ""));
  const fileLinks = { baseDir: "", onNavigate: browser.navigate };
  function refresh() {
    void listing.refetch();
    if (hasSkillMd) void overview.refetch();
  }
  const listingUnavailable = listing.isPending ? <p role="status">正在读取版本文件…</p>
    : listing.isError && !listing.data ? (
      <VersionError error={listing.error} canonicalUrl={canonicalUrl} pending={listing.isFetching} retry={refresh} />
    ) : undefined;
  const listingNotice = listing.isError ? (
    <p className="marketplace-file-notice" role="status">文件列表刷新失败，保留上次内容。</p>
  ) : null;

  return <section aria-label={"ClawHub 版本 v" + version}>
    <div className="marketplace-snapshot-notice">
      <p>ClawHub 版本 v{version} 的打包内容</p>
      <p>浏览内容不保证与将来安装时重新解析的内容完全一致；安装以实际下载内容为准。</p>
      {listing.data ? <p>许可证：{listing.data.license ?? "未提供"}</p> : null}
      <p>解析引用：v{version}（ClawHub archive 版本；安装按确认时的 latest 版本解析）</p>
      <Button variant="ghost" size="sm" disabled={listing.isFetching} onClick={refresh}>刷新版本内容</Button>
    </div>
    {versionChangelog ? (
      <section className="marketplace-detail-overview" aria-label="版本更新说明">
        <h2 className="text-sm font-semibold">更新说明</h2>
        <pre className="content-text" role="region" aria-label="版本更新说明原文" tabIndex={0}>{versionChangelog}</pre>
      </section>
    ) : null}
    <ContentTabs browser={browser}
      overview={
        <section className="marketplace-detail-overview" aria-label={"版本 v" + version + " 概述"}>
          {listingUnavailable ?? <>
            {listingNotice}
            {!hasSkillMd ? <p role="status">该版本未提供 SKILL.md，可在「文件」中查看该版本的其他文件。</p>
              : overview.isPending ? <p role="status">正在读取 SKILL.md…</p>
              : overview.isError && !overview.data ? (
                <VersionError error={overview.error} canonicalUrl={canonicalUrl} pending={overview.isFetching} retry={refresh} />
              ) : overview.data ? <>
                {overview.isError ? <p className="marketplace-file-notice" role="status">SKILL.md 刷新失败，保留上次内容。</p> : null}
                <SkillFileBody source="clawhub" file={overview.data} raw={browser.raw}
                  onToggleRaw={browser.toggleRaw} fileLinks={fileLinks} />
              </> : null}
          </>}
        </section>
      }
      files={listingUnavailable ?? (
        <div className="marketplace-files">
          <div className="marketplace-files-summary">
            <span>版本 v{version}{listing.data ? " · " + files.length + " 个文件" : ""}</span>
            <span>只读预览</span>
          </div>
          {listingNotice}
          {files.length === 0 ? <p className="marketplace-file-notice">此版本没有可浏览的文件。</p> : (
            <BrowserFileLayout browser={browser} files={files} path={path}>
              <ClawHubFile source={source} path={path} exists={files.some((file) => file.path === path)}
                onNavigate={browser.navigate} />
            </BrowserFileLayout>
          )}
        </div>
      )}
    />
  </section>;
}

function ClawHubFile({ source, path, exists, onNavigate }: {
  source: ClawHubContentSource; path: string; exists: boolean; onNavigate: (path: string) => void;
}) {
  const { coordinate, version, canonicalUrl } = source;
  const query = useQuery({
    queryKey: ["marketplace", "clawhub", "file", clawhubKey(coordinate), version, path],
    queryFn: () => readClawhubVersionFile(coordinate, version, path),
    enabled: exists, retry: false, staleTime: Infinity, gcTime: 300_000,
  });
  return <VersionedFilePreview source="clawhub" path={path} file={query.data} onNavigate={onNavigate}
    unavailable={!exists ? (
      <div className="notice">
        <p role="status">此版本中没有该文件，请从文件树选择其他文件。</p>
        <OfficialLink url={canonicalUrl} label="ClawHub 页面" />
      </div>
    ) : query.isPending ? <p role="status">正在读取文件…</p>
      : query.isError && !query.data ? (
        <div className="marketplace-file-notice" role="alert">
          <p>无法读取该文件。</p>
          <MutationError alertRole={false} error={asAppError(query.error) ?? fallbackError("ClawHub 文件读取失败。")} />
          <Button variant="outline" size="sm" disabled={query.isFetching} onClick={() => void query.refetch()}>重试读取文件</Button>
          <OfficialLink url={canonicalUrl} label="ClawHub 页面" />
        </div>
      ) : undefined}
    notice={query.isError ? <p className="marketplace-file-notice" role="status">文件刷新失败，保留上次内容。</p> : null}
  />;
}

function VersionError({ error, canonicalUrl, pending, retry }: {
  error: unknown; canonicalUrl: string; pending: boolean; retry: () => void;
}) {
  return <div className="notice" role="alert">
    <p>无法读取该版本内容。</p>
    <MutationError alertRole={false} error={asAppError(error) ?? fallbackError("ClawHub 暂未返回该版本。")} />
    <Button variant="outline" size="sm" disabled={pending} onClick={retry}>重试版本内容</Button>
    <OfficialLink url={canonicalUrl} label="ClawHub 页面" />
  </div>;
}
