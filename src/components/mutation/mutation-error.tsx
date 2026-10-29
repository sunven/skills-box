import type { AppError } from "../../lib/types";
import { cn } from "../../lib/utils";

export interface MutationErrorProps {
  error: AppError | null;
  testId?: string;
  monoDetail?: boolean;
  showCode?: boolean;
  className?: string;
}

export function MutationError({
  error,
  testId,
  monoDetail = false,
  showCode = false,
  className,
}: MutationErrorProps) {
  if (!error) return null;
  return (
    <div
      role="alert"
      data-testid={testId}
      className={cn("notice mt-3", className)}
      data-tone="negative"
    >
      <div>
        <p className="font-medium">{error.message}</p>
        {showCode && error.code ? (
          <p className="mt-0.5 font-mono text-[11.5px]">{error.code}</p>
        ) : null}
        {error.detail ? (
          <p
            className={cn(
              "mt-0.5 text-[12px]",
              monoDetail && "mono whitespace-pre-wrap break-all",
            )}
          >
            {error.detail}
          </p>
        ) : null}
        {error.hint ? <p className="mt-0.5">{error.hint}</p> : null}
      </div>
    </div>
  );
}
