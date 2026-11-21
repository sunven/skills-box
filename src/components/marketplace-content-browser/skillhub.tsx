import { useQuery } from "@tanstack/react-query";
import { asAppError, listMarketplaceFiles, readMarketplaceFile } from "../../lib/api";
import { marketplaceKey, type MarketplaceDetail } from "../../lib/marketplace";
import { Button } from "../ui/button";
import { SkillMarkdown } from "../skill-markdown";
import { BrowserFileLayout, ContentTabs, type Browser } from "./browser";
import { VersionedFilePreview } from "./preview";

export function SkillHubContent({ detail, browser }: { detail: MarketplaceDetail; browser: Browser }) {
  return <ContentTabs browser={browser}
    overview={
      <section className="marketplace-detail-overview" aria-label="技能概述">
        <div className="marketplace-detail-reading-tools">
          <span>SKILL.md</span>
          <Button variant="ghost" size="sm" aria-pressed={browser.raw} onClick={browser.toggleRaw}>
            {browser.raw ? "返回渲染" : "查看原文"}
          </Button>
        </div>
        {browser.raw ? <pre className="marketplace-detail-source" role="region" aria-label="SKILL.md 原文" tabIndex={0}>{detail.markdown}</pre>
          : <SkillMarkdown content={detail.markdown} fileLinks={{ baseDir: "", onNavigate: browser.navigate }} />}
      </section>
    }
    files={<SkillHubFiles detail={detail} browser={browser} />}
  />;
}

function SkillHubFiles({ detail, browser }: { detail: MarketplaceDetail; browser: Browser }) {
  const { coordinate, version } = detail;
  const listing = useQuery({
    queryKey: ["marketplace", "files", marketplaceKey(coordinate), version],
    queryFn: () => listMarketplaceFiles(coordinate, version),
    enabled: browser.tab === "files", retry: false, staleTime: Infinity, gcTime: 300_000,
  });
  const path = browser.path ?? "SKILL.md";
  if (browser.tab !== "files") return null;
  return <div className="marketplace-files">
    <div className="marketplace-files-summary">
      <span>版本 {version}{listing.data ? " · " + listing.data.files.length + " 个文件" : ""}</span>
      <span>只读预览</span>
    </div>
    {listing.isPending ? <p role="status" className="marketplace-file-notice">正在读取文件列表…</p>
      : listing.isError ? <FileError error={listing.error} label="重试文件列表" retry={() => void listing.refetch()} pending={listing.isFetching} />
      : listing.data?.files.length === 0 ? <p className="marketplace-file-notice">此版本没有可浏览的文件。</p>
      : listing.data ? (
        <BrowserFileLayout browser={browser} files={listing.data.files} path={path}>
          <SkillHubFile key={path} detail={detail} path={path}
            exists={listing.data.files.some((file) => file.path === path)} onNavigate={browser.navigate} />
        </BrowserFileLayout>
      ) : null}
  </div>;
}

function SkillHubFile({ detail, path, exists, onNavigate }: {
  detail: MarketplaceDetail; path: string; exists: boolean; onNavigate: (path: string) => void;
}) {
  const query = useQuery({
    queryKey: ["marketplace", "file", marketplaceKey(detail.coordinate), detail.version, path],
    queryFn: () => readMarketplaceFile(detail.coordinate, detail.version, path),
    enabled: exists, retry: false, staleTime: Infinity, gcTime: 300_000,
  });
  return <VersionedFilePreview source="skillhub" path={path} file={query.data} onNavigate={onNavigate}
    unavailable={!exists ? <p role="status">此版本中没有该文件，请从文件树选择其他文件。</p>
      : query.isPending ? <p role="status">正在读取文件…</p>
      : query.isError ? <FileError error={query.error} label="重试读取文件" retry={() => void query.refetch()} pending={query.isFetching} />
      : undefined} />;
}

function FileError({ error, label, retry, pending }: { error: unknown; label: string; retry: () => void; pending: boolean }) {
  const message = asAppError(error)?.message ?? (error instanceof Error ? error.message : "无法读取市场文件。");
  return <div role="alert" className="marketplace-file-notice">
    <p>{message}</p>
    <Button variant="outline" size="sm" disabled={pending} onClick={retry}>{label}</Button>
  </div>;
}
