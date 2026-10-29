import { useEffect, useRef, useState } from "react";

export const AUTO_UPDATE_CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

interface AutoUpdateCheckOptions {
  enabled: boolean;
  checking: boolean;
  lastFullAttemptAt: number | null | undefined;
  /** Return `false` when a due attempt was skipped and should be retried. */
  onCheck: () => boolean | void | Promise<boolean | void>;
}

/**
 * Starts one full check when the app becomes eligible, then schedules the next
 * full check six hours after the most recent full attempt. Manual full checks
 * reset the timer by advancing `lastFullAttemptAt`; scoped checks do not.
 */
export function useAutoUpdateCheck({
  enabled,
  checking,
  lastFullAttemptAt,
  onCheck,
}: AutoUpdateCheckOptions) {
  const onCheckRef = useRef(onCheck);
  const startedRef = useRef(false);
  const [baseline, setBaseline] = useState<number | null>(null);
  const [skipAnchor, setSkipAnchor] = useState<number | null>(null);

  useEffect(() => {
    onCheckRef.current = onCheck;
  }, [onCheck]);

  useEffect(() => {
    if (!enabled || startedRef.current) return;
    startedRef.current = true;
    if (lastFullAttemptAt != null) {
      // A full check already ran this session (for example while inventory was
      // still loading); treat it as the startup check and only schedule.
      setBaseline(lastFullAttemptAt);
      return;
    }
    setBaseline(Date.now());
    void Promise.resolve(onCheckRef.current()).catch(() => {});
  }, [enabled, lastFullAttemptAt]);

  useEffect(() => {
    if (!enabled || checking) return;
    const anchor = Math.max(
      lastFullAttemptAt ?? Number.NEGATIVE_INFINITY,
      skipAnchor ?? Number.NEGATIVE_INFINITY,
      baseline ?? Number.NEGATIVE_INFINITY,
    );
    if (!Number.isFinite(anchor)) return;
    const delay = Math.max(
      0,
      anchor + AUTO_UPDATE_CHECK_INTERVAL_MS - Date.now(),
    );
    const timer = setTimeout(() => {
      Promise.resolve(onCheckRef.current())
        .then((started) => {
          if (started === false) setSkipAnchor(Date.now());
        })
        .catch(() => setSkipAnchor(Date.now()));
    }, delay);
    return () => clearTimeout(timer);
  }, [enabled, checking, lastFullAttemptAt, skipAnchor, baseline]);
}
