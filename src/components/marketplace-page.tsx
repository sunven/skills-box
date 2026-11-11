import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ArrowLeft, ArrowRight, Download, ExternalLink, Search, Star, X } from "lucide-react";
import {
  asAppError,
  fallbackError,
  getMarketplaceSkill,
  listMarketplaceCategories,
  listMarketplaceSkills,
  openExternalUrl,
  prepareMarketplaceInstall,
  startMarketplaceInstall,
} from "../lib/api";
import {
  marketplaceAddress,
  marketplaceApiKeyOptions,
  marketplaceKey,
  marketplaceSortOptions,
  type MarketplaceCoordinate,
  type MarketplaceListing,
  type MarketplacePreview,
  type MarketplaceSort,
  type MarketplaceApiKeyFilter,
} from "../lib/marketplace";
import type { AppError, InventorySnapshot, ScopeId } from "../lib/types";
import { useMutation, taskEventDriver } from "../lib/mutation";
import { useModalFocus } from "../hooks/use-modal-focus";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { SkillMarkdown } from "./skill-markdown";
import { MutationError } from "./mutation/mutation-error";
import { MutationProgress } from "./mutation/mutation-progress";
import { MutationResult } from "./mutation/mutation-result";
import {
  ActualCliCommands,
  CliCommandPreviewList,
} from "./cli-command-display";
import { MarketplaceCliHelp } from "./marketplace-cli-help";

const compactCount = new Intl.NumberFormat("zh-CN", {
  notation: "compact",
  maximumFractionDigits: 1,
});
const exactCount = new Intl.NumberFormat("zh-CN");

function MarketplaceStats({ stars, downloads }: {
  stars: number | null;
  downloads: number | null;
}) {
  return (
    <div className="marketplace-stats">
      {[
        { label: "收藏", count: stars, Icon: Star },
        { label: "下载量", count: downloads, Icon: Download },
      ].map(({ label, count, Icon }) => {
        const description = `${label}：${count === null ? "平台未提供" : exactCount.format(count)}`;
        return (
          <span key={label} aria-label={description} title={description}>
            <Icon aria-hidden="true" />
            {count === null ? "—" : compactCount.format(count)}
          </span>
        );
      })}
    </div>
  );
}

interface Props {
  scope: ScopeId;
  snapshot?: InventorySnapshot;
  disabled: boolean;
  onView: (name: string) => void;
}
export function MarketplacePage({ scope, snapshot, disabled, onView }: Props) {
  const [input, setInput] = useState("");
  const [search, setSearch] = useState<{
    keyword: string;
    page: number;
    sort: MarketplaceSort;
    category: string;
    apiKey: MarketplaceApiKeyFilter;
  }>({ keyword: "", page: 1, sort: "score", category: "", apiKey: "all" });
  const [selected, setSelected] = useState<MarketplaceCoordinate | null>(null);
  const previous = useRef<{
    search: typeof search;
    listing: MarketplaceListing;
  } | null>(null);
  const categories = useQuery({
    queryKey: ["marketplace", "categories"],
    queryFn: listMarketplaceCategories,
    retry: false,
    staleTime: 600_000,
  });
  const catalog = useQuery({
    queryKey: ["marketplace", "list", search.keyword, search.page, search.sort, search.category, search.apiKey],
    queryFn: () => listMarketplaceSkills(search.keyword, search.page, search.sort, search.category, search.apiKey),
    retry: false,
    staleTime: 60_000,
  });
  useEffect(() => {
    if (catalog.data) previous.current = { search, listing: catalog.data };
  }, [catalog.data, search]);
  const fallback = catalog.isError && !catalog.data ? previous.current : null;
  const listing = catalog.data ?? fallback?.listing;
  const shownSearch = fallback?.search ?? search;
  const shownSortLabel = marketplaceSortOptions.find(
    (option) => option.value === shownSearch.sort,
  )!.label;
  const shownCategoryLabel = categories.data?.find(
    (category) => category.key === shownSearch.category,
  )?.name ?? shownSearch.category;
  const shownApiKeyLabel = marketplaceApiKeyOptions.find(
    (option) => option.value === shownSearch.apiKey,
  )!.label;
  return (
    <main className="discovery-page" aria-labelledby="marketplace-title">
      <div className="discovery-content">
        <div className="discovery-heading">
          <div>
            <h1 id="marketplace-title">技能市场</h1>
            <p className="text-muted-foreground">
              从 SkillHub
              浏览和安装公开技能。安装后的技能可在当前作用域统一管理。
            </p>
          </div>
          <span className="badge">来源 · SkillHub</span>
        </div>
        <div className="marketplace-toolbar">
          <div className="marketplace-sort" role="group" aria-label="技能排序">
            {marketplaceSortOptions.map((option) => (
              <Button
                key={option.value}
                type="button"
                variant="ghost"
                aria-pressed={search.sort === option.value}
                onClick={() => setSearch((s) => ({ ...s, sort: option.value, page: 1 }))}
              >
                {option.label}
              </Button>
            ))}
          </div>
          <div className="marketplace-filters">
            <select
              className="marketplace-filter-select"
              aria-label="场景分类"
              value={search.category}
              disabled={!categories.data}
              onChange={(e) => {
                const category = e.target.value;
                setSearch((s) => ({ ...s, category, page: 1 }));
              }}
            >
              <option value="">{categories.isPending ? "正在加载分类…" : "所有场景分类"}</option>
              {categories.data?.map((category) => (
                <option key={category.key} value={category.key}>{category.name}</option>
              ))}
            </select>
            <select
              className="marketplace-filter-select"
              aria-label="API Key 筛选"
              value={search.apiKey}
              onChange={(e) => {
                const apiKey = e.target.value as MarketplaceApiKeyFilter;
                setSearch((s) => ({ ...s, apiKey, page: 1 }));
              }}
            >
              {marketplaceApiKeyOptions.map((option) => (
                <option key={option.value} value={option.value}>{option.label}</option>
              ))}
            </select>
          </div>
        </div>
        {categories.isError ? (
          <div className="notice" role="alert">
            <p>场景分类加载失败。</p>
            <Button variant="outline" size="sm" onClick={() => void categories.refetch()}>
              重试分类
            </Button>
          </div>
        ) : null}
        <form
          className="discovery-controls"
          onSubmit={(e) => {
            e.preventDefault();
            setSearch((s) => ({ ...s, keyword: input.trim(), page: 1 }));
          }}
        >
          <label className="discovery-search">
            <Search className="size-4" aria-hidden="true" />
            <Input
              aria-label="搜索 SkillHub"
              placeholder="搜索技能名称或用途"
              value={input}
              onChange={(e) => setInput(e.target.value)}
            />
          </label>
          <Button type="submit" variant="outline">
            搜索
          </Button>
          <Button
            type="button"
            variant="ghost"
            disabled={catalog.isFetching}
            onClick={() => void catalog.refetch()}
          >
            刷新目录
          </Button>
        </form>
        {catalog.isError ? (
          <div className="notice" role="alert">
            <p>
              SkillHub 请求失败。
              {listing
                ? `保留上次成功结果：${shownSearch.keyword || "默认目录"}，第 ${shownSearch.page} 页（${shownSortLabel}${shownSearch.category ? ` · ${shownCategoryLabel}` : ""}${shownSearch.apiKey !== "all" ? ` · ${shownApiKeyLabel}` : ""}）。`
                : "尚无可显示的结果。"}
            </p>
            <p>{asAppError(catalog.error)?.detail}</p>
            <Button
              variant="outline"
              size="sm"
              onClick={() => void catalog.refetch()}
            >
              重试
            </Button>
          </div>
        ) : null}
        {catalog.isFetching ? <p role="status">正在加载 SkillHub…</p> : null}
        {listing ? (
          <>
            <p className="text-xs text-muted-foreground">
              {shownSearch.keyword
                ? `搜索「${shownSearch.keyword}」`
                : "默认目录"}{" "}
              · {shownSortLabel} · 共 {listing.total.toLocaleString()} 项 · 第 {listing.page} 页
              {shownSearch.category ? ` · ${shownCategoryLabel}` : ""}
              {shownSearch.apiKey !== "all" ? ` · ${shownApiKeyLabel}` : ""}
            </p>
            <div className="marketplace-grid">
              {listing.skills.map((skill) => {
                const installed = snapshot?.skills.find(
                  (s) =>
                    s.sourceType === "skillhub" &&
                    s.source === marketplaceAddress(skill.coordinate),
                );
                return (
                  <article
                    key={marketplaceKey(skill.coordinate)}
                    className="marketplace-card"
                  >
                    <button
                      className="marketplace-card-title"
                      onClick={() => setSelected(skill.coordinate)}
                    >
                      {skill.name}
                    </button>
                    <p className="font-mono text-xs text-muted-foreground break-all">
                      {marketplaceAddress(skill.coordinate)}
                    </p>
                    <p className="marketplace-description">
                      {skill.description || "平台未提供描述。"}
                    </p>
                    <MarketplaceStats stars={skill.stars} downloads={skill.downloads} />
                    <div className="flex items-center justify-between gap-2 mt-auto">
                      <span className="text-xs text-muted-foreground">
                        {skill.version}
                      </span>
                      {installed ? (
                        <Button
                          variant="outline"
                          size="sm"
                          onClick={() => onView(installed.name)}
                        >
                          已安装 · 查看
                        </Button>
                      ) : (
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => setSelected(skill.coordinate)}
                        >
                          查看详情
                        </Button>
                      )}
                    </div>
                  </article>
                );
              })}
            </div>
            {listing.skills.length === 0 ? (
              <p className="discovery-empty">
                没有找到相关技能，请调整搜索词或筛选条件。
              </p>
            ) : null}
            <div className="flex justify-between items-center mt-4">
              <Button
                variant="outline"
                disabled={catalog.isFetching || search.page <= 1}
                onClick={() => setSearch((s) => ({ ...s, page: s.page - 1 }))}
              >
                <ArrowLeft />
                上一页
              </Button>
              <span className="text-sm">第 {search.page} 页</span>
              <Button
                variant="outline"
                disabled={
                  catalog.isFetching ||
                  catalog.isError ||
                  search.page * listing.pageSize >= listing.total
                }
                onClick={() => setSearch((s) => ({ ...s, page: s.page + 1 }))}
              >
                下一页
                <ArrowRight />
              </Button>
            </div>
          </>
        ) : null}
      </div>
      {selected ? (
        <MarketplaceDetailDialog
          key={`${scope}:${marketplaceKey(selected)}`}
          coordinate={selected}
          scope={scope}
          snapshot={snapshot}
          disabled={disabled}
          onClose={() => setSelected(null)}
          onView={onView}
        />
      ) : null}
    </main>
  );
}

function MarketplaceDetailDialog({
  coordinate,
  scope,
  snapshot,
  disabled,
  onClose,
  onView,
}: Props & { coordinate: MarketplaceCoordinate; onClose: () => void }) {
  const panelRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const [agents, setAgents] = useState<string[]>([]);
  const [preview, setPreview] = useState<MarketplacePreview | null>(null);
  const [preparing, setPreparing] = useState(false);
  const [prepareError, setPrepareError] = useState<AppError | null>(null);
  const detail = useQuery({
    queryKey: ["marketplace", "detail", marketplaceKey(coordinate)],
    queryFn: () => getMarketplaceSkill(coordinate),
    retry: false,
  });
  const mutation = useMutation({
    action: "install",
    scope,
    fallbackFailure: "SkillHub 安装失败。",
    fallbackStart: "无法开始 SkillHub 安装。",
  });
  const {
    running,
    result,
    startError,
    error,
    taskId,
    message,
    phase,
    completed,
    total,
    cancelling,
  } = mutation.state;
  const close = () => {
    if (!preparing && mutation.requestClose()) onClose();
  };
  useModalFocus({
    open: true,
    panelRef,
    initialFocusRef: closeRef,
    onClose: close,
    canClose: () => !preparing && mutation.canClose(),
  });
  const data = preview?.detail ?? detail.data;
  const installed = snapshot?.skills.find(
    (s) =>
      s.sourceType === "skillhub" &&
      s.source === marketplaceAddress(coordinate),
  );
  const collision = data?.originalName
    ? snapshot?.skills.find(
        (s) => s.name === data.originalName && s !== installed,
      )
    : null;
  async function prepare() {
    setPreparing(true);
    setPrepareError(null);
    setPreview(null);
    mutation.reset();
    try {
      setPreview(await prepareMarketplaceInstall(scope, coordinate, agents));
    } catch (e) {
      setPrepareError(
        asAppError(e) ?? fallbackError("无法准备 SkillHub 安装。"),
      );
    } finally {
      setPreparing(false);
    }
  }
  const failure = prepareError ?? startError ?? error;
  return (
    <div
      className="modal-overlay"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) close();
      }}
    >
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="marketplace-detail-title"
        className="modal-panel max-w-3xl"
      >
        <div className="modal-head">
          <div>
            <h2 id="marketplace-detail-title" className="modal-title">
              {data?.name ?? coordinate.slug}
            </h2>
            <p className="modal-sub">
              SkillHub · {marketplaceAddress(coordinate)}
            </p>
          </div>
          <Button
            ref={closeRef}
            variant="ghost"
            size="icon"
            aria-label="关闭市场详情"
            disabled={preparing || !mutation.canClose()}
            onClick={close}
          >
            <X />
          </Button>
        </div>
        <div className="modal-body space-y-4">
          {detail.isPending ? (
            <p role="status">正在读取技能说明与版本…</p>
          ) : null}
          {detail.isError ? (
            <>
              <MutationError
                error={
                  asAppError(detail.error) ??
                  fallbackError("无法读取技能详情。")
                }
              />
              <Button
                variant="outline"
                disabled={running}
                onClick={() => void detail.refetch()}
              >
                重试详情
              </Button>
            </>
          ) : null}
          {data ? (
            <>
              <div className="panel-well space-y-2">
                <p>{data.description}</p>
                <p>
                  发布者：{data.publisher || coordinate.namespace} · 平台版本：
                  {data.version}
                </p>
                <p>原始技能名：{data.originalName ?? "未声明有效名称"}</p>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => void openExternalUrl(data.officialUrl)}
                >
                  <ExternalLink />
                  官方页面
                </Button>
              </div>
              {data.requirements.map((text) => (
                <p className="notice" key={text}>
                  {text}
                </p>
              ))}
              <details open={!preview && !running && !result}>
                <summary className="cursor-pointer font-medium">
                  技能说明与运行要求
                </summary>
                <div className="marketplace-markdown">
                  <SkillMarkdown content={data.markdown} />
                </div>
              </details>
              <MutationError error={data.installError} />
              {installed ? (
                <Button
                  onClick={() => {
                    close();
                    onView(installed.name);
                  }}
                >
                  已安装 · 查看
                </Button>
              ) : collision ? (
                <p className="notice" role="alert">
                  当前作用域已有同名技能 {collision.name}（
                  {collision.source ?? "本地技能"}）。请先卸载原技能。
                </p>
              ) : !running && !result && !data.installError ? (
                <>
                  <fieldset disabled={preparing || !!preview || disabled}>
                    <legend className="font-medium mb-2">
                      安装到 {scope === "global" ? "全局" : "当前项目"} · 选择
                      Agent
                    </legend>
                    <div className="flex flex-wrap gap-4">
                      {snapshot?.installTargets.map((agent) => (
                        <label
                          key={agent.id}
                          className="flex gap-2 items-center"
                        >
                          <input
                            type="checkbox"
                            disabled={!agent.detected && scope === "global"}
                            checked={agents.includes(agent.id)}
                            onChange={(e) =>
                              setAgents((old) =>
                                e.target.checked
                                  ? [...old, agent.id]
                                  : old.filter((id) => id !== agent.id),
                              )
                            }
                          />
                          {agent.displayName}
                          {agent.shared ? "（共享目录）" : ""}
                        </label>
                      ))}
                    </div>
                  </fieldset>
                  {!preview ? (
                    <Button
                      disabled={preparing || disabled || agents.length === 0}
                      onClick={() => void prepare()}
                    >
                      {preparing ? "正在核实版本与目标…" : "预览安装"}
                    </Button>
                  ) : (
                    <div className="panel-well space-y-3">
                      <p>
                        确认安装 {data.originalName} · {data.version} 到{" "}
                        {scope === "global" ? "全局" : "当前项目"}
                      </p>
                      <p className="text-sm">
                        挂载：
                        {agents
                          .map(
                            (id) =>
                              snapshot?.installTargets.find((a) => a.id === id)
                                ?.displayName ?? id,
                          )
                          .join("、")}
                      </p>
                      <CliCommandPreviewList
                        entries={preview.commands.entries}
                      />
                      <div className="flex gap-2">
                        <Button
                          disabled={disabled || !!startError}
                          onClick={() =>
                            void mutation.start(
                              taskEventDriver({
                                total: 1,
                                message: "正在准备 SkillHub 安装…",
                                launch: (onEvent) =>
                                  startMarketplaceInstall(
                                    scope,
                                    preview.commands.id,
                                    onEvent,
                                  ),
                              }),
                            )
                          }
                        >
                          确认安装
                        </Button>
                        <Button
                          variant="outline"
                          onClick={() => {
                            setPreview(null);
                            setPrepareError(null);
                            mutation.reset();
                          }}
                        >
                          重新预览
                        </Button>
                      </div>
                    </div>
                  )}
                </>
              ) : null}
            </>
          ) : null}
          <MutationError error={failure} />
          {failure?.code === "marketplace.cli_unavailable" ? (
            <MarketplaceCliHelp
              onReady={() => {
                setPrepareError(null);
                setPreview(null);
                mutation.reset();
              }}
            />
          ) : null}
          {running ? (
            <MutationProgress
              phase={phase}
              message={message}
              completed={completed}
              total={total}
              cancelling={cancelling}
              canCancel
              onCancel={() =>
                void mutation.cancel("正在写入技能，请等待完成。")
              }
            />
          ) : null}
          <ActualCliCommands taskId={taskId} />
          {result ? (
            <>
              <MutationResult
                action="install"
                result={result}
                testIdPrefix="marketplace"
              />
              <Button
                variant="outline"
                onClick={() => {
                  setPreview(null);
                  mutation.reset();
                }}
              >
                返回详情
              </Button>
              {result.outcome === "success" && data?.originalName ? (
                <Button onClick={() => onView(data.originalName!)}>
                  查看已安装技能
                </Button>
              ) : null}
            </>
          ) : null}
        </div>
      </div>
    </div>
  );
}
