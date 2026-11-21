import { useCallback, useMemo, useRef, useState, type ReactNode } from "react";
import { ChevronDown, ChevronRight, File, FileText, Folder, FolderOpen, ImageIcon } from "lucide-react";
import type { MarketplaceFileEntry } from "../../lib/marketplace";
import type { MarketplaceSourceId } from "../../lib/skills-sh";
import { formatBytes, isImagePath } from "../../lib/skill-content";

type Tab = "overview" | "files";
export type Browser = ReturnType<typeof useBrowser>;

export function BrowserSession({ kind, children }: {
  kind: MarketplaceSourceId;
  children: (browser: Browser) => ReactNode;
}) {
  return children(useBrowser(kind));
}

function useBrowser(kind: MarketplaceSourceId) {
  const [tab, setTab] = useState<Tab>("overview");
  const [path, setPath] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(new Set<string>());
  const [raw, setRaw] = useState(false);
  const [visitedFiles, setVisitedFiles] = useState(false);
  const tabsRef = useRef<HTMLDivElement>(null);
  const previewRef = useRef<HTMLDivElement>(null);
  const prefix = kind === "skillhub" ? "marketplace" : kind === "clawhub" ? "clawhub-version" : "skills-sh";

  function activate(next: Tab) {
    setTab(next);
    if (next === "files") setVisitedFiles(true);
    if (kind !== "skillhub") setRaw(false);
  }

  const navigate = useCallback((next: string) => {
    setPath(next);
    setTab("files");
    setVisitedFiles(true);
    if (kind !== "skillhub") setRaw(false);
    setExpanded((previous) => {
      const result = new Set(previous);
      const parts = next.split("/");
      for (let index = 1; index < parts.length; index++) result.add(parts.slice(0, index).join("/"));
      return result;
    });
    if (kind === "skillhub" && tab === "overview") {
      tabsRef.current?.querySelector<HTMLButtonElement>("#marketplace-tab-files")?.focus({ preventScroll: true });
      tabsRef.current?.scrollIntoView?.({ block: "start" });
    } else {
      previewRef.current?.focus({ preventScroll: true });
      previewRef.current?.scrollIntoView?.({ block: "nearest" });
    }
  }, [kind, tab]);

  function toggleDir(dir: string) {
    setExpanded((previous) => {
      const next = new Set(previous);
      if (next.has(dir)) next.delete(dir); else next.add(dir);
      return next;
    });
  }

  return {
    kind, prefix, tab, path, expanded, raw, visitedFiles, tabsRef, previewRef,
    activate, navigate, toggleDir, toggleRaw: () => setRaw((value) => !value),
  };
}

export function ContentTabs({ browser, overview, files }: {
  browser: Browser; overview: ReactNode; files: ReactNode;
}) {
  const { kind, prefix, tab, tabsRef, activate } = browser;
  const snapshot = kind === "skills-sh";
  return <>
    <div ref={tabsRef} className="marketplace-detail-tabs" role="tablist"
      aria-label={kind === "clawhub" ? "ClawHub 版本内容" : "市场详情内容"}
      onKeyDown={(event) => {
        if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
        event.preventDefault();
        const next = event.key === "Home" ? "overview" : event.key === "End" ? "files" : tab === "overview" ? "files" : "overview";
        activate(next);
        tabsRef.current?.querySelector<HTMLButtonElement>("#" + prefix + "-tab-" + next)?.focus();
      }}>
      {(["overview", "files"] as const).map((value) => (
        <button key={value} type={snapshot ? undefined : "button"} role="tab" id={prefix + "-tab-" + value}
          aria-controls={snapshot ? "skills-sh-content" : prefix + "-panel-" + value}
          aria-selected={tab === value} tabIndex={tab === value ? 0 : -1} onClick={() => activate(value)}>
          {value === "overview" ? "概述" : "文件"}
        </button>
      ))}
    </div>
    {snapshot ? (
      <div id="skills-sh-content" role="tabpanel" aria-labelledby={"skills-sh-tab-" + tab} tabIndex={0}>
        {tab === "overview" ? overview : files}
      </div>
    ) : <>
      <div id={prefix + "-panel-overview"} role="tabpanel" aria-labelledby={prefix + "-tab-overview"}
        hidden={tab !== "overview"} tabIndex={0}>
        {tab === "overview" ? overview : null}
      </div>
      <div id={prefix + "-panel-files"} role="tabpanel" aria-labelledby={prefix + "-tab-files"}
        hidden={tab !== "files"} tabIndex={0}>
        {(kind === "skillhub" ? browser.visitedFiles : tab === "files") ? files : null}
      </div>
    </>}
  </>;
}

export function BrowserFileLayout({ browser, files, path, showTree = true, children }: {
  browser: Browser; files: MarketplaceFileEntry[]; path: string; showTree?: boolean; children: ReactNode;
}) {
  const nodes = useMemo(() => fileTree(files), [files]);
  return <div className={showTree ? "marketplace-files-body" : "marketplace-detail-overview"}>
    {showTree ? <nav className="marketplace-file-tree" aria-label={browser.kind === "clawhub" ? "ClawHub 文件树" : "市场文件树"}>
      <Tree nodes={nodes} browser={browser} selectedPath={path} />
    </nav> : null}
    <div ref={browser.previewRef} className="marketplace-file-preview" role="region" tabIndex={-1} aria-label="文件预览">
      {children}
    </div>
  </div>;
}

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

function Tree({ nodes, browser, selectedPath, depth = 0 }: {
  nodes: FileNode[]; browser: Browser; selectedPath: string; depth?: number;
}) {
  return <ul className="tree-list">{nodes.map((node) => {
    const directory = !node.entry;
    const open = browser.expanded.has(node.path);
    const Icon = directory ? open ? FolderOpen : Folder : isImagePath(node.path) ? ImageIcon : /\.(md|markdown)$/i.test(node.path) ? FileText : File;
    return <li key={node.path}>
      <button type="button" className="tree-row" title={node.path}
        style={{ paddingLeft: 8 + Math.min(depth, 8) * 12 }}
        aria-expanded={directory ? open : undefined} aria-current={!directory && selectedPath === node.path ? "true" : undefined}
        data-selected={!directory && selectedPath === node.path ? "true" : "false"}
        onClick={() => directory ? browser.toggleDir(node.path) : browser.navigate(node.path)}>
        {directory ? open ? <ChevronDown className="tree-chevron" aria-hidden="true" /> : <ChevronRight className="tree-chevron" aria-hidden="true" />
          : <span className="tree-chevron" aria-hidden="true" />}
        <Icon className="tree-icon" aria-hidden="true" />
        <span className="tree-name">{node.name}</span>
        {node.entry ? <span className="tree-size">{formatBytes(node.entry.sizeBytes)}</span> : null}
      </button>
      {directory && open ? <Tree nodes={node.children} browser={browser} selectedPath={selectedPath} depth={depth + 1} /> : null}
    </li>;
  })}</ul>;
}
