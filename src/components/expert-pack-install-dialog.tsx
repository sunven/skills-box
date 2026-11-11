import { useRef, useState } from "react";
import { X } from "lucide-react";
import {
  asAppError,
  fallbackError,
  preparePackInstall,
  startPackInstall,
} from "../lib/api";
import type { PackPreview, LocalPack } from "../lib/expert-packs";
import type { AppError, InventorySnapshot, ScopeId } from "../lib/types";
import { taskEventDriver, useMutation } from "../lib/mutation";
import { useModalFocus } from "../hooks/use-modal-focus";
import { Button } from "./ui/button";
import { MutationError } from "./mutation/mutation-error";
import { MutationProgress } from "./mutation/mutation-progress";
import { MutationResult } from "./mutation/mutation-result";
import {
  ActualCliCommands,
  CliCommandPreviewList,
} from "./cli-command-display";
import { MarketplaceCliHelp } from "./marketplace-cli-help";
export function ExpertPackInstallDialog({
  slug,
  title,
  scope,
  scopeLabel,
  inventory,
  local: initialLocal,
  disabled,
  onClose,
}: {
  slug: string;
  title: string;
  scope: ScopeId;
  scopeLabel: string;
  inventory?: InventorySnapshot;
  local?: LocalPack;
  disabled: boolean;
  onClose: () => void;
}) {
  const [local] = useState(initialLocal);
  const panelRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const [agents, setAgents] = useState(local?.snapshot.agents ?? []);
  const [preview, setPreview] = useState<PackPreview | null>(null);
  const [preparing, setPreparing] = useState(false);
  const [prepareError, setPrepareError] = useState<AppError | null>(null);
  const mutation = useMutation({
    action: "install",
    scope,
    fallbackFailure: "专家包安装未完成。",
    fallbackStart: "无法开始专家包安装。",
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
  async function prepare() {
    setPreparing(true);
    setPrepareError(null);
    setPreview(null);
    mutation.reset();
    try {
      setPreview(await preparePackInstall(scope, slug, agents));
    } catch (e) {
      setPrepareError(asAppError(e) ?? fallbackError("无法准备专家包安装。"));
    } finally {
      setPreparing(false);
    }
  }
  const failure = prepareError ?? startError ?? error;
  const agentNames = (ids: string[]) =>
    ids
      .map(
        (id) =>
          inventory?.installTargets.find((a) => a.id === id)?.displayName ?? id,
      )
      .join("、");
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
        aria-labelledby="pack-install-title"
        className="modal-panel max-w-3xl"
      >
        <div className="modal-head">
          <div>
            <h2 id="pack-install-title" className="modal-title">
              {local ? "补齐安装" : "安装专家包"} · {title}
            </h2>
            <p className="modal-sub">安装到 {scopeLabel} · 工作流和全部成员</p>
          </div>
          <Button
            ref={closeRef}
            variant="ghost"
            size="icon"
            aria-label="关闭专家包安装"
            disabled={preparing || !mutation.canClose()}
            onClick={close}
          >
            <X />
          </Button>
        </div>
        <div className="modal-body space-y-4">
          {!running && !result && (
            <>
              {local ? (
                <p className="notice">
                  按本地安装快照补齐，保留当前干净成员。原目标：
                  {agentNames(agents)}
                </p>
              ) : (
                <fieldset disabled={preparing || !!preview || disabled}>
                  <legend className="font-medium mb-2">选择 Agent</legend>
                  <div className="flex flex-wrap gap-4">
                    {inventory?.installTargets.map((agent) => (
                      <label key={agent.id} className="flex items-center gap-2">
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
              )}
              {!preview ? (
                <Button
                  disabled={preparing || disabled || !agents.length}
                  onClick={() => void prepare()}
                >
                  {preparing ? "正在解析成员与检查冲突…" : "预览安装"}
                </Button>
              ) : (
                <>
                  <p>
                    目标：{scopeLabel} · {agentNames(preview.snapshot.agents)}
                  </p>
                  <div className="pack-table-wrap">
                    <table className="pack-table">
                      <thead>
                        <tr>
                          <th>技能 / 完整来源</th>
                          <th>采用版本</th>
                          <th>操作</th>
                        </tr>
                      </thead>
                      <tbody>
                        {preview.items.map((item) => {
                          const member = preview.snapshot.members.find(
                            (m) => m.name === item.name,
                          );
                          return (
                            <tr key={item.name}>
                              <td>
                                {item.name}
                                <small>
                                  {member
                                    ? `@${member.coordinate.namespace}/${member.coordinate.slug}`
                                    : "专家包工作流入口"}
                                </small>
                              </td>
                              <td>
                                {item.version ?? "确认的工作流内容"}
                                {item.snapshotVersion &&
                                  item.version !== item.snapshotVersion && (
                                    <small>
                                      快照版本：{item.snapshotVersion} ·
                                      保留当前版本
                                    </small>
                                  )}
                              </td>
                              <td>
                                {
                                  {
                                    install: "新增",
                                    reuse: "复用",
                                    attach: "补挂载",
                                  }[item.action]
                                }
                                {item.missingAgents.length > 0 && (
                                  <small>
                                    {agentNames(item.missingAgents)}
                                  </small>
                                )}
                              </td>
                            </tr>
                          );
                        })}
                      </tbody>
                    </table>
                  </div>
                  <details open>
                    <summary>
                      最终安装的工作流 · {preview.snapshot.workflowName}
                    </summary>
                    <p className="text-sm text-muted-foreground">
                      保留官方流程，补齐缺失的 name /
                      description，并追加成员引用与本地技能名的对照。
                    </p>
                    <pre className="pack-document">
                      {preview.snapshot.installedWorkflow}
                    </pre>
                  </details>
                  <details>
                    <summary>官方工作流原文</summary>
                    <pre className="pack-document">
                      {preview.snapshot.originalWorkflow}
                    </pre>
                  </details>
                  <CliCommandPreviewList entries={preview.commands.entries} />
                  <div className="flex gap-2">
                    <Button
                      disabled={disabled || !!startError}
                      onClick={() =>
                        void mutation.start(
                          taskEventDriver({
                            total: preview.items.length,
                            message: "正在准备专家包…",
                            launch: (onEvent) =>
                              startPackInstall(
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
                </>
              )}
            </>
          )}
          <MutationError error={failure} />
          {failure?.code === "marketplace.cli_unavailable" && (
            <MarketplaceCliHelp
              onReady={() => {
                setPrepareError(null);
                setPreview(null);
                mutation.reset();
              }}
            />
          )}
          {running && (
            <MutationProgress
              phase={phase}
              message={message}
              completed={completed}
              total={total}
              cancelling={cancelling}
              canCancel
              onCancel={() =>
                void mutation.cancel("当前技能写入完成后停止后续项。")
              }
            />
          )}
          <ActualCliCommands taskId={taskId} />
          {result && (
            <>
              <MutationResult
                action="install"
                result={{
                  ...result,
                  rows: result.rows.map((row) =>
                    row.status === "skipped"
                      ? { ...row, statusLabel: "未执行" }
                      : row,
                  ),
                }}
                testIdPrefix="expert-pack"
              />
              <p className="text-sm">
                已完成的技能保留。缺失项可从本地快照重新预览补齐。
              </p>
              <Button variant="outline" onClick={close}>
                返回专家包
              </Button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
