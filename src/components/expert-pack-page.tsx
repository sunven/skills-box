import { SkillMarkdown } from "./skill-markdown";
import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowLeft, ArrowRight, Search } from "lucide-react";
import {
  asAppError,
  forgetExpertPack,
  getExpertPack,
  listExpertPacks,
  listLocalPacks,
} from "../lib/api";
import type { AppError, InventorySnapshot, ScopeId } from "../lib/types";
import type {
  MarketplaceCoordinate,
  MarketplaceSkill,
} from "../lib/marketplace";
import type { PackListing } from "../lib/expert-packs";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { MutationError } from "./mutation/mutation-error";
import { MarketplaceDetailPage } from "./marketplace-detail-page";
import { ExpertPackInstallDialog } from "./expert-pack-install-dialog";
export interface ExpertPackPageProps {
  scope: ScopeId;
  scopeLabel: string;
  snapshot?: InventorySnapshot;
  disabled: boolean;
  onView: (name: string) => void;
  onInstallOpenChange: (open: boolean) => void;
  initialSlug?: string;
}
export function ExpertPackPage({
  scope,
  scopeLabel,
  snapshot,
  disabled,
  onView,
  onInstallOpenChange,
  initialSlug,
}: ExpertPackPageProps) {
  const [input, setInput] = useState("");
  const [search, setSearch] = useState({ keyword: "", page: 1 });
  const [slug, setSlug] = useState<string | null>(initialSlug ?? null);
  const [install, setInstall] = useState(false);
  const [member, setMember] = useState<MarketplaceSkill | null>(null);
  const [tab, setTab] = useState<"local" | "upstream">(
    initialSlug ? "local" : "upstream",
  );
  const [workflowRaw, setWorkflowRaw] = useState(false);
  const [forget, setForget] = useState(false);
  const [forgetting, setForgetting] = useState(false);
  const [forgetError, setForgetError] = useState<AppError | null>(null);
  const previous = useRef<PackListing | undefined>(undefined);
  const heading = useRef<HTMLHeadingElement>(null);
  const action = useRef<HTMLButtonElement>(null);
  const client = useQueryClient();
  const localQuery = useQuery({
    queryKey: ["expertPacks", "local", scope],
    queryFn: () => listLocalPacks(scope),
    retry: false,
  });
  const local = localQuery.data?.find((p) => p.snapshot.slug === slug);
  const catalog = useQuery({
    queryKey: ["expertPacks", "list", search.keyword, search.page],
    queryFn: () => listExpertPacks(search.keyword, search.page),
    retry: false,
    staleTime: 60_000,
    enabled: !slug,
  });
  const remote = useQuery({
    queryKey: ["expertPacks", "detail", slug],
    queryFn: () => getExpertPack(slug!),
    enabled: !!slug,
    retry: false,
    staleTime: 60_000,
  });
  useEffect(() => {
    if (catalog.data) previous.current = catalog.data;
  }, [catalog.data]);
  useEffect(() => {
    heading.current?.focus();
  }, [slug]);
  useEffect(() => () => onInstallOpenChange(false), [onInstallOpenChange]);
  const listing =
    catalog.data ?? (catalog.isError ? previous.current : undefined);
  const detail = remote.data;
  const title =
    detail?.displayName ?? local?.snapshot.displayName ?? slug ?? "专家包";
  function select(next: string, localTab = false) {
    setSlug(next);
    setTab(localTab ? "local" : "upstream");
    setWorkflowRaw(false);
    setForget(false);
    setForgetError(null);
  }
  function openMember(coordinate: MarketplaceCoordinate) {
    setMember({
      coordinate,
      name: coordinate.slug,
      publisher: coordinate.namespace,
      description: "",
      version: "",
      officialUrl: `https://skillhub.cn/skills/${coordinate.namespace}/${coordinate.slug}`,
      stars: null,
      downloads: null,
    });
  }
  function closeInstall() {
    setInstall(false);
    onInstallOpenChange(false);
    action.current?.focus();
  }
  const agentNames = (ids: string[]) =>
    ids
      .map(
        (id) =>
          snapshot?.installTargets.find((a) => a.id === id)?.displayName ?? id,
      )
      .join("、");
  if (member)
    return (
      <MarketplaceDetailPage
        summary={member}
        scope={scope}
        scopeLabel={scopeLabel}
        snapshot={snapshot}
        disabled={disabled}
        onView={onView}
        onBack={() => setMember(null)}
        onInstallOpenChange={onInstallOpenChange}
      />
    );
  return (
    <>
      <main className="discovery-page" inert={install} aria-label="专家包">
        <div className="discovery-content space-y-5">
          {slug ? (
            <>
              <div className="flex items-center justify-between gap-4">
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => {
                    setSlug(null);
                    setForget(false);
                  }}
                >
                  <ArrowLeft />
                  返回专家包列表
                </Button>
                <div className="flex items-center gap-3">
                  <span className="text-sm text-muted-foreground">
                    {scopeLabel}
                  </span>
                  <Button
                    ref={action}
                    disabled={
                      disabled ||
                      localQuery.isPending ||
                      localQuery.isError ||
                      !!local?.complete ||
                      (!local &&
                        (!detail ||
                          !!detail.installError ||
                          detail.members.some((m) => m.error)))
                    }
                    onClick={() => {
                      setInstall(true);
                      onInstallOpenChange(true);
                    }}
                  >
                    {local?.complete
                      ? "已安装"
                      : local
                        ? "补齐安装"
                        : "安装专家包"}
                  </Button>
                </div>
              </div>
              <header>
                <h1 ref={heading} tabIndex={-1}>
                  {title}
                </h1>
                <p className="text-muted-foreground mt-2">
                  {detail?.summary ?? local?.snapshot.summary}
                </p>
              </header>
              <MutationError error={asAppError(localQuery.error)} />
              <div
                className="marketplace-sort"
                role="group"
                aria-label="专家包详情来源"
              >
                <Button
                  variant="ghost"
                  aria-pressed={tab === "upstream"}
                  onClick={() => setTab("upstream")}
                >
                  市场当前内容
                </Button>
                <Button
                  variant="ghost"
                  aria-pressed={tab === "local"}
                  disabled={!local}
                  onClick={() => setTab("local")}
                >
                  本地安装快照
                  {local ? (local.complete ? " · 完整" : " · 未完成") : ""}
                </Button>
              </div>
              {tab === "local" && local ? (
                <>
                  <div className="panel-well space-y-2">
                    <p>
                      {local.complete
                        ? "原目标上的工作流与成员齐全"
                        : "存在缺失技能或原目标挂载"}{" "}
                      · {scopeLabel}
                    </p>
                    <p>原目标：{agentNames(local.snapshot.agents)}</p>
                    <p className="text-sm text-muted-foreground">
                      快照保留此次确认的组成和版本。成员独立更新后的版本差异单独显示。
                    </p>
                  </div>
                  <div className="pack-table-wrap">
                    <table className="pack-table">
                      <thead>
                        <tr>
                          <th>本地技能</th>
                          <th>当前状态</th>
                          <th>挂载</th>
                        </tr>
                      </thead>
                      <tbody>
                        {local.skills.map((skill) => {
                          const saved = local.snapshot.members.find(
                            (m) => m.name === skill.name,
                          );
                          return (
                            <tr key={skill.name}>
                              <td>
                                <Button
                                  variant="ghost"
                                  size="sm"
                                  disabled={
                                    !snapshot?.skills.some(
                                      (s) => s.name === skill.name,
                                    )
                                  }
                                  onClick={() => onView(skill.name)}
                                >
                                  {skill.name}
                                </Button>
                                <small>
                                  {saved
                                    ? `@${saved.coordinate.namespace}/${saved.coordinate.slug}`
                                    : "工作流入口"}
                                </small>
                              </td>
                              <td>
                                {skill.conflict
                                  ? "身份冲突"
                                  : skill.present
                                    ? "已安装"
                                    : "缺失"}
                                {skill.modified && <small>本地修改</small>}
                                {saved && (
                                  <small>
                                    {skill.version ?? "未安装"}
                                    {skill.versionChanged
                                      ? `（快照 ${saved.version}）`
                                      : ""}
                                  </small>
                                )}
                              </td>
                              <td>
                                {skill.missingAgents.length
                                  ? `缺少：${agentNames(skill.missingAgents)}`
                                  : "原目标齐全"}
                                <small>
                                  实际：
                                  {agentNames(skill.attachedAgents) || "无"}
                                </small>
                              </td>
                            </tr>
                          );
                        })}
                      </tbody>
                    </table>
                  </div>
                  <details open>
                    <summary>
                      已确认的工作流 · {local.snapshot.workflowName}
                    </summary>
                    <pre className="pack-document">
                      {local.snapshot.installedWorkflow}
                    </pre>
                  </details>
                  <details>
                    <summary>安装时的官方原文</summary>
                    <pre className="pack-document">
                      {local.snapshot.originalWorkflow}
                    </pre>
                  </details>
                  {!forget ? (
                    <Button
                      variant="outline"
                      disabled={disabled}
                      onClick={() => setForget(true)}
                    >
                      忘记专家包
                    </Button>
                  ) : (
                    <div className="panel-well space-y-3">
                      <p>
                        只移除此作用域的包分组和关联，保留工作流、成员、挂载及来源记录。
                      </p>
                      <div className="flex gap-2">
                        <Button
                          disabled={disabled || forgetting}
                          onClick={async () => {
                            setForgetting(true);
                            onInstallOpenChange(true);
                            setForgetError(null);
                            try {
                              await forgetExpertPack(
                                scope,
                                local.snapshot.slug,
                              );
                              await client.invalidateQueries({
                                queryKey: ["expertPacks", "local", scope],
                              });
                              setForget(false);
                              setTab("upstream");
                            } catch (e) {
                              setForgetError(asAppError(e));
                            } finally {
                              setForgetting(false);
                              onInstallOpenChange(false);
                            }
                          }}
                        >
                          确认忘记
                        </Button>
                        <Button
                          variant="ghost"
                          disabled={forgetting}
                          onClick={() => setForget(false)}
                        >
                          取消
                        </Button>
                      </div>
                      <MutationError error={forgetError} />
                    </div>
                  )}
                </>
              ) : (
                <>
                  {remote.isPending && <p role="status">正在获取专家包…</p>}
                  {remote.isError && (
                    <div className="notice" role="alert">
                      <p>
                        市场当前内容暂不可用。
                        {local ? "本地安装快照仍可查看。" : ""}
                      </p>
                      <MutationError error={asAppError(remote.error)} />
                      <Button
                        variant="outline"
                        onClick={() => void remote.refetch()}
                      >
                        重试
                      </Button>
                    </div>
                  )}
                  {detail && (
                    <>
                      <MutationError error={detail.installError} />
                      <h2 className="font-semibold">
                        全部成员 · {detail.members.length}
                      </h2>
                      <div className="pack-members">
                        {detail.members.map((m, index) => (
                          <div
                            className="panel-well"
                            key={`${m.slug}:${index}`}
                          >
                            <Button
                              variant="ghost"
                              disabled={!m.coordinate}
                              onClick={() =>
                                m.coordinate && openMember(m.coordinate)
                              }
                            >
                              {m.coordinate
                                ? `@${m.coordinate.namespace}/${m.coordinate.slug}`
                                : m.slug}
                            </Button>
                            <MutationError error={m.error} />
                          </div>
                        ))}
                      </div>
                      <section aria-label="官方工作流">
                        <div className="flex items-center justify-between gap-3 mb-3">
                          <h2 className="font-semibold">官方工作流</h2>
                          <Button
                            variant="ghost"
                            size="sm"
                            aria-pressed={workflowRaw}
                            onClick={() => setWorkflowRaw((value) => !value)}
                          >
                            {workflowRaw ? "返回渲染" : "查看原文"}
                          </Button>
                        </div>
                        {workflowRaw ? (
                          <pre
                            className="pack-document"
                            role="region"
                            aria-label="官方工作流原文"
                            tabIndex={0}
                          >
                            {detail.workflow}
                          </pre>
                        ) : (
                          <SkillMarkdown content={detail.workflow} />
                        )}
                      </section>
                    </>
                  )}
                </>
              )}
            </>
          ) : (
            <>
              <header className="discovery-heading">
                <div>
                  <h1>专家包</h1>
                  <p className="text-muted-foreground">
                    围绕任务组合的工作流与技能，一次安装到同一组 Agent。
                  </p>
                </div>
                <span className="badge">SkillHub</span>
              </header>
              <MutationError error={asAppError(localQuery.error)} />
              {!!localQuery.data?.length && (
                <section aria-label="本地专家包">
                  <h2 className="font-semibold mb-3">
                    本地安装 · {scopeLabel}
                  </h2>
                  <div className="pack-local-list">
                    {localQuery.data.map((p) => (
                      <Button
                        key={p.snapshot.slug}
                        variant="outline"
                        onClick={() => select(p.snapshot.slug, true)}
                      >
                        {p.snapshot.displayName} ·{" "}
                        {p.complete ? "完整" : "未完成"}
                      </Button>
                    ))}
                  </div>
                </section>
              )}
              <form
                className="discovery-controls"
                onSubmit={(e) => {
                  e.preventDefault();
                  setSearch({ keyword: input.trim(), page: 1 });
                }}
              >
                <label className="discovery-search">
                  <Search className="size-4" />
                  <Input
                    aria-label="搜索专家包"
                    value={input}
                    onChange={(e) => setInput(e.target.value)}
                    placeholder="搜索专家包"
                  />
                </label>
                <Button type="submit">搜索</Button>
              </form>
              {catalog.isPending && <p role="status">正在加载专家包…</p>}
              {catalog.isError && (
                <div className="notice" role="alert">
                  <p>
                    专家包列表加载失败。{listing ? "以下保留上次结果。" : ""}
                  </p>
                  <MutationError error={asAppError(catalog.error)} />
                  <Button
                    variant="outline"
                    onClick={() => void catalog.refetch()}
                  >
                    重试
                  </Button>
                </div>
              )}
              {listing && (
                <>
                  <p className="text-sm text-muted-foreground">
                    共 {listing.total} 个专家包
                  </p>
                  <div className="pack-catalog">
                    {listing.packs.map((p) => (
                      <button
                        className="pack-card"
                        key={p.slug}
                        onClick={() => select(p.slug)}
                      >
                        <span className="pack-card-title">{p.displayName}</span>
                        <span className="text-muted-foreground">
                          {p.summary}
                        </span>
                        <span className="text-sm">
                          {p.memberCount} 个成员{" "}
                          <ArrowRight className="size-4 inline" />
                        </span>
                      </button>
                    ))}
                  </div>
                  {!listing.packs.length && <p>没有匹配的专家包。</p>}
                  <div className="flex justify-between items-center">
                    <Button
                      variant="outline"
                      disabled={catalog.isFetching || listing.page <= 1}
                      onClick={() =>
                        setSearch((s) => ({ ...s, page: listing.page - 1 }))
                      }
                    >
                      <ArrowLeft />
                      上一页
                    </Button>
                    <span>第 {listing.page} 页</span>
                    <Button
                      variant="outline"
                      disabled={
                        catalog.isFetching ||
                        listing.page * listing.pageSize >= listing.total
                      }
                      onClick={() =>
                        setSearch((s) => ({ ...s, page: listing.page + 1 }))
                      }
                    >
                      下一页
                      <ArrowRight />
                    </Button>
                  </div>
                </>
              )}
            </>
          )}
        </div>
      </main>
      {install && slug && (
        <ExpertPackInstallDialog
          slug={slug}
          title={title}
          scope={scope}
          scopeLabel={scopeLabel}
          inventory={snapshot}
          local={local}
          disabled={disabled}
          onClose={closeInstall}
        />
      )}
    </>
  );
}
