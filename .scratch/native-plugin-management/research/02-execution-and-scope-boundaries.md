# 原生插件执行、兼容与作用域边界

日期：2026-11-19。只读核查，未读取个人配置/凭据，未执行安装或配置变更。参考项目路径均以 `/Users/sunven/github/package-manager` 为根。

## 版本与证据限制

本机版本为 Codex `0.170.0`、Claude Code `2.1.323`；参考项目生命周期实验主要基于 Codex `0.162.1` 与 Claude Code `2.1.300`。旧版本实验只能说明当时行为，不能证明当前版本继续成立。CLI 帮助证明命令和参数存在，不证明实际副作用。

本轮 web 工具未返回可引用内容；官方资料通过只读 HTTP 获取官方文档和 OpenAI 源码补充核查。下列明确区分通用官方契约、参考实现和当前版本待验证项。

## Codex 官方配置写入

- 本机 `plugin --help` 没有 enable/disable 子命令，但官方 App Server 提供 `config/value/write`。参数包括 `keyPath`、`value`、`mergeStrategy`、`filePath`、`expectedVersion`，结果区分 `ok` 与 `okOverridden`。这不是实验性的 plugin/install 接口，也不是名为 config/write 的方法。
- 官方来源：`https://developers.openai.com/codex/app-server/`；协议定义：`https://github.com/openai/codex/blob/main/codex-rs/app-server-protocol/src/protocol/v2/config.rs`。
- 参考项目已有实际调用：`src-tauri/src/codex_hooks.rs:146` 用此接口修改 Hook 设置；`src-tauri/src/codex_cli.rs:178` 初始化未开启 experimentalApi。
- `src-tauri/src/codex_plugins.rs:684` 自行编辑 TOML 是参考项目的实现选择，不能推断为唯一可行路径。
- **待验证**：本机版本对 `plugins."<id>".enabled` 的写入、原生身份转义、缺省值、版本冲突和覆盖结果。当前证据尚未证明此插件路径已完成隔离验证。

## 配置归属与外部变更

- 参考项目市场维护资格核对所有有效字段的 origin 是否来自当前用户文件；组织/系统/profile 等覆盖即使值相同，也不授予用户维护资格。见 `src-tauri/src/codex_marketplaces.rs:136`。
- 插件的用户默认启用值不等于某个项目或既有会话的最终生效状态。
- 参考项目在操作前复核目标、home、安装记录、来源和用户设置，旧快照使用后失效。见 `src-tauri/src/codex_plugins.rs:287`、`:389`。
- 自身串行化和写前比较不构成跨进程事务；现有“重读再 rename”仍存在外部竞态窗口，见同文件 `:722`。官方 expectedVersion 可用于配置冲突检测，不能替代插件安装身份复核。

## Codex 同步、重新安装与移除

- 当前官方说明：市场刷新可以安装或刷新已配置插件，即使插件已禁用；保留 workspace-managed 启用状态。`marketplace upgrade <name>` 是写操作，省略名称会作用于全部 Git 市场。
- 官方来源：`https://learn.chatgpt.com/docs/config-file/config-reference`；`https://developers.openai.com/plugins/build/plugins.md` 的仓库插件启停部分。
- 参考项目 ADR-0012 记录旧版本实验中禁用插件内容更新而禁用配置保留；这是旧实验，不是本机实测。
- 当前无独立 plugin update 命令。参考项目 `src-tauri/src/codex_plugins.rs:428` 使用 add 表示安装/重新安装；ADR-0014 记录旧版本重新安装会启用插件且不保证取得上游最新版。
- 市场 remove 的当前帮助只承诺移除市场配置来源。参考项目 `src-tauri/src/codex_marketplaces.rs:1033` 的 fixture 没有真实已安装插件，不能据此保证完整缓存、安装记录或共享来源的后果。
- **待验证**：本机重新安装的启用与内容行为、同步实际影响集合、市场移除对真实安装/缓存的后果。

## Claude 市场移除与版本兼容

- `src-tauri/src/claude_cli.rs:130` 把插件/市场写入硬性限制到 `2.1.300`；照搬会使本机 `2.1.323` 只能查看。
- 当前 marketplace remove 帮助中的 scope 限定移除市场声明的位置；不能单凭 `--scope user` 推断只影响用户安装。
- 参考项目 ADR-0018 记录 `2.1.300` 的实验：其他项目仍保留市场声明时，删除 user 声明也会删除 user/project/local 插件安装记录。当前实现因而在移除前检查全作用域，存在非 user 安装或信息不明时阻止；见 `src-tauri/src/claude_plugins.rs:221`。
- 当前官方移除说明涉及市场所属插件的卸载，不能当作单纯隐藏来源。官方入口：`https://code.claude.com/docs/en/plugins/install#manage-marketplaces`。
- **待验证**：`2.1.323` 在其他作用域声明仍存在时的具体行为；不能宣称旧实验已被修复或仍逐项成立。

## 更新、批准与认证

- Claude 官方 manifest 说明更新保留既有 enabledPlugins 设置；未显式设置的插件还受 defaultEnabled 等默认值影响。参考测试先 enable 后 update，没有证明禁用插件更新行为；见 `src-tauri/src/claude_plugin_tests.rs:527`。
- 官方来源：`https://code.claude.com/docs/en/plugins/manifest-reference#defaultenabled`。
- 当前 Claude install/update 支持 command-source 与 headersHelper；非交互执行需要显式批准相应命令。`--accept-command` 绑定展示的命令、插件和目录摘要，不能视为通用自动同意。
- 官方来源：`https://code.claude.com/docs/en/plugins/cli-reference#accept-a-displayed-install-command`。
- 参考项目把 needs-approval 交给用户在 CLI 中完成，见 `src-tauri/src/claude_cli.rs:136`。这与已确认 Q8 的认证/信任由宿主处理一致；Skills Box 应报告需要外部操作，不能计为安装成功或自动批准。
- 私有市场可依赖既有本机 Git 认证；连接服务认证与插件安装是不同阶段。未获得“所有安装都不会执行插件脚本”的保证，不应作这种产品承诺。

## 实现前验证集合

1. Codex 配置接口对插件开关的实际支持、转义、版本冲突、覆盖与重读结果。
2. 当前 Codex add/reinstall、market upgrade/remove 的实际变更范围及禁用插件行为。
3. 当前 Claude 写命令的 JSON/退出码、插件更新启用状态、市场移除的跨作用域影响。
4. 操作成功但重读失败、外部配置变化、需要批准/认证时的结果分类。

验证应在隔离的配置目录和临时项目中进行；上述待验证事实不是已完成的产品能力承诺。
