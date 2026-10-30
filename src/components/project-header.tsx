import { RefreshCw } from "lucide-react";
import { Button } from "./ui/button";
import type { ProjectHeaderView } from "../lib/workspace-view";

interface ProjectHeaderProps {
  view: ProjectHeaderView;
  refreshing: boolean;
  onRefresh: () => void;
}

/**
 * Project view header: identity, root and canonical paths, lock state and the
 * last scoped update check. Ticket 05 makes the check itself per project.
 */
export function ProjectHeader({
  view,
  refreshing,
  onRefresh,
}: ProjectHeaderProps) {
  return (
    <section
      data-testid="project-header"
      className="canvas-head project-header"
      aria-label="项目概览"
    >
      <div className="min-w-0">
        <h1 className="canvas-title">{view.project.displayName}</h1>
        <dl className="project-meta">
          <div>
            <dt>项目路径</dt>
            <dd className="mono" title={view.root}>
              {view.root}
            </dd>
          </div>
          <div>
            <dt>canonical</dt>
            <dd className="mono" title={view.canonicalPath}>
              {view.canonicalPath}
            </dd>
          </div>
          <div>
            <dt>锁状态</dt>
            <dd
              data-testid="project-lock-state"
              data-tone={view.lockTone}
            >
              {view.lockLabel}
            </dd>
          </div>
        </dl>
      </div>
      <div className="project-header-side">
        <span data-testid="project-last-check" className="canvas-stamp">
          {view.lastCheckLabel}
        </span>
        <Button
          variant="outline"
          size="sm"
          disabled={refreshing}
          aria-busy={refreshing}
          onClick={onRefresh}
          title="检查当前项目的更新并刷新清单（ticket 05 按 scope 隔离）"
        >
          <RefreshCw
            className={refreshing ? "size-3.5 animate-spin" : "size-3.5"}
            aria-hidden="true"
          />
          刷新项目
        </Button>
      </div>
    </section>
  );
}
