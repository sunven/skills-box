import type { PropsWithChildren } from "react";
import { StrictMode } from "react";
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ScopeId } from "../lib/types";
import { GLOBAL_SCOPE } from "../lib/types";
import {
  AUTO_UPDATE_CHECK_INTERVAL_MS,
  useAutoUpdateCheck,
} from "./use-auto-update-check";

const NOW = Date.UTC(2026, 9, 29, 0, 0, 0);
const HOUR = 60 * 60 * 1000;
const PROJECT_SCOPE: ScopeId =
  "project:11111111-1111-4111-8111-111111111111";

interface Props {
  scope: ScopeId;
  enabled: boolean;
  checking: boolean;
  lastFullAttemptAt: number | null;
}

describe("useAutoUpdateCheck", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(NOW);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("checks a never-checked scope once and schedules the next after six hours", () => {
    const onCheck = vi.fn();
    renderHook((props: Props) => useAutoUpdateCheck({ ...props, onCheck }), {
      initialProps: {
        scope: GLOBAL_SCOPE,
        enabled: true,
        checking: false,
        lastFullAttemptAt: null,
      },
    });

    expect(onCheck).toHaveBeenCalledTimes(1);

    act(() => {
      vi.advanceTimersByTime(AUTO_UPDATE_CHECK_INTERVAL_MS - 1);
    });
    expect(onCheck).toHaveBeenCalledTimes(1);

    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(onCheck).toHaveBeenCalledTimes(2);
  });

  it("does not check a fresh scope immediately and uses its persisted attempt", () => {
    const onCheck = vi.fn();
    renderHook((props: Props) => useAutoUpdateCheck({ ...props, onCheck }), {
      initialProps: {
        scope: PROJECT_SCOPE,
        enabled: true,
        checking: false,
        lastFullAttemptAt: NOW - HOUR,
      },
    });

    expect(onCheck).not.toHaveBeenCalled();

    act(() => {
      vi.advanceTimersByTime(AUTO_UPDATE_CHECK_INTERVAL_MS - HOUR - 1);
    });
    expect(onCheck).not.toHaveBeenCalled();

    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(onCheck).toHaveBeenCalledTimes(1);
  });

  it("checks a stale scope as soon as it becomes active", () => {
    const onCheck = vi.fn();
    renderHook((props: Props) => useAutoUpdateCheck({ ...props, onCheck }), {
      initialProps: {
        scope: PROJECT_SCOPE,
        enabled: true,
        checking: false,
        lastFullAttemptAt: NOW - AUTO_UPDATE_CHECK_INTERVAL_MS - 1,
      },
    });

    expect(onCheck).toHaveBeenCalledTimes(1);
  });

  it("keeps independent six-hour anchors while switching scopes", () => {
    const onCheck = vi.fn();
    const { rerender } = renderHook(
      (props: Props) => useAutoUpdateCheck({ ...props, onCheck }),
      {
        initialProps: {
          scope: GLOBAL_SCOPE,
          enabled: true,
          checking: false,
          lastFullAttemptAt: NOW - HOUR,
        },
      },
    );
    expect(onCheck).not.toHaveBeenCalled();

    // Switching to a stale project triggers exactly that scope's check.
    rerender({
      scope: PROJECT_SCOPE,
      enabled: true,
      checking: false,
      lastFullAttemptAt: NOW - 2 * AUTO_UPDATE_CHECK_INTERVAL_MS,
    });
    expect(onCheck).toHaveBeenCalledTimes(1);

    // Switching back to the still-fresh global scope does not re-check it.
    rerender({
      scope: GLOBAL_SCOPE,
      enabled: true,
      checking: false,
      lastFullAttemptAt: NOW - HOUR,
    });
    expect(onCheck).toHaveBeenCalledTimes(1);

    act(() => {
      vi.advanceTimersByTime(AUTO_UPDATE_CHECK_INTERVAL_MS - HOUR);
    });
    expect(onCheck).toHaveBeenCalledTimes(2);
  });

  it("does not re-check a scope that was already evaluated this session", () => {
    const onCheck = vi.fn();
    const { rerender } = renderHook(
      (props: Props) => useAutoUpdateCheck({ ...props, onCheck }),
      {
        initialProps: {
          scope: PROJECT_SCOPE,
          enabled: true,
          checking: false,
          lastFullAttemptAt: null as number | null,
        },
      },
    );
    expect(onCheck).toHaveBeenCalledTimes(1);

    rerender({
      scope: PROJECT_SCOPE,
      enabled: true,
      checking: false,
      lastFullAttemptAt: NOW,
    });
    expect(onCheck).toHaveBeenCalledTimes(1);
  });

  it("does nothing while disabled", () => {
    const onCheck = vi.fn();
    renderHook((props: Props) => useAutoUpdateCheck({ ...props, onCheck }), {
      initialProps: {
        scope: GLOBAL_SCOPE,
        enabled: false,
        checking: false,
        lastFullAttemptAt: null,
      },
    });

    expect(onCheck).not.toHaveBeenCalled();
    act(() => {
      vi.advanceTimersByTime(AUTO_UPDATE_CHECK_INTERVAL_MS * 2);
    });
    expect(onCheck).not.toHaveBeenCalled();
  });

  it("waits for an in-flight check and catches up when it ends", () => {
    const onCheck = vi.fn();
    const { rerender } = renderHook(
      (props: Props) => useAutoUpdateCheck({ ...props, onCheck }),
      {
        initialProps: {
          scope: GLOBAL_SCOPE,
          enabled: true,
          checking: true,
          lastFullAttemptAt: NOW,
        },
      },
    );

    act(() => {
      vi.advanceTimersByTime(AUTO_UPDATE_CHECK_INTERVAL_MS);
    });
    expect(onCheck).not.toHaveBeenCalled();

    rerender({
      scope: GLOBAL_SCOPE,
      enabled: true,
      checking: false,
      lastFullAttemptAt: NOW,
    });
    // The due time is already in the past, so the next tick catches up.
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(onCheck).toHaveBeenCalledTimes(1);
  });

  it("resets only the current scope's timer after a manual full attempt", () => {
    const onCheck = vi.fn();
    const { rerender } = renderHook(
      (props: Props) => useAutoUpdateCheck({ ...props, onCheck }),
      {
        initialProps: {
          scope: PROJECT_SCOPE,
          enabled: true,
          checking: false,
          lastFullAttemptAt: NOW,
        },
      },
    );

    expect(onCheck).not.toHaveBeenCalled();

    rerender({
      scope: PROJECT_SCOPE,
      enabled: true,
      checking: false,
      lastFullAttemptAt: NOW + HOUR,
    });

    act(() => {
      vi.advanceTimersByTime(AUTO_UPDATE_CHECK_INTERVAL_MS);
    });
    expect(onCheck).not.toHaveBeenCalled();

    act(() => {
      vi.advanceTimersByTime(HOUR);
    });
    expect(onCheck).toHaveBeenCalledTimes(1);
  });

  it("schedules a retry after a skipped due attempt", async () => {
    const onCheck = vi.fn(() => false);
    renderHook((props: Props) => useAutoUpdateCheck({ ...props, onCheck }), {
      initialProps: {
        scope: GLOBAL_SCOPE,
        enabled: true,
        checking: false,
        lastFullAttemptAt: NOW,
      },
    });

    expect(onCheck).not.toHaveBeenCalled();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(AUTO_UPDATE_CHECK_INTERVAL_MS);
    });
    expect(onCheck).toHaveBeenCalledTimes(1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(AUTO_UPDATE_CHECK_INTERVAL_MS - 1);
    });
    expect(onCheck).toHaveBeenCalledTimes(1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(onCheck).toHaveBeenCalledTimes(2);
  });

  it("does not start an initial check when a full check already ran", () => {
    const onCheck = vi.fn();
    const { rerender } = renderHook(
      (props: Props) => useAutoUpdateCheck({ ...props, onCheck }),
      {
        initialProps: {
          scope: GLOBAL_SCOPE,
          enabled: false,
          checking: false,
          lastFullAttemptAt: null as number | null,
        },
      },
    );

    rerender({
      scope: GLOBAL_SCOPE,
      enabled: true,
      checking: false,
      lastFullAttemptAt: NOW,
    });

    expect(onCheck).not.toHaveBeenCalled();
    act(() => {
      vi.advanceTimersByTime(AUTO_UPDATE_CHECK_INTERVAL_MS);
    });
    expect(onCheck).toHaveBeenCalledTimes(1);
  });

  it("still starts only one initial check under StrictMode", () => {
    const onCheck = vi.fn();
    const wrapper = ({ children }: PropsWithChildren) => (
      <StrictMode>{children}</StrictMode>
    );
    renderHook((props: Props) => useAutoUpdateCheck({ ...props, onCheck }), {
      wrapper,
      initialProps: {
        scope: GLOBAL_SCOPE,
        enabled: true,
        checking: false,
        lastFullAttemptAt: null,
      },
    });

    expect(onCheck).toHaveBeenCalledTimes(1);
  });
});
