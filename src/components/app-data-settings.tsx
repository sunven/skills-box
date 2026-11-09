import { useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { FolderOpen, RefreshCw, ShieldCheck, Trash2 } from "lucide-react";
import { asAppError, cleanAppData, fallbackError, getAppData, previewAppDataCleanup, revealAppData, revealBackup, saveDataPreferences } from "../lib/api";
import type { AppDataCategory, AppDataCleanupResult, AppDataPreview, AppDataSelection, DataPreferences, AppError } from "../lib/types";
import { formatEpochDateTime } from "../lib/update-view";
import { useProjectsQuery } from "../hooks/use-projects";
import { useModalFocus } from "../hooks/use-modal-focus";
import { Button } from "./ui/button";
import { SelectionCheckbox } from "./selection-checkbox";

export const appDataQueryKey = ["app-data"] as const;
const emptySelection = (): AppDataSelection => ({ backupIds: [], allBackups: false, logs: false, cache: false });
const categoryLabels: Record<AppDataCategory, string> = { backups: "操作备份", logs: "诊断日志", cache: "缓存", preserved: "需保留的数据", other: "其他文件" };
const actionLabels: Record<string,string> = { install: "安装", update: "更新", uninstall: "卸载", detach: "移除挂载", clean_records: "清理记录", restore: "恢复技能" };
type DataActionError = { value: AppError; cleanupSelection?: AppDataSelection };
export function formatDataBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), 4);
  return `${(bytes / 1024 ** index).toLocaleString("zh-CN", { maximumFractionDigits: 1 })} ${["B", "KiB", "MiB", "GiB", "TiB"][index]}`;
}

export function AppDataSettings({ active, critical }: { active: boolean; critical: boolean }) {
  const queryClient = useQueryClient();
  const query = useQuery({ queryKey: appDataQueryKey, queryFn: getAppData, enabled: active, staleTime: 0 });
  const projects = useProjectsQuery();
  const [selected, setSelected] = useState<string[]>([]);
  const [categories, setCategories] = useState({ cache: true, logs: false, backups: false });
  const [pending, setPending] = useState<"save" | "preview" | "clean" | null>(null);
  const [error, setError] = useState<DataActionError | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [preview, setPreview] = useState<AppDataPreview | null>(null);
  const [lastSelection, setLastSelection] = useState<AppDataSelection | null>(null);
  const [result, setResult] = useState<AppDataCleanupResult | null>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const busy = pending !== null || critical;
  useModalFocus({ open: active && preview !== null, panelRef, initialFocusRef: cancelRef, onClose: () => setPreview(null), canClose: () => pending !== "clean" });
  const data = query.data;
  const selectableBackupIds = data?.backups.filter((backup) => !backup.protectedReason).map((backup) => backup.id) ?? [];
  const selectedIds = new Set(selected);
  const selectedBackupIds = selectableBackupIds.filter((id) => selectedIds.has(id));
  const allBackupsSelected = selectableBackupIds.length > 0 && selectedBackupIds.length === selectableBackupIds.length;
  const displayedError = error?.value ?? (query.isError ? asAppError(query.error) ?? fallbackError("无法读取应用数据。") : null);
  const retrySelection = error?.cleanupSelection;
  const usage = (category: AppDataCategory) => data?.categories.find((item) => item.category === category);
  const failures = result?.items.filter((item) => item.error) ?? [];

  async function save(key: keyof DataPreferences, value: boolean) {
    if (!data || busy) return;
    setPending("save"); setError(null); setNotice(null);
    try {
      const preferences = await saveDataPreferences({ ...data.preferences, [key]: value });
      queryClient.setQueryData(appDataQueryKey, { ...data, preferences });
      setNotice("设置已保存，后续操作生效。");
    } catch (e) { setError({ value: asAppError(e) ?? fallbackError("无法保存应用数据设置。") }); }
    finally { setPending(null); }
  }
  async function prepare(selection: AppDataSelection) {
    if (busy) return;
    setLastSelection(selection); setPending("preview"); setError(null); setNotice(null); setResult(null);
    try { setPreview(await previewAppDataCleanup(selection)); }
    catch (e) { setError({ value: asAppError(e) ?? fallbackError("无法预览清理内容。"), cleanupSelection: selection }); }
    finally { setPending(null); }
  }
  async function clean() {
    if (!preview || busy) return;
    setPending("clean"); setError(null);
    try {
      const outcome = await cleanAppData(preview.id);
      setResult(outcome); setPreview(null);
      const deleted = new Set(outcome.items.filter((i) => !i.error).map((i) => i.id));
      setSelected((previous) => previous.filter((id) => !deleted.has(id)));
      setLastSelection({ ...emptySelection(), itemIds: outcome.items.filter((item) => item.error).map((item) => item.id) });
      await query.refetch();
    } catch (e) {
      const failure = asAppError(e) ?? fallbackError("无法清理应用数据。");
      setError({
        value: failure.code === "task.busy" ? { ...failure, message: "有技能任务正在执行，请完成后重试清理。" } : failure,
        cleanupSelection: lastSelection ?? undefined,
      });
      setPreview(null);
    } finally { setPending(null); }
  }
  async function open(action: () => Promise<void>) {
    setError(null);
    try { await action(); } catch (e) { setError({ value: asAppError(e) ?? fallbackError("无法打开目录。") }); }
  }
  const scopeLabel = (scope: string | null) => {
    if (!scope || scope === "global") return "全局";
    const project = projects.data?.projects.find((entry) => `project:${entry.id}` === scope);
    return project?.displayName ?? `${projects.isSuccess ? "已移除的项目" : "项目"} · ${scope.replace("project:", "").slice(0, 8)}`;
  };

  return <>
    <div className="data-section-heading">
      <div><h2 id="app-data-title">应用数据</h2><p className="text-muted-foreground mt-1">查看本机占用，管理操作备份和诊断日志。</p></div>
      <Button variant="outline" size="sm" disabled={busy || query.isFetching} onClick={() => void query.refetch()}><RefreshCw aria-hidden="true" />{query.isFetching ? "统计中…" : "刷新占用"}</Button>
    </div>
    {displayedError ? <div className="notice data-notice-actions" data-tone="negative" role="alert">
      <div className="min-w-0 data-path"><p>{displayedError.message}</p>{displayedError.hint ? <p className="mt-1">{displayedError.hint}</p> : null}</div>
      {retrySelection ? <Button className="shrink-0" variant="outline" disabled={busy} onClick={() => void prepare(retrySelection)}>重新预览清理</Button> : null}
    </div> : null}
    {notice ? <p className="notice" role="status">{notice}</p> : null}
    {result ? <div className="notice" role="status" data-tone={failures.length ? "negative" : "positive"}>
      <p>已清理 {result.items.length - failures.length} 项，释放 {formatDataBytes(result.freedBytes)}。{failures.length ? ` ${failures.length} 项失败。` : ""}</p>
      {failures.map((item) => <p key={item.id} className="data-path mt-2">{item.label}：{item.error}</p>)}
      {failures.length && lastSelection ? <Button variant="outline" className="mt-2" disabled={busy} onClick={() => void prepare(lastSelection)}>重新预览失败项</Button> : null}
    </div> : null}
    {!data ? <p role="status" className="text-muted-foreground">{query.isPending ? "正在统计应用数据…" : "占用统计暂不可用，请刷新重试。"}</p> : <>
      <div className="data-overview">
        <div><span>应用数据总占用</span><strong className="num">{formatDataBytes(data.totalBytes)}</strong></div>
        <div><span>可清理占用</span><strong className="num">{formatDataBytes(data.cleanableBytes)}</strong></div>
      </div>
      <div className="text-[12px] text-muted-foreground"><p className="data-path">{data.directory}</p><p className="mt-1">按文件大小统计；不包含已安装技能、Agent 挂载和外部工具缓存。</p><p className="mt-1">上次统计：{formatEpochDateTime(data.scannedAtEpochMs)}</p></div>
      {data.warnings.length ? <div className="notice" role="status"><p>部分数据未能完整统计，显示值可能偏小。</p>{data.warnings.map((warning, index) => <p className="data-path mt-1" key={index}>{warning}</p>)}</div> : null}
      <section className="data-section" aria-labelledby="data-backups-title">
        <div className="data-section-heading"><h3 id="data-backups-title">操作备份 <span className="num text-muted-foreground">{formatDataBytes(usage("backups")?.bytes ?? 0)}</span></h3><Button variant="ghost" size="sm" onClick={() => void open(() => revealAppData("backups"))}><FolderOpen aria-hidden="true" />打开备份目录</Button></div>
        <label className="data-toggle"><input type="checkbox" role="switch" checked={data.preferences.retainBackups} disabled={busy} onChange={(e) => void save("retainBackups", e.target.checked)} /><span>保留操作备份</span></label>
        <p className="text-[12px] text-muted-foreground">关闭后仍先备份，成功后删除本次备份；失败或中断时保留。已有备份需手动清理。</p>
        <div className="flex flex-wrap items-center gap-3">
          <label className="data-toggle">
            <SelectionCheckbox
              label="全选可删除备份"
              checked={allBackupsSelected}
              mixed={selectedBackupIds.length > 0 && !allBackupsSelected}
              disabled={busy || !selectableBackupIds.length}
              onChange={() => setSelected(allBackupsSelected ? [] : selectableBackupIds)}
            />
            <span>全选可删除备份</span>
          </label>
          <Button variant="outline" size="sm" disabled={busy || !selectedBackupIds.length} onClick={() => void prepare({ ...emptySelection(), backupIds: selectedBackupIds })}>删除所选备份{selectedBackupIds.length ? `（${selectedBackupIds.length}）` : ""}</Button>
          <Button variant="ghost" size="sm" disabled={busy || !selectableBackupIds.length} onClick={() => void prepare({ ...emptySelection(), allBackups: true })}>清空可删除备份</Button>
        </div>
        {data.backups.length ? <ul className="data-backup-list" aria-label="备份列表">{data.backups.map((backup) => <li key={backup.id}>
          <input type="checkbox" aria-label={`选择备份 ${backup.name}`} checked={!backup.protectedReason && selectedIds.has(backup.id)} disabled={busy || Boolean(backup.protectedReason)} onChange={(e) => setSelected((previous) => e.target.checked ? [...previous, backup.id] : previous.filter((id) => id !== backup.id))} />
          <div className="min-w-0 flex-1"><p className="font-medium data-path">{backup.action === "install" ? "安装任务 · " : ""}{backup.name}</p><p className="text-[12px] text-muted-foreground">{actionLabels[backup.action ?? ""] ?? backup.action ?? "未知操作"} · {scopeLabel(backup.scope)} · {backup.createdAtEpochMs ? formatEpochDateTime(backup.createdAtEpochMs) : "时间未知"}</p>{backup.protectedReason ? <p className="data-protected"><ShieldCheck className="size-3" aria-hidden="true" />受保护：{backup.protectedReason}</p> : null}</div>
          <span className="num text-[12px]">{formatDataBytes(backup.bytes)}</span><Button variant="ghost" size="icon" aria-label={`打开备份 ${backup.name}`} onClick={() => void open(() => revealBackup(backup.id))}><FolderOpen aria-hidden="true" /></Button>
        </li>)}</ul> : <p className="text-muted-foreground">暂无操作备份。</p>}
      </section>
      <section className="data-section" aria-labelledby="data-logs-title">
        <div className="data-section-heading"><h3 id="data-logs-title">诊断日志 <span className="num text-muted-foreground">{formatDataBytes(usage("logs")?.bytes ?? 0)}</span></h3><Button variant="ghost" size="sm" onClick={() => void open(() => revealAppData("logs"))}><FolderOpen aria-hidden="true" />打开日志目录</Button></div>
        <label className="data-toggle"><input type="checkbox" role="switch" checked={data.preferences.operationLogging} disabled={busy} onChange={(e) => void save("operationLogging",e.target.checked)} /><span>记录诊断日志</span></label>
        <p className="text-[12px] text-muted-foreground">记录部分技能操作的结果与错误。关闭后续操作的日志记录，已有日志保留。最多保留 5 份，每份 10 MiB，超出后替换最旧日志。</p>
        <Button className="justify-self-start" variant="outline" size="sm" disabled={busy || !(usage("logs")?.cleanableBytes)} onClick={() => void prepare({ ...emptySelection(), logs: true })}>清空日志</Button>
      </section>
      <section className="data-section" aria-labelledby="data-cache-title">
        <div className="data-section-heading"><h3 id="data-cache-title">缓存 <span className="num text-muted-foreground">{formatDataBytes(usage("cache")?.bytes ?? 0)}</span></h3><Button variant="outline" size="sm" disabled={busy || !(usage("cache")?.cleanableBytes)} onClick={() => void prepare({ ...emptySelection(), cache: true })}>清理缓存</Button></div>
        <p className="text-[12px] text-muted-foreground">更新检查的来源目录与源码归档。清理后按需重新下载，首次检查可能稍慢。</p>
      </section>
      <section className="data-section" aria-labelledby="data-preserved-title">
        <div className="data-section-heading"><h3 id="data-preserved-title">需保留的数据</h3><span className="num">{formatDataBytes(usage("preserved")?.bytes ?? 0)}</span></div>
        <p className="text-[12px] text-muted-foreground">设置、项目登记、发现记录、检查结果和本地修改基线，用于保留你的配置与使用状态。</p>
        {(usage("other")?.count ?? 0) > 0 ? <p className="text-[12px] text-muted-foreground">其他文件：{formatDataBytes(usage("other")?.bytes ?? 0)}，仅统计，不参与清理。</p> : null}
      </section>
      <section className="data-section" aria-labelledby="data-cleanup-title">
        <h3 id="data-cleanup-title">集中清理</h3>
        <div className="flex flex-wrap gap-5">{(["cache", "logs", "backups"] as const).map((category) => <label key={category} className="data-toggle"><input type="checkbox" checked={categories[category]} disabled={busy} onChange={(e) => setCategories((previous) => ({ ...previous, [category]: e.target.checked }))} /><span>{categoryLabels[category]}</span></label>)}</div>
        <p className="text-[12px] text-muted-foreground">预览后确认删除。受保护的备份会跳过，清理在任务空闲时执行。</p>
        <Button className="justify-self-start" disabled={busy || !Object.values(categories).some(Boolean)} onClick={() => void prepare({ ...emptySelection(), cache: categories.cache, logs: categories.logs, allBackups: categories.backups })}><Trash2 aria-hidden="true" />{pending === "preview" ? "正在计算…" : "预览清理"}</Button>
      </section>
    </>}
    {active && preview ? <div className="modal-overlay"><div ref={panelRef} className="modal-panel max-w-lg" role="dialog" aria-modal="true" aria-labelledby="data-cleanup-confirm-title">
      <div className="modal-head"><div><h2 id="data-cleanup-confirm-title" className="modal-title">确认清理应用数据</h2><p className="modal-sub">{preview.items.length} 项 · 预计释放 {formatDataBytes(preview.totalBytes)}</p></div></div>
      <div className="modal-body"><p>{preview.items.some((item) => item.category === "backups") ? "删除的备份将无法用于恢复技能。" : ""}{preview.items.some((item) => item.category === "logs") ? "删除的日志将无法用于排查历史问题。" : ""}{preview.items.some((item) => item.category === "cache") ? "缓存会在需要时重新下载。" : ""}</p><ul className="data-cleanup-list mt-4">{preview.items.map((item) => <li key={item.id}><span className="data-path">{item.label}</span><span className="num">{formatDataBytes(item.bytes)}</span></li>)}</ul></div>
      <div className="modal-foot"><Button ref={cancelRef} variant="outline" disabled={pending === "clean"} onClick={() => setPreview(null)}>取消</Button><Button variant="destructive" disabled={busy} onClick={() => void clean()}>{pending === "clean" ? "正在清理…" : "确认清理"}</Button></div>
    </div></div> : null}
  </>;
}
