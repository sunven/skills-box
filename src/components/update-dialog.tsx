import { useCallback, useEffect, useRef, useState } from "react";
import {
  AlertTriangle,
  ArrowUpCircle,
  HardDrive,
  ShieldAlert,
  X,
} from "lucide-react";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { MutationError } from "./mutation/mutation-error";
import { MutationProgress } from "./mutation/mutation-progress";
import { MutationResult } from "./mutation/mutation-result";
import { useModalFocus } from "../hooks/use-modal-focus";
import { startUpdate } from "../lib/api";
import {
  UPDATE_REASON_ORDER,
  type UpdateDialogView,
} from "../lib/update-view";
import {
  LOCAL_MODIFICATION_LABELS,
  shortHash,
} from "../lib/inventory-view";
import { taskEventDriver, useMutation } from "../lib/mutation";
import type { ScopeId } from "../lib/types";
import { useCliCommandPreview } from "../hooks/use-cli-commands";
import { ActualCliCommands, CliPreviewHelp, CliPreviewStatus, CliRetryControl } from "./cli-command-display";

interface UpdateDialogProps {
  open: boolean;
  scope: ScopeId;
  view: UpdateDialogView;
  onClose: () => void;
}

export function UpdateDialog({
  open,
  scope,
  view,
  onClose,
}: UpdateDialogProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  const [confirmed, setConfirmed] = useState(false);
  const [attemptNames, setAttemptNames] = useState<string[] | null>(null);

  const mutation = useMutation({
    action: "update",
    scope,
    fallbackFailure: "更新失败，请重试。",
    fallbackStart: "无法开始更新，请稍后重试。",
  });
  const {
    taskId,
    running,
    phase,
    completed,
    total,
    message,
    cancelling,
    result,
    error,
    startError,
  } = mutation.state;

  const {
    mode,
    summary,
    grouped,
    initialNames,
    singleSkill,
    singleEligible,
    singleNeedsConfirmation,
    singleCopyAgents,
    singleAffectedAgents,
    copyCandidates,
  } = view;
  const executionNames = attemptNames ?? initialNames;
  const commandPreview = useCliCommandPreview(scope, open && !running && !result && executionNames.length > 0 ? { action: "update", skills: executionNames } : null);
  const canStart =
    !running &&
    executionNames.length > 0 &&
    (!singleNeedsConfirmation || confirmed) && !!commandPreview.data && !commandPreview.isFetching && !commandPreview.isError;

  const launch = useCallback(
    (names: string[], previewId = commandPreview.data?.id) => {
      if (names.length === 0 || !previewId || mutation.stateRef.current.running) return;
      setAttemptNames(names);
      void mutation.start(
        taskEventDriver({
          total: names.length,
          message: "正在准备更新…",
          launch: (onEvent) => startUpdate(names, onEvent, scope, previewId),
        }),
      );
    },
    [mutation.start, mutation.stateRef, commandPreview.data?.id, scope],
  );

  useEffect(() => {
    if (!open) return;
    mutation.reset();
    setConfirmed(false);
    setAttemptNames(null);
  }, [open, mode, mutation.reset]);
  useEffect(() => {
    if ((startError ?? error)?.code === "cli.command_changed") void commandPreview.refetch();
  }, [startError, error]);

  const closeDialog = useCallback(() => {
    if (mutation.requestClose()) onCloseRef.current();
  }, [mutation.requestClose]);

  useModalFocus({
    open,
    panelRef,
    initialFocusRef: closeRef,
    onClose: closeDialog,
    canClose: mutation.canClose,
  });

  const handleCancelTask = () =>
    mutation.cancel("当前 skill 正在写入，无法取消。");

  if (!open) return null;

  const title =
    mode === "single" && singleSkill
      ? `更新 skill · ${singleSkill.name}`
      : "更新全部";
  const showInitial = !running && !result;
  const closeDisabled = !mutation.canClose();

  return (
    <div
      className="modal-overlay"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) closeDialog();
      }}
    >
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="update-dialog-title"
        data-testid="update-dialog"
        data-mode={mode}
        className="modal-panel max-w-2xl"
      >
        <div className="modal-head">
          <div>
            <h2 id="update-dialog-title" className="modal-title">
              <ArrowUpCircle
                className="size-4 text-muted-foreground"
                aria-hidden="true"
              />
              {title}
            </h2>
            <p className="modal-sub">
              更新前强制备份；按 skill 逐个更新。
            </p>
          </div>
          <Button
            ref={closeRef}
            variant="ghost"
            size="icon"
            aria-label="关闭更新"
            disabled={closeDisabled}
            title={closeDisabled ? "更新进行中，可在 skill 之间取消" : undefined}
            onClick={closeDialog}
            className="size-7 shrink-0"
          >
            <X className="size-4" aria-hidden="true" />
          </Button>
        </div>

        <div className="modal-body">
          {showInitial && mode === "single" ? (
            <div data-testid="update-single-confirm">
              {!singleSkill || !singleEligible ? (
                <div className="notice" data-tone="warning">
                  当前 skill 没有可执行的更新候选；请先检查更新，或先处理本地改动 /
                  仅副本 / 来源异常。
                </div>
              ) : (
                <>
                  <div className="panel-well">
                    <div className="kv-row">
                      <span className="text-muted-foreground">来源</span>
                      <strong className="font-mono">
                        {singleSkill.source ?? "来源未知"}
                        {singleSkill.updateResult?.ref
                          ? `#${singleSkill.updateResult.ref}`
                          : ""}
                      </strong>
                    </div>
                    <div className="kv-row">
                      <span className="text-muted-foreground">当前版本</span>
                      <strong
                        className="font-mono"
                        title={singleSkill.updateResult?.lockHash ?? undefined}
                      >
                        {singleSkill.updateResult?.lockHash
                          ? shortHash(singleSkill.updateResult.lockHash, 10)
                          : "—"}
                      </strong>
                    </div>
                    <div className="kv-row">
                      <span className="text-muted-foreground">目标版本</span>
                      <strong
                        className="font-mono"
                        title={singleSkill.updateResult?.remoteHash ?? undefined}
                      >
                        {singleSkill.updateResult?.remoteHash
                          ? shortHash(singleSkill.updateResult.remoteHash, 10)
                          : "—"}
                      </strong>
                    </div>
                    <div className="kv-row">
                      <span className="text-muted-foreground">受影响 agent</span>
                      <strong>
                        {singleAffectedAgents.length > 0
                          ? singleAffectedAgents.join("、")
                          : "无（仅写入共享 canonical）"}
                      </strong>
                    </div>
                    <div className="kv-row">
                      <span className="text-muted-foreground">本地改动</span>
                      <strong>
                        {LOCAL_MODIFICATION_LABELS[singleSkill.localModification]}
                      </strong>
                    </div>
                  </div>

                  {singleSkill.localModification !== "clean" ? (
                    <div
                      data-testid="update-local-warning"
                      className="notice mt-3" data-tone="warning"
                    >
                      <AlertTriangle
                        className="mt-0.5 size-3.5 shrink-0"
                        aria-hidden="true"
                      />
                      <div>
                        <p className="font-medium">
                          本地改动将被覆盖（需确认）
                        </p>
                        <p className="mt-0.5">
                          共享目录与基线不一致，或首次扫描无法确认。更新会覆盖这些修改；备份会保留更新前内容。
                        </p>
                      </div>
                    </div>
                  ) : null}

                  {singleCopyAgents.length > 0 ? (
                    <div
                      data-testid="update-copy-warning"
                      className="notice mt-3" data-tone="warning"
                    >
                      <ShieldAlert
                        className="mt-0.5 size-3.5 shrink-0"
                        aria-hidden="true"
                      />
                      <div>
                        <p className="font-medium">
                          copy 挂载将转为 symlink
                        </p>
                        <p className="mt-0.5">
                          {singleCopyAgents.join("、")} 当前是副本目录；更新前会先备份这些副本，更新后改为指向共享 canonical 的符号链接。其他已挂载 agent
                          会按当前物理挂载重建。
                        </p>
                      </div>
                    </div>
                  ) : null}

                  {singleNeedsConfirmation ? (
                    <label className="mt-3 flex cursor-pointer items-start gap-2 rounded-md border border-border px-3 py-2.5 text-[12.5px] leading-relaxed">
                      <input
                        type="checkbox"
                        className="mt-0.5"
                        aria-label="确认本地改动"
                        checked={confirmed}
                        onChange={(event) => setConfirmed(event.target.checked)}
                      />
                      <span>
                        我已了解本地改动会被覆盖；本次更新会先创建备份，不自动回滚。
                      </span>
                    </label>
                  ) : null}

                  <div className="notice mt-3">
                    <HardDrive
                      className="mt-0.5 size-3.5 shrink-0"
                      aria-hidden="true"
                    />
                    更新当前已挂载的 agent；已断开的 agent 不会被重挂，无挂载时仅更新共享目录。
                  </div>
                </>
              )}
            </div>
          ) : null}

          {showInitial && mode === "all" ? (
            <div className="grid gap-2 sm:grid-cols-2">
              <section
                data-testid="update-candidates"
                className="panel-well"
              >
                <div className="flex items-baseline gap-2">
                  <strong className="num text-xl font-semibold leading-none">
                    {summary.candidates.length}
                  </strong>
                  <span className="text-[12.5px] font-medium">更新候选</span>
                </div>
                <p className="mt-1.5 text-[12px] leading-relaxed text-muted-foreground">
                  仅包含受管、本地干净且来源可更新的 skill；将串行执行。
                </p>
                {summary.candidates.length === 0 ? (
                  <p className="mt-2 text-[12px] text-muted-foreground">
                    没有可直接更新的 skill。
                  </p>
                ) : (
                  <ul className="mt-2 space-y-1 text-[12px]">
                    {summary.candidates.map((skill) => (
                      <li
                        key={skill.name}
                        data-testid="update-candidate"
                        data-skill-name={skill.name}
                        className="rounded-sm bg-card px-2 py-1.5"
                      >
                        <span className="font-medium">{skill.name}</span>
                        <span className="ml-1 text-muted-foreground">
                          {skill.source ?? "来源未知"}
                        </span>
                        {copyCandidates.has(skill.name) ? (
                          <Badge variant="warning" className="ml-1">
                            copy → symlink
                          </Badge>
                        ) : null}
                      </li>
                    ))}
                  </ul>
                )}
              </section>

              <section
                data-testid="update-skipped"
                className="panel-well"
              >
                <div className="flex items-baseline gap-2">
                  <strong className="num text-xl font-semibold leading-none">
                    {summary.skipped.length}
                  </strong>
                  <span className="text-[12.5px] font-medium">跳过项</span>
                </div>
                <p className="mt-1.5 text-[12px] leading-relaxed text-muted-foreground">
                  本地改动、未知、源路径缺失、检查失败与仅副本不会进入更新执行。
                </p>
                {summary.skipped.length === 0 ? (
                  <p className="mt-2 text-[12px] text-muted-foreground">
                    没有被跳过的 skill。
                  </p>
                ) : (
                  <div className="mt-2 space-y-2">
                    {UPDATE_REASON_ORDER.filter((reason) => grouped.has(reason)).map(
                      (reason) => {
                        const items = grouped.get(reason)!;
                        return (
                          <div key={reason}>
                            <p className="text-[12px] font-medium text-warning">
                              {items[0]!.label} · {items.length}
                            </p>
                            <ul className="mt-0.5 space-y-0.5 text-[12px] text-muted-foreground">
                              {items.map((item) => (
                                <li key={item.name} title={item.detail}>
                                  <span className="block font-medium text-foreground">
                                    {item.name}
                                  </span>
                                  <span className="block leading-relaxed">
                                    {item.detail}
                                  </span>
                                </li>
                              ))}
                            </ul>
                          </div>
                        );
                      },
                    )}
                  </div>
                )}
              </section>

              {copyCandidates.size > 0 ? (
                <div
                  data-testid="update-copy-warning"
                  className="notice sm:col-span-2" data-tone="warning"
                >
                  <ShieldAlert
                    className="mt-0.5 size-3.5 shrink-0"
                    aria-hidden="true"
                  />
                  <div>
                    <p className="font-medium">copy 挂载将转为 symlink</p>
                    <p className="mt-0.5">
                      有 {copyCandidates.size} 个候选带 copy 挂载；更新前会先备份副本目录，更新后改为指向共享 canonical 的符号链接。
                    </p>
                  </div>
                </div>
              ) : null}
            </div>
          ) : null}

          {showInitial ? <CliPreviewStatus preview={commandPreview} /> : null}
          <ActualCliCommands taskId={taskId} />
          {running ? (
            <>
              <MutationProgress
                phase={phase}
                message={message}
                completed={completed}
                total={total}
                cancelling={cancelling}
                canCancel={Boolean(taskId) && !cancelling}
                cancelLabel="取消更新"
                onCancel={() => void handleCancelTask()}
              />
              <p className="mt-2 text-[12px] text-muted-foreground">
                每个 skill 写入前都会先备份；当前 skill 写入期间不可取消，skill 之间可取消。
              </p>
            </>
          ) : null}

          <MutationError error={startError} testId="update-start-error" />
          <MutationError error={error} testId="update-error" />

          {result ? (
            <MutationResult
              action="update"
              result={result}
              message={message}
              testIdPrefix="update"
              copy="不自动回滚；失败项可单独重试。备份与日志保留在应用数据目录。"
              showReveal
              retryMode="failed"
              onRetry={(names) => launch(names)}
              onRetryRow={(name) => launch([name])}
              warningsTestId="update-warnings"
              disabled={running}
              renderRetry={(names, label) => <CliRetryControl scope={scope} request={{ action: "update", skills: names }} label={label} disabled={running} onExecute={(id) => launch(names, id)} />}
            />
          ) : null}
        </div>

        <div className="modal-foot">
          <div className="modal-foot-note">
            {running
              ? "更新进行中；当前 skill 写入期间不可取消"
              : "更新前自动备份 · 不调用 skills update / check"}
          </div>
          <div className="flex items-center gap-2">
            {!running && !result ? (
              <>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={closeDialog}
                  disabled={closeDisabled}
                >
                  取消
                </Button>
                <Button
                  size="sm"
                  onClick={() => void launch(executionNames)}
                  disabled={!canStart}
                  title={
                    singleNeedsConfirmation && !confirmed
                      ? "请先勾选确认本地改动"
                      : undefined
                  }
                >
                  开始更新
                  {mode === "all" && executionNames.length > 0
                    ? ` (${executionNames.length})`
                    : ""}                </Button>
                <CliPreviewHelp preview={commandPreview} />
              </>
            ) : null}
            {running ? (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => void handleCancelTask()}
                disabled={!taskId || cancelling}
              >
                {cancelling ? "正在取消…" : "取消更新"}
              </Button>
            ) : null}
            {!running && result ? (
              <Button size="sm" onClick={closeDialog}>
                {result.outcome === "success" ? "完成" : "关闭"}
              </Button>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}
