import type { PropsWithChildren } from "react";
import { StrictMode } from "react";
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  AUTO_UPDATE_CHECK_INTERVAL_MS,
  useAutoUpdateCheck,
} from "./use-auto-update-check";

const NOW = Date.UTC(2026, 9, 29, 0, 0, 0);
const HOUR = 60 * 60 * 1000;

interface Props {
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

  it("runs one initial check and schedules the next after six hours", () => {
    const onCheck = vi.fn();
    renderHook((props: Props) => useAutoUpdateCheck({ ...props, onCheck }), {
      initialProps: {
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

  it("does nothing while disabled", () => {
    const onCheck = vi.fn();
    renderHook((props: Props) => useAutoUpdateCheck({ ...props, onCheck }), {
      initialProps: {
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

  it("waits for an in-flight check and anchors on the recorded attempt", () => {
    const onCheck = vi.fn();
    const { rerender } = renderHook(
      (props: Props) => useAutoUpdateCheck({ ...props, onCheck }),
      {
        initialProps: {
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

  it("resets the six-hour timer after a manual full attempt", () => {
    const onCheck = vi.fn();
    const { rerender } = renderHook(
      (props: Props) => useAutoUpdateCheck({ ...props, onCheck }),
      {
        initialProps: {
          enabled: true,
          checking: false,
          lastFullAttemptAt: NOW,
        },
      },
    );

    // A full attempt already exists for this session, so no initial check.
    expect(onCheck).not.toHaveBeenCalled();

    rerender({
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
          enabled: false,
          checking: false,
          lastFullAttemptAt: null as number | null,
        },
      },
    );

    rerender({
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
        enabled: true,
        checking: false,
        lastFullAttemptAt: null,
      },
    });

    expect(onCheck).toHaveBeenCalledTimes(1);
  });
});
