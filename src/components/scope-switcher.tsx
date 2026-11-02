import { useId, useRef } from "react";
import { ChevronDown, Folder, FolderPen, Globe2, Plus, X } from "lucide-react";
import { Badge } from "./ui/badge";
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
  const failed = entry.error;
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
              className="scope-badge"
              data-testid="scope-update-badge"
              aria-label={`${entry.updateCount} 个更新`}
            >
              {entry.updateCount}
            </Badge>
          ) : null}
          {unavailable ? (
            <Badge
              className="scope-badge"
              variant="warning"
              data-testid="scope-unavailable-badge"
            >
              不可用
            </Badge>
          ) : null}
          {failed ? (
            <Badge
              className="scope-badge"
              variant="destructive"
              data-testid="scope-error-badge"
            >
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

/** Header scope controls with the registered projects in a native popover. */
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
  const menuId = useId();
  const menuRef = useRef<HTMLDivElement>(null);
  const activeProject = projects.find((entry) => entry.active);
  const closeMenu = () => menuRef.current?.hidePopover?.();
  const selectScope = (scope: ScopeId) => {
    closeMenu();
    onSelect(scope);
  };

  return (
    <section aria-label="作用域" className="scope-switcher">
      <button
        type="button"
        aria-label="切换到全局"
        aria-pressed={activeScope === GLOBAL_SCOPE}
        onClick={() => selectScope(GLOBAL_SCOPE)}
        className="scope-tab"
        title="全局 · ~/.agents/skills"
      >
        <Globe2 className="size-3.5 shrink-0" aria-hidden="true" />
        全局
      </button>
      <button
        type="button"
        aria-label="切换项目"
        aria-pressed={activeScope !== GLOBAL_SCOPE}
        aria-haspopup="dialog"
        popoverTarget={menuId}
        className="scope-tab"
        title={
          activeProject
            ? `${activeProject.project.displayName} · ${activeProject.project.root}`
            : "选择项目"
        }
      >
        <Folder className="size-3.5 shrink-0" aria-hidden="true" />
        项目
        <ChevronDown className="size-3 shrink-0" aria-hidden="true" />
      </button>

      <div
        ref={menuRef}
        id={menuId}
        popover="auto"
        role="dialog"
        aria-label="选择项目"
        className="scope-menu"
      >
        <h3 className="filter-label">项目</h3>
        {projects.length === 0 ? (
          <p className="filter-note">尚未添加项目</p>
        ) : (
          projects.map((entry) => (
            <ProjectRow
              key={entry.project.id}
              entry={entry}
              onSelect={selectScope}
              onRelocate={(project) => {
                closeMenu();
                onRelocate(project);
              }}
              onForget={(project) => {
                closeMenu();
                onForget(project);
              }}
            />
          ))
        )}
        <button
          type="button"
          aria-label="添加项目"
          onClick={() => {
            closeMenu();
            onAddProject();
          }}
          className="scope-add-button"
        >
          <Plus className="size-3.5" aria-hidden="true" />
          添加项目
        </button>
      </div>
      {notice || warnings.length > 0 ? (
        <div className="scope-feedback">
          {notice ? (
            <p role="alert" className="filter-note text-destructive">
              {notice}
            </p>
          ) : null}
          {warnings.map((warning) => (
            <p key={warning} role="alert" className="filter-note text-destructive">
              {warning}
            </p>
          ))}
        </div>
      ) : null}
    </section>
  );
}
