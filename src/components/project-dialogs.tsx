import { useEffect, useRef, useState } from "react";
import { AlertTriangle, FolderOpen, X } from "lucide-react";
import { Button } from "./ui/button";
import { useModalFocus } from "../hooks/use-modal-focus";
import {
  addProject,
  asAppError,
  fallbackError,
  forgetProject,
  relocateProject,
} from "../lib/api";
import { pickProjectFolder } from "../lib/folder-picker";
import type { AppError, ProjectEntry } from "../lib/types";

export type ProjectDialogState =
  | { mode: "add" }
  | { mode: "relocate"; project: ProjectEntry }
  | { mode: "forget"; project: ProjectEntry }
  | null;

interface ProjectDialogsProps {
  state: ProjectDialogState;
  onClose: () => void;
  onAdded: (entry: ProjectEntry) => void;
  onRelocated: (entry: ProjectEntry) => void;
  onForgotten: (projectId: string) => void;
}

/**
 * Add / relocate / forget project flows. Add and relocate only record a path;
 * forget never touches the filesystem. All three surface backend validation
 * errors inline.
 */
export function ProjectDialogs({
  state,
  onClose,
  onAdded,
  onRelocated,
  onForgotten,
}: ProjectDialogsProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const [path, setPath] = useState("");
  const [picking, setPicking] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<AppError | null>(null);

  const mode = state?.mode ?? null;
  const project = state?.mode === "relocate" || state?.mode === "forget"
    ? state.project
    : null;

  useEffect(() => {
    if (!state) return;
    setPath("");
    setPicking(false);
    setBusy(false);
    setError(null);
  }, [state]);

  useModalFocus({
    open: state !== null,
    panelRef,
    initialFocusRef: closeRef,
    onClose,
    canClose: () => !busy,
  });

  if (!state) return null;

  const dialogTitle =
    mode === "add"
      ? "添加项目"
      : mode === "relocate"
        ? `重新定位项目 · ${project?.displayName ?? ""}`
        : `遗忘项目 · ${project?.displayName ?? ""}`;

  const handlePick = async () => {
    setPicking(true);
    setError(null);
    try {
      const selected = await pickProjectFolder();
      if (selected) setPath(selected);
    } catch (cause) {
      setError(asAppError(cause) ?? fallbackError("无法打开文件夹选择器。"));
    } finally {
      setPicking(false);
    }
  };

  const handleConfirm = async () => {
    if (mode === "forget") {
      if (!project) return;
      setBusy(true);
      setError(null);
      try {
        await forgetProject(project.id);
        onForgotten(project.id);
      } catch (cause) {
        setError(asAppError(cause) ?? fallbackError("遗忘项目失败。"));
        setBusy(false);
      }
      return;
    }
    if (!path) {
      setError(fallbackError("请选择一个项目目录。"));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const entry =
        mode === "relocate" && project
          ? await relocateProject(project.id, path)
          : await addProject(path);
      if (mode === "relocate") onRelocated(entry);
      else onAdded(entry);
    } catch (cause) {
      setError(asAppError(cause) ?? fallbackError("保存项目失败。"));
      setBusy(false);
    }
  };

  return (
    <div
      className="modal-overlay"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !busy) onClose();
      }}
    >
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="project-dialog-title"
        data-testid="project-dialog"
        data-mode={mode}
        className="modal-panel max-w-lg"
      >
        <div className="modal-head">
          <div>
            <h2 id="project-dialog-title" className="modal-title">
              {dialogTitle}
            </h2>
            <p className="modal-sub">
              {mode === "add"
                ? "选择一个已存在的项目目录。应用只记录路径，不会创建或修改任何文件。"
                : mode === "relocate"
                  ? "保留项目 id 与添加时间；保存后会清除该 scope 的更新检查状态。"
                  : "只会从项目列表移除，不会删除磁盘上的任何文件。"}
            </p>
          </div>
          <Button
            ref={closeRef}
            variant="ghost"
            size="icon"
            aria-label="关闭项目对话框"
            disabled={busy}
            onClick={onClose}
            className="size-7 shrink-0"
          >
            <X className="size-4" aria-hidden="true" />
          </Button>
        </div>

        <div className="modal-body">
          {mode === "forget" ? (
            <div
              className="notice"
              data-tone="warning"
              data-testid="project-forget-copy"
            >
              <AlertTriangle
                className="mt-0.5 size-4 shrink-0"
                aria-hidden="true"
              />
              <div>
                <p className="font-medium">
                  不会删除磁盘上的任何文件：项目目录、skill 文件与 lock
                  文件都会保留。
                </p>
                <p className="mt-0.5">
                  只移除注册条目与该 scope
                  的更新检查状态；之后可以重新添加同一目录。
                </p>
              </div>
            </div>
          ) : (
            <>
              <p className="text-[12.5px] text-muted-foreground">
                {mode === "add" ? "项目目录" : "新的项目目录"}
              </p>
              <div className="mt-1.5 flex items-center gap-2">
                <code
                  className="mono min-w-0 flex-1 truncate rounded-md border border-border bg-card px-2.5 py-1.5 text-[12.5px]"
                  title={path}
                >
                  {path || "尚未选择"}
                </code>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={picking || busy}
                  onClick={() => void handlePick()}
                >
                  <FolderOpen className="size-3.5" aria-hidden="true" />
                  选择文件夹
                </Button>
              </div>
              <p className="mt-2 text-[12px] text-muted-foreground">
                允许项目互相嵌套；拒绝文件系统根目录、home 目录以及全局
                canonical 目录。
              </p>
            </>
          )}

          {error ? (
            <p
              role="alert"
              data-testid="project-dialog-error"
              className="mt-3 text-[12.5px] text-destructive"
            >
              {error.message}
              {error.detail ? ` · ${error.detail}` : ""}
            </p>
          ) : null}
        </div>

        <div className="modal-foot">
          <div className="modal-foot-note">
            {mode === "forget" ? "不删除任何文件" : "只记录路径"}
          </div>
          <div className="flex items-center gap-2">
            <Button variant="ghost" size="sm" onClick={onClose} disabled={busy}>
              取消
            </Button>
            <Button
              size="sm"
              variant={mode === "forget" ? "destructive" : "default"}
              disabled={busy || (mode !== "forget" && path.length === 0)}
              onClick={() => void handleConfirm()}
            >
              {busy
                ? "正在保存…"
                : mode === "add"
                  ? "添加项目"
                  : mode === "relocate"
                    ? "重新定位"
                    : "遗忘项目"}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
