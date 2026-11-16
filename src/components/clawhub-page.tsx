import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { ExternalLink, Search } from "lucide-react";
import { asAppError, listClawhubSkills, openExternalUrl, searchClawhubSkills } from "../lib/api";
import {
  CLAWHUB_OFFICIAL_URL,
  clawhubAddress,
  clawhubKey,
  clawhubSorts,
  type ClawHubSearch,
  type ClawHubSkill,
  type ClawHubSort,
} from "../lib/clawhub";
import { useMarketplaceSession } from "../hooks/use-marketplace-session";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { ClawHubDetailPage, ClawHubMetrics } from "./clawhub-detail-page";

interface Props {
  active: boolean;
  onSwitchSkillsSh: () => void;
}
interface Session {
  sort: ClawHubSort;
  scroll: number;
  input: string;
  submitted: string | null;
  searchScroll: number;
}

const initialSession: Session = {
  sort: "recommended",
  scroll: 0,
  input: "",
  submitted: null,
  searchScroll: 0,
};

export function ClawHubPage({ active, onSwitchSkillsSh }: Props) {
  const [session, setSession] = useMarketplaceSession("clawhub", initialSession);
  const listRef = useRef<HTMLElement>(null);
  const opener = useRef<HTMLElement | null>(null);
  const [selected, setSelected] = useState<ClawHubSkill | null>(null);
  const catalog = useInfiniteQuery({
    queryKey: ["marketplace", "clawhub", "list", session.sort],
    queryFn: ({ pageParam }) => listClawhubSkills(session.sort, pageParam),
    initialPageParam: null as string | null,
    getNextPageParam: (lastPage, _pages, _lastPageParam, pageParams) => {
      // A repeated cursor would loop upstream; never request the same page twice.
      const cursor = lastPage.nextCursor;
      return cursor && !pageParams.includes(cursor) ? cursor : undefined;
    },
    enabled: active,
  });
  const search = useQuery({
    queryKey: ["marketplace", "clawhub", "search", session.submitted],
    queryFn: () => searchClawhubSkills(session.submitted!),
    enabled: active && session.submitted !== null,
    retry: false,
  });
  useLayoutEffect(() => {
    if (!active || !listRef.current) return;
    // Restore the position of whichever view is showing. Scrolling updates the
    // session but must not re-run this effect.
    listRef.current.scrollTop = session.submitted ? session.searchScroll : session.scroll;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, session.submitted, selected]);
  useLayoutEffect(() => {
    if (!selected) opener.current?.focus({ preventScroll: true });
  }, [selected]);
  function openDetail(skill: ClawHubSkill, button: HTMLElement) {
    opener.current = button;
    setSelected(skill);
  }
  const skills = useMemo(() => {
    const seen = new Set<string>();
    const combined: ClawHubSkill[] = [];
    for (const page of catalog.data?.pages ?? []) {
      for (const skill of page.skills) {
        const identity = `${skill.coordinate.namespace}/${skill.coordinate.slug}`;
        if (seen.has(identity)) continue;
        seen.add(identity);
        combined.push(skill);
      }
    }
    return combined;
  }, [catalog.data]);
  const previous = useRef<{ sort: ClawHubSort; skills: ClawHubSkill[] } | null>(null);
  useEffect(() => {
    if (catalog.data) previous.current = { sort: session.sort, skills };
  }, [catalog.data, session.sort, skills]);
  const previousSearch = useRef<{ keyword: string; result: ClawHubSearch } | null>(null);
  useEffect(() => {
    if (search.data && session.submitted) {
      previousSearch.current = { keyword: session.submitted, result: search.data };
    }
  }, [search.data, session.submitted]);

  const listError = catalog.isError ? asAppError(catalog.error) : null;
  const searchError = search.isError ? asAppError(search.error) : null;
  const noticeError = session.submitted ? searchError : listError;
  const rateLimited = noticeError?.code === "marketplace.rate_limited";
  const retryAfter = rateLimited ? Math.max(1, noticeError?.retryAfterSeconds ?? 5) : 0;
  const [cooldownUntil, setCooldownUntil] = useState(0);
  const [, setTick] = useState(0);
  useEffect(() => {
    setCooldownUntil(rateLimited ? Date.now() + retryAfter * 1000 : 0);
  }, [rateLimited, retryAfter, noticeError?.detail]);
  useEffect(() => {
    if (cooldownUntil <= Date.now()) return;
    const timer = setTimeout(() => setTick((value) => value + 1), cooldownUntil - Date.now() + 20);
    return () => clearTimeout(timer);
  }, [cooldownUntil]);
  const coolingDown = cooldownUntil > Date.now();

  // A failed list query keeps its own successful pages; a failed *new* sort
  // falls back to the last successful list and labels it with that old sort.
  const shown = catalog.data ? { sort: session.sort, skills } : previous.current;
  // A failed search keeps the last successful result under its old keyword.
  const retained = !search.data && search.isError ? previousSearch.current : null;
  const searchResult = search.data ?? retained?.result;
  const searchKeyword = search.data ? session.submitted : retained?.keyword ?? session.submitted;
  const sortLabel = (sort: ClawHubSort) =>
    clawhubSorts.find((option) => option.value === sort)!.label;

  function submitSearch() {
    const keyword = session.input.trim();
    if (!keyword) {
      setSession((value) => ({ ...value, input: "", submitted: null }));
      return;
    }
    if (listRef.current) listRef.current.scrollTop = 0;
    if (keyword === session.submitted) void search.refetch();
    setSession((value) => ({ ...value, submitted: keyword, searchScroll: 0 }));
  }
  const retry = () => void (session.submitted ? search.refetch() : catalog.refetch());
  const refresh = () => void (session.submitted ? search.refetch() : catalog.refetch());

  return (
    <>
    <main
      ref={listRef}
      hidden={selected !== null}
      className="discovery-page"
      aria-labelledby="clawhub-title"
      onScroll={(event) => {
        const scroll = event.currentTarget.scrollTop;
        setSession((value) =>
          value.submitted
            ? { ...value, searchScroll: scroll }
            : { ...value, scroll },
        );
      }}
    >
      <div className="discovery-content">
        <div className="discovery-heading">
          <div>
            <h1 id="clawhub-title">技能市场</h1>
            <p className="text-muted-foreground">
              从 ClawHub 浏览公开技能目录。下载量、安装量与星标按 ClawHub 平台口径展示。
            </p>
          </div>
          <span className="badge">来源 · ClawHub</span>
        </div>
        <div className="marketplace-toolbar">
          <div className="marketplace-sort" role="group" aria-label="ClawHub 排序">
            {clawhubSorts.map((option) => (
              <Button
                key={option.value}
                type="button"
                variant="ghost"
                aria-pressed={session.sort === option.value}
                disabled={session.submitted !== null}
                onClick={() => {
                  if (listRef.current) listRef.current.scrollTop = 0;
                  setSession((value) => ({ ...value, sort: option.value, scroll: 0 }));
                }}
              >
                {option.label}
              </Button>
            ))}
          </div>
          <Button
            type="button"
            variant="ghost"
            disabled={session.submitted ? search.isFetching : catalog.isFetching}
            onClick={refresh}
          >
            {session.submitted ? "刷新搜索结果" : "刷新目录"}
          </Button>
        </div>
        <form
          className="discovery-controls"
          onSubmit={(event) => {
            event.preventDefault();
            submitSearch();
          }}
        >
          <label className="discovery-search">
            <Search className="size-4" aria-hidden="true" />
            <Input
              aria-label="搜索 ClawHub"
              placeholder="搜索技能名称或用途（留空返回列表）"
              value={session.input}
              maxLength={200}
              onChange={(event) => {
                const input = event.target.value;
                setSession((value) => ({ ...value, input }));
              }}
            />
          </label>
          <Button type="submit" variant="outline">
            搜索
          </Button>
          {session.submitted ? (
            <Button
              type="button"
              variant="ghost"
              onClick={() => setSession((value) => ({ ...value, input: "", submitted: null }))}
            >
              清除搜索
            </Button>
          ) : null}
        </form>
        {session.submitted ? (
          <p className="text-xs text-muted-foreground">
            已排除 skills.sh 别名条目；skills.sh 条目请在 skills.sh 来源浏览/安装。
          </p>
        ) : null}
        {!session.submitted && listError ? (
          <div className="notice" role="alert">
            {rateLimited ? (
              <>
                <p>
                  ClawHub 请求受限（HTTP 429）。
                  {shown
                    ? `保留上次成功结果：${sortLabel(shown.sort)}，已加载 ${shown.skills.length} 项。`
                    : "尚无可显示的结果。"}
                </p>
                <p>{listError.detail ?? listError.message}</p>
                <p>
                  {coolingDown
                    ? `已按 Retry-After 退避，请在约 ${Math.ceil((cooldownUntil - Date.now()) / 1000)} 秒后重试。`
                    : "退避时间已结束，可以重试。"}
                </p>
              </>
            ) : (
              <>
                <p>
                  ClawHub 请求失败。
                  {shown
                    ? `保留上次成功结果：${sortLabel(shown.sort)}，已加载 ${shown.skills.length} 项。`
                    : "尚无可显示的结果。"}
                </p>
                <p>{listError.detail ?? listError.message}</p>
              </>
            )}
            <Button variant="outline" size="sm" disabled={coolingDown} onClick={retry}>
              重试
            </Button>
            <ClawHubOfficialLink />
          </div>
        ) : null}
        {session.submitted && searchError ? (
          <div className="notice" role="alert">
            {rateLimited ? (
              <>
                <p>
                  ClawHub 搜索受限（HTTP 429）。
                  {retained
                    ? `保留上次成功结果：搜索「${retained.keyword}」，返回 ${retained.result.returned} 项。`
                    : "尚无可显示的结果。"}
                </p>
                <p>{searchError.detail ?? searchError.message}</p>
                <p>
                  {coolingDown
                    ? `已按 Retry-After 退避，请在约 ${Math.ceil((cooldownUntil - Date.now()) / 1000)} 秒后重试。`
                    : "退避时间已结束，可以重试。"}
                </p>
              </>
            ) : (
              <>
                <p>
                  ClawHub 搜索失败。
                  {retained
                    ? `保留上次成功结果：搜索「${retained.keyword}」，返回 ${retained.result.returned} 项。`
                    : "尚无可显示的结果。"}
                </p>
                <p>{searchError.detail ?? searchError.message}</p>
              </>
            )}
            <Button variant="outline" size="sm" disabled={coolingDown} onClick={retry}>
              重试
            </Button>
            <ClawHubOfficialLink />
          </div>
        ) : null}
        {!session.submitted && catalog.isPending ? <p role="status">正在加载 ClawHub…</p> : null}
        {session.submitted && search.isPending ? <p role="status">正在搜索 ClawHub…</p> : null}
        {!session.submitted && shown ? (
          <>
            <p className="text-xs text-muted-foreground">
              {sortLabel(shown.sort)} · 已加载 {shown.skills.length} 项 · 按上游顺序
            </p>
            <div className="marketplace-grid">
              {shown.skills.map((skill) => (
                <ClawHubCard
                  key={clawhubKey(skill.coordinate)}
                  skill={skill}
                  onOpen={(button) => openDetail(skill, button)}
                />
              ))}
            </div>
            {shown.skills.length === 0 ? (
              <p className="discovery-empty">ClawHub 暂无技能条目。</p>
            ) : null}
            {catalog.data && catalog.hasNextPage ? (
              <div className="flex justify-end mt-4">
                <Button
                  type="button"
                  variant="outline"
                  disabled={catalog.isFetchingNextPage || coolingDown}
                  onClick={() => void catalog.fetchNextPage()}
                >
                  {catalog.isFetchingNextPage ? "正在加载更多…" : "加载更多"}
                </Button>
              </div>
            ) : null}
          </>
        ) : null}
        {session.submitted && searchResult ? (
          <>
            <p className="text-xs text-muted-foreground">
              搜索「{searchKeyword}」 · 返回 {searchResult.returned} 项（上游上限{" "}
              {searchResult.limit}） · 按上游顺序
            </p>
            {searchResult.upstreamReturned >= searchResult.limit ? (
              <p role="status">
                上游仅返回本次上限内的有限结果，不代表全量搜索；可尝试更具体的关键词。
              </p>
            ) : null}
            {searchResult.filteredAliases > 0 ? (
              <p className="text-xs text-muted-foreground">
                本次已排除 {searchResult.filteredAliases} 个 skills.sh 别名条目。
              </p>
            ) : null}
            {searchResult.filteredOthers > 0 ? (
              <p className="text-xs text-muted-foreground">
                本次已排除 {searchResult.filteredOthers} 个非技能条目。
              </p>
            ) : null}
            <div className="marketplace-grid">
              {searchResult.skills.map((skill) => (
                <ClawHubCard
                  key={clawhubKey(skill.coordinate)}
                  skill={skill}
                  onOpen={(button) => openDetail(skill, button)}
                />
              ))}
            </div>
            {searchResult.skills.length === 0 ? (
              <p className="discovery-empty">未找到匹配的 ClawHub 技能。请尝试其他关键词。</p>
            ) : null}
          </>
        ) : null}
      </div>
    </main>
    {selected ? (
      <ClawHubDetailPage
        key={clawhubKey(selected.coordinate)}
        coordinate={selected.coordinate}
        summary={selected}
        active={active}
        onBack={() => setSelected(null)}
        onSwitchSkillsSh={onSwitchSkillsSh}
      />
    ) : null}
    </>
  );
}

function ClawHubOfficialLink() {
  return (
    <Button variant="ghost" size="sm" onClick={() => void openExternalUrl(CLAWHUB_OFFICIAL_URL)}>
      <ExternalLink aria-hidden="true" />
      ClawHub 官网
    </Button>
  );
}

function ClawHubCard({
  skill,
  onOpen,
}: {
  skill: ClawHubSkill;
  onOpen: (button: HTMLElement) => void;
}) {
  return (
    <article className="marketplace-card">
      <h3 className="text-sm font-semibold break-all">
        <button
          className="marketplace-card-title"
          onClick={(event) => onOpen(event.currentTarget)}
        >
          {skill.name ?? skill.coordinate.slug}
        </button>
      </h3>
      <p className="font-mono text-xs text-muted-foreground break-all">
        {clawhubAddress(skill.coordinate)}
      </p>
      {skill.description ? <p className="marketplace-description">{skill.description}</p> : null}
      {skill.topics.length ? (
        <p className="flex flex-wrap gap-1">
          {skill.topics.map((topic) => (
            <span key={topic} className="badge">
              {topic}
            </span>
          ))}
        </p>
      ) : null}
      {skill.official ? <span className="badge">官方</span> : null}
      <ClawHubMetrics metrics={skill.metrics} />
      <div className="flex justify-end mt-auto">
        <Button variant="ghost" size="sm" onClick={(event) => onOpen(event.currentTarget)}>
          查看详情
        </Button>
      </div>
    </article>
  );
}
