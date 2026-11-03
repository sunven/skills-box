import { StrictMode } from "react";
import { act, fireEvent, render, renderHook, screen, waitFor, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import App from "./App";
import { useAppUpdate } from "./hooks/use-app-update";
import { APP_UPDATE_INTERVAL, appUpdateQueryKey, initialAppUpdate, type AppUpdateSnapshot } from "./lib/app-update";
import { createQueryClient } from "./lib/query-client";
import { createFakeBackend } from "./test/fake-backend";
import { cliMissingReport } from "./test/fixtures";
import { discoveryFixture } from "./test/discovery-fixtures";

const available = () => initialAppUpdate({ enabled: true, phase: "available", lastAttemptAt: Date.now(),
  candidate: { version: "0.2.0", notes: "## Changes\n\nFaster updates." } });
function mount() {
  const client = createQueryClient();
  const result = render(<QueryClientProvider client={client}><App /></QueryClientProvider>);
  return { client, ...result };
}
async function settings() {
  fireEvent.click(await screen.findByRole("button", { name: "设置" }));
  fireEvent.click(await screen.findByRole("button", { name: "应用更新" }));
  return screen.getByRole("region", { name: "应用更新" });
}
async function publish(client: ReturnType<typeof createQueryClient>, state: AppUpdateSnapshot) {
  await act(async () => { client.setQueryData(appUpdateQueryKey, state); });
}

describe("App updates", () => {
  it("viewing a reminder opens update settings without starting installation", async () => {
    const backend = createFakeBackend({ data: { appUpdate: available(), environment: cliMissingReport() } }).install();
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "查看应用更新" }));
    const region = screen.getByRole("region", { name: "应用更新" });
    expect(within(region).getByText("当前版本：", { exact: false })).toHaveTextContent("0.1.0");
    expect(within(region).getByRole("heading", { name: "Changes" })).toBeVisible();
    expect(backend.count("start_app_update")).toBe(0);
    expect(backend.count("test_github_auth")).toBe(0);
    expect(screen.queryByRole("button", { name: "查看应用更新" })).toBeNull();
  });

  it("dismisses only this session's version and shows a subsequent version", async () => {
    createFakeBackend({ data: { appUpdate: available() } }).install();
    const { client } = mount();
    fireEvent.click(await screen.findByRole("button", { name: "关闭应用更新提醒" }));
    await publish(client, { ...available(), revision: 2 });
    expect(screen.queryByRole("button", { name: "查看应用更新" })).toBeNull();
    await publish(client, { ...available(), revision: 3, candidate: { version: "0.3.0", notes: null } });
    expect(await screen.findByText("Skills Box 0.3.0 已发布")).toBeVisible();
  });

  it("never labels a failed check as the latest version", async () => {
    createFakeBackend({ data: { appUpdate: initialAppUpdate({ enabled: true, lastAttemptAt: Date.now() }) },
      commands: { check_app_update: () => { throw { code: "app_update.check_failed", message: "更新清单无法读取。", retryable: true }; } } }).install();
    mount(); const region = await settings();
    expect(within(region).getByText("尚未检查应用更新")).toBeVisible();
    fireEvent.click(within(region).getByRole("button", { name: "检查应用更新" }));
    expect(await within(region).findByText("更新清单无法读取。")).toBeVisible();
    expect(screen.queryByText("当前已是最新稳定版")).toBeNull();
  });

  it("preserves downloading state across navigation and renders unknown and known sizes", async () => {
    createFakeBackend({ data: { appUpdate: available() } }).install();
    const { client } = mount();
    let region = await settings();
    fireEvent.click(within(region).getByRole("button", { name: "更新到 0.2.0" }));
    await within(region).findByText("正在下载更新包…");
    expect(within(region).getByRole("progressbar")).not.toHaveAttribute("value");
    await publish(client, { ...available(), revision: 4, phase: "downloading", downloadedBytes: 60, totalBytes: 100 });
    expect(await within(region).findByText("已下载 60%")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "返回 skills" }));
    region = await settings();
    expect(within(region).getByText("已下载 60%")).toBeVisible();
    await publish(client, { ...available(), revision: 5, phase: "ready", downloadedBytes: 100, totalBytes: 100,
      error: { code: "task.busy", message: "已有变更任务正在执行。", retryable: true } });
    expect(await within(region).findByRole("button", { name: "安装并重启" })).toBeEnabled();
    expect(within(region).getByRole("button", { name: "检查应用更新" })).toBeDisabled();
  });

  it("blocks configuration and Skill writes during installation", async () => {
    const backend = createFakeBackend({ data: { appUpdate: { ...available(), phase: "installing" }, discovery: discoveryFixture() } }).install();
    mount(); await settings();
    fireEvent.click(screen.getByRole("button", { name: "GitHub 认证" }));
    expect(await screen.findByLabelText("GitHub token")).toBeDisabled();
    expect(screen.getByRole("button", { name: "保存" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "额外 Agent" }));
    expect(await screen.findByLabelText("id")).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "返回 skills" }));
    expect(screen.getByRole("button", { name: "安装 skill" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "切换到全局" })).toBeDisabled();
    fireEvent.click(await screen.findByRole("button", { name: "发现 (2)" }));
    expect(await screen.findByRole("button", { name: "全部已读" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "忽略 gamma" })).toBeDisabled();
    expect(screen.getByLabelText("发现目标作用域")).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "gamma" }));
    expect(screen.getByRole("complementary", { name: "发现项详情" })).toBeVisible();
    expect(backend.count("acknowledge_discoveries")).toBe(0);
  });

  it("retains newer snapshots when an older query completes", async () => {
    let resolve!: (state: AppUpdateSnapshot) => void;
    createFakeBackend({ commands: { get_app_update_state: () => new Promise((done) => { resolve = done; }) } }).install();
    const { client } = mount();
    await waitFor(() => expect(resolve).toBeTypeOf("function"));
    await publish(client, { ...available(), revision: 8 });
    await act(async () => resolve({ ...available(), revision: 1, candidate: null }));
    expect(client.getQueryData<AppUpdateSnapshot>(appUpdateQueryKey)?.revision).toBe(8);
    expect(await screen.findByRole("button", { name: "查看应用更新" })).toBeVisible();
  });

  it("startup is single-flight in StrictMode and manual checks reset six-hour scheduling", async () => {
    vi.useFakeTimers();
    const backend = createFakeBackend({ data: { appUpdate: initialAppUpdate({ enabled: true }) } }).install();
    const client = createQueryClient();
    const hook = renderHook(() => useAppUpdate(), { wrapper: ({ children }) => <StrictMode><QueryClientProvider client={client}>{children}</QueryClientProvider></StrictMode> });
    try {
      await act(async () => { await vi.advanceTimersByTimeAsync(20); });
      await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
      expect(backend.count("check_app_update")).toBe(1);
      await act(async () => { await vi.advanceTimersByTimeAsync(APP_UPDATE_INTERVAL / 2); hook.result.current.check(); });
      await act(async () => { await vi.advanceTimersByTimeAsync(20); });
      expect(backend.count("check_app_update")).toBe(2);
      await act(async () => { await vi.advanceTimersByTimeAsync(APP_UPDATE_INTERVAL - 100); });
      expect(backend.count("check_app_update")).toBe(2);
      await act(async () => { await vi.advanceTimersByTimeAsync(200); });
      expect(backend.count("check_app_update")).toBe(3);
    } finally { hook.unmount(); backend.reset(); vi.useRealTimers(); }
  });

  it("keeps a verified package after installation failure without restarting automatic checks", async () => {
    vi.useFakeTimers();
    const backend = createFakeBackend({ data: { appUpdate: { ...available(), phase: "error",
      lastAttemptAt: Date.now() - APP_UPDATE_INTERVAL, hasVerifiedDownload: true,
      error: { code: "app_update.install_failed", message: "安装失败。", retryable: true } } } }).install();
    const client = createQueryClient();
    const hook = renderHook(() => useAppUpdate(), { wrapper: ({ children }) => <QueryClientProvider client={client}>{children}</QueryClientProvider> });
    try {
      await act(async () => { await vi.advanceTimersByTimeAsync(20); });
      expect(hook.result.current.canCheck).toBe(false);
      expect(hook.result.current.busy).toBe(false);
      await act(async () => { await vi.advanceTimersByTimeAsync(APP_UPDATE_INTERVAL * 2); });
      expect(backend.count("check_app_update")).toBe(0);
    } finally { hook.unmount(); backend.reset(); vi.useRealTimers(); }
  });
});
