import { useCallback, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ChevronDown, ChevronRight, File, FileText, Folder, FolderOpen, ImageIcon } from "lucide-react";
import { asAppError, listMarketplaceFiles, readMarketplaceFile } from "../lib/api";
import { marketplaceKey, type MarketplaceDetail, type MarketplaceFileEntry } from "../lib/marketplace";
import { fileName, formatBytes, isImagePath, parentDir } from "../lib/skill-content";
import { Button } from "./ui/button";
import { SkillMarkdown } from "./skill-markdown";

interface FileNode {
  path: string;
  name: string;
  entry?: MarketplaceFileEntry;
  children: FileNode[];
}

function fileTree(files: MarketplaceFileEntry[]): FileNode[] {
  const roots: FileNode[] = [];
  const nodes = new Map<string, FileNode>();
  for (const entry of files) {
    const parts = entry.path.split("/");
    let children = roots;
    for (let index = 0; index < parts.length; index++) {
      const path = parts.slice(0, index + 1).join("/");
      let node = nodes.get(path);
      if (!node) {
        node = { path, name: parts[index], children: [] };
        nodes.set(path, node);
        children.push(node);
      }
      if (index === parts.length - 1) node.entry = entry;
      children = node.children;
    }
  }
  function sort(children: FileNode[]) {
    children.sort((a, b) => Number(!!a.entry) - Number(!!b.entry) || a.name.localeCompare(b.name));
    children.forEach((node) => sort(node.children));
  }
  sort(roots);
  return roots;
}

interface Props {
  detail: MarketplaceDetail;
  active: boolean;
  selectedPath: string;
  expanded: Set<string>;
  onSelectFile: (path: string) => void;
  onToggleDir: (path: string) => void;
}

export function MarketplaceFilesTab({ detail, active, selectedPath, expanded, onSelectFile, onToggleDir }: Props) {
  const { coordinate, version } = detail;
  const listing = useQuery({
    queryKey: ["marketplace", "files", marketplaceKey(coordinate), version],
    queryFn: () => listMarketplaceFiles(coordinate, version),
    enabled: active, retry: false, staleTime: Infinity, gcTime: 300_000,
  });
  const nodes = useMemo(() => fileTree(listing.data?.files ?? []), [listing.data]);
  const previewRef = useRef<HTMLDivElement>(null);
  const navigate = useCallback((path: string) => {
    onSelectFile(path);
    previewRef.current?.focus({ preventScroll: true });
    previewRef.current?.scrollIntoView?.({ block: "nearest" });
  }, [onSelectFile]);
  if (!active) return null;

  return (
    <div className="marketplace-files">
      <div className="marketplace-files-summary">
        <span>版本 {version}{listing.data ? ` · ${listing.data.files.length} 个文件` : ""}</span>
        <span>只读预览</span>
      </div>
      {listing.isPending ? <p role="status" className="marketplace-file-notice">正在读取文件列表…</p>
        : listing.isError ? <FileError error={listing.error} label="重试文件列表" retry={() => void listing.refetch()} pending={listing.isFetching} />
        : listing.data?.files.length === 0 ? <p className="marketplace-file-notice">此版本没有可浏览的文件。</p>
        : listing.data ? (
          <div className="marketplace-files-body">
            <nav className="marketplace-file-tree" aria-label="市场文件树">
              <Tree nodes={nodes} selectedPath={selectedPath} expanded={expanded} onToggleDir={onToggleDir} onSelectFile={navigate} />
            </nav>
            <div ref={previewRef} className="marketplace-file-preview" role="region" tabIndex={-1} aria-label="文件预览">
              <FilePreview key={selectedPath} detail={detail} path={selectedPath}
                exists={listing.data.files.some((file) => file.path === selectedPath)} onNavigate={navigate} />
            </div>
          </div>
        ) : null}
    </div>
  );
}

function Tree({ nodes, selectedPath, expanded, onToggleDir, onSelectFile, depth = 0 }: {
  nodes: FileNode[]; selectedPath: string; expanded: Set<string>;
  onToggleDir: (path: string) => void; onSelectFile: (path: string) => void; depth?: number;
}) {
  return <ul className="tree-list">{nodes.map((node) => {
    const directory = !node.entry;
    const open = expanded.has(node.path);
    const Icon = directory ? open ? FolderOpen : Folder : isImagePath(node.path) ? ImageIcon : /\.(md|markdown)$/i.test(node.path) ? FileText : File;
    return <li key={node.path}>
      <button type="button" className="tree-row" title={node.path}
        style={{ paddingLeft: 8 + Math.min(depth, 8) * 12 }}
        aria-expanded={directory ? open : undefined} aria-current={!directory && selectedPath === node.path ? "true" : undefined}
        data-selected={!directory && selectedPath === node.path ? "true" : "false"}
        onClick={() => directory ? onToggleDir(node.path) : onSelectFile(node.path)}>
        {directory ? open ? <ChevronDown className="tree-chevron" aria-hidden="true" /> : <ChevronRight className="tree-chevron" aria-hidden="true" />
          : <span className="tree-chevron" aria-hidden="true" />}
        <Icon className="tree-icon" aria-hidden="true" />
        <span className="tree-name">{node.name}</span>
        {node.entry ? <span className="tree-size">{formatBytes(node.entry.sizeBytes)}</span> : null}
      </button>
      {directory && open ? <Tree nodes={node.children} selectedPath={selectedPath} expanded={expanded}
        onToggleDir={onToggleDir} onSelectFile={onSelectFile} depth={depth + 1} /> : null}
    </li>;
  })}</ul>;
}

function FilePreview({ detail, path, exists, onNavigate }: {
  detail: MarketplaceDetail; path: string; exists: boolean; onNavigate: (path: string) => void;
}) {
  const [raw, setRaw] = useState(false);
  const query = useQuery({
    queryKey: ["marketplace", "file", marketplaceKey(detail.coordinate), detail.version, path],
    queryFn: () => readMarketplaceFile(detail.coordinate, detail.version, path),
    enabled: exists, retry: false, staleTime: Infinity, gcTime: 300_000,
  });
  const links = useMemo(() => ({ baseDir: parentDir(path), onNavigate }), [path, onNavigate]);
  const data = query.data;
  return <>
    <div className="marketplace-file-head">
      <span className="mono" title={path}>{path}</span>
      <div>
        {data ? <span>{formatBytes(data.sizeBytes)}</span> : null}
        {data?.status === "ok" && data.kind === "markdown" ? (
          <Button variant="ghost" size="sm" aria-pressed={raw} onClick={() => setRaw((value) => !value)}>{raw ? "返回渲染" : "查看原文"}</Button>
        ) : null}
      </div>
    </div>
    <div className="marketplace-file-content">
      {!exists ? <p role="status">此版本中没有该文件，请从文件树选择其他文件。</p>
        : query.isPending ? <p role="status">正在读取文件…</p>
        : query.isError ? <FileError error={query.error} label="重试读取文件" retry={() => void query.refetch()} pending={query.isFetching} />
        : data?.status === "too_large" ? <p>文件超过 {formatBytes(data.limitBytes)} 预览上限，当前大小 {formatBytes(data.sizeBytes)}。</p>
        : data?.status === "binary" ? <p>此文件为二进制或非 UTF-8 文本，暂不支持预览。</p>
        : data?.status === "unreadable" ? <p role="alert">{data.error ?? "无法读取该文件。"}</p>
        : data?.kind === "image" && data.dataUrl ? <img src={data.dataUrl} alt={fileName(path)} className="content-image" draggable={false} />
        : data?.kind === "markdown" && !raw ? <SkillMarkdown content={data.content ?? ""} fileLinks={links} />
        : data ? <pre className="content-text" role="region" aria-label={`${path} 原文`} tabIndex={0}>{data.content}</pre> : null}
    </div>
  </>;
}

function FileError({ error, label, retry, pending }: { error: unknown; label: string; retry: () => void; pending: boolean }) {
  const message = asAppError(error)?.message ?? (error instanceof Error ? error.message : "无法读取市场文件。");
  return <div role="alert" className="marketplace-file-notice">
    <p>{message}</p>
    <Button variant="outline" size="sm" disabled={pending} onClick={retry}>{label}</Button>
  </div>;
}
