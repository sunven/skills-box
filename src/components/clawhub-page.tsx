import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useInfiniteQuery } from "@tanstack/react-query";
import { ExternalLink } from "lucide-react";
import { asAppError, listClawhubSkills, openExternalUrl } from "../lib/api";
import {
  CLAWHUB_OFFICIAL_URL,
  clawhubAddress,
  clawhubKey,
  clawhubMetricLabels,
  clawhubSorts,
  type ClawHubMetrics as ClawHubMetricsValue,
  type ClawHubSkill,
  type ClawHubSort,
} from "../lib/clawhub";
import { useMarketplaceSession } from "../hooks/use-marketplace-session";
import { Button } from "./ui/button";

interface Props {
  active: boolean;
}
interface Session {
  sort: ClawHubSort;
  scroll: number;
}

const initialSession: Session = { sort: "recommended", scroll: 0 };

export function ClawHubPage({ active }: Props) {
  const [session, setSession] = useMarketplaceSession("clawhub", initialSession);
  const listRef = useRef<HTMLElement>(null);
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
  useLayoutEffect(() => {
    if (active && listRef.current) listRef.current.scrollTop = session.scroll;
    // Restore only when the source becomes active; scrolling must not re-run this.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active]);
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
  const error = catalog.isError ? asAppError(catalog.error) : null;
  const rateLimited = error?.code === "marketplace.rate_limited";
  const retryAfter = rateLimited ? Math.max(1, error?.retryAfterSeconds ?? 5) : 0;
  const [cooldownUntil, setCooldownUntil] = useState(0);
  const [, setTick] = useState(0);
  useEffect(() => {
    setCooldownUntil(rateLimited ? Date.now() + retryAfter * 1000 : 0);
  }, [rateLimited, retryAfter, error?.detail]);
  useEffect(() => {
    if (cooldownUntil <= Date.now()) return;
    const timer = setTimeout(() => setTick((value) => value + 1), cooldownUntil - Date.now() + 20);
    return () => clearTimeout(timer);
  }, [cooldownUntil]);
  const coolingDown = cooldownUntil > Date.now();
  // A failed query keeps its own successful pages; a failed *new* sort falls
  // back to the last successful list and labels it with that old sort.
  const shown = catalog.data ? { sort: session.sort, skills } : previous.current;
  const sortLabel = (sort: ClawHubSort) =>
    clawhubSorts.find((option) => option.value === sort)!.label;
  return (
    <main
      ref={listRef}
      className="discovery-page"
      aria-labelledby="clawhub-title"
      onScroll={(event) => {
        const scroll = event.currentTarget.scrollTop;
        setSession((value) => ({ ...value, scroll }));
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
            disabled={catalog.isFetching}
            onClick={() => void catalog.refetch()}
          >
            刷新目录
          </Button>
        </div>
        {error ? (
          <div className="notice" role="alert">
            {rateLimited ? (
              <>
                <p>
                  ClawHub 请求受限（HTTP 429）。
                  {shown
                    ? `保留上次成功结果：${sortLabel(shown.sort)}，已加载 ${shown.skills.length} 项。`
                    : "尚无可显示的结果。"}
                </p>
                <p>{error.detail ?? error.message}</p>
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
                <p>{error.detail ?? error.message}</p>
              </>
            )}
            <Button
              variant="outline"
              size="sm"
              disabled={coolingDown}
              onClick={() => void catalog.refetch()}
            >
              重试
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => void openExternalUrl(CLAWHUB_OFFICIAL_URL)}
            >
              <ExternalLink aria-hidden="true" />
              ClawHub 官网
            </Button>
          </div>
        ) : null}
        {catalog.isPending ? <p role="status">正在加载 ClawHub…</p> : null}
        {shown ? (
          <>
            <p className="text-xs text-muted-foreground">
              {sortLabel(shown.sort)} · 已加载 {shown.skills.length} 项 · 按上游顺序
            </p>
            <div className="marketplace-grid">
              {shown.skills.map((skill) => (
                <article key={clawhubKey(skill.coordinate)} className="marketplace-card">
                  <h3 className="text-sm font-semibold break-all">
                    {skill.name ?? skill.coordinate.slug}
                  </h3>
                  <p className="font-mono text-xs text-muted-foreground break-all">
                    {clawhubAddress(skill.coordinate)}
                  </p>
                  {skill.description ? (
                    <p className="marketplace-description">{skill.description}</p>
                  ) : null}
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
                </article>
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
      </div>
    </main>
  );
}

function ClawHubMetrics({ metrics }: { metrics: ClawHubMetricsValue }) {
  return (
    <div className="marketplace-stats" role="group" aria-label="ClawHub 指标">
      {clawhubMetricLabels.map(({ key, label }) => {
        const count = metrics[key];
        return (
          <span
            key={key}
            role="group"
            aria-label={`${label}：${count === null ? "未提供" : count.toLocaleString()}`}
          >
            {label}：{count === null ? "未提供" : count.toLocaleString()}
          </span>
        );
      })}
    </div>
  );
}
