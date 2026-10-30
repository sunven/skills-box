import { useEffect, useRef, useState } from "react";
import type { ScopeId } from "../lib/types";
import { GLOBAL_SCOPE } from "../lib/types";

export const AUTO_UPDATE_CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

interface AutoUpdateCheckOptions {
  /** The scope whose freshness and check this hook owns. */
  scope: ScopeId;
  checkRevision?: string;
  enabled: boolean;
  checking: boolean;
  /** Persisted last full attempt for this scope, or `null` when never checked. */
  lastFullAttemptAt: number | null | undefined;
  /** Return `false` when a due attempt was skipped and should be retried. */
  onCheck: () => boolean | void | Promise<boolean | void>;
}

/**
 * Per-scope six-hour update checking. The first evaluation of an app session
 * always checks the active scope, even when its persisted attempt is recent:
 * every launch gets one check. Later scope switches are only checked when that
 * scope has no state or its last attempt is older than six hours; a fresh scope
 * only schedules the remainder of its interval. Manual full checks advance that
 * scope's anchor and never reset another scope's timer. Changing `scope` starts
 * a fresh evaluation without touching the previous scope.
 * A new Project check revision bypasses the earlier root's six-hour anchor.
 */
export function useAutoUpdateCheck({
  scope = GLOBAL_SCOPE,
  checkRevision,
  enabled,
  checking,
  lastFullAttemptAt,
  onCheck,
}: AutoUpdateCheckOptions) {
  const onCheckRef = useRef(onCheck);
  const checkIdentity = JSON.stringify([scope, checkRevision ?? null]);
  const startedForScope = useRef<string | null>(null);
  const evaluatedRevisions = useRef(new Map<ScopeId, string | undefined>());
  const evaluatedOnce = useRef(false);
  const [anchor, setAnchor] = useState<{ identity: string; at: number } | null>(
    null,
  );

  useEffect(() => {
    onCheckRef.current = onCheck;
  }, [onCheck]);

  useEffect(() => {
    if (!enabled || startedForScope.current === checkIdentity) return;
    startedForScope.current = checkIdentity;
    const now = Date.now();
    const firstEvaluation = !evaluatedOnce.current;
    const revisionChanged =
      evaluatedRevisions.current.has(scope) &&
      evaluatedRevisions.current.get(scope) !== checkRevision;
    evaluatedRevisions.current.set(scope, checkRevision);
    evaluatedOnce.current = true;
    if (
      !firstEvaluation &&
      !revisionChanged &&
      lastFullAttemptAt != null &&
      now - lastFullAttemptAt < AUTO_UPDATE_CHECK_INTERVAL_MS
    ) {
      // This scope was checked recently; only schedule the remainder.
      setAnchor({ identity: checkIdentity, at: lastFullAttemptAt });
      return;
    }
    setAnchor({ identity: checkIdentity, at: now });
    void Promise.resolve(onCheckRef.current()).catch(() => {});
  }, [enabled, scope, checkRevision, checkIdentity, lastFullAttemptAt]);

  useEffect(() => {
    if (!enabled || checking) return;
    // Wait until this scope's initial evaluation recorded an anchor; until
    // then a stale persisted attempt would schedule an immediate duplicate.
    if (!anchor || anchor.identity !== checkIdentity) return;
    const latest = Math.max(
      lastFullAttemptAt ?? Number.NEGATIVE_INFINITY,
      anchor.at,
    );
    if (!Number.isFinite(latest)) return;
    const delay = Math.max(
      0,
      latest + AUTO_UPDATE_CHECK_INTERVAL_MS - Date.now(),
    );
    const timer = setTimeout(() => {
      Promise.resolve(onCheckRef.current())
        .then((started) => {
          if (started === false)
            setAnchor((previous) =>
              previous?.identity === checkIdentity
                ? { identity: checkIdentity, at: Date.now() }
                : previous,
            );
        })
        .catch(() =>
          setAnchor((previous) =>
            previous?.identity === checkIdentity
              ? { identity: checkIdentity, at: Date.now() }
              : previous,
          ),
        );
    }, delay);
    return () => clearTimeout(timer);
  }, [enabled, checking, scope, checkIdentity, lastFullAttemptAt, anchor]);
}
