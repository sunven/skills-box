import { useRef, useState, type ReactNode } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Plus,
  RefreshCw,
  Search,
  X,
  Package,
  ChevronRight,
} from "lucide-react";
import {
  executeNativePlugin,
  nativePluginDetails,
  prepareNativePlugin,
  readNativePlugins,
  verifyNativePluginHost,
} from "../lib/api";
import {
  pluginLayerLabels,
  pluginContextLabel,
  type PluginContext,
  type PluginLayer,
  pluginActionLabels,
  pluginHostLabels,
  pluginOutcomeLabels,
  pluginQueryKey,
  pluginRequest,
  type PluginHost,
  type PluginAction,
  type NativePlugin,
  type PluginPreview,
  type PluginRequest,
  type PluginOutcome,
  type PluginPageState,
} from "../lib/native-plugins";
import type { ScopeId, ProjectEntry } from "../lib/types";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { SkillMarkdown } from "./skill-markdown";
import { useModalFocus } from "../hooks/use-modal-focus";
const message = (error: unknown) =>
  error instanceof Error
    ? error.message
    : typeof error === "string"
      ? error
      : "操作未完成，请重新读取并核对原生状态。";
const enabledLabel = (v: boolean | null) =>
  v === null ? "未设置" : v ? "启用" : "禁用";

function Dialog({
  title,
  children,
  onClose,
  busy = false,
}: {
  title: string;
  children: ReactNode;
  onClose: () => void;
  busy?: boolean;
}) {
  const panelRef = useRef<HTMLDivElement>(null);
  useModalFocus({ open: true, panelRef, onClose, canClose: () => !busy });
  return (
    <div
      className="modal-overlay"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && !busy) onClose();
      }}
    >
      <div
        className="modal-panel native-plugin-dialog max-w-2xl"
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="plugin-dialog-title"
      >
        <div className="modal-head">
          <h2 className="modal-title" id="plugin-dialog-title">
            {title}
          </h2>
          <Button
            variant="ghost"
            size="icon"
            aria-label="关闭插件对话框"
            disabled={busy}
            onClick={onClose}
          >
            <X className="size-4" />
          </Button>
        </div>
        {children}
      </div>
    </div>
  );
}
export function NativePluginsPage({
  disabled = false,
  scope = "global",
  project,
}: {
  disabled?: boolean;
  scope?: ScopeId;
  project?: ProjectEntry;
}) {
  const [host, setHost] = useState<PluginHost>("codex");
  const [selection, setSelection] = useState<{
    scope: ScopeId;
    layer: PluginLayer;
  }>({ scope, layer: "project" });
  const layer: PluginLayer =
    scope === "global"
      ? "user"
      : host === "codex"
        ? "project"
        : selection.scope === scope
          ? selection.layer
          : "project";
  const context: PluginContext = { scope, layer };
  const rootRevision = `${project?.root ?? ""}:${project?.checkRevision ?? ""}`;
  return (
    <NativeHostPage
      key={`${host}:${scope}:${layer}:${rootRevision}`}
      host={host}
      onHost={setHost}
      context={context}
      rootRevision={rootRevision}
      project={project}
      onLayer={(layer) => setSelection({ scope, layer })}
      disabled={disabled}
    />
  );
}
function NativeHostPage({
  host,
  onHost,
  disabled,
  context,
  rootRevision,
  project,
  onLayer,
}: {
  host: PluginHost;
  onHost: (host: PluginHost) => void;
  disabled: boolean;
  context: PluginContext;
  rootRevision: string;
  project?: ProjectEntry;
  onLayer: (layer: PluginLayer) => void;
}) {
  const client = useQueryClient();
  const [search, setSearch] = useState("");
  const [filter, setFilter] = useState("all");
  const [market, setMarket] = useState("all");
  const [selected, setSelected] = useState<NativePlugin | null>(null);
  const [adding, setAdding] = useState(false);
  const [preview, setPreview] = useState<PluginPreview | null>(null);
  const [result, setResult] = useState<PluginOutcome | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [working, setWorking] = useState<string | null>(null);
  const key = pluginQueryKey(host, context, rootRevision);
  const projectMode = context.scope !== "global";
  const query = useQuery({
    queryKey: key,
    queryFn: () => readNativePlugins(host, context),
    retry: false,
    refetchOnWindowFocus: false,
    refetchInterval: (q) =>
      ["writing", "verifying"].includes(q.state.data?.operation?.phase ?? "")
        ? 1500
        : false,
  });
  const data = query.data?.snapshot;
  const operation = query.data?.operation;
  const running =
    working !== null ||
    operation?.phase === "writing" ||
    operation?.phase === "verifying";
  const busy = disabled || running;
  const blocked = (action: PluginAction) =>
    data?.capabilities.find((c) => c.action === action)?.blockedReason ??
    (data?.capabilities.some((c) => c.action === action)
      ? null
      : "当前操作不可用");
  const visible = (data?.plugins ?? []).filter(
    (p) =>
      (market === "all" || p.marketplace === market) &&
      (filter === "all" ||
        (filter === "installed"
          ? p.installationState !== "absent"
          : p.installationState === "absent")) &&
      `${p.name} ${p.id} ${p.description ?? ""}`
        .toLowerCase()
        .includes(search.toLowerCase()),
  );
  const selectedMarket = data?.markets.find((m) => m.name === market);
  const shownResult = result ?? operation?.outcome;
  const incomplete = data?.markets.some((m) => m.catalogError);
  async function prepare(request: PluginRequest) {
    setError(null);
    setWorking("正在核对操作范围…");
    setResult(null);
    try {
      setPreview(await prepareNativePlugin({ ...request, context }));
      setAdding(false);
      setSelected(null);
    } catch (e) {
      setError(message(e));
    } finally {
      setWorking(null);
    }
  }
  async function execute() {
    if (!preview || busy) return;
    const confirmed = preview;
    setPreview(null);
    setWorking("正在执行原生命令；随后核验状态…");
    setError(null);
    try {
      const outcome = await executeNativePlugin(confirmed.token);
      setResult(outcome);
      client.setQueryData<PluginPageState>(key, {
        snapshot: outcome.snapshot,
        error: null,
        operation: {
          phase: "finished",
          preview: confirmed,
          outcome,
          message: null,
        },
      });
    } catch (e) {
      setError(message(e));
    } finally {
      setWorking(null);
      await Promise.all([
        client.invalidateQueries({ queryKey: ["native-plugins"] }),
        client.invalidateQueries({ queryKey: ["native-plugin-detail"] }),
      ]);
    }
  }
  async function verify() {
    setWorking("正在临时目录检查插件操作兼容性…");
    setError(null);
    try {
      await verifyNativePluginHost(host);
      await client.invalidateQueries({ queryKey: ["native-plugins"] });
    } catch (e) {
      setError(message(e));
    } finally {
      setWorking(null);
    }
  }
  const actions = (p: NativePlugin) => {
    const actions: PluginAction[] =
      p.installationState === "absent"
        ? ["install"]
        : [
            ...(!p.installedInTarget && projectMode
              ? ["install" as const]
              : []),
            (p.selectedEnabled ?? p.effectiveEnabled) ? "disable" : "enable",
            ...(p.installedInTarget
              ? [
                  host === "codex"
                    ? ("reinstall" as const)
                    : ("update" as const),
                  "uninstall" as const,
                ]
              : []),
          ];
    return actions.map((action) => {
      const reason =
        blocked(action) ??
        p.actionBlocks[action] ??
        p.readOnlyReason ??
        (["install", "reinstall", "update"].includes(action)
          ? p.installBlockedReason
          : null);
      return (
        <span key={action} title={reason ?? undefined}>
          <Button
            size="sm"
            variant={
              ["install", "enable", "disable"].includes(action)
                ? "outline"
                : "ghost"
            }
            disabled={busy || !!reason}
            onClick={() => void prepare(pluginRequest(host, action, p.id))}
          >
            {pluginActionLabels[action]}
          </Button>
        </span>
      );
    });
  };
  return (
    <main className="native-plugin-page" aria-labelledby="native-plugin-title">
      <div className="native-plugin-heading">
        <div>
          <h1 id="native-plugin-title">插件</h1>
          <p>管理 Codex 与 Claude Code 的原生插件与市场</p>
        </div>
        <div className="native-host-context">
          <div
            className="native-host-switch"
            role="group"
            aria-label="插件宿主"
          >
            {(["codex", "claude"] as const).map((h) => (
              <Button
                key={h}
                variant={host === h ? "outline" : "ghost"}
                size="sm"
                disabled={running}
                aria-pressed={host === h}
                onClick={() => onHost(h)}
              >
                {pluginHostLabels[h]}
              </Button>
            ))}
          </div>
          {projectMode && host === "claude" ? (
            <label className="native-layer-control">
              配置层
              <select
                aria-label="插件配置层"
                value={context.layer}
                disabled={running}
                onChange={(event) => onLayer(event.target.value as PluginLayer)}
              >
                <option value="project">项目共享</option>
                <option value="local">仅自己在此项目</option>
              </select>
            </label>
          ) : (
            <span className="native-user-scope">
              {projectMode ? "项目共享 · 只读" : "用户级"}
            </span>
          )}
        </div>
      </div>
      {projectMode && (
        <div className="native-project-context">
          <strong>{project?.displayName ?? "当前项目"}</strong>
          <span className="mono">{project?.root ?? "项目不可用"}</span>
          <span>{pluginLayerLabels[context.layer]}</span>
        </div>
      )}
      <div className="native-plugin-tools">
        <p className="mono truncate" title={data?.configRoot}>
          {data
            ? `${data.configRoot} · ${data.version}`
            : `${pluginHostLabels[host]} · ${pluginLayerLabels[context.layer]}`}
        </p>
        <div className="flex flex-wrap gap-2">
          <Button
            size="sm"
            variant="ghost"
            disabled={running || query.isFetching}
            onClick={() => {
              setError(null);
              void query.refetch();
            }}
          >
            <RefreshCw
              className={query.isFetching ? "animate-spin" : undefined}
            />
            重新读取
          </Button>
          <Button
            size="sm"
            variant="outline"
            title={blocked("add_market") ?? undefined}
            disabled={busy || !data || !!blocked("add_market")}
            onClick={() => setAdding(true)}
          >
            <Plus />
            添加市场
          </Button>
        </div>
      </div>
      {query.isPending && (
        <p className="notice" role="status">
          正在读取原生配置与市场目录…
        </p>
      )}
      {(error || query.isError || query.data?.error) && (
        <p className="notice" data-tone="negative" role="alert">
          {error ?? query.data?.error ?? message(query.error)}
        </p>
      )}
      {data?.warnings.map((w) => (
        <p className="notice" data-tone="warning" key={w}>
          {w}
        </p>
      ))}
      {running && (
        <p className="notice" role="status">
          {working ??
            (operation?.phase === "verifying"
              ? "写入已结束，正在核对原生状态…"
              : "原生命令正在执行…")}{" "}
          正常退出会等待操作完成。
        </p>
      )}
      {operation?.phase === "interrupted" && (
        <p className="notice" data-tone="warning" role="alert">
          {operation.message}
        </p>
      )}
      {shownResult && !running && (
        <div
          className="notice native-plugin-result"
          role="status"
          data-tone={
            shownResult.status === "succeeded" ? "positive" : "warning"
          }
        >
          <strong>{pluginOutcomeLabels[shownResult.status]}</strong>
          <span>{pluginContextLabel(shownResult.preview.context)}</span>
          <span>{shownResult.message}</span>
          <span className="text-muted-foreground">
            命令：
            {shownResult.commandSucceeded === null
              ? "未知／未执行"
              : shownResult.commandSucceeded
                ? "退出成功"
                : "退出失败"}{" "}
            · 核验：
            {shownResult.verification === "verified" ? "通过" : "需要核对"}
          </span>
        </div>
      )}
      {data?.capabilities.some((c) => c.blockedReason) && (
        <div className="notice native-compatibility">
          <div>
            <strong>部分操作暂不可用</strong>
            <p>
              {projectMode
                ? "当前检查只验证用户层基本操作，不能解除项目层限制。项目操作若因异常停用，请在宿主中核对。"
                : "可在临时配置中重新检查基本插件操作。此检查不覆盖远程来源、市场同步或移除市场的连带影响；这些操作若因异常停用，请在宿主中核对。"}
            </p>
            <ul>
              {data.capabilities
                .filter((c) => c.blockedReason)
                .map((c) => (
                  <li key={c.action}>
                    {pluginActionLabels[c.action]}：{c.blockedReason}
                  </li>
                ))}
            </ul>
          </div>
          <Button
            size="sm"
            variant="outline"
            disabled={busy}
            onClick={() => void verify()}
          >
            检查兼容性
          </Button>
        </div>
      )}
      {data && (
        <div className="native-plugin-layout">
          <aside className="native-market-list" aria-label="插件市场">
            <h2>
              已配置市场
              <span className="native-plugin-count">{data.markets.length}</span>
            </h2>
            <button
              className="native-market-item"
              data-selected={market === "all"}
              aria-pressed={market === "all"}
              onClick={() => setMarket("all")}
            >
              <span>全部市场与安装记录</span>
              <span className="native-plugin-count">{data.plugins.length}</span>
            </button>
            {data.markets.map((m) => (
              <button
                key={m.name}
                className="native-market-item"
                data-selected={market === m.name}
                aria-pressed={market === m.name}
                onClick={() => setMarket(m.name)}
              >
                <span className="min-w-0">
                  <strong>{m.name}</strong>
                  <small>
                    {m.catalogError
                      ? "目录读取失败"
                      : m.readOnlyReason
                        ? "只读来源"
                        : m.sourceKind === "git"
                          ? "Git 仓库"
                          : m.sourceKind === "local"
                            ? "本地来源"
                            : m.sourceKind === "url"
                              ? "JSON 地址"
                              : m.sourceKind}
                  </small>
                </span>
                <ChevronRight />
              </button>
            ))}
            {data.markets.length === 0 && (
              <p className="native-empty-note">
                还没有市场，添加本地来源或 Git 仓库开始浏览
              </p>
            )}
            <p className="native-empty-note">
              仅搜索已配置市场；市场缺失时仍保留安装记录
            </p>
          </aside>
          <section className="native-plugin-list" aria-label="原生插件列表">
            {selectedMarket && (
              <div className="native-market-detail">
                <div>
                  <h2>{selectedMarket.name}</h2>
                  <p className="mono break-all">{selectedMarket.source}</p>
                  <p>
                    市场声明：
                    {selectedMarket.declarations.join("；") ||
                      "共享缓存，当前未发现声明"}
                  </p>
                  {selectedMarket.reference && (
                    <p>分支／标签：{selectedMarket.reference}</p>
                  )}
                  {selectedMarket.sparsePaths.length > 0 && (
                    <p>部分目录：{selectedMarket.sparsePaths.join("、")}</p>
                  )}
                </div>
                <div className="flex flex-wrap gap-1">
                  {(["sync_market", "remove_market"] as const).map((action) => {
                    const reason =
                      blocked(action) ??
                      (action === "remove_market" || host === "codex"
                        ? selectedMarket.readOnlyReason
                        : null) ??
                      (action === "sync_market"
                        ? (selectedMarket.syncBlockedReason ??
                          (selectedMarket.catalogError
                            ? "市场目录读取不完整，无法确认同步影响。"
                            : null))
                        : selectedMarket.removeBlockedReason);
                    return (
                      <span key={action} title={reason ?? undefined}>
                        <Button
                          size="sm"
                          variant="outline"
                          disabled={busy || !!reason}
                          onClick={() =>
                            void prepare(
                              pluginRequest(host, action, selectedMarket.name),
                            )
                          }
                        >
                          {pluginActionLabels[action]}
                        </Button>
                      </span>
                    );
                  })}
                </div>
                {[
                  selectedMarket.readOnlyReason,
                  selectedMarket.removeBlockedReason,
                  selectedMarket.syncBlockedReason,
                ]
                  .filter(Boolean)
                  .map((s, i) => (
                    <p key={i} className="native-market-message">
                      {s}
                    </p>
                  ))}
              </div>
            )}
            {incomplete && (
              <div
                className="notice native-catalog-warning"
                data-tone="warning"
              >
                市场目录读取不完整，搜索结果可能缺失。
                {data.markets
                  .filter((m) => m.catalogError)
                  .map((m) => (
                    <p key={m.name}>
                      {m.name}：{m.catalogError}
                    </p>
                  ))}
              </div>
            )}
            <div className="native-plugin-filters">
              <label className="native-search">
                <Search />
                <Input
                  aria-label="搜索已配置市场中的插件"
                  placeholder="搜索插件"
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                />
              </label>
              <select
                aria-label="插件安装状态"
                value={filter}
                onChange={(e) => setFilter(e.target.value)}
              >
                <option value="all">全部状态</option>
                <option value="installed">已安装／待核对</option>
                <option value="absent">未安装</option>
              </select>
              <span className="native-plugin-count">
                {visible.length} 个插件
              </span>
            </div>
            {visible.length === 0 && (
              <div className="native-plugin-empty">
                <Package />
                <h3>
                  {search || filter !== "all"
                    ? "没有匹配的插件"
                    : "此处还没有插件"}
                </h3>
                <p>
                  {incomplete
                    ? "部分目录未能读取，请修复来源后重新读取。"
                    : search || filter !== "all"
                      ? "调整搜索词或安装状态筛选"
                      : "选择其他市场或添加一个来源"}
                </p>
              </div>
            )}
            {visible.map((p) => (
              <article key={p.id} className="native-plugin-row">
                <div className="min-w-0">
                  <div className="native-plugin-summary">
                    <button
                      className="native-plugin-name"
                      onClick={() => setSelected(p)}
                    >
                      {p.name}
                      <ChevronRight />
                    </button>
                    <p className="native-plugin-coordinate mono">
                      <span>{p.id}</span>
                      {p.version && (
                        <span className="native-plugin-version">
                          <span className="sr-only">版本 </span>
                          {p.version}
                        </span>
                      )}
                    </p>
                  </div>
                  <p className="native-plugin-description">
                    {p.description ?? "目录未提供介绍"}
                  </p>
                  <div className="native-plugin-state">
                    <span
                      className="native-installation-state"
                      data-state={p.installationState}
                    >
                      {p.installationState === "installed"
                        ? p.installedInTarget
                          ? "当前层已安装"
                          : "继承的安装"
                        : p.installationState === "unconfirmed"
                          ? "配置仍在 · 安装待核对"
                          : "当前范围未安装"}
                    </span>
                    {p.installationState !== "absent" && (
                      <span
                        className="native-enabled-state"
                        data-enabled={
                          p.selectedEnabled === null
                            ? "unset"
                            : String(p.selectedEnabled)
                        }
                      >
                        {projectMode ? "当前层设置" : "用户设置"}：
                        {p.selectedEnabled === null && projectMode
                          ? "继承"
                          : enabledLabel(p.selectedEnabled)}
                      </span>
                    )}
                    {p.enabledSource && projectMode && (
                      <span>
                        {p.enabledOverridden
                          ? "被覆盖，生效配置来源"
                          : "配置来源"}
                        ：{p.enabledSource}
                      </span>
                    )}
                    {projectMode && (
                      <span>
                        宿主报告：
                        {p.effectiveEnabled === null
                          ? "未知"
                          : enabledLabel(p.effectiveEnabled)}
                      </span>
                    )}
                  </div>
                  {p.readOnlyReason && (
                    <p className="native-empty-note">{p.readOnlyReason}</p>
                  )}
                  {p.installBlockedReason && (
                    <p className="native-empty-note">
                      {p.installBlockedReason}
                    </p>
                  )}
                </div>
                <div className="native-plugin-actions">{actions(p)}</div>
              </article>
            ))}
            <p className="native-plugin-footnote">
              配置设置可能与已有会话的实际加载状态不同，请在{" "}
              {pluginHostLabels[host]} 新会话中核对。认证、信任与安装批准需在{" "}
              {pluginHostLabels[host]} 中完成。
            </p>
          </section>
        </div>
      )}
      {adding && data && (
        <AddMarket
          host={host}
          formats={data.sourceKinds}
          busy={running}
          error={error}
          onClose={() => setAdding(false)}
          onPrepare={prepare}
        />
      )}
      {preview && (
        <Dialog
          title={`确认${pluginActionLabels[preview.request.action]}`}
          onClose={() => setPreview(null)}
          busy={busy}
        >
          <div className="modal-body native-confirm">
            <dl>
              <dt>宿主与范围</dt>
              <dd>
                {pluginHostLabels[preview.request.host]} ·{" "}
                {pluginContextLabel(preview.context)}
              </dd>
              {preview.context.projectRoot && (
                <>
                  <dt>项目根目录</dt>
                  <dd className="mono">{preview.context.projectRoot}</dd>
                </>
              )}
              <dt>目标</dt>
              <dd>{preview.request.target || preview.request.source}</dd>
              <dt>配置目录</dt>
              <dd className="mono">{preview.configRoot}</dd>
            </dl>
            <ul>
              {preview.consequences.map((c) => (
                <li key={c}>{c}</li>
              ))}
            </ul>
            {preview.affectedPlugins.length > 0 && (
              <div>
                <h3>受影响插件</h3>
                <ul>
                  {preview.affectedPlugins.map((p) => (
                    <li className="mono" key={p}>
                      {p}
                    </li>
                  ))}
                </ul>
              </div>
            )}
            <details open>
              <summary>将执行的命令或配置操作</summary>
              <pre>{preview.command}</pre>
            </details>
          </div>
          <div className="modal-foot justify-end">
            <Button
              variant="ghost"
              size="sm"
              disabled={busy}
              onClick={() => setPreview(null)}
            >
              取消
            </Button>
            <Button size="sm" disabled={busy} onClick={() => void execute()}>
              确认执行
            </Button>
          </div>
        </Dialog>
      )}
      {selected && (
        <PluginDetail
          host={host}
          plugin={selected}
          context={context}
          rootRevision={rootRevision}
          onClose={() => setSelected(null)}
        />
      )}
    </main>
  );
}
function AddMarket({
  host,
  formats,
  busy,
  error,
  onClose,
  onPrepare,
}: {
  host: PluginHost;
  formats: string[];
  busy: boolean;
  error: string | null;
  onClose: () => void;
  onPrepare: (r: PluginRequest) => Promise<void>;
}) {
  const [kind, setKind] = useState(formats[0] ?? "local");
  const [source, setSource] = useState("");
  const [reference, setReference] = useState("");
  const [sparse, setSparse] = useState("");
  return (
    <Dialog
      title={`添加 ${pluginHostLabels[host]} 市场`}
      onClose={onClose}
      busy={busy}
    >
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void onPrepare({
            ...pluginRequest(host, "add_market"),
            source: source.trim(),
            sourceKind: kind,
            reference: kind === "git" ? reference.trim() : "",
            sparsePaths:
              kind === "git"
                ? sparse
                    .split("\n")
                    .map((s) => s.trim())
                    .filter(Boolean)
                : [],
          });
        }}
      >
        <div className="modal-body native-source-form">
          <label>
            来源格式
            <select
              aria-label="来源格式"
              value={kind}
              onChange={(e) => setKind(e.target.value)}
            >
              {formats.map((f) => (
                <option key={f} value={f}>
                  {f === "git"
                    ? "Git 仓库"
                    : f === "url"
                      ? "市场 JSON URL"
                      : host === "claude"
                        ? "本地目录／JSON 文件"
                        : "本地目录"}
                </option>
              ))}
            </select>
          </label>
          <label>
            来源
            <Input
              aria-label="市场来源"
              autoFocus
              required
              value={source}
              onChange={(e) => setSource(e.target.value)}
              placeholder={
                kind === "local"
                  ? "/绝对路径/marketplace"
                  : kind === "url"
                    ? "https://example.com/marketplace.json"
                    : "owner/repo 或 Git URL"
              }
            />
          </label>
          {kind === "git" && (
            <>
              <label>
                分支／标签（可选）
                <Input
                  aria-label="分支或标签"
                  value={reference}
                  onChange={(e) => setReference(e.target.value)}
                />
              </label>
              <label>
                仅检出指定目录（可选，每行一个）
                <textarea
                  aria-label="仅检出指定目录"
                  rows={3}
                  value={sparse}
                  onChange={(e) => setSparse(e.target.value)}
                />
              </label>
            </>
          )}
          <p>使用本机已有凭据。下一步查看操作内容与影响，确认后执行。</p>
          {error && (
            <p role="alert" className="text-destructive">
              {error}
            </p>
          )}
        </div>
        <div className="modal-foot justify-end">
          <Button
            type="button"
            size="sm"
            variant="ghost"
            disabled={busy}
            onClick={onClose}
          >
            取消
          </Button>
          <Button type="submit" size="sm" disabled={busy || !source.trim()}>
            {busy ? "正在核对…" : "查看操作"}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
function PluginDetail({
  host,
  plugin,
  onClose,
  context,
  rootRevision,
}: {
  host: PluginHost;
  context: PluginContext;
  rootRevision: string;
  plugin: NativePlugin;
  onClose: () => void;
}) {
  const query = useQuery({
    queryKey: [
      "native-plugin-detail",
      host,
      context.scope,
      context.layer,
      rootRevision,
      plugin.id,
    ],
    queryFn: () => nativePluginDetails(host, plugin.id, context),
    retry: false,
    staleTime: 0,
  });
  return (
    <Dialog title={plugin.name} onClose={onClose}>
      <div className="modal-body native-plugin-detail">
        <p className="mono break-all">{plugin.id}</p>
        <p>{plugin.description ?? "目录未提供介绍"}</p>
        {!!plugin.source && (
          <p className="mono break-all">
            插件来源：
            {typeof plugin.source === "string"
              ? plugin.source
              : Object.values(plugin.source)
                  .filter((v) => typeof v === "string")
                  .join(" · ") || "目录未提供"}
          </p>
        )}
        <p>
          当前层设置：
          {plugin.selectedEnabled === null
            ? "继承／未设置"
            : enabledLabel(plugin.selectedEnabled)}{" "}
          ·{pluginHostLabels[host]} 报告的状态：
          {plugin.effectiveEnabled === null
            ? "未知"
            : enabledLabel(plugin.effectiveEnabled)}
        </p>
        {plugin.localRoot && (
          <p className="mono break-all">本地内容：{plugin.localRoot}</p>
        )}
        {plugin.installations.map((i, n) => (
          <p key={n}>
            {i.scope} · {i.version ?? "版本未知"}
            {i.projectPath ? ` · ${i.projectPath}` : ""}
          </p>
        ))}
        {query.isPending && <p role="status">读取本地说明…</p>}
        {query.isError && <p role="alert">{message(query.error)}</p>}
        {query.data && (
          <>
            {typeof query.data.manifest.version === "string" && (
              <p>本地清单版本：{query.data.manifest.version}</p>
            )}
            <h3>组件声明</h3>
            <p>只读清单；不表示已加载、连接或获得授权。</p>
            <dl className="native-components">
              {Object.entries(query.data.components).map(([kind, entries]) => (
                <div key={kind}>
                  <dt>{kind}</dt>
                  <dd>{entries.join("、") || "未发现"}</dd>
                </div>
              ))}
            </dl>
            {query.data.messages.map((m) => (
              <p className="native-empty-note" key={m}>
                {m}
              </p>
            ))}
            {query.data.readme && <SkillMarkdown content={query.data.readme} />}
          </>
        )}
      </div>
      <div className="modal-foot justify-end">
        <Button size="sm" variant="outline" onClick={onClose}>
          关闭详情
        </Button>
      </div>
    </Dialog>
  );
}
