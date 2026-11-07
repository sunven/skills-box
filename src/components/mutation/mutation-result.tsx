import { useState, type ReactNode } from "react";
import { CheckCircle2 } from "lucide-react";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { asAppError, fallbackError, revealAppData } from "../../lib/api";
import {
  mutationHeading,
  type MutationAction,
  type MutationResultView,
  type MutationRow,
  type MutationRowStatus,
} from "../../lib/mutation";

export type MutationRetryMode = "none" | "failed" | "incomplete";

export interface MutationResultProps {
  action: MutationAction;
  result: MutationResultView;
  message?: string;
  /** Test ids are `${testIdPrefix}-result`, `-skill-result`, … */
  testIdPrefix: string;
  copy?: ReactNode;
  backupLabel?: string;
  showLog?: boolean;
  showReveal?: boolean;
  retryMode?: MutationRetryMode;
  onRetry?: (names: string[]) => void;
  onRetryRow?: (name: string) => void;
  warningsTestId?: string;
  disabled?: boolean;
  renderRetry?: (names: string[], label: string) => ReactNode;
}

function rowBadgeVariant(
  status: MutationRowStatus,
): "success" | "destructive" | "warning" {
  if (status === "success") return "success";
  if (status === "failed") return "destructive";
  return "warning";
}

function retryableRow(row: MutationRow, retryMode: MutationRetryMode): boolean {
  if (retryMode === "failed") return row.status === "failed";
  if (retryMode === "incomplete") return row.status !== "success";
  return false;
}

export function MutationResult({
  action,
  result,
  message,
  testIdPrefix,
  copy,
  backupLabel = "备份",
  showLog = true,
  showReveal = false,
  retryMode = "none",
  onRetry,
  onRetryRow,
  warningsTestId,
  disabled = false,
  renderRetry,
}: MutationResultProps) {
  const [revealError, setRevealError] = useState<string | null>(null);
  const retryNames = result.rows
    .filter((row) => retryableRow(row, retryMode))
    .map((row) => row.name);

  const doReveal = async (kind: "backups" | "logs") => {
    setRevealError(null);
    try {
      await revealAppData(kind);
    } catch (error) {
      setRevealError(
        (asAppError(error) ?? fallbackError("无法打开目录。")).message,
      );
    }
  };

  return (
    <div
      data-testid={`${testIdPrefix}-result`}
      data-overall={result.outcome}
      role={result.outcome === "success" ? undefined : "alert"}
    >
      <div className="flex items-baseline gap-2">
        <h3 className="flex items-center gap-1.5 text-sm font-semibold">
          {result.outcome === "success" ? (
            <CheckCircle2 className="size-4 text-success" aria-hidden="true" />
          ) : null}
          {mutationHeading(action, result.outcome)}
        </h3>
        {message ? (
          <span className="text-[12px] text-muted-foreground">{message}</span>
        ) : null}
      </div>
      {copy ? (
        <p className="mt-1 text-[12px] text-muted-foreground">{copy}</p>
      ) : null}

      <ul className="mt-2 space-y-2">
        {result.rows.map((row) => (
          <li
            key={row.name}
            data-testid={`${testIdPrefix}-skill-result`}
            data-skill-name={row.name}
            data-skill-status={row.taskStatus ?? row.status}
            className="rounded-md border border-border px-3 py-2.5"
          >
            <div className="flex items-center gap-2">
              <span className="min-w-0 break-words text-[13px] font-medium">
                {row.name}
              </span>
              <Badge
                variant={rowBadgeVariant(row.status)}
                className="ml-auto shrink-0"
              >
                {row.statusLabel}
              </Badge>
              {onRetryRow && !renderRetry && retryableRow(row, retryMode) ? (
                <Button
                  variant="ghost"
                  size="sm"
                  className="ml-auto h-7"
                  disabled={disabled}
                  onClick={() => onRetryRow(row.name)}
                >
                  重试
                </Button>
              ) : null}
            </div>
            {row.error ? (
              <p className="mt-1 text-[12px] leading-relaxed text-muted-foreground">
                {row.error.message}
                {row.error.detail ? ` · ${row.error.detail}` : ""}
              </p>
            ) : null}
            {!row.error && row.reason ? (
              <p className="mt-1 text-[12px] leading-relaxed text-muted-foreground">
                {row.reason}
              </p>
            ) : null}
            {row.backupPath ? (
              <p className="mono mt-1 break-all text-muted-foreground">
                备份：{row.backupPath}
              </p>
            ) : null}
            {renderRetry && retryableRow(row, retryMode) ? renderRetry([row.name], "重试") : null}
          </li>
        ))}
      </ul>

      {result.warnings.length > 0 ? (
        <ul
          data-testid={warningsTestId}
          className="notice mt-2 list-disc pl-8"
          data-tone="warning"
        >
          {result.warnings.map((warning) => (
            <li key={warning.code ?? warning.message}>
              {warning.message}
              {warning.detail ? ` · ${warning.detail}` : ""}
            </li>
          ))}
        </ul>
      ) : null}

      <div className="mt-2 space-y-0.5">
        {result.backupPath ? (
          <p
            data-testid={`${testIdPrefix}-backup-path`}
            className="mono break-all text-muted-foreground"
          >
            {backupLabel}：{result.backupPath}
          </p>
        ) : null}
        {showLog && result.logPath ? (
          <p
            data-testid={`${testIdPrefix}-log-path`}
            className="mono break-all text-muted-foreground"
          >
            日志：{result.logPath}
          </p>
        ) : null}
      </div>

      {showReveal || (onRetry && retryNames.length > 0) ? (
        <div className="mt-2 flex flex-wrap gap-1.5">
          {showReveal ? (
            <>
              <Button
                variant="outline"
                size="sm"
                onClick={() => void doReveal("backups")}
              >
                打开备份目录
              </Button>
              <Button
                variant="outline"
                size="sm"
                onClick={() => void doReveal("logs")}
              >
                打开日志目录
              </Button>
            </>
          ) : null}
          {onRetry && !renderRetry && retryNames.length > 0 ? (
            <Button
              variant="ghost"
              size="sm"
              disabled={disabled}
              onClick={() => onRetry(retryNames)}
            >
              重试失败项
            </Button>
          ) : null}
        </div>
      ) : null}
      {renderRetry && retryNames.length > 0 ? renderRetry(retryNames, "重试失败项") : null}
      {revealError ? (
        <p className="mt-1.5 text-[12px] text-destructive">{revealError}</p>
      ) : null}
    </div>
  );
}
