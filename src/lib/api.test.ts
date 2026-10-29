import { describe, expect, it } from "vitest";
import { asAppError, fallbackError, isLockError } from "./api";

describe("AppError normalization", () => {
  it("preserves every backend field, including retryable", () => {
    expect(
      asAppError({
        code: "network.fetch_failed",
        message: "网络失败",
        detail: "GET repo",
        hint: "稍后重试",
        retryable: true,
      }),
    ).toEqual({
      code: "network.fetch_failed",
      message: "网络失败",
      detail: "GET repo",
      hint: "稍后重试",
      retryable: true,
    });
  });

  it("normalizes JS errors and strings without inventing a code", () => {
    expect(asAppError(new Error("boom"))).toEqual({
      message: "boom",
      retryable: false,
    });
    expect(asAppError("boom")).toEqual({
      message: "boom",
      retryable: false,
    });
    expect(asAppError({ message: "plain" })).toEqual({
      message: "plain",
      retryable: false,
    });
    expect(asAppError(null)).toBeNull();
    expect(fallbackError("fallback")).toEqual({
      message: "fallback",
      retryable: false,
    });
  });

  it("recognizes the lock error family", () => {
    expect(
      isLockError({
        code: "lock.corrupt",
        message: "lock",
        retryable: false,
      }),
    ).toBe(true);
    expect(
      isLockError({
        code: "env.node_missing",
        message: "node",
        retryable: true,
      }),
    ).toBe(false);
    expect(isLockError(null)).toBe(false);
  });
});
