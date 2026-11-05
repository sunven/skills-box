import { useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Loader2, X } from "lucide-react";
import { Button } from "./ui/button";
import { useModalFocus } from "../hooks/use-modal-focus";
import { asAppError, listStaleRecords } from "../lib/api";
import { staleRecordsQueryKey } from "../lib/batch";
import type { ScopeId } from "../lib/types";
import type { BatchSelection } from "./batch-dialog";

export function StaleRecordsDialog({ scope, scopeLabel, disabled, onClose, onSelect }: {
  scope: ScopeId; scopeLabel?: string; disabled: boolean; onClose: () => void; onSelect: (selection: BatchSelection) => void;
}) {
  const query = useQuery({ queryKey: staleRecordsQueryKey(scope), queryFn: () => listStaleRecords(scope), staleTime: 0 });
  const [selected, setSelected] = useState<Set<string>>(() => new Set());
  const records = query.data ?? [];
  const chosen = records.filter((r) => selected.has(r.id));
  const panelRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  useModalFocus({ open: true, panelRef, initialFocusRef: closeRef, onClose });
  const select = (action: "clean_records" | "restore") => onSelect({ action, skills: chosen });
  return <div className="modal-overlay" onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}>
    <div ref={panelRef} role="dialog" aria-modal="true" aria-labelledby="stale-records-title" className="modal-panel max-w-2xl" data-testid="stale-records-dialog">
      <div className="modal-head"><div>
        <h2 id="stale-records-title" className="modal-title">处理失效安装记录</h2>
        <p className="modal-sub">{scopeLabel ?? (scope === "global" ? "全局" : "项目")} · {records.length} 条记录 · 已选 {chosen.length} 条</p>
      </div><Button ref={closeRef} variant="ghost" size="icon" className="size-7" aria-label="关闭失效记录" onClick={onClose}><X className="size-4" aria-hidden="true" /></Button></div>
      <div className="modal-body">
        <p className="notice mb-3">清理记录会移除残留安装记录；恢复技能会按原来源重新安装。选择记录后预览操作。</p>
        {query.isPending ? <p role="status" className="flex gap-2 text-sm"><Loader2 className="size-4 animate-spin" aria-hidden="true" />正在读取完整清单…</p>
          : query.isError ? <div role="alert"><p>{asAppError(query.error)?.message ?? "无法读取失效记录。"}</p><Button variant="outline" size="sm" onClick={() => void query.refetch()}>重新读取</Button></div>
          : records.length ? <>
            <label className="batch-confirm mb-3"><input type="checkbox" aria-label="全选失效记录" checked={chosen.length === records.length} disabled={disabled}
              onChange={(e) => setSelected(e.target.checked ? new Set(records.map((r) => r.id)) : new Set())} />全选 {records.length} 条记录</label>
            <ul className="batch-preview-list">{records.map((r) => <li key={r.id} data-testid="stale-record">
              <label className="flex items-start gap-3"><input type="checkbox" className="mt-1" aria-label={`选择记录 ${r.name}`} checked={selected.has(r.id)} disabled={disabled}
                onChange={() => setSelected((current) => { const next = new Set(current); if (next.has(r.id)) next.delete(r.id); else next.add(r.id); return next; })} />
                <span className="min-w-0"><strong className="break-all">{r.name}</strong><span className="block text-muted-foreground break-all">{r.source || "来源未记录"} · {r.reference ?? "默认分支"}</span>
                  {r.cleanReason ? <span className="block text-warning">无法清理：{r.cleanReason}</span> : null}
                  {r.restoreReason ? <span className="block text-warning">无法恢复：{r.restoreReason}</span> : null}</span>
              </label></li>)}</ul>
          </> : <p role="status">当前作用域没有失效安装记录。</p>}
      </div>
      <div className="modal-foot batch-foot"><span className="modal-foot-note">已选 {chosen.length} 条 · 先预览，再确认</span><div className="flex flex-wrap gap-2">
        <Button variant="outline" size="sm" disabled={disabled || !chosen.some((r) => !r.cleanReason)} onClick={() => select("clean_records")}>清理记录</Button>
        <Button size="sm" disabled={disabled || !chosen.some((r) => !r.restoreReason)} onClick={() => select("restore")}>恢复技能</Button>
      </div></div>
    </div>
  </div>;
}
