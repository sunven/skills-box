import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ArrowLeft, Search } from "lucide-react";
import { asAppError, getSkillsShSnapshot, listSkillsSh } from "../lib/api";
import { useMarketplaceSession } from "../hooks/use-marketplace-session";
import { skillsShInstallation, skillsShKey, skillsShViews, type MarketplaceInstallSeed, type SkillsShListing, type SkillsShSkill, type SkillsShSnapshot, type SkillsShView } from "../lib/skills-sh";
import type { InventorySnapshot } from "../lib/types";
import { formatBytes, parentDir } from "../lib/skill-content";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { MarketplaceFileTree } from "./marketplace-files-tab";
import { OfficialLink } from "./official-link";
import { SkillMarkdown } from "./skill-markdown";

interface Props {
  active: boolean;
  scopeLabel: string;
  snapshot?: InventorySnapshot;
  disabled: boolean;
  gitEnabled: boolean;
  onRecheck: () => void;
  onInstall: (seed: MarketplaceInstallSeed) => void;
  onView: (name: string) => void;
}
interface SearchState { keyword: string; view: SkillsShView; page: number }
const initialSearch: SearchState = { keyword: "", view: "all-time", page: 0 };

export function SkillsShPage(props: Props) {
  const [session, setSession] = useMarketplaceSession("skills-sh", { input: "", search: initialSearch, scroll: 0 });
  const { input, search } = session;
  const [selected, setSelected] = useState<{ skill: SkillsShSkill; metric: string } | null>(null);
  const listRef = useRef<HTMLElement>(null);
  const opener = useRef<HTMLButtonElement | null>(null);
  const previous = useRef<{ search: SearchState; listing: SkillsShListing } | null>(null);
  const catalog = useQuery({
    queryKey: ["marketplace", "skills-sh", "list", search.keyword, search.view, search.page],
    queryFn: () => listSkillsSh(search.keyword, search.view, search.page),
    enabled: props.active, retry: false, staleTime: 60_000,
  });
  useEffect(() => { if (catalog.data) previous.current = { search, listing: catalog.data }; }, [catalog.data, search]);
  useLayoutEffect(() => {
    if (props.active && !selected && listRef.current) {
      listRef.current.scrollTop = session.scroll;
    }
  }, [props.active, selected]);
  useLayoutEffect(() => {
    if (!selected) opener.current?.focus({ preventScroll: true });
  }, [selected]);
  const fallback = catalog.isError && !catalog.data ? previous.current : null;
  const listing = catalog.data ?? fallback?.listing;
  const shownSearch = fallback?.search ?? search;
  const shownView = skillsShViews.find((view) => view.value === shownSearch.view)!;
  const metric = shownSearch.keyword ? "累计安装量" : shownView.metric;
  function changeSearch(next: SearchState) {
    if (listRef.current) listRef.current.scrollTop = 0;
    setSession((value) => ({ ...value, search: next, scroll: 0 }));
  }
  function openDetail(skill: SkillsShSkill, button: HTMLButtonElement) {
    const scroll = listRef.current?.scrollTop ?? 0;
    setSession((value) => ({ ...value, scroll }));
    opener.current = button;
    setSelected({ skill, metric });
  }
  return <>
    <main ref={listRef} hidden={selected !== null} className="discovery-page" aria-labelledby="skills-sh-title"
      onScroll={(event) => { const scroll = event.currentTarget.scrollTop; setSession((value) => ({ ...value, scroll })); }}>
      <div className="discovery-content">
        <div className="discovery-heading">
          <div><h1 id="skills-sh-title">技能市场</h1><p className="text-muted-foreground">从 skills.sh 查找技能，通过 GitHub 仓库安装到当前作用域。</p></div>
          <span className="badge">来源 · skills.sh</span>
        </div>
        <div className="marketplace-toolbar">
          <div className="marketplace-sort" role="group" aria-label="skills.sh 榜单">
            {skillsShViews.map((view) => <Button key={view.value} variant="ghost"
              aria-pressed={!search.keyword && search.view === view.value}
              onClick={() => {
                setSession((value) => ({ ...value, input: "" }));
                changeSearch({ keyword: "", view: view.value, page: 0 });
              }}>
              {view.label}
            </Button>)}
          </div>
        </div>
        <form className="discovery-controls" onSubmit={(event) => {
          event.preventDefault();
          if (!input.trim() || Array.from(input.trim()).length >= 2) changeSearch({ ...search, keyword: input.trim(), page: 0 });
        }}>
          <label className="discovery-search"><Search className="size-4" aria-hidden="true" />
            <Input aria-label="搜索 skills.sh" placeholder="搜索技能名称或用途（至少两个字符）" value={input} maxLength={400}
              onChange={(event) => { const input = event.target.value; setSession((value) => ({ ...value, input })); }} />
          </label>
          <Button type="submit" variant="outline" disabled={Boolean(input.trim()) && Array.from(input.trim()).length < 2}>搜索</Button>
          <Button type="button" variant="ghost" disabled={catalog.isFetching} onClick={() => void catalog.refetch()}>刷新目录</Button>
        </form>
        {catalog.isError ? <div className="notice" role="alert">
          <p>skills.sh 请求失败。{listing ? `保留上次成功结果：${shownSearch.keyword ? `搜索「${shownSearch.keyword}」` : shownView.label}，第 ${shownSearch.page + 1} 页。` : "尚无可显示的结果。"}</p>
          <p>{asAppError(catalog.error)?.detail}</p>
          <Button variant="outline" size="sm" onClick={() => void catalog.refetch()}>重试</Button>
          <OfficialLink url="https://www.skills.sh/" />
        </div> : null}
        {catalog.isFetching ? <p role="status">正在加载 skills.sh…</p> : null}
        {listing ? <>
          <p className="text-xs text-muted-foreground">{shownSearch.keyword
            ? `搜索「${shownSearch.keyword}」 · 返回 ${listing.skills.length} 项（最多 200 项）`
            : `${shownView.label} · 榜单可浏览 ${listing.total.toLocaleString()} 项 · 第 ${listing.page + 1} 页`} · {metric}</p>
          {shownSearch.keyword && listing.skills.length >= 200 ? <p role="status">已达到搜索返回上限，请缩小搜索范围。</p> : null}
          <div className="marketplace-grid">
            {listing.skills.map((skill) => {
              const { installed } = skillsShInstallation(skill, props.snapshot);
              return <article key={skillsShKey(skill.coordinate)} className="marketplace-card">
                <button className="marketplace-card-title" onClick={(event) => openDetail(skill, event.currentTarget)}>{skill.name}</button>
                <p className="font-mono text-xs text-muted-foreground break-all">{skillsShKey(skill.coordinate)}</p>
                <p className="text-xs text-muted-foreground">{metric}：{skill.installs?.toLocaleString() ?? "—"}</p>
                {!skill.gitSource ? <p className="text-xs text-muted-foreground">此来源暂不支持应用内安装</p> : null}
                <div className="flex justify-end mt-auto">
                  {installed ? <Button variant="outline" size="sm" onClick={() => props.onView(installed.name)}>已安装 · 查看</Button>
                    : <Button variant="ghost" size="sm" onClick={(event) => openDetail(skill, event.currentTarget)}>查看详情</Button>}
                </div>
              </article>;
            })}
          </div>
          {!listing.skills.length ? <p className="discovery-empty">没有找到相关技能，请调整搜索词。</p> : null}
          {!shownSearch.keyword ? <div className="flex justify-between items-center mt-4">
            <Button variant="outline" disabled={catalog.isFetching || listing.page <= 0}
              onClick={() => changeSearch({ ...shownSearch, page: listing.page - 1 })}>上一页</Button>
            <Button variant="outline" disabled={catalog.isFetching || !listing.hasMore}
              onClick={() => changeSearch({ ...shownSearch, page: listing.page + 1 })}>下一页</Button>
          </div> : null}
        </> : null}
      </div>
    </main>
    {selected ? <SkillsShDetail {...props} key={skillsShKey(selected.skill.coordinate)} skill={selected.skill}
      metric={selected.metric} onBack={() => setSelected(null)} /> : null}
  </>;
}

function SkillsShDetail({ skill, metric, onBack, ...props }: Props & { skill: SkillsShSkill; metric: string; onBack: () => void }) {
  const heading = useRef<HTMLHeadingElement>(null);
  const content = useQuery({
    queryKey: ["marketplace", "skills-sh", "snapshot", skillsShKey(skill.coordinate)],
    queryFn: () => getSkillsShSnapshot(skill.coordinate), enabled: props.active, retry: false, staleTime: 60_000,
  });
  const { installed, collision } = skillsShInstallation(skill, props.snapshot);
  useEffect(() => { heading.current?.focus({ preventScroll: true }); }, []);
  return <main className="marketplace-detail-page" aria-labelledby="skills-sh-detail-title">
    <div className="marketplace-detail-bar"><div className="marketplace-detail-bar-inner">
      <Button variant="ghost" size="sm" onClick={onBack}><ArrowLeft aria-hidden="true" />返回列表</Button>
      <span className="marketplace-detail-bar-name">{skill.name}</span>
      <div className="marketplace-detail-action">
        <span className="marketplace-detail-target">安装到：{props.scopeLabel}</span>
        <Button size="sm" disabled={!installed && (props.disabled || !props.gitEnabled || !skill.gitSource || !!collision)}
          onClick={() => installed ? props.onView(installed.name) : skill.gitSource && props.onInstall({
            source: skill.gitSource, name: skill.name, origin: "skills-sh", officialUrl: skill.officialUrl,
          })}>{installed ? "查看已安装技能" : "安装"}</Button>
      </div>
    </div></div>
    <div className="marketplace-detail-content">
      <header className="marketplace-detail-header">
        <h1 id="skills-sh-detail-title" ref={heading} tabIndex={-1}>{skill.name}</h1>
        <p className="marketplace-detail-coordinate">{skillsShKey(skill.coordinate)}</p>
        <p className="marketplace-detail-metadata">收录于 skills.sh{skill.gitSource ? ` · GitHub 仓库：${skill.gitSource}` : ` · 来源：${skill.coordinate.source}`}</p>
        <div className="marketplace-detail-links">
          <span>{metric}：{skill.installs?.toLocaleString() ?? "—"}</span><OfficialLink url={skill.officialUrl} />
          {skill.gitSource ? <OfficialLink url={`https://github.com/${skill.gitSource}`} label="GitHub 仓库" /> : null}
        </div>
        {!skill.gitSource ? <p className="notice">此来源暂不支持应用内安装，可前往官方页面查看。</p> : null}
        {collision ? <p className="notice" role="alert">当前作用域已有同名技能 {collision.name}（{collision.source ?? "本地技能"}），请先处理原安装。</p> : null}
        {!props.gitEnabled && skill.gitSource && !installed ? <div className="notice" role="status">
          <p>当前 Git 安装不可用，请检查本地环境与作用域状态。目录和内容仍可浏览。</p>
          <Button variant="outline" size="sm" onClick={props.onRecheck}>重新检测环境</Button>
        </div> : null}
      </header>
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
      {content.data?.files ? <SnapshotContent key={content.dataUpdatedAt} snapshot={content.data} />
        : content.data ? <p className="notice">此条目暂无可浏览的内容快照，可打开官方页面查看。{skill.gitSource ? "仍可通过 Git 仓库预览后安装。" : ""}</p> : null}
    </div>
  </main>;
}

function SnapshotContent({ snapshot }: { snapshot: SkillsShSnapshot }) {
  const [tab, setTab] = useState<"overview" | "files">("overview");
  const [path, setPath] = useState("SKILL.md");
  const [expanded, setExpanded] = useState(new Set<string>());
  const [raw, setRaw] = useState(false);
  const tabs = useRef<HTMLDivElement>(null);
  const preview = useRef<HTMLDivElement>(null);
  const files = snapshot.files ?? [];
  const shownPath = tab === "overview" ? "SKILL.md" : path;
  const file = files.find((file) => file.path === shownPath);
  function navigate(next: string) {
    setPath(next); setTab("files"); setRaw(false);
    setExpanded((previous) => { const result = new Set(previous); const parts = next.split("/");
      for (let i = 1; i < parts.length; i++) result.add(parts.slice(0, i).join("/")); return result; });
    preview.current?.focus({ preventScroll: true });
    preview.current?.scrollIntoView?.({ block: "nearest" });
  }
  const markdown = /\.(md|markdown)$/i.test(shownPath);
  return <>
    <div ref={tabs} className="marketplace-detail-tabs" role="tablist" aria-label="市场详情内容" onKeyDown={(event) => {
      if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
      event.preventDefault();
      const next = event.key === "Home" ? "overview" : event.key === "End" ? "files" : tab === "overview" ? "files" : "overview";
      setTab(next); setRaw(false);
      tabs.current?.querySelector<HTMLButtonElement>(`#skills-sh-tab-${next}`)?.focus();
    }}>
      {(["overview", "files"] as const).map((value) => <button key={value} role="tab" id={`skills-sh-tab-${value}`}
        aria-controls="skills-sh-content" aria-selected={tab === value} tabIndex={tab === value ? 0 : -1}
        onClick={() => { setTab(value); setRaw(false); }}>{value === "overview" ? "概述" : "文件"}</button>)}
    </div>
    <div id="skills-sh-content" role="tabpanel" aria-labelledby={`skills-sh-tab-${tab}`} tabIndex={0}>
      <div className={tab === "files" ? "marketplace-files-body" : "marketplace-detail-overview"}>
        {tab === "files" ? <nav className="marketplace-file-tree" aria-label="市场文件树">
          <MarketplaceFileTree files={files} selectedPath={path} expanded={expanded} onSelectFile={navigate}
            onToggleDir={(path) => setExpanded((previous) => { const next = new Set(previous); if (next.has(path)) next.delete(path); else next.add(path); return next; })} />
        </nav> : null}
        <div ref={preview} className="marketplace-file-preview" role="region" tabIndex={-1} aria-label="文件预览">
          <div className="marketplace-file-head"><span className="mono">{shownPath}</span><div>
            {file ? <span>{formatBytes(file.sizeBytes)}</span> : null}
            {file?.contents != null && markdown ? <Button variant="ghost" size="sm" aria-pressed={raw} onClick={() => setRaw(!raw)}>{raw ? "返回渲染" : "查看原文"}</Button> : null}
          </div></div>
          <div className="marketplace-file-content">
            {!file ? <p role="status">此快照未包含该文件，请选择其他文件或打开官方页面。</p>
              : file.contents === null ? <p>此文件不是可预览文本，或超过 1 MB 预览上限。</p>
              : markdown && !raw ? <SkillMarkdown content={file.contents} fileLinks={{ baseDir: parentDir(shownPath), onNavigate: navigate }} />
              : <pre className="content-text" role="region" aria-label={`${shownPath} 原文`} tabIndex={0}>{file.contents}</pre>}
          </div>
        </div>
      </div>
    </div>
  </>;
}
