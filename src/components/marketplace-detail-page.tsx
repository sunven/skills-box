import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ArrowLeft, ExternalLink } from "lucide-react";
import { asAppError, fallbackError, getMarketplaceSkill, openExternalUrl } from "../lib/api";
import { marketplaceAddress, marketplaceKey, type MarketplaceSkill } from "../lib/marketplace";
import type { InventorySnapshot, ScopeId } from "../lib/types";
import { Button } from "./ui/button";
import { MarketplaceDetailTabs } from "./marketplace-detail-tabs";
import { MarketplaceStats } from "./marketplace-stats";
import { MarketplaceInstallDialog } from "./marketplace-install-dialog";
import { MutationError } from "./mutation/mutation-error";

interface Props {
  summary: MarketplaceSkill;
  scope: ScopeId;
  scopeLabel: string;
  snapshot?: InventorySnapshot;
  disabled: boolean;
  onBack: () => void;
  onView: (name: string) => void;
  onInstallOpenChange: (open: boolean) => void;
}

export function MarketplaceDetailPage({
  summary, scope, scopeLabel, snapshot, disabled, onBack, onView, onInstallOpenChange,
}: Props) {
  const headingRef = useRef<HTMLHeadingElement>(null);
  const actionRef = useRef<HTMLButtonElement>(null);
  const wasInstallOpen = useRef(false);
  const [installTarget, setInstallTarget] = useState<{ scope: ScopeId; label: string } | null>(null);
  const coordinate = summary.coordinate;
  const detail = useQuery({
    queryKey: ["marketplace", "detail", marketplaceKey(coordinate)],
    queryFn: () => getMarketplaceSkill(coordinate),
    retry: false,
    staleTime: 60_000,
  });
  const data = detail.data;
  const skill = data ?? summary;
  const installed = snapshot?.skills.find(
    (entry) => entry.sourceType === "skillhub" && entry.source === marketplaceAddress(coordinate),
  );
  const collision = data?.originalName
    ? snapshot?.skills.find((entry) => entry.name === data.originalName && entry !== installed)
    : undefined;

  useEffect(() => {
    headingRef.current?.focus({ preventScroll: true });
  }, []);
  useEffect(() => () => onInstallOpenChange(false), [onInstallOpenChange]);
  useEffect(() => {
    if (installTarget) {
      wasInstallOpen.current = true;
    } else if (wasInstallOpen.current) {
      wasInstallOpen.current = false;
      actionRef.current?.focus({ preventScroll: true });
    }
  }, [installTarget]);

  function closeInstall() {
    setInstallTarget(null);
    onInstallOpenChange(false);
  }

  return (
    <>
      <main className="marketplace-detail-page" aria-labelledby="marketplace-detail-title" inert={installTarget !== null}>
        <div className="marketplace-detail-bar">
          <div className="marketplace-detail-bar-inner">
            <Button variant="ghost" size="sm" onClick={onBack}>
              <ArrowLeft aria-hidden="true" />
              返回列表
            </Button>
            <span className="marketplace-detail-bar-name" title={skill.name}>{skill.name}</span>
            <div className="marketplace-detail-action">
              <span className="marketplace-detail-target" aria-live="polite">安装到：{scopeLabel}</span>
              <Button
                ref={actionRef}
                size="sm"
                title={`当前作用域：${scopeLabel}`}
                disabled={!snapshot || (!installed && (disabled || !data || !!collision || !!data.installError))}
                onClick={() => {
                  if (installed) {
                    onView(installed.name);
                  } else {
                    setInstallTarget({ scope, label: scopeLabel });
                    onInstallOpenChange(true);
                  }
                }}
              >
                {installed ? "查看已安装技能" : "安装"}
              </Button>
            </div>
          </div>
        </div>
        <div className="marketplace-detail-content">
          <header className="marketplace-detail-header">
            <div className="marketplace-detail-identity">
              <span className="marketplace-detail-icon" aria-hidden="true">{Array.from(skill.name.trim())[0] ?? "S"}</span>
              <div>
                <h1 id="marketplace-detail-title" ref={headingRef} tabIndex={-1}>{skill.name}</h1>
                <p className="marketplace-detail-coordinate">{marketplaceAddress(coordinate)}</p>
              </div>
            </div>
            <div className="marketplace-detail-metadata">
              <span>源自 SkillHub</span>
              {skill.publisher ? <span>发布者：{skill.publisher}</span> : null}
              {skill.version ? <span className="badge">平台版本：{skill.version}</span> : null}
            </div>
            {skill.description ? <p className="marketplace-detail-description">{skill.description}</p> : null}
            <div className="marketplace-detail-links">
              <MarketplaceStats stars={skill.stars} downloads={skill.downloads} />
              <Button variant="ghost" size="sm" onClick={() => void openExternalUrl(skill.officialUrl)}>
                <ExternalLink aria-hidden="true" />
                官方页面
              </Button>
            </div>
            {data?.requirements.length ? (
              <div className="marketplace-detail-requirements">
                {data.requirements.map((requirement) => <p key={requirement}>{requirement}</p>)}
              </div>
            ) : null}
          </header>
          {detail.isPending ? <p className="marketplace-detail-status" role="status">正在读取技能说明与版本…</p> : null}
          {detail.isError ? (
            <div className="marketplace-detail-status" role="alert">
              {data ? <p>详情刷新失败，当前保留上次成功加载的内容。</p> : null}
              <MutationError error={asAppError(detail.error) ?? fallbackError("无法读取技能详情。")} />
              <Button variant="outline" size="sm" disabled={detail.isFetching} onClick={() => void detail.refetch()}>重试详情</Button>
            </div>
          ) : null}
          {data ? (
            <>
              <MutationError error={data.installError} />
              {collision ? (
                <p className="notice" role="alert">
                  当前作用域已有同名技能 {collision.name}（{collision.source ?? "本地技能"}）。请先卸载原技能。
                </p>
              ) : null}
              <MarketplaceDetailTabs key={data.version} detail={data} />
            </>
          ) : null}
        </div>
      </main>
      {installTarget && data ? (
        <MarketplaceInstallDialog
          coordinate={coordinate}
          initialDetail={data}
          scope={installTarget.scope}
          scopeLabel={installTarget.label}
          snapshot={scope === installTarget.scope ? snapshot : undefined}
          disabled={disabled || scope !== installTarget.scope}
          onClose={closeInstall}
          onView={onView}
        />
      ) : null}
    </>
  );
}
