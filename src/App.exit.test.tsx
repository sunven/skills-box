import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { API_COMMANDS } from "./lib/api";
import { createQueryClient } from "./lib/query-client";
import type {
  ActiveMutationInfo,
  EnvReport,
  InterruptedReport,
} from "./lib/types";
import { createFakeBackend } from "./test/fake-backend";
import { healthyReport } from "./test/fixtures";

const lifecycle = vi.hoisted(() => ({
  handler: null as ((info: ActiveMutationInfo) => void) | null,
}));

vi.mock("./lib/lifecycle", () => ({
  listenForExitRequest: async (
    handler: (info: ActiveMutationInfo) => void,
  ) => {
    lifecycle.handler = handler;
    return () => {
      lifecycle.handler = null;
    };
  },
}));

function renderApp() {
  const queryClient = createQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>,
  );
}

function mockCommands(report: EnvReport | (() => EnvReport)) {
  return createFakeBackend({
    data: { environment: report },
  }).install();
}

const activeMutation: ActiveMutationInfo = {
  taskId: "task-3",
  kind: "update",
  skills: ["alpha", "beta"],
  startedAtEpochMs: 1_793_163_988_000,
  snapshotPaths: ["~/Library/.../backups/update-alpha"],
};

const interrupted: InterruptedReport = {
  markerPath: "~/Library/.../runtime/interrupted-mutation.json",
  markerCorrupt: false,
  taskId: "task-3",
  kind: "update",
  skills: ["alpha"],
  startedAtEpochMs: 1_793_163_988_000,
  snapshotPaths: ["~/Library/.../backups/update-alpha"],
  snapshotsPresent: ["~/Library/.../backups/update-alpha"],
  snapshotsMissing: [],
  lock: {
    path: "~/.agents/.skill-lock.json",
    exists: true,
    status: "ok",
    version: 3,
    skillCount: 61,
    error: null,
  },
  detectedAtEpochMs: 1_793_164_000_000,
};

afterEach(() => {
  lifecycle.handler = null;
});

describe("exit protection", () => {
  it("shows the risk dialog when Rust vetoes a close during a mutation", async () => {
    mockCommands(healthyReport());
    renderApp();
    expect(await screen.findByText("环境正常")).toBeInTheDocument();

    act(() => lifecycle.handler?.(activeMutation));

    const dialog = await screen.findByTestId("exit-dialog");
    expect(dialog).toHaveTextContent("更新任务正在运行");
    expect(dialog).toHaveTextContent("alpha、beta");
    expect(dialog).toHaveTextContent("写入可能只完成一部分");
    expect(dialog).toHaveTextContent("lock");

    // Escape keeps the app running.
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByTestId("exit-dialog")).toBeNull();
  });

  it("confirms the exit through the quit_app command", async () => {
    const backend = mockCommands(healthyReport());
    renderApp();
    expect(await screen.findByText("环境正常")).toBeInTheDocument();
    act(() => lifecycle.handler?.(activeMutation));

    fireEvent.click(await screen.findByRole("button", { name: "确认退出" }));
    await waitFor(() =>
      expect(backend.count(API_COMMANDS.quitApp)).toBe(1),
    );
  });

  it("continue waiting closes the dialog without invoking quit_app", async () => {
    const backend = mockCommands(healthyReport());
    renderApp();
    expect(await screen.findByText("环境正常")).toBeInTheDocument();
    act(() => lifecycle.handler?.(activeMutation));

    fireEvent.click(
      await screen.findByRole("button", { name: "继续等待" }),
    );
    expect(screen.queryByTestId("exit-dialog")).toBeNull();
    expect(backend.count(API_COMMANDS.quitApp)).toBe(0);
  });
});

describe("interrupted mutation warning", () => {
  it("surfaces the retained snapshot and lock verification on startup", async () => {
    mockCommands(healthyReport({ interrupted }));
    renderApp();

    const banner = await screen.findByTestId("interrupted-banner");
    expect(banner).toHaveTextContent("检测到上次变更被中断");
    expect(banner).toHaveTextContent("更新 · alpha");
    expect(banner).toHaveTextContent("lock v3 正常");
    expect(banner).toHaveTextContent("已保留快照 1 个");
  });

  it("dismisses the warning and removes it without re-probing the environment", async () => {
    const backend = mockCommands(healthyReport({ interrupted }));
    renderApp();
    expect(await screen.findByTestId("interrupted-banner")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "知道了" }));
    await waitFor(() =>
      expect(backend.count(API_COMMANDS.dismissInterruptedMutation)).toBe(1),
    );
    expect(screen.queryByTestId("interrupted-banner")).toBeNull();
    expect(backend.count(API_COMMANDS.getEnvironment)).toBe(1);
  });

  it("keeps a project-scope interruption warning out of the global scope", async () => {
    mockCommands(
      healthyReport({
        interrupted: { ...interrupted, scope: "project:p1" },
      }),
    );
    renderApp();
    expect(await screen.findByText("环境正常")).toBeInTheDocument();
    expect(screen.queryByTestId("interrupted-banner")).toBeNull();
  });

  it("shows the warning when the owning project scope is active", async () => {
    createFakeBackend({
      data: {
        environment: healthyReport({
          interrupted: { ...interrupted, scope: "project:p1" },
        }),
        projects: {
          projects: [
            {
              id: "p1",
              root: "/tmp/proj",
              displayName: "proj",
              addedAt: 1,
              lastOpenedAt: 1,
              available: true,
            },
          ],
          lastActiveScope: "project:p1",
          warnings: [],
        },
      },
    }).install();
    renderApp();
    const banner = await screen.findByTestId("interrupted-banner");
    expect(banner).toHaveTextContent("检测到上次变更被中断");
  });

  it("warns about a corrupt marker and missing snapshots", async () => {
    mockCommands(
      healthyReport({
        interrupted: {
          ...interrupted,
          markerCorrupt: true,
          markerError: "expected value",
          taskId: undefined,
          kind: undefined,
          skills: [],
          snapshotPaths: [],
          snapshotsPresent: [],
          snapshotsMissing: ["~/missing"],
        },
      }),
    );
    renderApp();
    const banner = await screen.findByTestId("interrupted-banner");
    expect(banner).toHaveTextContent("中断记录无法解析");
    expect(banner).toHaveTextContent("记录的备份路径已不存在");
  });
});
