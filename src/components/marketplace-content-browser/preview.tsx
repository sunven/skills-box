import { useMemo, useState, type ReactNode } from "react";
import type { SkillFileContent } from "../../lib/types";
import { fileName, formatBytes, parentDir } from "../../lib/skill-content";
import { Button } from "../ui/button";
import { SkillMarkdown } from "../skill-markdown";

type VersionedSource = "skillhub" | "clawhub";

export function VersionedFilePreview({ source, path, file, unavailable, notice, onNavigate }: {
  source: VersionedSource; path: string; file: SkillFileContent | undefined;
  unavailable?: ReactNode; notice?: ReactNode; onNavigate: (path: string) => void;
}) {
  const [raw, setRaw] = useState(false);
  const fileLinks = useMemo(() => ({ baseDir: parentDir(path), onNavigate }), [path, onNavigate]);
  return <>
    <div className="marketplace-file-head">
      <span className="mono" title={path}>{path}</span>
      <div>
        {file ? <span>{formatBytes(file.sizeBytes)}</span> : null}
        {file?.status === "ok" && file.kind === "markdown" ? (
          <Button variant="ghost" size="sm" aria-pressed={raw} onClick={() => setRaw((value) => !value)}>
            {raw ? "返回渲染" : "查看原文"}
          </Button>
        ) : null}
      </div>
    </div>
    <div className="marketplace-file-content">
      {unavailable ?? (file ? <>
        {notice}
        <SkillFileBody source={source} file={file} raw={raw} onToggleRaw={() => setRaw((value) => !value)} fileLinks={fileLinks} hideHead />
      </> : null)}
    </div>
  </>;
}

export function SkillFileBody({ source, file, raw, onToggleRaw, fileLinks, hideHead = false }: {
  source: VersionedSource; file: SkillFileContent; raw: boolean; onToggleRaw: () => void;
  fileLinks: { baseDir: string; onNavigate: (path: string) => void }; hideHead?: boolean;
}) {
  if (file.status === "too_large") {
    return <p>文件超过 {formatBytes(file.limitBytes)} 预览上限，当前大小 {formatBytes(file.sizeBytes)}
      {source === "clawhub" ? "，仅显示元数据。" : "。"}</p>;
  }
  if (file.status === "binary") return <p>此文件为二进制或非 UTF-8 文本，暂不支持预览。</p>;
  if (file.status === "unreadable") return <p role="alert">{file.error ?? "无法读取该文件。"}</p>;
  return <>
    {hideHead ? null : (
      <div className="marketplace-detail-reading-tools">
        <span className="mono">{file.path}</span>
        <div>
          <span>{formatBytes(file.sizeBytes)}</span>
          {file.kind === "markdown" ? (
            <Button variant="ghost" size="sm" aria-pressed={raw} onClick={onToggleRaw}>
              {raw ? "返回渲染" : "查看原文"}
            </Button>
          ) : null}
        </div>
      </div>
    )}
    {file.kind === "image" && file.dataUrl ? (
      <img src={file.dataUrl} alt={fileName(file.path)} className="content-image" draggable={false} />
    ) : file.kind === "markdown" && !raw ? (
      <SkillMarkdown content={file.content ?? ""} fileLinks={fileLinks} />
    ) : (
      <pre className="content-text" role="region" aria-label={file.path + " 原文"} tabIndex={0}>{file.content}</pre>
    )}
  </>;
}
