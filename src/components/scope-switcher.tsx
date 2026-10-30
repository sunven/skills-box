import { Folder, FolderPen, Globe2, Plus, X } from "lucide-react";
import { Badge } from "./ui/badge";
import { cn } from "../lib/utils";
import type { ProjectEntry, ScopeId } from "../lib/types";
import { GLOBAL_SCOPE } from "../lib/types";
import type { ScopeProjectView } from "../lib/workspace-view";

interface ScopeSwitcherProps {
  activeScope: ScopeId;
  projects: ScopeProjectView[];
  warnings: string[];
  notice?: string | null;
  onSelect: (scope: ScopeId) => void;
  onAddProject: () => void;
  onRelocate: (project: ProjectEntry) => void;
  onForget: (project: ProjectEntry) => void;
}

function ProjectRow({
  entry,
  onSelect,
  onRelocate,
  onForget,
}: {
  entry: ScopeProjectView;
  onSelect: (scope: ScopeId) => void;
  onRelocate: (project: ProjectEntry) => void;
  onForget: (project: ProjectEntry) => void;
}) {
  const { project } = entry;
  const unavailable = entry.unavailable;
  const failed = entry.checkFailed || entry.error;
  return (
    <div className="scope-project" data-active={entry.active ? "true" : "false"}>
      <button
        type="button"
        aria-label={`切换到项目 ${project.displayName}`}
        aria-pressed={entry.active}
        title={project.root}
        onClick={() => onSelect(entry.scope)}
        className="filter-row scope-project-row"
      >
        <Folder className="size-3.5 shrink-0" aria-hidden="true" />
        <span className="scope-project-text">
          <span className="filter-name">{project.displayName}</span>
          <span className="scope-project-path mono">{project.root}</span>
        </span>
        <span className="scope-project-badges">
          {entry.updateCount > 0 ? (
            <Badge
              data-testid="scope-update-badge"
              aria-label={`${entry.updateCount} 个更新`}
            >
              {entry.updateCount}
            </Badge>
          ) : null}
          {unavailable ? (
            <Badge variant="warning" data-testid="scope-unavailable-badge">
              不可用
            </Badge>
          ) : null}
          {failed ? (
            <Badge variant="destructive" data-testid="scope-error-badge">
              错误
            </Badge>
          ) : null}
        </span>
      </button>
      <span className="scope-project-actions">
        {unavailable ? (
          <button
            type="button"
            className="scope-icon-button"
            aria-label={`重新定位项目 ${project.displayName}`}
            title="重新定位项目"
            onClick={() => onRelocate(project)}
          >
            <FolderPen className="size-3.5" aria-hidden="true" />
          </button>
        ) : null}
        <button
          type="button"
          className="scope-icon-button"
          aria-label={`遗忘项目 ${project.displayName}`}
          title="从列表移除（不删除文件）"
          onClick={() => onForget(project)}
        >
          <X className="size-3.5" aria-hidden="true" />
        </button>
      </span>
    </div>
  );
}

/** Fixed global entry plus the app-registered project group. */
export function ScopeSwitcher({
  activeScope,
  projects,
  warnings,
  notice,
  onSelect,
  onAddProject,
  onRelocate,
  onForget,
}: ScopeSwitcherProps) {
  return (
    <section aria-label="作用域" className="filter-group scope-switcher">
      <h3 className="filter-label">作用域</h3>
      <button
        type="button"
        aria-label="切换到全局"
        aria-pressed={activeScope === GLOBAL_SCOPE}
        onClick={() => onSelect(GLOBAL_SCOPE)}
        className={cn(
          "filter-row",
          activeScope === GLOBAL_SCOPE && "scope-global-active",
        )}
      >
        <Globe2 className="size-3.5 shrink-0" aria-hidden="true" />
        <span className="filter-name">全局</span>
        <span className="scope-project-path mono">~/.agents/skills</span>
      </button>

      <p className="scope-group-title">项目</p>
      {projects.length === 0 ? (
        <p className="filter-note">尚未添加项目</p>
      ) : (
        projects.map((entry) => (
          <ProjectRow
            key={entry.project.id}
            entry={entry}
            onSelect={onSelect}
            onRelocate={onRelocate}
            onForget={onForget}
          />
        ))
      )}
      <button
        type="button"
        aria-label="添加项目"
        onClick={onAddProject}
        className="scope-add-button"
      >
        <Plus className="size-3.5" aria-hidden="true" />
        + 添加项目
      </button>
      {notice ? (
        <p role="alert" className="filter-note text-destructive">
          {notice}
        </p>
      ) : null}
      {warnings.map((warning) => (
        <p
          key={warning}
          role="alert"
          className="filter-note text-destructive"
        >
          {warning}
        </p>
      ))}
    </section>
  );
}
