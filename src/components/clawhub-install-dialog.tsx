import { useRef, useState } from "react";
import { X } from "lucide-react";
import {
  asAppError,
  fallbackError,
  prepareClawhubInstall,
  startClawhubInstall,
} from "../lib/api";
import {
  clawhubAddress,
  type ClawHubCoordinate,
  type ClawHubInstallPreview,
  type ClawHubNativeDetail,
} from "../lib/clawhub";
import type { AppError, InventorySnapshot, ScopeId } from "../lib/types";
import { useMutation, taskEventDriver } from "../lib/mutation";
import { useModalFocus } from "../hooks/use-modal-focus";
import { Button } from "./ui/button";
import { MutationError } from "./mutation/mutation-error";
import { MutationProgress } from "./mutation/mutation-progress";
import { MutationResult } from "./mutation/mutation-result";
import { CliCommandPreviewList } from "./cli-command-display";
import { OfficialLink } from "./official-link";

export function ClawHubInstallDialog({
  coordinate,
  detail,
  scope,
  scopeLabel,
  snapshot,
  disabled,
  onClose,
  onView,
}: {
  coordinate: ClawHubCoordinate;
  detail: ClawHubNativeDetail;
  scope: ScopeId;
  scopeLabel: string;
  snapshot?: InventorySnapshot;
  disabled: boolean;
  onClose: () => void;
  onView: (name: string) => void;
}) {
  const panelRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const [agents, setAgents] = useState<string[]>([]);
  const [preview, setPreview] = useState<ClawHubInstallPreview | null>(null);
  const [preparing, setPreparing] = useState(false);
  const [prepareError, setPrepareError] = useState<AppError | null>(null);
  const mutation = useMutation({
    action: "install",
    scope,
    fallbackFailure: "ClawHub 安装失败。",
    fallbackStart: "无法开始 ClawHub 安装。",
  });
  const {
    running,
    result,
    startError,
    error,
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
  const name = preview?.name ?? detail.name ?? coordinate.slug;
  const referenceLabel =
    preview?.referenceLabel ??
    (detail.latestVersion
      ? `v${detail.latestVersion}`
      : detail.resolvedReference
        ? `${detail.resolvedReference.repo}@${detail.resolvedReference.commit}:${detail.resolvedReference.path}`
        : "未提供");
  const canonicalUrl = preview?.canonicalUrl ?? detail.canonicalUrl;
  async function prepare() {
    setPreparing(true);
    setPrepareError(null);
    setPreview(null);
    mutation.reset();
    try {
      setPreview(await prepareClawhubInstall(scope, coordinate, agents));
    } catch (failure) {
      setPrepareError(
        asAppError(failure) ?? fallbackError("无法准备 ClawHub 安装。"),
      );
    } finally {
      setPreparing(false);
    }
  }
  const failure = prepareError ?? startError ?? error;
  return (
    <div
      className="modal-overlay"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) close();
      }}
    >
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="clawhub-install-title"
        className="modal-panel max-w-3xl"
      >
        <div className="modal-head">
          <div>
            <h2 id="clawhub-install-title" className="modal-title">
              安装 · {name}
            </h2>
            <p className="modal-sub">
              ClawHub · {clawhubAddress(coordinate)} · 安装到 {scopeLabel}
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
          <div className="panel-well space-y-2">
            <p>来源：ClawHub · {clawhubAddress(coordinate)}</p>
            <p>
              原始技能名：{name} · 解析引用：{referenceLabel}
            </p>
            <p>安装到 {scopeLabel}</p>
            <p>
              安装会按确认时解析出的引用下载并逐文件校验；实际内容以本次下载为准，不执行外部
              CLI 命令。
            </p>
            <OfficialLink url={canonicalUrl} label="ClawHub 页面" />
          </div>
          {!running && !result ? (
            <>
              <fieldset disabled={preparing || !!preview || disabled}>
                <legend className="font-medium mb-2">
                  安装到 {scopeLabel} · 选择 Agent
                </legend>
                <div className="flex flex-wrap gap-4">
                  {snapshot?.installTargets.map((agent) => (
                    <label key={agent.id} className="flex gap-2 items-center">
                      <input
                        type="checkbox"
                        disabled={!agent.detected && scope === "global"}
                        checked={agents.includes(agent.id)}
                        onChange={(event) =>
                          setAgents((old) =>
                            event.target.checked
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
                    确认安装 {preview.name} · {preview.referenceLabel} 到 {scopeLabel}
                  </p>
                  <p className="text-sm">
                    挂载：
                    {agents
                      .map(
                        (id) =>
                          snapshot?.installTargets.find((agent) => agent.id === id)
                            ?.displayName ?? id,
                      )
                      .join("、")}
                  </p>
                  <CliCommandPreviewList entries={preview.commands.entries} />
                  <div className="flex gap-2">
                    <Button
                      disabled={disabled || !!startError}
                      onClick={() =>
                        void mutation.start(
                          taskEventDriver({
                            total: 1,
                            message: "正在准备 ClawHub 安装…",
                            launch: (onEvent) =>
                              startClawhubInstall(
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
          <MutationError error={failure} />
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
          {result ? (
            <>
              <MutationResult
                action="install"
                result={result}
                testIdPrefix="clawhub"
              />
              <Button variant="outline" onClick={close}>
                返回详情
              </Button>
              {result.outcome === "success" ? (
                <Button onClick={() => onView(name)}>查看已安装技能</Button>
              ) : null}
            </>
          ) : null}
        </div>
      </div>
    </div>
  );
}
