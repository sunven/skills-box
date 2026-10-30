import { useEffect, useRef, useState } from "react";
import type { ScopeId } from "../lib/types";
import { GLOBAL_SCOPE } from "../lib/types";

export const AUTO_UPDATE_CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

interface AutoUpdateCheckOptions {
  /** The scope whose freshness and check this hook owns. */
  scope: ScopeId;
  enabled: boolean;
  checking: boolean;
  /** Persisted last full attempt for this scope, or `null` when never checked. */
  lastFullAttemptAt: number | null | undefined;
  /** Return `false` when a due attempt was skipped and should be retried. */
  onCheck: () => boolean | void | Promise<boolean | void>;
}

/**
 * Per-scope six-hour update checking. Each scope is checked when it first
 * becomes active and is stale (no state, or the last attempt is older than six
 * hours); a fresh scope only schedules the remainder of its interval. Manual
 * full checks advance that scope's anchor and never reset another scope's
 * timer. Changing `scope` starts a fresh evaluation without touching the
 * previous scope.
 */
export function useAutoUpdateCheck({
  scope = GLOBAL_SCOPE,
  enabled,
  checking,
  lastFullAttemptAt,
  onCheck,
}: AutoUpdateCheckOptions) {
  const onCheckRef = useRef(onCheck);
  const startedForScope = useRef<ScopeId | null>(null);
  const [baseline, setBaseline] = useState<{ scope: ScopeId; at: number } | null>(
    null,
  );
  const [skipAnchor, setSkipAnchor] = useState<{
    scope: ScopeId;
    at: number;
  } | null>(null);

  useEffect(() => {
    onCheckRef.current = onCheck;
  }, [onCheck]);

  useEffect(() => {
    if (!enabled || startedForScope.current === scope) return;
    startedForScope.current = scope;
    const now = Date.now();
    setSkipAnchor(null);
    if (
      lastFullAttemptAt != null &&
      now - lastFullAttemptAt < AUTO_UPDATE_CHECK_INTERVAL_MS
    ) {
      // This scope was checked recently; only schedule the remainder.
      setBaseline({ scope, at: lastFullAttemptAt });
      return;
    }
    setBaseline({ scope, at: now });
    void Promise.resolve(onCheckRef.current()).catch(() => {});
  }, [enabled, scope, lastFullAttemptAt]);

  useEffect(() => {
    if (!enabled || checking) return;
    // Wait until this scope's initial evaluation recorded a baseline; until
    // then a stale persisted attempt would schedule an immediate duplicate.
    if (!baseline || baseline.scope !== scope) return;
    const baselineAt = baseline.at;
    const skipAt = skipAnchor?.scope === scope ? skipAnchor.at : null;
    const anchor = Math.max(
      lastFullAttemptAt ?? Number.NEGATIVE_INFINITY,
      skipAt ?? Number.NEGATIVE_INFINITY,
      baselineAt ?? Number.NEGATIVE_INFINITY,
    );
    if (!Number.isFinite(anchor)) return;
    const delay = Math.max(
      0,
      anchor + AUTO_UPDATE_CHECK_INTERVAL_MS - Date.now(),
    );
    const timer = setTimeout(() => {
      Promise.resolve(onCheckRef.current())
        .then((started) => {
          if (started === false) setSkipAnchor({ scope, at: Date.now() });
        })
        .catch(() => setSkipAnchor({ scope, at: Date.now() }));
    }, delay);
    return () => clearTimeout(timer);
  }, [enabled, checking, scope, lastFullAttemptAt, skipAnchor, baseline]);
}
