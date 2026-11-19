import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import App from "./App";
import { createQueryClient } from "./lib/query-client";
import { createFakeBackend } from "./test/fake-backend";
import { nativePluginFixture } from "./test/native-plugin-fixtures";
import { ExitDialog } from "./components/exit-dialog";
import type { PluginOutcome } from "./lib/native-plugins";
function renderApp() {
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <App />
    </QueryClientProvider>,
  );
}
async function openPlugins() {
  fireEvent.click(screen.getByRole("button", { name: "插件" }));
  await screen.findByText("workspace-tools");
}
describe("native plugin management", () => {
  it("keeps native hosts and user scope independent from skills, and refresh only reads", async () => {
    const backend = createFakeBackend().install();
    renderApp();
    await openPlugins();
    expect(screen.getByText("用户级")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "添加项目" }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "重新读取" }));
    fireEvent.click(screen.getByRole("button", { name: "Claude Code" }));
    await screen.findByText("/Users/demo/.claude · 2.1.323");
    expect(backend.callsOf("prepare_native_plugin")).toHaveLength(0);
    expect(backend.callsOf("execute_native_plugin")).toHaveLength(0);
    fireEvent.change(
      screen.getByRole("textbox", { name: "搜索已配置市场中的插件" }),
      { target: { value: "review" } },
    );
    expect(screen.queryByText("workspace-tools")).not.toBeInTheDocument();
    expect(screen.getByText("review-notes")).toBeInTheDocument();
  });
  it("requires a fresh confirmation before a Codex reinstall and explains enablement", async () => {
    const backend = createFakeBackend().install();
    renderApp();
    await openPlugins();
    fireEvent.click(screen.getByRole("button", { name: "重新安装" }));
    const dialog = await screen.findByRole("dialog", { name: "确认重新安装" });
    expect(dialog).toHaveTextContent("会重新启用");
    expect(backend.callsOf("execute_native_plugin")).toHaveLength(0);
    fireEvent.click(within(dialog).getByRole("button", { name: "取消" }));
    expect(backend.callsOf("execute_native_plugin")).toHaveLength(0);
    fireEvent.click(screen.getByRole("button", { name: "重新安装" }));
    const next = await screen.findByRole("dialog", { name: "确认重新安装" });
    fireEvent.click(within(next).getByRole("button", { name: "确认执行" }));
    await screen.findByText("已完成并核对");
    expect(backend.callsOf("execute_native_plugin")).toHaveLength(1);
    expect(screen.getByText("用户设置：启用")).toBeInTheDocument();
  });
  it("shows cross-scope blocking and isolated market errors without hiding installed plugins", async () => {
    const state = nativePluginFixture();
    const market = state.snapshot!.markets[0]!;
    market.catalogError = "目录文件不可读";
    market.removeBlockedReason = "存在项目安装，移除会连带卸载，已阻止。";
    createFakeBackend({
      commands: { read_native_plugins: () => state },
    }).install();
    renderApp();
    await openPlugins();
    fireEvent.click(
      screen.getByRole("button", { name: /team-plugins.*目录读取失败/ }),
    );
    expect(screen.getByRole("button", { name: "移除市场" })).toBeDisabled();
    expect(
      screen.getByText("存在项目安装，移除会连带卸载，已阻止。"),
    ).toBeInTheDocument();
    expect(screen.getByText(/搜索结果可能缺失/)).toBeInTheDocument();
    expect(screen.getByText("workspace-tools")).toBeInTheDocument();
  });
  it("preserves partial outcomes and never retries a write automatically", async () => {
    let resolve!: (o: PluginOutcome) => void;
    let confirmed: PluginOutcome["preview"] | undefined;
    const backend = createFakeBackend({
      commands: {
        prepare_native_plugin: async (args, fake) => {
          confirmed = await fake.dispatch("prepare_native_plugin", args);
          return confirmed;
        },
        execute_native_plugin: () =>
          new Promise<PluginOutcome>((r) => {
            resolve = r;
          }),
      },
    }).install();
    renderApp();
    await openPlugins();
    fireEvent.click(screen.getByRole("button", { name: "启用" }));
    const dialog = await screen.findByRole("dialog", { name: "确认启用" });
    fireEvent.click(within(dialog).getByRole("button", { name: "确认执行" }));
    await screen.findByText(/正在执行原生命令/);
    expect(screen.getByRole("button", { name: "Claude Code" })).toBeDisabled();
    await act(async () =>
      resolve({
        preview: confirmed!,
        status: "partial",
        message: "命令失败但已修改用户设置；实际结果保留。",
        executed: true,
        commandSucceeded: false,
        verification: "observed",
        snapshot: nativePluginFixture().snapshot,
      }),
    );
    await screen.findByText("部分完成");
    expect(screen.getByText(/实际结果保留/)).toBeInTheDocument();
    expect(backend.callsOf("execute_native_plugin")).toHaveLength(1);
  });
  it("shows local component details and native wait-before-exit language", async () => {
    createFakeBackend().install();
    const app = renderApp();
    await openPlugins();
    fireEvent.click(screen.getByRole("button", { name: "workspace-tools" }));
    await screen.findByText("project-map、review-guide");
    fireEvent.click(screen.getByRole("button", { name: "关闭详情" }));
    app.unmount();
    render(
      <ExitDialog
        open
        info={{
          taskId: "plugin",
          kind: "native_plugin",
          skills: ["Codex · 安装 · demo@team"],
          startedAtEpochMs: 1,
          snapshotPaths: [],
        }}
        pending={false}
        error={null}
        onCancel={vi.fn()}
        onConfirm={vi.fn()}
      />,
    );
    expect(screen.getByText(/等待原生写入与状态核验结束/)).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "完成后退出" }),
    ).toBeInTheDocument();
    expect(screen.queryByText(/lock 快照/)).not.toBeInTheDocument();
  });
});
