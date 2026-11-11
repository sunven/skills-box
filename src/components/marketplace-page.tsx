import { ExpertPackPage } from "./expert-pack-page";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ArrowLeft, ArrowRight, Search } from "lucide-react";
import { asAppError, listMarketplaceCategories, listMarketplaceSkills } from "../lib/api";
import { marketplaceAddress, marketplaceApiKeyOptions, marketplaceKey, marketplaceSortOptions, type MarketplaceSkill, type MarketplaceListing, type MarketplaceSort, type MarketplaceApiKeyFilter } from "../lib/marketplace";
import type { InventorySnapshot, ScopeId } from "../lib/types";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { MarketplaceStats } from "./marketplace-stats";
import { MarketplaceDetailPage } from "./marketplace-detail-page";

interface Props {
  initialPack?: { slug: string; key: number };
  scope: ScopeId;
  scopeLabel: string;
  onInstallOpenChange: (open: boolean) => void;
  snapshot?: InventorySnapshot;
  disabled: boolean;
  onView: (name: string) => void;
}
function MarketplaceSkillsPage({ scope, scopeLabel, snapshot, disabled, onView, onInstallOpenChange }: Props) {
  const [input, setInput] = useState("");
  const [search, setSearch] = useState<{
    keyword: string;
    page: number;
    sort: MarketplaceSort;
    category: string;
    apiKey: MarketplaceApiKeyFilter;
  }>({ keyword: "", page: 1, sort: "score", category: "", apiKey: "all" });
  const [selected, setSelected] = useState<MarketplaceSkill | null>(null);
  const listRef = useRef<HTMLElement>(null);
  const listScroll = useRef(0);
  const opener = useRef<HTMLButtonElement | null>(null);
  function openDetail(skill: MarketplaceSkill, button: HTMLButtonElement) {
    listScroll.current = listRef.current?.scrollTop ?? 0;
    opener.current = button;
    setSelected(skill);
  }
  useLayoutEffect(() => {
    if (!selected && listRef.current) {
      listRef.current.scrollTop = listScroll.current;
      opener.current?.focus({ preventScroll: true });
    }
  }, [selected]);
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
    <>
    <main ref={listRef} hidden={selected !== null} className="discovery-page" aria-labelledby="marketplace-title">
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
                      onClick={(event) => openDetail(skill, event.currentTarget)}
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
                          onClick={(event) => openDetail(skill, event.currentTarget)}
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
    </main>
    {selected ? (
      <MarketplaceDetailPage
        key={marketplaceKey(selected.coordinate)}
        summary={selected}
        scope={scope}
        scopeLabel={scopeLabel}
        snapshot={snapshot}
        disabled={disabled}
        onBack={() => setSelected(null)}
        onView={onView}
        onInstallOpenChange={onInstallOpenChange}
      />
    ) : null}
    </>
  );
}

export function MarketplacePage(props: Props) {
  const [tab, setTab] = useState<"skills" | "packs">(props.initialPack ? "packs" : "skills");
  const [installOpen, setInstallOpen] = useState(false);
  const notifyInstallOpen = props.onInstallOpenChange;
  const onInstallOpenChange = useCallback((open: boolean) => {
    setInstallOpen(open);
    notifyInstallOpen(open);
  }, [notifyInstallOpen]);
  return <div className="marketplace-workspace">
    <div className="marketplace-kind-tabs" role="group" aria-label="市场类型" inert={installOpen}>
      <Button variant="ghost" aria-pressed={tab === "skills"} onClick={() => setTab("skills")}>技能</Button>
      <Button variant="ghost" aria-pressed={tab === "packs"} onClick={() => setTab("packs")}>专家包</Button>
    </div>
    {tab === "skills" ? <MarketplaceSkillsPage {...props} onInstallOpenChange={onInstallOpenChange} /> : <ExpertPackPage key={props.initialPack?.key ?? "catalog"} {...props} initialSlug={props.initialPack?.slug} onInstallOpenChange={onInstallOpenChange} />}
  </div>;
}
