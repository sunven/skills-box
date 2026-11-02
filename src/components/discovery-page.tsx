import { useDeferredValue, useMemo, useState } from "react";
import { Download, RefreshCw, Search, X } from "lucide-react";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { asAppError, fallbackError } from "../lib/api";
import { discoveryKey, filterDiscoveries, installationFor, installationLabels, isDiscoveryInstallable,
  type DiscoveredSkill, type DiscoveryFilter, type DiscoveryInstallSeed } from "../lib/discovery";
import type { ProjectEntry, ScopeId } from "../lib/types";
import type { useDiscovery } from "../hooks/use-discovery";
import { GLOBAL_SCOPE } from "../lib/types";
import { projectScope } from "../lib/scope";
import { formatEpochDateTime } from "../lib/update-view";

interface Props {
  discovery: ReturnType<typeof useDiscovery>;
  scope: ScopeId; projects: ProjectEntry[]; mutationsEnabled: boolean;
  onSelectScope: (scope: ScopeId) => void;
  onInstall: (seed: DiscoveryInstallSeed) => void;
  initialFilter: DiscoveryFilter;
  initialSource: string;
}
function SourceAddress({ source }: { source: string }) {
  return <span className="mono discovery-source-address" title={source.startsWith("https://") ? source : `https://github.com/${source}`}>{source}</span>;
}
export function DiscoveryPage({ discovery, scope, projects, mutationsEnabled, onSelectScope, onInstall, initialFilter, initialSource }: Props) {
  const { query, pending, error } = discovery;
  const data = query.data;
  const [filter, setFilter] = useState<DiscoveryFilter>(initialFilter);
  const [search, setSearch] = useState("");
  const [source, setSource] = useState(initialSource);
  const [selected, setSelected] = useState<string | null>(null);
  const deferredSearch = useDeferredValue(search);
  const visible = useMemo(() => filterDiscoveries(data?.skills ?? [], scope, filter, deferredSearch, source),
    [data?.skills, scope, filter, deferredSearch, source]);
  const detail = data?.skills.find((s) => discoveryKey(s.id) === selected);
  const displayedError = error ?? data?.error ?? (query.isError ? asAppError(query.error) ?? fallbackError("无法读取发现记录。") : null);
  function install(skill: DiscoveredSkill) { onInstall({ source: skill.id.source, name: skill.name }); }
  function canInstall(skill: DiscoveredSkill) {
    return mutationsEnabled && !pending && isDiscoveryInstallable(skill, scope);
  }
  function showAllNew() { setFilter("new"); setSearch(""); setSource("all"); }
  return <main className="discovery-page" aria-labelledby="discovery-title">
    <div className="discovery-content">
      <div className="discovery-heading">
        <div><h1 id="discovery-title">发现</h1><p className="text-muted-foreground">从已安装来源发现可用 skills。应用运行时每 6 小时检查，上游新增会在这里提醒。</p></div>
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" size="sm" disabled={pending || data?.progress.running} onClick={() => void discovery.scan()}>
            <RefreshCw className={data?.progress.running ? "animate-spin" : ""} aria-hidden="true" />立即检查上游
          </Button>
          <Button variant="ghost" size="sm" disabled={pending || !data?.unreadCount} onClick={() => void discovery.acknowledge()}>全部已读</Button>
        </div>
      </div>
      <div className="discovery-summary" role="status">
        <span>{data?.sources.length ?? 0} 个活跃来源 · {data?.unreadCount ?? 0} 个未读新增</span>
        <Button variant="ghost" size="sm" onClick={showAllNew}>查看全部新增</Button>
        {data?.progress.running ? <span>正在扫描：{data.progress.completed} / {data.progress.total}</span> : null}
      </div>
      {displayedError ? <div role="alert" className="notice" data-tone="error"><p>{displayedError.message}</p>{displayedError.hint ? <p>{displayedError.hint}</p> : null}</div> : null}
      {data?.warnings.map((warning) => <p key={warning} role="status" className="notice" data-tone="warning">{warning}</p>)}
      <div className="discovery-controls">
        <label className="discovery-scope">安装到
          <select aria-label="发现目标作用域" value={scope} onChange={(e) => onSelectScope(e.target.value as ScopeId)}>
            <option value={GLOBAL_SCOPE}>全局</option>
            {projects.map((p) => <option key={p.id} value={projectScope(p.id)}>{p.displayName}{p.available ? "" : "（不可用）"}</option>)}
          </select>
        </label>
        <div className="discovery-tabs" role="group" aria-label="发现筛选">
          {([['uninstalled', '当前作用域未安装'], ['available', '可安装'], ['all', '全部'], ['new', '新增'], ['ignored', '已忽略']] as const).map(([value, label]) =>
            <Button key={value} variant={filter === value ? "outline" : "ghost"} size="sm" aria-pressed={filter === value} onClick={() => setFilter(value)}>{label}</Button>)}
        </div>
        <label className="discovery-search"><Search className="size-4" aria-hidden="true" /><Input aria-label="搜索发现的 skills" placeholder="搜索名称或描述" value={search} onChange={(e) => setSearch(e.target.value)} /></label>
        <select aria-label="发现来源筛选" value={source} onChange={(e) => setSource(e.target.value)}><option value="all">所有来源</option>{data?.sources.map((s) => <option key={s.source} value={s.source}>{s.source}</option>)}</select>
      </div>
      {data?.sources.length ? <details className="discovery-sources">
        <summary>来源扫描记录{data.sources.some((s) => s.error || s.pausedUntil) ? " · 有来源需重试" : ""}</summary>
        {data.sources.map((s) => <div key={s.source} className="discovery-source-record">
          <SourceAddress source={s.source} />
          <span>{s.scanning ? "扫描中" : s.pausedUntil ? `限流暂停至 ${formatEpochDateTime(s.pausedUntil)}` : s.error ? "扫描失败，已保留上次目录" : s.lastSuccessAt ? "已检查" : "等待首次扫描"}{s.needsRevalidation && s.lastSuccessAt ? " · 目录待重验" : ""}</span>
          <span className="text-muted-foreground">上次尝试：{s.lastAttemptAt ? formatEpochDateTime(s.lastAttemptAt) : "尚未尝试"} · 上次成功：{s.lastSuccessAt ? formatEpochDateTime(s.lastSuccessAt) : "尚无成功记录"}</span>
          {s.error ? <p className="text-destructive">{s.error}</p> : null}
        </div>)}
      </details> : null}
      <div className="discovery-body">
        <section className="discovery-list" aria-label="发现的 skills">
          <p className="text-muted-foreground text-xs">显示 {visible.length} 项</p>
          {query.isPending ? <p role="status">正在读取发现记录…</p> : !data?.sources.length ? <div className="discovery-empty"><p>暂无活跃来源</p><p className="text-muted-foreground">安装一个来自公开 Git 仓库的 skill 后，会自动检查同一来源的其他 skills。</p></div> : visible.length === 0 ? <p className="discovery-empty">没有符合条件的 skill。</p> : null}
          {visible.map((skill) => <article key={discoveryKey(skill.id)} className="discovery-row" data-selected={selected === discoveryKey(skill.id) ? "true" : undefined}>
            <div className="min-w-0"><button className="discovery-name" onClick={() => { setSelected(discoveryKey(skill.id)); void discovery.acknowledge([skill.id]); }}>{skill.name}</button>{skill.unread ? <span className="discovery-new">新增</span> : null}
              <p className="discovery-description">{skill.description || "暂无描述"}</p>
              <div className="discovery-meta"><SourceAddress source={skill.id.source} />{skill.group ? <span>{skill.group}</span> : null}<span>{installationLabels[installationFor(skill, scope)]}</span>
                {skill.installations.filter((s) => s.scope !== scope && s.status === "installed").map((s) => <span key={s.scope}>{s.label} 已安装</span>)}
              </div>{skill.installIssue ? <p className="text-xs text-destructive">{skill.installIssue}</p> : null}
            </div>
            <div className="discovery-row-actions"><Button size="sm" variant="outline" disabled={!canInstall(skill)} onClick={() => install(skill)} aria-label={`安装 ${skill.name}`}><Download aria-hidden="true" />安装</Button>
              <Button size="sm" variant="ghost" disabled={pending} onClick={() => void discovery.ignore(skill.id, !skill.ignored)} aria-label={`${skill.ignored ? "恢复" : "忽略"} ${skill.name}`}>{skill.ignored ? "恢复" : "忽略"}</Button></div>
          </article>)}
        </section>
        {detail ? <aside className="discovery-detail" aria-label="发现项详情">
          <div className="flex items-center justify-between gap-3"><h2>{detail.name}</h2><Button variant="ghost" size="icon" aria-label="关闭发现详情" onClick={() => setSelected(null)}><X aria-hidden="true" /></Button></div>
          <p className="discovery-detail-description">{detail.description || "暂无描述"}</p>
          <dl><dt>来源</dt><dd><SourceAddress source={detail.id.source} /></dd>{detail.group ? <><dt>分组</dt><dd>{detail.group}</dd></> : null}<dt>首次观察</dt><dd>{formatEpochDateTime(detail.firstSeenAt)}</dd></dl>
          <h3>各作用域安装状态</h3><ul>{detail.installations.map((s) => <li key={s.scope}><span>{s.label}</span><span>{installationLabels[s.status]}</span></li>)}</ul>
          {detail.installIssue ? <p className="text-destructive">{detail.installIssue}</p> : null}
          <Button className="mt-4" disabled={!canInstall(detail)} onClick={() => install(detail)}>安装到当前作用域</Button>
        </aside> : null}
      </div>
    </div>
  </main>;
}
