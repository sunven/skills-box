import { useQuery } from "@tanstack/react-query";
import { asAppError, getSkillsShSnapshot } from "../../lib/api";
import { skillsShKey, type SkillsShSkill, type SkillsShSnapshot } from "../../lib/skills-sh";
import { formatBytes, parentDir } from "../../lib/skill-content";
import { Button } from "../ui/button";
import { SkillMarkdown } from "../skill-markdown";
import { BrowserFileLayout, BrowserSession, ContentTabs, type Browser } from "./browser";

export function SkillsShContent({ skill, active }: { skill: SkillsShSkill; active: boolean }) {
  const content = useQuery({
    queryKey: ["marketplace", "skills-sh", "snapshot", skillsShKey(skill.coordinate)],
    queryFn: () => getSkillsShSnapshot(skill.coordinate), enabled: active, retry: false, staleTime: 60_000,
  });
  return <>
    <div className="marketplace-snapshot-notice">
      <p>市场内容快照 · 只读</p>
      <p>由 skills.sh 收录，可能与 Git 上游当前内容不同，文件列表也可能不完整。</p>
      <Button variant="ghost" size="sm" disabled={content.isFetching} onClick={() => void content.refetch()}>刷新内容快照</Button>
    </div>
    {content.isPending ? <p role="status">正在读取内容快照…</p> : null}
    {content.isError ? <div role="alert" className="notice">
      <p>内容快照读取失败。{content.data?.files ? "保留上次成功的快照。" : "未获取到可浏览的快照；这不代表上游没有文件。"}</p>
      <p>{asAppError(content.error)?.message}</p>
      <Button variant="outline" size="sm" onClick={() => void content.refetch()}>重试内容快照</Button>
    </div> : null}
    {content.data?.files ? (
      <BrowserSession key={content.dataUpdatedAt} kind="skills-sh">
        {(browser) => <SnapshotContent snapshot={content.data!} browser={browser} />}
      </BrowserSession>
    ) : content.data ? <p className="notice">此条目暂无可浏览的内容快照，可打开官方页面查看。{skill.gitSource ? "仍可通过 Git 仓库预览后安装。" : ""}</p> : null}
  </>;
}

function SnapshotContent({ snapshot, browser }: { snapshot: SkillsShSnapshot; browser: Browser }) {
  const files = snapshot.files ?? [];
  const path = browser.path ?? "SKILL.md";
  const shownPath = browser.tab === "overview" ? "SKILL.md" : path;
  const file = files.find((file) => file.path === shownPath);
  const markdown = /\.(md|markdown)$/i.test(shownPath);
  const content = (
    <BrowserFileLayout browser={browser} files={files} path={path} showTree={browser.tab === "files"}>
      <div className="marketplace-file-head"><span className="mono">{shownPath}</span><div>
        {file ? <span>{formatBytes(file.sizeBytes)}</span> : null}
        {file?.contents != null && markdown ? (
          <Button variant="ghost" size="sm" aria-pressed={browser.raw} onClick={browser.toggleRaw}>
            {browser.raw ? "返回渲染" : "查看原文"}
          </Button>
        ) : null}
      </div></div>
      <div className="marketplace-file-content">
        {!file ? <p role="status">此快照未包含该文件，请选择其他文件或打开官方页面。</p>
          : file.contents === null ? <p>此文件不是可预览文本，或超过 1 MB 预览上限。</p>
          : markdown && !browser.raw ? <SkillMarkdown content={file.contents} fileLinks={{ baseDir: parentDir(shownPath), onNavigate: browser.navigate }} />
          : <pre className="content-text" role="region" aria-label={shownPath + " 原文"} tabIndex={0}>{file.contents}</pre>}
      </div>
    </BrowserFileLayout>
  );
  return <ContentTabs browser={browser} overview={content} files={content} />;
}
