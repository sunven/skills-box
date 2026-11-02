import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ExternalLink, KeyRound, Settings2 } from "lucide-react";
import {
  asAppError, clearGitHubToken, fallbackError, getGitHubSettings,
  openExternalUrl, saveGitHubToken, testGitHubAuth,
} from "../lib/api";
import type { AppError, GitHubAuthTestResult, GitHubTokenSource } from "../lib/types";
import { formatEpochDateTime } from "../lib/update-view";
import { GLOBAL_SCOPE } from "../lib/types";
import { useInventoryQuery } from "../hooks/use-inventory";
import { ExtraAgentSettings } from "./extra-agent-settings";
import { Button } from "./ui/button";
import { Input } from "./ui/input";

export const githubSettingsQueryKey = ["github-settings"] as const;
const sourceLabels: Record<GitHubTokenSource, string> = {
  settings: "设置页配置",
  environment: "环境变量 GITHUB_TOKEN",
  anonymous: "匿名请求",
};

export function SettingsPage({ onAgentsChanged }: { onAgentsChanged: () => void }) {
  const [activeSection, setActiveSection] = useState<"github" | "agents">("github");
  const panelRef = useRef<HTMLDivElement>(null);
  const queryClient = useQueryClient();
  const settings = useQuery({ queryKey: githubSettingsQueryKey, queryFn: getGitHubSettings });
  const inventory = useInventoryQuery(GLOBAL_SCOPE, activeSection === "agents");
  // Keep drafts outside React Query: mutation variables would retain a token.
  const [token, setToken] = useState("");
  const [pending, setPending] = useState<"save" | "clear" | "test" | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [test, setTest] = useState<GitHubAuthTestResult | null>(null);
  const generation = useRef(0);
  useEffect(() => () => { generation.current += 1; }, []);

  async function changeToken(action: "save" | "clear") {
    const run = ++generation.current;
    setPending(action);
    setError(null);
    setNotice(null);
    setTest(null);
    try {
      const status = await (action === "save" ? saveGitHubToken(token) : clearGitHubToken());
      queryClient.setQueryData(githubSettingsQueryKey, status);
      if (generation.current !== run) return;
      setToken("");
      setNotice(action === "save" ? "Token 已保存，后续请求立即生效。" : "已删除保存的 token。后续请求使用环境变量或匿名访问。");
    } catch (failure) {
      if (generation.current === run) setError(asAppError(failure) ?? fallbackError("无法保存 GitHub 认证配置。"));
    } finally {
      if (generation.current === run) setPending(null);
    }
  }

  async function testAuth() {
    const run = ++generation.current;
    setPending("test");
    setError(null);
    setNotice(null);
    setTest(null);
    try {
      const result = await testGitHubAuth();
      if (generation.current === run) setTest(result);
    } catch (failure) {
      if (generation.current === run) setError(asAppError(failure) ?? fallbackError("无法测试 GitHub 认证。"));
    } finally {
      if (generation.current === run) setPending(null);
    }
  }

  const readError = settings.isError
    ? asAppError(settings.error) ?? fallbackError("无法读取 GitHub 认证配置。请重新保存或删除配置。")
    : null;
  const displayedError = error ?? readError;
  const status = settings.data;
  const inventoryError = inventory.isError
    ? asAppError(inventory.error) ?? fallbackError("无法读取额外 Agent 配置。")
    : null;

  function handleAgentsChanged() {
    void inventory.refetch();
    onAgentsChanged();
  }

  function selectSection(section: "github" | "agents") {
    setActiveSection(section);
    if (panelRef.current) panelRef.current.scrollTop = 0;
  }

  return (
    <main className="settings-page" aria-labelledby="settings-title">
      <nav className="settings-menu" aria-label="设置菜单">
        <p className="settings-menu-title">设置</p>
        <button type="button" aria-current={activeSection === "github" ? "page" : undefined}
          aria-controls="github-settings" onClick={() => selectSection("github")}>
          <KeyRound className="size-4" aria-hidden="true" />GitHub 认证
        </button>
        <button type="button" aria-current={activeSection === "agents" ? "page" : undefined}
          aria-controls="extra-agent-settings" onClick={() => selectSection("agents")}>
          <Settings2 className="size-4" aria-hidden="true" />额外 Agent
        </button>
      </nav>
      <div ref={panelRef} className="settings-panel">
        <div className="settings-content">
          <h1 id="settings-title">设置</h1>
          <p className="text-muted-foreground">配置 GitHub 认证和额外 Agent，适用于全局和所有项目。</p>
          <section id="github-settings" className="settings-card" aria-labelledby="github-settings-title" hidden={activeSection !== "github"}>
            <div className="flex items-center gap-2">
              <KeyRound className="size-4 text-muted-foreground" aria-hidden="true" />
              <h2 id="github-settings-title">GitHub 认证</h2>
            </div>
            <p className="text-muted-foreground">使用 token 提高检查更新的 API 额度。访问公开仓库无需授予私有仓库权限。</p>
            <div className="settings-source" role="status">
              {settings.isPending ? "正在读取配置…" : status ? `当前认证来源：${sourceLabels[status.effectiveSource]}` : "认证配置读取失败"}
              {status?.hasEnvironmentToken ? <p className="mt-1 text-muted-foreground">已检测到 GITHUB_TOKEN。设置页配置优先，删除后使用环境变量。</p> : null}
            </div>
            <form onSubmit={(event) => { event.preventDefault(); if (!pending && token.trim()) void changeToken("save"); }}>
              <label htmlFor="github-token" className="mb-2 block font-medium">GitHub token</label>
              <Input id="github-token" type="password" autoComplete="off" spellCheck={false}
                value={token} disabled={pending !== null}
                placeholder={status?.hasSavedToken ? "已保存 token；输入新 token 替换" : "输入 GitHub personal access token"}
                onChange={(event) => { setToken(event.target.value); setNotice(null); }}
                aria-describedby="github-token-help" />
              <p id="github-token-help" className="mt-2 text-[12px] text-muted-foreground">Token 以明文保存在本机应用配置文件中，仅当前用户可读写。已保存的值不会回显。</p>
              <div className="mt-4 flex flex-wrap gap-2">
                <Button type="submit" disabled={!token.trim() || pending !== null} aria-busy={pending === "save"}>{pending === "save" ? "保存中…" : "保存"}</Button>
                <Button type="button" variant="outline" disabled={pending !== null || (!status?.hasSavedToken && !settings.isError)}
                  aria-busy={pending === "clear"} onClick={() => void changeToken("clear")}>删除已保存 token</Button>
                <Button type="button" variant="ghost" onClick={() => {
                  void openExternalUrl("https://github.com/settings/personal-access-tokens/new").catch((failure) => {
                    setError(asAppError(failure) ?? fallbackError("无法打开 token 创建页面。"));
                  });
                }}>创建 token <ExternalLink aria-hidden="true" /></Button>
              </div>
            </form>
            <div className="settings-test">
              <Button variant="outline" disabled={pending !== null || !status || settings.isError}
                aria-busy={pending === "test"} onClick={() => void testAuth()}>{pending === "test" ? "测试中…" : "测试当前认证"}</Button>
              <p className="mt-2 text-[12px] text-muted-foreground">测试当前生效的配置；输入新 token 后请先保存。</p>
              {test ? <div className="notice mt-3" role="status" data-tone="positive">
                <p>{test.source === "anonymous" ? "匿名连接成功" : "GitHub 认证成功"} · {sourceLabels[test.source]}</p>
                <p className="mt-1 num">剩余额度：{test.remaining.toLocaleString()} / {test.limit.toLocaleString()}</p>
                <p className="mt-1">额度恢复时间：{formatEpochDateTime(test.resetAtEpochMs)}</p>
              </div> : null}
            </div>
            {notice ? <p className="notice" role="status">{notice}</p> : null}
            {displayedError ? <div className="notice" data-tone="negative" role="alert">
              <p>{displayedError.message}</p>
              {displayedError.hint ? <p className="mt-1">{displayedError.hint}</p> : null}
            </div> : null}
            <p className="text-[12px] text-muted-foreground">环境变量仅读取应用启动时继承的 GITHUB_TOKEN，修改后需重启应用。从 Finder 启动时推荐在此配置 token。</p>
          </section>
          <section id="extra-agent-settings" className="settings-card" aria-labelledby="extra-agent-settings-title" hidden={activeSection !== "agents"}>
            <div className="flex items-center gap-2">
              <Settings2 className="size-4 text-muted-foreground" aria-hidden="true" />
              <h2 id="extra-agent-settings-title">额外 Agent</h2>
            </div>
            <p className="text-muted-foreground">添加内置列表之外的 Agent，用于 Skill 的安装与挂载。内置定义只读，自定义定义可编辑或删除。</p>
            {inventoryError ? <div className="notice" data-tone="negative" role="alert">
              <p>{inventoryError.message}</p>
              {inventoryError.hint ? <p className="mt-1">{inventoryError.hint}</p> : null}
              <Button className="mt-3" variant="outline" disabled={inventory.isFetching}
                onClick={() => void inventory.refetch()}>重新读取额外 Agent</Button>
            </div> : inventory.data ? <ExtraAgentSettings
              agents={inventory.data.agents}
              skills={inventory.data.skills}
              warnings={inventory.data.warnings}
              onChanged={handleAgentsChanged}
            /> : <p className="text-muted-foreground" role="status">正在读取额外 Agent…</p>}
          </section>
        </div>
      </div>
    </main>
  );
}
