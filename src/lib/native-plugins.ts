export type PluginHost = "codex" | "claude";
export type PluginAction =
  | "add_market"
  | "sync_market"
  | "remove_market"
  | "install"
  | "enable"
  | "disable"
  | "update"
  | "reinstall"
  | "uninstall";
export const pluginActionLabels: Record<PluginAction, string> = {
  add_market: "添加市场",
  sync_market: "同步市场",
  remove_market: "移除市场",
  install: "安装",
  enable: "启用",
  disable: "禁用",
  update: "更新",
  reinstall: "重新安装",
  uninstall: "卸载",
};
export const pluginHostLabels: Record<PluginHost, string> = {
  codex: "Codex",
  claude: "Claude Code",
};
export interface PluginRequest {
  host: PluginHost;
  action: PluginAction;
  target: string;
  source: string;
  sourceKind: string;
  reference: string;
  sparsePaths: string[];
}
export interface PluginCapability {
  action: PluginAction;
  blockedReason: string | null;
}
export interface NativeMarket {
  name: string;
  sourceKind: string;
  source: string;
  root: string | null;
  reference: string | null;
  sparsePaths: string[];
  readOnlyReason: string | null;
  removeBlockedReason: string | null;
  syncBlockedReason: string | null;
  catalogError: string | null;
  affectedPlugins: string[];
}
export interface NativeInstallation {
  scope: string;
  version: string | null;
  path: string | null;
  projectPath: string | null;
}
export interface NativePlugin {
  id: string;
  name: string;
  marketplace: string;
  description: string | null;
  version: string | null;
  installationState: "installed" | "absent" | "unconfirmed";
  installations: NativeInstallation[];
  userEnabled: boolean | null;
  effectiveEnabled: boolean | null;
  source: unknown;
  localRoot: string | null;
  readOnlyReason: string | null;
  installBlockedReason: string | null;
}
export interface PluginSnapshot {
  host: PluginHost;
  configRoot: string;
  executable: string;
  version: string;
  capabilities: PluginCapability[];
  markets: NativeMarket[];
  plugins: NativePlugin[];
  warnings: string[];
  fingerprint: string;
  sourceKinds: string[];
}
export interface PluginPreview {
  token: string;
  request: PluginRequest;
  configRoot: string;
  version: string;
  command: string;
  consequences: string[];
  affectedPlugins: string[];
  fingerprint: string;
}
export interface PluginOutcome {
  status:
    | "succeeded"
    | "conflict"
    | "partial"
    | "failed"
    | "needs_host"
    | "unconfirmed";
  message: string;
  executed: boolean;
  commandSucceeded: boolean | null;
  verification: string;
  snapshot: PluginSnapshot | null;
  preview: PluginPreview;
}
export interface PluginOperationRecord {
  phase: "writing" | "verifying" | "finished" | "interrupted";
  preview: PluginPreview;
  outcome: PluginOutcome | null;
  message: string | null;
}
export interface PluginPageState {
  snapshot: PluginSnapshot | null;
  error: string | null;
  operation: PluginOperationRecord | null;
}
export interface PluginDetails {
  readme: string | null;
  manifest: Record<string, unknown>;
  components: Record<string, string[]>;
  messages: string[];
}
export const pluginQueryKey = (host: PluginHost) =>
  ["native-plugins", host] as const;
export const pluginRequest = (
  host: PluginHost,
  action: PluginAction,
  target = "",
): PluginRequest => ({
  host,
  action,
  target,
  source: "",
  sourceKind: "local",
  reference: "",
  sparsePaths: [],
});
export const pluginOutcomeLabels: Record<PluginOutcome["status"], string> = {
  succeeded: "已完成并核对",
  conflict: "确认已失效",
  partial: "部分完成",
  failed: "执行失败",
  needs_host: "需要宿主处理",
  unconfirmed: "结果待核对",
};
