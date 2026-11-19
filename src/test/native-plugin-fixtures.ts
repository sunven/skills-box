import {
  pluginActionLabels,
  type PluginHost,
  type PluginPageState,
  type PluginPreview,
  type PluginRequest,
  type PluginOutcome,
  type PluginDetails,
  type PluginAction,
} from "../lib/native-plugins";
export function nativePluginFixture(
  host: PluginHost = "codex",
): PluginPageState {
  return {
    error: null,
    operation: null,
    snapshot: {
      host,
      configRoot: `/Users/demo/.${host}`,
      executable: `/usr/local/bin/${host}`,
      version: host === "codex" ? "0.170.0" : "2.1.323",
      fingerprint: "fixture-1",
      sourceKinds:
        host === "codex" ? ["local", "git"] : ["local", "git", "url"],
      capabilities: (Object.keys(pluginActionLabels) as PluginAction[])
        .filter((a) => a !== (host === "codex" ? "update" : "reinstall"))
        .map((action) => ({ action, blockedReason: null })),
      warnings: [],
      markets: [
        {
          name: "team-plugins",
          sourceKind: "git",
          source: "https://github.com/example/team-plugins",
          root: "/Users/demo/.plugins/team-plugins",
          reference: "main",
          sparsePaths: [],
          readOnlyReason: null,
          syncBlockedReason: null,
          removeBlockedReason: null,
          catalogError: null,
          affectedPlugins: ["workspace-tools@team-plugins"],
        },
      ],
      plugins: [
        {
          id: "workspace-tools@team-plugins",
          name: "workspace-tools",
          marketplace: "team-plugins",
          description:
            "项目导航与团队工作流，包含可复用的 Skills 和只读组件信息。",
          version: "1.2.0",
          installationState: "installed",
          installations: [
            {
              scope: "user",
              version: "1.2.0",
              path: "/Users/demo/.plugins/workspace-tools",
              projectPath: null,
            },
          ],
          userEnabled: false,
          effectiveEnabled: false,
          source: "./workspace-tools",
          localRoot: "/Users/demo/.plugins/workspace-tools",
          readOnlyReason: null,
          installBlockedReason: null,
        },
        {
          id: "review-notes@team-plugins",
          name: "review-notes",
          marketplace: "team-plugins",
          description: "整理代码审查记录与后续检查事项。",
          version: null,
          installationState: "absent",
          installations: [],
          userEnabled: null,
          effectiveEnabled: null,
          source: "./review-notes",
          localRoot: null,
          readOnlyReason: null,
          installBlockedReason: null,
        },
      ],
    },
  };
}
export function nativePluginFake() {
  const states = {
    codex: nativePluginFixture("codex"),
    claude: nativePluginFixture("claude"),
  };
  const previews = new Map<string, PluginPreview>();
  let sequence = 0;
  return {
    read_native_plugins: ({ host }: { host: PluginHost }) =>
      structuredClone(states[host]),
    verify_native_plugin_host: ({ host }: { host: PluginHost }) =>
      structuredClone(states[host]),
    native_plugin_details: (): PluginDetails => ({
      readme: "# Workspace tools\n\n团队插件的本地说明。",
      manifest: { name: "workspace-tools", version: "1.2.0" },
      components: {
        Skills: ["project-map", "review-guide"],
        Agents: [],
        Hooks: [],
        MCP: [],
      },
      messages: ["组件声明不代表已经加载。"],
    }),
    prepare_native_plugin: ({
      request,
    }: {
      request: PluginRequest;
    }): PluginPreview => {
      const state = states[request.host];
      const snapshot = state.snapshot!;
      const market = snapshot.markets.find((m) => m.name === request.target);
      if (request.action === "remove_market" && market?.removeBlockedReason)
        throw new Error(market.removeBlockedReason);
      const preview: PluginPreview = {
        token: `native-${++sequence}`,
        request,
        configRoot: snapshot.configRoot,
        version: snapshot.version,
        fingerprint: snapshot.fingerprint,
        affectedPlugins: market?.affectedPlugins ?? [],
        consequences:
          request.action === "reinstall"
            ? [
                "Codex 会重新启用此插件，包括原先禁用的插件；重新安装不保证取得最新版。",
              ]
            : request.action === "sync_market"
              ? ["同步是写操作，可能更新禁用插件的文件。"]
              : request.action === "remove_market"
                ? ["移除会影响所列插件及宿主保存的数据，没有自动回滚。"]
                : ["操作仅指定用户级设置，开始写入后正常退出会等待操作完成。"],
        command: `${request.host} plugin ${request.action} ${request.target || request.source} --json`,
      };
      previews.set(preview.token, preview);
      return preview;
    },
    execute_native_plugin: ({ token }: { token: string }): PluginOutcome => {
      const preview = previews.get(token);
      if (!preview) throw new Error("确认已失效");
      previews.delete(token);
      const state = states[preview.request.host];
      const snapshot = state.snapshot!;
      const p = snapshot.plugins.find((p) => p.id === preview.request.target);
      const action = preview.request.action;
      if (p) {
        if (
          action === "enable" ||
          action === "reinstall" ||
          action === "install"
        ) {
          p.installationState = "installed";
          p.userEnabled = true;
          p.effectiveEnabled = true;
        }
        if (action === "disable") {
          p.userEnabled = false;
          p.effectiveEnabled = false;
        }
        if (action === "uninstall") {
          p.installationState = "absent";
          p.userEnabled = null;
          p.installations = [];
        }
      }
      if (action === "remove_market")
        snapshot.markets = snapshot.markets.filter(
          (m) => m.name !== preview.request.target,
        );
      snapshot.fingerprint = `fixture-${++sequence}`;
      const outcome: PluginOutcome = {
        status: "succeeded",
        message: "原生命令完成，用户设置和原生记录已核对。",
        executed: true,
        commandSucceeded: true,
        verification: "verified",
        snapshot: structuredClone(snapshot),
        preview,
      };
      state.operation = { phase: "finished", preview, outcome, message: null };
      return structuredClone(outcome);
    },
  };
}
