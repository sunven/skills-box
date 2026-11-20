import type { ProjectEntry } from "../lib/types";
import {
  pluginActionLabels,
  pluginContextLabel,
  globalPluginContext,
  type PluginContext,
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
      context: {
        ...globalPluginContext,
        projectRoot: null,
        projectName: null,
        revision: null,
      },
      configRoot: `/Users/demo/.${host}`,
      executable: `/usr/local/bin/${host}`,
      version: host === "codex" ? "0.170.0" : "2.1.325",
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
          declarations: ["全局"],
          declaredInTarget: true,
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
          selectedEnabled: false,
          installedInTarget: true,
          enabledSource: "全局",
          enabledOverridden: false,
          actionBlocks: {},
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
          selectedEnabled: null,
          installedInTarget: false,
          enabledSource: null,
          enabledOverridden: false,
          actionBlocks: {},
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
export function nativePluginFake(projects: () => ProjectEntry[] = () => []) {
  const states = {
    codex: nativePluginFixture("codex"),
    claude: nativePluginFixture("claude"),
  };
  const scoped = new Map<string, PluginPageState>();
  function stateFor(
    host: PluginHost,
    context: PluginContext = globalPluginContext,
  ): PluginPageState {
    if (context.scope === "global") return states[host];
    const project = projects().find((p) => `project:${p.id}` === context.scope);
    if (!project?.available) throw new Error("项目不可用，请重新定位或遗忘");
    const key = `${host}:${context.scope}:${context.layer}:${project.root}:${project.checkRevision}`;
    let state = scoped.get(key);
    if (!state) {
      state = nativePluginFixture(host);
      const snapshot = state.snapshot!;
      snapshot.context = {
        ...context,
        projectRoot: project.root,
        projectName: project.displayName,
        revision: project.checkRevision ?? "initial",
      };
      for (const p of snapshot.plugins) {
        p.installedInTarget = false;
        p.selectedEnabled = null;
        p.actionBlocks = {
          uninstall: "请切换到安装所属层操作",
          update: "请切换到安装所属层操作",
        };
      }
      snapshot.markets[0]!.declaredInTarget = false;
      snapshot.markets[0]!.readOnlyReason = "继承的全局市场";
      if (host === "codex")
        for (const c of snapshot.capabilities)
          c.blockedReason =
            "Codex 官方接口目前仅允许写用户配置，项目配置只读。";
      scoped.set(key, state);
    }
    for (const plugin of state.snapshot!.plugins) {
      const user = states[host].snapshot!.plugins.find(
        (p) => p.id === plugin.id,
      );
      plugin.userEnabled = user?.selectedEnabled ?? null;
      const layers = [...scoped.values()].filter(
        (s) =>
          s.snapshot!.host === host &&
          s.snapshot!.context.projectRoot === project.root,
      );
      const shared = layers.find(
        (s) => s.snapshot!.context.layer === "project",
      )?.snapshot;
      const local = layers.find(
        (s) => s.snapshot!.context.layer === "local",
      )?.snapshot;
      const winner = [local, shared, states[host].snapshot].find(
        (s) =>
          s?.plugins.find((p) => p.id === plugin.id)?.selectedEnabled != null,
      );
      plugin.effectiveEnabled =
        winner?.plugins.find((p) => p.id === plugin.id)?.selectedEnabled ??
        null;
      plugin.enabledSource = winner ? pluginContextLabel(winner.context) : null;
      plugin.enabledOverridden =
        plugin.selectedEnabled !== null &&
        !!winner &&
        winner.context.layer !== context.layer;
    }
    return state;
  }
  const previews = new Map<string, PluginPreview>();
  let sequence = 0;
  return {
    read_native_plugins: ({
      host,
      context,
    }: {
      host: PluginHost;
      context?: PluginContext;
    }) => structuredClone(stateFor(host, context)),
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
      const state = stateFor(request.host, request.context);
      const snapshot = state.snapshot!;
      const blocked = snapshot.capabilities.find(
        (c) => c.action === request.action,
      )?.blockedReason;
      if (blocked) throw new Error(blocked);
      const market = snapshot.markets.find((m) => m.name === request.target);
      if (request.action === "remove_market" && market?.removeBlockedReason)
        throw new Error(market.removeBlockedReason);
      const preview: PluginPreview = {
        token: `native-${++sequence}`,
        request,
        context: structuredClone(snapshot.context),
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
                : [
                    `操作目标：${pluginContextLabel(snapshot.context)}。开始写入后正常退出会等待操作完成。`,
                  ],
        command: `${request.host} plugin ${request.action} ${request.target || request.source}${request.host === "claude" ? ` --scope ${request.context.layer}` : ""} --json`,
      };
      previews.set(preview.token, preview);
      return preview;
    },
    execute_native_plugin: ({ token }: { token: string }): PluginOutcome => {
      const preview = previews.get(token);
      if (!preview) throw new Error("确认已失效");
      previews.delete(token);
      const state = stateFor(preview.request.host, preview.request.context);
      if (
        JSON.stringify(state.snapshot!.context) !==
        JSON.stringify(preview.context)
      )
        throw new Error("项目已变化，请重新预览");
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
          p.selectedEnabled = true;
          if (preview.context.scope === "global") p.userEnabled = true;
          if (action === "install" || action === "reinstall")
            p.installedInTarget = true;
          p.effectiveEnabled = true;
        }
        if (action === "disable") {
          p.selectedEnabled = false;
          if (preview.context.scope === "global") p.userEnabled = false;
          p.effectiveEnabled = false;
        }
        if (action === "uninstall") {
          p.installationState = "absent";
          p.selectedEnabled = null;
          p.installedInTarget = false;
          if (preview.context.scope === "global") p.userEnabled = null;
          p.installations = [];
        }
      }
      if (action === "remove_market")
        snapshot.markets = snapshot.markets.filter(
          (m) => m.name !== preview.request.target,
        );
      snapshot.fingerprint = `fixture-${++sequence}`;
      if (p?.installedInTarget) p.actionBlocks = {};
      const outcome: PluginOutcome = {
        status: "succeeded",
        message: "原生命令完成，所选配置层设置和原生记录已核对。",
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
