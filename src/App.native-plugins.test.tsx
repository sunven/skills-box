import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
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
  it("shares the active project with skills and keeps Codex project writes unavailable", async () => {
    const id = "11111111-1111-4111-8111-111111111111";
    const scope = `project:${id}` as const;
    const backend = createFakeBackend({
      data: {
        projects: {
          projects: [
            {
              id,
              root: "/tmp/plugin-project",
              displayName: "plugin-project",
              addedAt: 1,
              lastOpenedAt: 1,
              available: true,
            },
          ],
          lastActiveScope: scope,
          warnings: [],
        },
      },
    }).install();
    renderApp();
    await screen.findByTestId("project-header");
    await openPlugins();
    await screen.findByText("项目共享 · 只读");
    expect(screen.getByRole("button", { name: "添加市场" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "启用" })).toBeDisabled();
    expect(
      backend.callsOf("read_native_plugins").slice(-1)[0]?.payload.context,
    ).toEqual({ scope, layer: "project" });
    fireEvent.click(screen.getByRole("button", { name: "切换到全局" }));
    await screen.findByText("用户级");
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "启用" })).toBeEnabled(),
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: "切换到项目 plugin-project",
        hidden: true,
      }),
    );
    await screen.findByText("项目共享 · 只读");
    fireEvent.click(screen.getByRole("button", { name: "返回 skills" }));
    expect(await screen.findByTestId("project-header")).toHaveTextContent(
      "plugin-project",
    );
    expect(backend.callsOf("prepare_native_plugin")).toHaveLength(0);
  });
  it("shares the scope selector while keeping refresh read-only", async () => {
    const backend = createFakeBackend().install();
    renderApp();
    await openPlugins();
    expect(screen.getByText("用户级")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "切换项目" }));
    expect(
      screen.queryByRole("button", { name: "添加项目", hidden: true }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "切换项目" }));
    fireEvent.click(screen.getByRole("button", { name: "重新读取" }));
    fireEvent.click(screen.getByRole("button", { name: "Claude Code" }));
    await screen.findByText("/Users/demo/.claude · 2.1.325");
    expect(backend.callsOf("prepare_native_plugin")).toHaveLength(0);
    expect(backend.callsOf("execute_native_plugin")).toHaveLength(0);
    fireEvent.change(
      screen.getByRole("textbox", { name: "搜索已配置市场中的插件" }),
      { target: { value: "review" } },
    );
    expect(screen.queryByText("workspace-tools")).not.toBeInTheDocument();
    expect(screen.getByText("review-notes")).toBeInTheDocument();
  });
  it.each([
    { host: "claude" as const, version: "2.1.325" },
    { host: "claude" as const, version: "99.1.0" },
    { host: "codex" as const, version: "0.171.0" },
  ])(
    "allows market previews on $host $version without a compatibility check",
    async ({ host, version }) => {
      const state = nativePluginFixture(host);
      state.snapshot!.version = version;
      const backend = createFakeBackend({
        commands: {
          read_native_plugins: ({ host: requested }) =>
            requested === host ? state : nativePluginFixture(requested),
        },
      }).install();
      renderApp();
      await openPlugins();
      if (host === "claude") {
        fireEvent.click(screen.getByRole("button", { name: "Claude Code" }));
      }
      await screen.findByText(`/Users/demo/.${host} · ${version}`);
      fireEvent.click(
        screen.getByRole("button", { name: /team-plugins.*Git 仓库/ }),
      );
      expect(screen.queryByText("部分操作暂不可用")).not.toBeInTheDocument();
      expect(screen.getByRole("button", { name: "同步市场" })).toBeEnabled();
      expect(screen.getByRole("button", { name: "移除市场" })).toBeEnabled();
      fireEvent.click(screen.getByRole("button", { name: "同步市场" }));
      const sync = await screen.findByRole("dialog", { name: "确认同步市场" });
      fireEvent.click(within(sync).getByRole("button", { name: "取消" }));
      fireEvent.click(screen.getByRole("button", { name: "移除市场" }));
      await screen.findByRole("dialog", { name: "确认移除市场" });
      expect(backend.callsOf("prepare_native_plugin")).toHaveLength(2);
      expect(backend.callsOf("verify_native_plugin_host")).toHaveLength(0);
      expect(backend.callsOf("execute_native_plugin")).toHaveLength(0);
    },
  );
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
    const state = nativePluginFixture("claude");
    const market = state.snapshot!.markets[0]!;
    market.catalogError = "目录文件不可读";
    market.removeBlockedReason = "存在项目安装，移除会连带卸载，已阻止。";
    createFakeBackend({
      commands: {
        read_native_plugins: ({ host }) =>
          host === "claude" ? state : nativePluginFixture(host),
      },
    }).install();
    renderApp();
    await openPlugins();
    fireEvent.click(screen.getByRole("button", { name: "Claude Code" }));
    await screen.findByText("/Users/demo/.claude · 2.1.325");
    fireEvent.click(
      screen.getByRole("button", { name: /team-plugins.*目录读取失败/ }),
    );
    expect(screen.getByRole("button", { name: "移除市场" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "同步市场" })).toBeDisabled();
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
