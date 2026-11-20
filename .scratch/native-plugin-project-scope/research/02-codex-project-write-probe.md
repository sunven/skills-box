# Codex 项目写入接口隔离核查

日期：2026-11-20。仅用于设计访谈的能力事实，不是完整兼容性验收，也不修改产品实现或既有 ADR。

## 结论

本机 `/opt/homebrew/bin/codex` 报告 `codex-cli 0.170.0`。官方配置协议能读取项目配置层，但 `config/value/write` 和 `config/batchWrite` 明确拒绝写项目 `.codex/config.toml`。因此，在 ADR-0038 的“仅官方接口、禁止直接编辑兜底”约束下，这个版本没有已证明可用的项目 Plugin 启用/禁用写入路径。

CLI 的进程 cwd 影响项目配置读取和相对市场路径解析，但不把市场写入目标变成项目层：从项目 cwd 执行 `plugin marketplace add` 仍写用户配置；删除仅由项目配置声明的市场会被拒绝。

未找到原生“仅清除当前项目 Plugin enabled 覆盖、恢复继承”接口；不得把 `value: null` 或替换整个父表推定为受支持的 reset。此次未执行这类猜测性写入。

## 隔离边界与证据位置

隔离目录（规范化路径）：

```text
/private/var/folders/38/s1qf24m11tn897xgx9khf2gm0000gn/T/skills-box-codex-project-probe-3c1updjf
```

Python `subprocess` 显式传入独立 `env` 字典：`HOME=<root>/home`、`CODEX_HOME=<root>/codex-home`、独立 XDG 配置/缓存目录和 `TMPDIR`，只沿用系统 `PATH`。没有覆盖当前 shell 环境变量，没有读取或写入真实用户配置/安装。

临时项目为 `<root>/project`，含 `.git` 标记目录和 `.codex/config.toml`。项目 trust 声明、测试配置和空市场 catalog 均是临时 fixture；它们不是产品直接编辑配置的兜底。没有执行 `plugin add`、`plugin remove` 或市场 upgrade，没有下载或安装 Plugin。app-server 自行在隔离 CODEX_HOME 初始化数据库和内置系统 Skills。

原始证据保留在该目录：

- `probe-env.json`：独立环境与路径。
- `help-*.txt`：本机命令帮助与版本输出。
- `schema/`、`schema-experimental/`：本机 CLI 官方协议生成器输出。
- `rpc-canonical-transcript.json`、`rpc-canonical-stderr.txt`：规范化路径、受信任项目的 RPC 实验。
- `marketplace-transcript.json`：市场 add 写用户配置的前后内容；也保留第一次 fixture 缺少 `source_type` 的失败，不作为成功列表证据。
- `marketplace-list-corrected.json`、`marketplace-remove-corrected.json`：修正为 `source_type="local"` 后的列表与删除结果。
- `probe.py`、`probe-canonical.py`、`probe-marketplace.py`：最小实验脚本。
- `app-server.md`、`cli.md`、`plugins.md`：通过只读 HTTPS 获取的官方正文。

首次实验使用 macOS `/var` 别名，启动告警提示实际信任根应为 `/private/var`。最终证据已用规范化路径重试，正确配置 trust，且没有该告警；项目写入失败并非路径或信任问题。

## 官方配置写入实验

先运行：

```text
codex app-server generate-json-schema --out <root>/schema
codex app-server generate-json-schema --experimental --out <root>/schema-experimental
codex app-server
```

初始化后调用 `config/read`，参数为 `{"includeLayers":true,"cwd":"<root>/project"}`。返回项目 `plugins.nonexistent@scope-probe.enabled=true`，origin 为 `type="project"`，并包含项目层 `version`。

将该版本作为 `expectedVersion`，发送：

```json
{
  "method": "config/value/write",
  "params": {
    "keyPath": "plugins.\"nonexistent@scope-probe\".enabled",
    "value": false,
    "mergeStrategy": "replace",
    "filePath": "<root>/project/.codex/config.toml",
    "expectedVersion": "sha256:cdd5c839df9feaeebeb9e6ea27c410de408f15940bf89caefa7d0ffd68588b82"
  }
}
```

原生响应：

```json
{
  "error": {
    "code": -32600,
    "data": {"config_write_error_code": "configLayerReadonly"},
    "message": "Only writes to the user config are allowed"
  }
}
```

同一项目路径、版本和 edit 使用 `config/batchWrite` 返回同样错误。再次 `config/read` 和读取隔离 fixture 文件确认 enabled 仍为 true，项目与用户文件均未被这些写入请求改变。

生成的 `ConfigValueWriteParams` / `ConfigBatchWriteParams` 虽有可选 `filePath`，并描述省略时默认用户配置，但未承诺项目层可写。`ConfigReadParams.cwd` 则明确描述用于解析项目配置层；读能力不能推导写能力。

## 市场 CLI 的 cwd 与目标

空 catalog 仅含 `name` 和 `plugins: []`，没有插件内容。

| 操作 | 本机观测 |
|---|---|
| 在项目 cwd 运行 `codex plugin marketplace list --json` | 可列出项目配置声明 `project-declared`，以及用户声明的市场。 |
| 在项目外 cwd 运行同一 list | 不列出 `project-declared`，仍列出用户市场。 |
| 在项目 cwd 运行 `codex plugin marketplace add ./added-catalog --json` | 相对路径从项目 cwd 解析；返回成功，新增 `[marketplaces.added-from-project]` 写入 `<CODEX_HOME>/config.toml`，项目配置未变。 |
| 在项目 cwd 运行 `codex plugin marketplace remove project-declared --json` | exit 1，提示该市场配置于 project 的 `.codex/config.toml`，要求从那个配置来源移除；项目文件未变。 |

删除原文核心信息：`marketplace ... is configured in project (.../.codex/config.toml); remove it from that configuration source instead`。

本机 plugin add/remove、marketplace add/remove/list 帮助没有 `--scope` 或项目写入目标参数。协议 `PluginInstallParams` 有 pluginName、marketplacePath、remoteMarketplaceName 等字段；`PluginUninstallParams` 仅有 pluginId，均未提供项目配置目标。`PluginListParams.cwds` 和实验性 `PluginInstalledParams.cwds` 用于发现 repo marketplace。

这些证据不等于安装、重装、卸载、市场同步的全部副作用已验收。遵守本次实验边界，没有安装 Plugin，也未执行同步；这些动作是否影响共享缓存、其他项目及配置层仍需独立验收。仅凭设置 cwd 不能把它们标为项目独立管理。

## 恢复继承

标准和 `--experimental` 生成协议的配置方法包含 `config/read`、`config/value/write`、`config/batchWrite` 和 `config/mcpServer/reload`，没有 config reset/remove 方法；`MergeStrategy` 只有 replace/upsert。Plugin 方法也未提供 enabled/reset 写入。

即使某个值删除技巧未来可用于用户配置，当前项目层写入已被接口明确拒绝。因此项目“恢复继承”应记录为当前官方接口不可用，不能伪装为写入相同的继承值；用户层 null/delete 的语义此次未验证。

## 官方文档与限制

本轮调用 web 搜索仍返回空结果，无可引用结果编号；以只读 HTTPS 获取以下官方文档补充核对，来源正文保留于隔离目录：

- `https://learn.chatgpt.com/docs/app-server.md`：仅承诺 config/value/write 与 config/batchWrite 写用户 config.toml。
- `https://learn.chatgpt.com/docs/cli/reference.md`：plugin 与 marketplace 命令参数，没有 scope 参数。
- `https://developers.openai.com/plugins/build/plugins.md`：项目 config 可控制 enabled；项目配置要求 trust；市场刷新可能安装/更新 disabled Plugin 的文件。

本机实验解决的是 Codex 0.170.0 的项目官方写入边界，不建立版本白名单，不替代其他版本或完整原生副作用验收。
