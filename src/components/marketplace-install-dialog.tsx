import { useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { X } from "lucide-react";
import { asAppError, fallbackError, prepareMarketplaceInstall, startMarketplaceInstall } from "../lib/api";
import { marketplaceAddress, marketplaceKey, type MarketplaceCoordinate, type MarketplaceDetail, type MarketplacePreview } from "../lib/marketplace";
import type { AppError, InventorySnapshot, ScopeId } from "../lib/types";
import { useMutation, taskEventDriver } from "../lib/mutation";
import { useModalFocus } from "../hooks/use-modal-focus";
import { Button } from "./ui/button";
import { MutationError } from "./mutation/mutation-error";
import { MutationProgress } from "./mutation/mutation-progress";
import { MutationResult } from "./mutation/mutation-result";
import { ActualCliCommands, CliCommandPreviewList } from "./cli-command-display";
import { MarketplaceCliHelp } from "./marketplace-cli-help";

export function MarketplaceInstallDialog({
  coordinate,
  scope,
  snapshot,
  disabled,
  onClose,
  onView,
  initialDetail,
  scopeLabel,
}: {
  coordinate: MarketplaceCoordinate;
  scope: ScopeId;
  scopeLabel: string;
  snapshot?: InventorySnapshot;
  disabled: boolean;
  initialDetail: MarketplaceDetail;
  onClose: () => void;
  onView: (name: string) => void;
}) {
  const panelRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const [agents, setAgents] = useState<string[]>([]);
  const [preview, setPreview] = useState<MarketplacePreview | null>(null);
  const [preparing, setPreparing] = useState(false);
  const [prepareError, setPrepareError] = useState<AppError | null>(null);
  const queryClient = useQueryClient();
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
  const data = preview?.detail ?? initialDetail;
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
      const prepared = await prepareMarketplaceInstall(scope, coordinate, agents);
      setPreview(prepared);
      queryClient.setQueryData(["marketplace", "detail", marketplaceKey(coordinate)], prepared.detail);
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
        aria-labelledby="marketplace-install-title"
        className="modal-panel max-w-3xl"
      >
        <div className="modal-head">
          <div>
            <h2 id="marketplace-install-title" className="modal-title">
              安装 · {data.name}
            </h2>
            <p className="modal-sub">
              SkillHub · {marketplaceAddress(coordinate)} · 安装到 {scopeLabel}
            </p>
          </div>
          <Button
            ref={closeRef}
            variant="ghost"
            size="icon"
            aria-label="关闭安装"
            disabled={preparing || !mutation.canClose()}
            onClick={close}
          >
            <X />
          </Button>
        </div>
        <div className="modal-body space-y-4">
          {data ? (
            <>
              <div className="panel-well space-y-2">
                <p>发布者：{data.publisher || coordinate.namespace}</p>
                <p>原始技能名：{data.originalName ?? "未声明有效名称"} · 平台版本：{data.version}</p>
                <p>安装到 {scopeLabel}</p>
              </div>
              <MutationError error={data.installError} />
              {installed && !result ? (
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
                      安装到 {scopeLabel} · 选择
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
                        {scopeLabel}
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
                onClick={close}
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
