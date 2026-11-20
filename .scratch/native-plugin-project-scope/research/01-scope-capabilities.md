# 原生插件项目能力核查

日期：2026-11-20。用于设计访谈的事实调查，不是产品决策或实现验收。仅读取仓库与公开官方文档，没有访问用户真实插件配置或执行插件写入。

## 官方文档确认的能力

### Claude Code

- `user` 使用用户配置；`project` 使用项目 `.claude/settings.json`，可随仓库提交；`local` 使用项目 `.claude/settings.local.json`，用于本人在该项目的配置。
- 一般配置层级为 local > project > user；启动参数和 managed 配置还会影响最终结果。项目共享声明不等于其他协作者已下载插件，每人仍需完成本机安装。
- 插件 install、enable、disable、update、uninstall 支持 scope 参数。
- 市场 add/remove 的 scope 指声明的位置；已获取市场记录 `known_marketplaces.json`、市场目录和插件缓存由用户共享。一个项目获取的市场可能在其他项目中可用，不能据当前项目选择宣称物理隔离。
- 市场 update 没有项目 scope 参数。官方 remove 描述为移除最后一个声明作用域时连带卸载该市场插件；多项目场景的本机准确行为仍需隔离验证。

来源：

- [安装作用域](https://code.claude.com/docs/en/plugins/install#choose-an-install-scope)
- [CLI 参数与市场移除](https://code.claude.com/docs/en/plugins/cli-reference)
- [配置加载与共享缓存](https://code.claude.com/docs/en/plugins/loading)

### Codex

- 官方文档描述 trusted-project `.codex/config.toml` 可以定义市场与插件 enabled 配置；仓库 `.agents/plugins/marketplace.json` 可提供项目发现目录。
- 官方 plugin 与 marketplace 命令表未提供 `--scope`；app-server 文档仅承诺 `config/value/write` 写用户配置。请求有 `filePath` 参数不能单独证明可以写项目配置。
- 项目插件安装、卸载、市场同步及配置写入的准确目标，需要在隔离配置和临时项目中验证。当前不能承诺与 Claude 的项目写入能力对等。

后续[隔离接口核查](02-codex-project-write-probe.md)已确认 Codex 0.170.0 拒绝项目配置写入，市场 add 在项目 cwd 仍写用户配置。这个结论替代本节关于项目配置写入的未知状态；插件安装、卸载和同步的完整副作用仍未在该最小实验中验证。

来源：

- [插件配置](https://developers.openai.com/plugins/build/plugins.md)
- [配置参考](https://learn.chatgpt.com/docs/config-file/config-reference.md)
- [CLI 命令参考](https://learn.chatgpt.com/docs/cli/reference.md)
- [App-server](https://learn.chatgpt.com/docs/app-server.md)

本轮 web 工具多次返回空结果，没有可用的引用编号；调查代理通过只读 HTTP 获取上述官方正文补充核对。以上描述仅是文档能力，不冒充本机隔离实测。

## 仓库实现与已有证据

- `src-tauri/src/native_plugins/model.rs:50`：请求没有项目或作用域字段。
- `src-tauri/src/native_plugins/transport.rs:121`：进程 cwd 固定 `/`。
- `src-tauri/src/native_plugins/operations.rs`：Claude 命令固定 `--scope user`；Codex 启停写入用户配置。
- `src-tauri/src/native_plugins/inventory.rs`：Claude 能保留非 user 安装记录及 `projectPath`，但主状态仍以 user 为准；Codex 项目扫描只用于保守阻止可能跨范围的市场移除。
- [原验收记录](../../native-plugin-management/implementation.md) 记录 Codex 0.170.0、Claude Code 2.1.323；这是既有证据，本轮没有重新获取或实测版本。
- `src-tauri/src/native_plugins/tests.rs:399` 的 Claude 隔离测试只有一处 user 市场声明，同时安装了 user/project/local 插件；移除该唯一声明会删除三层安装。它没有验证其他项目仍有市场声明时的移除行为，不能据此推导所有跨项目移除都会级联。
- Codex 项目测试仅创建临时项目配置来验证应用阻止市场移除，未证明原生项目加载或官方接口写入成功。

## 继续核查与验收

1. Codex 官方项目配置写入接口是否可用，以及安装、卸载、重装和市场操作的实际目标。
2. Claude 同时存在用户、两个项目及个人层声明时，更新、卸载、市场同步及移除的确切副作用。
3. 外部配置覆盖、信任要求、共享缓存及无法确认的运行中会话状态，如何从原生观测中准确呈现。

既有 ADR-0038 禁止官方接口失败后直接编辑配置兜底；能力未知先作为未验证项，不自动重开该决定。最终验收不使用用户真实安装。
