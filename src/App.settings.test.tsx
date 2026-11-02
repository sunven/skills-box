import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import App from "./App";
import { API_COMMANDS } from "./lib/api";
import { createQueryClient } from "./lib/query-client";
import { githubSettingsQueryKey } from "./components/settings-page";
import { createFakeBackend } from "./test/fake-backend";
import { cliMissingReport } from "./test/fixtures";
import { sampleSnapshot } from "./test/inventory-fixtures";

function renderApp() {
  const queryClient = createQueryClient();
  const view = render(<QueryClientProvider client={queryClient}><App /></QueryClientProvider>);
  return { queryClient, ...view };
}

async function openSettings() {
  fireEvent.click(await screen.findByRole("button", { name: "设置" }));
  await screen.findByRole("heading", { name: "设置" });
}

const environmentSettings = { hasSavedToken: false, hasEnvironmentToken: true, effectiveSource: "environment" as const };

describe("settings navigation", () => {
  it("shows one configuration at a time and preserves drafts when switching", async () => {
    createFakeBackend().install();
    renderApp();
    await openSettings();
    const menu = within(screen.getByRole("navigation", { name: "设置菜单" }));
    const githubMenu = menu.getByRole("button", { name: "GitHub 认证" });
    const agentsMenu = menu.getByRole("button", { name: "额外 Agent" });
    expect(githubMenu).toHaveAttribute("aria-current", "page");
    expect(screen.queryByRole("region", { name: "额外 Agent" })).toBeNull();
    const github = within(screen.getByRole("region", { name: "GitHub 认证" }));
    fireEvent.change(github.getByLabelText("GitHub token"), { target: { value: "draft-token" } });

    fireEvent.click(agentsMenu);
    expect(agentsMenu).toHaveAttribute("aria-current", "page");
    expect(githubMenu).not.toHaveAttribute("aria-current");
    expect(screen.queryByRole("region", { name: "GitHub 认证" })).toBeNull();
    const agents = within(screen.getByRole("region", { name: "额外 Agent" }));
    fireEvent.change(await agents.findByLabelText("id"), { target: { value: "draft-agent" } });

    fireEvent.click(githubMenu);
    expect(screen.getByRole("region", { name: "GitHub 认证" })).toBeVisible();
    expect(github.getByLabelText("GitHub token")).toHaveValue("draft-token");
    expect(screen.queryByRole("region", { name: "额外 Agent" })).toBeNull();
    fireEvent.click(agentsMenu);
    expect(agents.getByLabelText("id")).toHaveValue("draft-agent");
  });
});

describe("GitHub authentication settings", () => {
  it("opens a settings page with anonymous status and a password field", async () => {
    createFakeBackend().install();
    renderApp();
    await openSettings();
    expect(await screen.findByText("当前认证来源：匿名请求")).toBeInTheDocument();
    expect(screen.getByLabelText("GitHub token")).toHaveAttribute("type", "password");
    expect(screen.getByLabelText("GitHub token")).toHaveValue("");
    expect(screen.getByRole("button", { name: "保存" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "删除已保存 token" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "测试当前认证" }));
    expect(await screen.findByText(/匿名连接成功/)).toBeInTheDocument();
    expect(screen.getByText("剩余额度：59 / 60")).toBeInTheDocument();
  });

  it("saves without retaining the token in queries, mutations, or fake call history", async () => {
    let submitted = "";
    const backend = createFakeBackend({ commands: {
      save_github_token: (args, backend) => {
        submitted = args.token;
        return backend.dispatch("save_github_token", args);
      },
    } }).install();
    const { queryClient } = renderApp();
    await openSettings();
    await screen.findByText("当前认证来源：匿名请求");
    fireEvent.change(screen.getByLabelText("GitHub token"), { target: { value: "dummy-ui-secret" } });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    expect(await screen.findByText("Token 已保存，后续请求立即生效。")).toBeInTheDocument();
    expect(submitted).toBe("dummy-ui-secret");
    expect(screen.getByLabelText("GitHub token")).toHaveValue("");
    expect(screen.getByText("当前认证来源：设置页配置")).toBeInTheDocument();
    expect(JSON.stringify(queryClient.getQueryCache().getAll().map((query) => query.state.data))).not.toContain("dummy-ui-secret");
    expect(JSON.stringify(queryClient.getMutationCache().getAll().map((mutation) => mutation.state.variables))).not.toContain("dummy-ui-secret");
    expect(JSON.stringify(backend.calls)).not.toContain("dummy-ui-secret");
    expect(queryClient.getQueryData(githubSettingsQueryKey)).toEqual({
      hasSavedToken: true, hasEnvironmentToken: false, effectiveSource: "settings",
    });
  });

  it("uses saved configuration ahead of the environment and restores the environment on deletion", async () => {
    createFakeBackend({ data: { githubSettings: environmentSettings } }).install();
    renderApp();
    await openSettings();
    expect(await screen.findByText("当前认证来源：环境变量 GITHUB_TOKEN")).toBeInTheDocument();
    expect(screen.getByLabelText("GitHub token")).toHaveValue("");
    fireEvent.change(screen.getByLabelText("GitHub token"), { target: { value: "dummy-saved-token" } });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await screen.findByText("当前认证来源：设置页配置");
    fireEvent.click(screen.getByRole("button", { name: "测试当前认证" }));
    expect(await screen.findByText(/GitHub 认证成功 · 设置页配置/)).toBeInTheDocument();
    expect(screen.getByText("剩余额度：4,999 / 5,000")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "删除已保存 token" }));
    await screen.findByText("当前认证来源：环境变量 GITHUB_TOKEN");
    expect(screen.queryByText(/GitHub 认证成功/)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "删除已保存 token" })).toBeDisabled();
  });

  it("preserves workspace filters and discards unsaved drafts when returning", async () => {
    createFakeBackend({ data: { snapshot: sampleSnapshot() } }).install();
    renderApp();
    const search = await screen.findByRole("textbox", { name: /搜索/ });
    fireEvent.change(search, { target: { value: "browser" } });
    await openSettings();
    fireEvent.change(screen.getByLabelText("GitHub token"), { target: { value: "unsaved-dummy-token" } });
    fireEvent.click(screen.getByRole("button", { name: "返回 skills" }));
    expect(await screen.findByRole("textbox", { name: /搜索/ })).toHaveValue("browser");
    await openSettings();
    expect(screen.getByLabelText("GitHub token")).toHaveValue("");
  });

  it("is available with a broken environment or lock and permits clearing corrupt settings", async () => {
    let corrupt = true;
    const backend = createFakeBackend({
      data: { environment: cliMissingReport(), githubSettings: environmentSettings },
      commands: {
        get_inventory: () => { throw { code: "lock.corrupt", message: "Lock 读取失败", retryable: false }; },
        get_github_settings: (_, backend) => {
          if (corrupt) throw { code: "settings.read_failed", message: "无法读取 GitHub 认证配置。", retryable: false };
          return backend.dispatch("get_github_settings", {});
        },
        clear_github_token: (_, backend) => {
          corrupt = false;
          return backend.dispatch("clear_github_token", {});
        },
      },
    }).install();
    renderApp();
    await openSettings();
    const github = within(screen.getByRole("region", { name: "GitHub 认证" }));
    expect(await github.findByRole("alert")).toHaveTextContent("无法读取 GitHub 认证配置。");
    expect(screen.getByRole("button", { name: "删除已保存 token" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "删除已保存 token" }));
    expect(await screen.findByText("当前认证来源：环境变量 GITHUB_TOKEN")).toBeInTheDocument();
    expect(github.queryByRole("alert")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "额外 Agent" }));
    expect(await within(screen.getByRole("region", { name: "额外 Agent" })).findByRole("alert")).toHaveTextContent("Lock 读取失败");
    expect(backend.count(API_COMMANDS.clearGitHubToken)).toBe(1);
  });

  it("keeps the previous source and draft after a failed save", async () => {
    createFakeBackend({ data: { githubSettings: environmentSettings }, commands: {
      save_github_token: () => { throw { code: "settings.write_failed", message: "无法保存 GitHub 认证配置，原配置已保留。", retryable: true }; },
    } }).install();
    renderApp();
    await openSettings();
    await screen.findByText("当前认证来源：环境变量 GITHUB_TOKEN");
    fireEvent.change(screen.getByLabelText("GitHub token"), { target: { value: "dummy-draft-token" } });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("原配置已保留");
    expect(screen.getByText("当前认证来源：环境变量 GITHUB_TOKEN")).toBeInTheDocument();
    expect(screen.getByLabelText("GitHub token")).toHaveValue("dummy-draft-token");
    expect(screen.getByRole("button", { name: "保存" })).toBeEnabled();
  });

  it("reports invalid authentication without displaying a success or retrying", async () => {
    const backend = createFakeBackend({ commands: {
      test_github_auth: () => { throw { code: "network.auth_failed", message: "GitHub token 无效或已失效。", hint: "请替换 token。", retryable: false }; },
    } }).install();
    renderApp();
    await openSettings();
    await screen.findByText("当前认证来源：匿名请求");
    fireEvent.click(screen.getByRole("button", { name: "测试当前认证" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("GitHub token 无效或已失效。");
    expect(screen.queryByText(/认证成功/)).not.toBeInTheDocument();
    expect(backend.count(API_COMMANDS.testGitHubAuth)).toBe(1);
  });

  it("does not publish a stale auth test when the page is left and reopened", async () => {
    let complete!: (result: { source: "anonymous"; limit: number; remaining: number; resetAtEpochMs: number }) => void;
    const result = new Promise<Parameters<typeof complete>[0]>((resolve) => { complete = resolve; });
    createFakeBackend({ commands: { test_github_auth: () => result } }).install();
    renderApp();
    await openSettings();
    await screen.findByText("当前认证来源：匿名请求");
    fireEvent.click(screen.getByRole("button", { name: "测试当前认证" }));
    fireEvent.click(screen.getByRole("button", { name: "返回 skills" }));
    await openSettings();
    complete({ source: "anonymous", limit: 60, remaining: 59, resetAtEpochMs: 100_000 });
    await waitFor(() => expect(screen.getByRole("button", { name: "测试当前认证" })).toBeEnabled());
    expect(screen.queryByText(/连接成功/)).not.toBeInTheDocument();
  });
});
