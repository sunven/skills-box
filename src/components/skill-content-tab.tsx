import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  useQueryClient,
  type UseQueryResult,
} from "@tanstack/react-query";
import {
  AlertTriangle,
  Check,
  ChevronDown,
  ChevronRight,
  Copy,
  File,
  FileText,
  Folder,
  FolderOpen,
  Image as ImageIcon,
  Link2,
  Loader2,
  RefreshCw,
} from "lucide-react";
import { Button } from "./ui/button";
import { SkillMarkdown } from "./skill-markdown";
import { asAppError, openSkillFile, openSkillFolder } from "../lib/api";
import {
  fileName,
  formatBytes,
  isImagePath,
  parentDir,
} from "../lib/skill-content";
import {
  useSkillDirQuery,
  useSkillFileQuery,
} from "../hooks/use-skill-content";
import type {
  SkillDirEntry,
  SkillDirListing,
  SkillFileContent,
} from "../lib/types";
import { cn } from "../lib/utils";

interface SkillContentTabProps {
  skillId: string;
  frontmatterWarning: boolean;
}

type DetailView = "rendered" | "raw";
type ContentAction = "folder" | "file";

export const SkillContentTab = memo(function SkillContentTab({
  skillId,
  frontmatterWarning,
}: SkillContentTabProps) {
  const queryClient = useQueryClient();
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());
  const [view, setView] = useState<DetailView>("rendered");
  const [pendingAction, setPendingAction] = useState<ContentAction | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [actionError, setActionError] = useState<ReturnType<typeof asAppError>>(
    null,
  );
  const actionErrorRef = useRef<HTMLDivElement>(null);
  const rootQuery = useSkillDirQuery(skillId, "");

  useEffect(() => {
    if (selectedPath !== null) return;
    const entries = rootQuery.data?.entries;
    if (!entries) return;
    const defaultEntry = entries.find(
      (entry) => entry.path === "SKILL.md" && entry.kind === "file",
    );
    if (defaultEntry) setSelectedPath("SKILL.md");
  }, [rootQuery.data, selectedPath]);

  useEffect(() => {
    if (actionError) actionErrorRef.current?.focus();
  }, [actionError]);

  const navigateTo = useCallback((path: string) => {
    setSelectedPath(path);
    setExpanded((previous) => {
      const next = new Set(previous);
      const parts = path.split("/");
      for (let index = 1; index < parts.length; index += 1) {
        next.add(parts.slice(0, index).join("/"));
      }
      return next;
    });
  }, []);

  const toggleDir = useCallback((path: string) => {
    setExpanded((previous) => {
      const next = new Set(previous);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }, []);

  const refresh = async () => {
    setRefreshing(true);
    setActionError(null);
    try {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["skill", skillId] }),
        queryClient.invalidateQueries({ queryKey: ["skill-dir", skillId] }),
        queryClient.invalidateQueries({ queryKey: ["skill-file", skillId] }),
      ]);
    } finally {
      setRefreshing(false);
    }
  };

  const runOpen = async (kind: ContentAction) => {
    if (kind === "file" && selectedPath === null) return;
    setActionError(null);
    setPendingAction(kind);
    try {
      if (kind === "folder") await openSkillFolder(skillId);
      else await openSkillFile(skillId, selectedPath as string);
    } catch (error) {
      setActionError(asAppError(error) ?? { message: "未知错误" });
    } finally {
      setPendingAction(null);
    }
  };

  return (
    <div className="content-tab">
      <div className="content-toolbar" data-testid="content-toolbar">
        <Button
          variant="ghost"
          size="sm"
          className="h-7 px-2 text-[12px]"
          onClick={() => void refresh()}
          disabled={refreshing}
          aria-busy={refreshing}
        >
          <RefreshCw
            className={cn("size-3.5", refreshing && "animate-spin")}
            aria-hidden="true"
          />
          刷新
        </Button>
        <span className="flex-1" />
        <Button
          variant="outline"
          size="sm"
          disabled={pendingAction !== null}
          onClick={() => void runOpen("folder")}
        >
          {pendingAction === "folder" ? (
            <Loader2 className="animate-spin" aria-hidden="true" />
          ) : (
            <FolderOpen aria-hidden="true" />
          )}
          在 Finder 中查看
        </Button>
        <Button
          variant="outline"
          size="sm"
          disabled={selectedPath === null || pendingAction !== null}
          onClick={() => void runOpen("file")}
        >
          {pendingAction === "file" ? (
            <Loader2 className="animate-spin" aria-hidden="true" />
          ) : (
            <FileText aria-hidden="true" />
          )}
          用默认应用打开
        </Button>
      </div>

      {actionError ? (
        <div
          ref={actionErrorRef}
          role="alert"
          tabIndex={-1}
          className="content-alert notice"
          data-tone="negative"
        >
          <div>
            {actionError.message}
            {actionError.detail ? ` · ${actionError.detail}` : ""}
            {actionError.hint ? ` ${actionError.hint}` : ""}
          </div>
        </div>
      ) : null}

      <div className="content-body">
        <nav className="content-tree" aria-label="文件树">
          {rootQuery.data ? (
            <p
              className="content-tree-root mono"
              title={rootQuery.data.displayPath}
            >
              {rootQuery.data.displayPath}
            </p>
          ) : null}
          <SkillTree
            skillId={skillId}
            rootQuery={rootQuery}
            selectedPath={selectedPath}
            expanded={expanded}
            onToggleDir={toggleDir}
            onSelectFile={setSelectedPath}
          />
        </nav>
        <div className="content-preview">
          <SkillFilePreview
            skillId={skillId}
            path={selectedPath}
            view={view}
            onViewChange={setView}
            frontmatterWarning={frontmatterWarning}
            onNavigate={navigateTo}
            onOpenFile={() => void runOpen("file")}
            openingFile={pendingAction === "file"}
          />
        </div>
      </div>
    </div>
  );
});

function SkillTree({
  skillId,
  rootQuery,
  selectedPath,
  expanded,
  onToggleDir,
  onSelectFile,
}: {
  skillId: string;
  rootQuery: UseQueryResult<SkillDirListing, Error>;
  selectedPath: string | null;
  expanded: Set<string>;
  onToggleDir: (path: string) => void;
  onSelectFile: (path: string) => void;
}) {
  if (rootQuery.isPending) {
    return <TreeState loading text="正在读取目录…" />;
  }
  if (rootQuery.error) {
    return (
      <TreeState
        error={asAppError(rootQuery.error) ?? { message: "无法读取目录。" }}
        onRetry={() => void rootQuery.refetch()}
      />
    );
  }
  const listing = rootQuery.data;
  if (!listing) return null;
  if (listing.entries.length === 0) {
    return <p className="content-tree-note">此 skill 目录为空。</p>;
  }
  return (
    <>
      <ul className="tree-list">
        {listing.entries.map((entry) => (
          <SkillTreeEntry
            key={entry.path}
            entry={entry}
            depth={0}
            skillId={skillId}
            selectedPath={selectedPath}
            expanded={expanded}
            onToggleDir={onToggleDir}
            onSelectFile={onSelectFile}
          />
        ))}
      </ul>
      {listing.truncated ? (
        <p className="content-tree-note">
          已显示前 {listing.maxEntries} 项
        </p>
      ) : null}
    </>
  );
}

function SkillTreeEntry({
  entry,
  depth,
  skillId,
  selectedPath,
  expanded,
  onToggleDir,
  onSelectFile,
}: {
  entry: SkillDirEntry;
  depth: number;
  skillId: string;
  selectedPath: string | null;
  expanded: Set<string>;
  onToggleDir: (path: string) => void;
  onSelectFile: (path: string) => void;
}) {
  if (entry.kind === "dir") {
    return (
      <SkillTreeDir
        entry={entry}
        depth={depth}
        skillId={skillId}
        selectedPath={selectedPath}
        expanded={expanded}
        onToggleDir={onToggleDir}
        onSelectFile={onSelectFile}
      />
    );
  }
  const disabled = entry.kind === "broken" || entry.kind === "outside";
  const selected = entry.path === selectedPath;
  const Icon = entry.markdown
    ? FileText
    : isImagePath(entry.name)
      ? ImageIcon
      : File;
  const title = disabled
    ? entry.kind === "broken"
      ? "符号链接目标缺失"
      : "符号链接指向 skill 目录之外"
    : entry.path;
  return (
    <li>
      <button
        type="button"
        data-testid="tree-row"
        data-path={entry.path}
        className="tree-row"
        data-selected={selected ? "true" : "false"}
        data-disabled={disabled ? "true" : "false"}
        style={{ paddingLeft: 8 + depth * 12 }}
        disabled={disabled}
        title={title}
        aria-current={selected ? "true" : undefined}
        onClick={() => onSelectFile(entry.path)}
      >
        <span className="tree-chevron" aria-hidden="true" />
        <Icon className="tree-icon" aria-hidden="true" />
        <span className="tree-name">{entry.name}</span>
        {entry.symlink ? (
          <>
            <Link2 className="tree-link" aria-hidden="true" />
            <span className="sr-only">（符号链接）</span>
          </>
        ) : null}
        {entry.sizeBytes !== null ? (
          <span className="tree-size">{formatBytes(entry.sizeBytes)}</span>
        ) : null}
      </button>
    </li>
  );
}

function SkillTreeDir({
  entry,
  depth,
  skillId,
  selectedPath,
  expanded,
  onToggleDir,
  onSelectFile,
}: {
  entry: SkillDirEntry;
  depth: number;
  skillId: string;
  selectedPath: string | null;
  expanded: Set<string>;
  onToggleDir: (path: string) => void;
  onSelectFile: (path: string) => void;
}) {
  const isOpen = expanded.has(entry.path);
  const listing = useSkillDirQuery(skillId, entry.path, isOpen);
  const expandable = entry.expandable || isOpen;
  return (
    <li>
      <button
        type="button"
        data-testid="tree-row"
        data-path={entry.path}
        className="tree-row"
        data-selected="false"
        style={{ paddingLeft: 8 + depth * 12 }}
        aria-expanded={expandable ? isOpen : undefined}
        aria-disabled={expandable ? undefined : true}
        disabled={!expandable}
        title={expandable ? entry.path : "已达到最大目录层级"}
        onClick={() => {
          if (expandable) onToggleDir(entry.path);
        }}
      >
        {expandable ? (
          isOpen ? (
            <ChevronDown className="tree-chevron" aria-hidden="true" />
          ) : (
            <ChevronRight className="tree-chevron" aria-hidden="true" />
          )
        ) : (
          <span className="tree-chevron" aria-hidden="true" />
        )}
        {isOpen ? (
          <FolderOpen className="tree-icon" aria-hidden="true" />
        ) : (
          <Folder className="tree-icon" aria-hidden="true" />
        )}
        <span className="tree-name">{entry.name}</span>
        {entry.symlink ? (
          <>
            <Link2 className="tree-link" aria-hidden="true" />
            <span className="sr-only">（符号链接）</span>
          </>
        ) : null}
      </button>
      {isOpen ? (
        listing.isPending ? (
          <TreeState loading indent={depth + 1} text="读取中…" />
        ) : listing.error ? (
          <TreeState
            indent={depth + 1}
            error={
              asAppError(listing.error) ?? { message: "无法读取该目录。" }
            }
            onRetry={() => void listing.refetch()}
          />
        ) : listing.data ? (
          listing.data.entries.length === 0 ? (
            <p
              className="content-tree-note"
              style={{ paddingLeft: 8 + (depth + 1) * 12 + 14 }}
            >
              空目录
            </p>
          ) : (
            <>
              <ul className="tree-list">
                {listing.data.entries.map((child) => (
                  <SkillTreeEntry
                    key={child.path}
                    entry={child}
                    depth={depth + 1}
                    skillId={skillId}
                    selectedPath={selectedPath}
                    expanded={expanded}
                    onToggleDir={onToggleDir}
                    onSelectFile={onSelectFile}
                  />
                ))}
              </ul>
              {listing.data.truncated ? (
                <p
                  className="content-tree-note"
                  style={{ paddingLeft: 8 + (depth + 1) * 12 + 14 }}
                >
                  已显示前 {listing.data.maxEntries} 项
                </p>
              ) : null}
            </>
          )
        ) : null
      ) : null}
    </li>
  );
}

function TreeState({
  loading,
  text,
  error,
  onRetry,
  indent = 0,
}: {
  loading?: boolean;
  text?: string;
  error?: { message: string; hint?: string };
  onRetry?: () => void;
  indent?: number;
}) {
  if (loading) {
    return (
      <p
        className="content-tree-note inline-flex items-center gap-1.5"
        style={{ paddingLeft: 8 + indent * 12 + 14 }}
      >
        <Loader2 className="size-3 animate-spin" aria-hidden="true" />
        {text}
      </p>
    );
  }
  return (
    <div
      className="content-tree-note"
      style={{ paddingLeft: 8 + indent * 12 + 14 }}
    >
      <p className="text-destructive">{error?.message ?? "读取失败。"}</p>
      {error?.hint ? <p>{error.hint}</p> : null}
      {onRetry ? (
        <button
          type="button"
          className="text-primary underline underline-offset-2 hover:text-primary/80"
          onClick={onRetry}
        >
          重试
        </button>
      ) : null}
    </div>
  );
}

function SkillFilePreview({
  skillId,
  path,
  view,
  onViewChange,
  frontmatterWarning,
  onNavigate,
  onOpenFile,
  openingFile,
}: {
  skillId: string;
  path: string | null;
  view: DetailView;
  onViewChange: (view: DetailView) => void;
  frontmatterWarning: boolean;
  onNavigate: (path: string) => void;
  onOpenFile: () => void;
  openingFile: boolean;
}) {
  const query = useSkillFileQuery(skillId, path);
  const [copiedPath, setCopiedPath] = useState<string | null>(null);

  if (path === null) {
    return (
      <div className="content-empty">
        <FileText className="size-5" aria-hidden="true" />
        <p>从左侧选择文件查看内容</p>
        <p className="text-[12px]">
          SKILL.md 默认打开；references、scripts、assets 等其他文件同样只读。
        </p>
      </div>
    );
  }

  const data = query.data;
  const copied = copiedPath === path;
  const copyPath = async () => {
    try {
      await navigator.clipboard.writeText(path);
      setCopiedPath(path);
      window.setTimeout(() => {
        setCopiedPath((current) => (current === path ? null : current));
      }, 1500);
    } catch {
      // Clipboard access is best-effort; the path is visible above.
    }
  };

  return (
    <>
      <div className="content-preview-head">
        <div className="content-preview-path">
          <span
            className="mono"
            title={path}
            data-testid="content-preview-path"
          >
            {path}
          </span>
          <Button
            variant="ghost"
            size="icon"
            className="size-6"
            aria-label={copied ? "已复制路径" : "复制路径"}
            title={copied ? "已复制" : "复制路径"}
            onClick={() => void copyPath()}
          >
            {copied ? (
              <Check className="size-3.5" aria-hidden="true" />
            ) : (
              <Copy className="size-3.5" aria-hidden="true" />
            )}
          </Button>
        </div>
        <div className="content-preview-meta">
          {data ? <span>{formatBytes(data.sizeBytes)}</span> : null}
          {data?.kind === "markdown" && data.status === "ok" ? (
            <div
              role="group"
              aria-label="Markdown 显示方式"
              className="flex items-center gap-1"
            >
              <Button
                size="sm"
                variant={view === "rendered" ? "default" : "ghost"}
                aria-pressed={view === "rendered"}
                className="h-7 px-2 text-[12px]"
                onClick={() => onViewChange("rendered")}
              >
                渲染
              </Button>
              <Button
                size="sm"
                variant={view === "raw" ? "default" : "ghost"}
                aria-pressed={view === "raw"}
                className="h-7 px-2 text-[12px]"
                onClick={() => onViewChange("raw")}
              >
                原文
              </Button>
            </div>
          ) : null}
        </div>
      </div>

      <div className="content-preview-body">
        {query.isPending ? (
          <p
            role="status"
            className="inline-flex items-center gap-1.5 text-[12px] text-muted-foreground"
          >
            <Loader2 className="size-3 animate-spin" aria-hidden="true" />
            正在读取文件…
          </p>
        ) : query.error ? (
          <div role="alert" className="notice" data-tone="negative">
            <div>
              <p className="font-medium">文件读取失败</p>
              <p className="mt-0.5">
                {asAppError(query.error)?.message ?? "未知错误"}
              </p>
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
        ) : data ? (
          <FileBody
            data={data}
            skillId={skillId}
            path={path}
            view={view}
            frontmatterWarning={frontmatterWarning}
            baseDir={parentDir(path)}
            onNavigate={onNavigate}
            onRetry={() => void query.refetch()}
            onOpenFile={onOpenFile}
            openingFile={openingFile}
          />
        ) : null}
      </div>
    </>
  );
}

function FileBody({
  data,
  skillId,
  path,
  view,
  frontmatterWarning,
  baseDir,
  onNavigate,
  onRetry,
  onOpenFile,
  openingFile,
}: {
  data: SkillFileContent;
  skillId: string;
  path: string;
  view: DetailView;
  frontmatterWarning: boolean;
  baseDir: string;
  onNavigate: (path: string) => void;
  onRetry: () => void;
  onOpenFile: () => void;
  openingFile: boolean;
}) {
  const internalLinks = useMemo(
    () => ({ skillId, baseDir, onNavigate }),
    [skillId, baseDir, onNavigate],
  );

  if (data.status === "too_large") {
    return (
      <FileState
        tone="warning"
        title={`文件超过 ${formatBytes(data.limitBytes)} 上限`}
        detail={`当前大小 ${formatBytes(data.sizeBytes)}，未加载内容。`}
        onOpenFile={onOpenFile}
        openingFile={openingFile}
      />
    );
  }
  if (data.status === "binary") {
    return (
      <FileState
        tone="warning"
        title="该文件不是 UTF-8 文本"
        detail={`大小 ${formatBytes(data.sizeBytes)}；使用默认应用查看。`}
        onOpenFile={onOpenFile}
        openingFile={openingFile}
      />
    );
  }
  if (data.status === "unreadable") {
    return (
      <div role="alert" className="notice" data-tone="negative">
        <div>
          <p className="font-medium">无法读取该文件</p>
          <p className="mt-0.5">{data.error ?? "未知错误"}</p>
          <div className="mt-2 flex flex-wrap gap-2">
            <Button variant="outline" size="sm" onClick={onRetry}>
              重试读取
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={openingFile}
              onClick={onOpenFile}
            >
              用默认应用打开
            </Button>
          </div>
        </div>
      </div>
    );
  }

  if (data.kind === "image") {
    return data.dataUrl ? (
      <img
        data-testid="skill-file-image"
        src={data.dataUrl}
        alt={fileName(path)}
        className="content-image"
        draggable={false}
      />
    ) : (
      <FileState
        tone="warning"
        title="图片未加载"
        detail="没有可显示的数据。"
        onOpenFile={onOpenFile}
        openingFile={openingFile}
      />
    );
  }

  const content = data.content ?? "";
  if (data.kind === "markdown") {
    return (
      <>
        {frontmatterWarning && path === "SKILL.md" ? (
          <div className="notice mb-2" data-tone="warning">
            渲染视图可能无法完整隐藏 frontmatter：解析失败，原文可查看完整内容。
          </div>
        ) : null}
        {view === "raw" ? (
          <pre data-testid="skill-file-raw" className="content-text">
            {content}
          </pre>
        ) : (
          <SkillMarkdown content={content} internalLinks={internalLinks} />
        )}
      </>
    );
  }

  return (
    <pre data-testid="skill-file-text" className="content-text">
      {content}
    </pre>
  );
}

function FileState({
  tone,
  title,
  detail,
  onOpenFile,
  openingFile,
}: {
  tone: "warning";
  title: string;
  detail: string;
  onOpenFile: () => void;
  openingFile: boolean;
}) {
  return (
    <div className="notice" data-tone={tone}>
      <div>
        <p className="font-medium">{title}</p>
        <p className="mt-0.5">{detail}</p>
        <Button
          variant="outline"
          size="sm"
          className="mt-2"
          disabled={openingFile}
          onClick={onOpenFile}
        >
          <AlertTriangle className="size-3.5" aria-hidden="true" />
          用默认应用打开
        </Button>
      </div>
    </div>
  );
}
