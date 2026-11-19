# 原生插件管理：初步事实调查

日期：2026-11-19。只读调查，不代表已确认产品决策。

## 调查边界

- Skills Box 基准提交：`61b3a29e15a9c35c870761f3946b45e6311f9360`。
- 参考项目：`/Users/sunven/github/package-manager`，基准提交 `2adc535f94f4b33a33591483a18d557b955bcce3`。
- 本机只读版本及帮助检查：`codex-cli 0.170.0`、`Claude Code 2.1.323`。
- 未读取个人凭据或配置，未执行安装、启停、更新或卸载。CLI 帮助证明命令存在，不单独证明运行副作用或实际兼容性。

## Skills Box 现状

- `GLOSSARY.md` 已把 Skill 与 Expert pack 定义为不同对象，两者都避免与 plugin 混称。Attach/Detach 是 Skill 的 Agent attachment 操作，不是原生插件启停。
- `src/components/marketplace-page.tsx:316` 提供 SkillHub、skills.sh、ClawHub 三个来源；专家包属于 SkillHub。
- `src-tauri/src/marketplace/records.rs:9` 的 Installation 是应用拥有的 Skill 安装记录，保存到各 Scope 的 `.skills-box-marketplaces.json`；不是原生 Plugin 安装记录。
- ADR-0029 区分技能市场与已有 Git 安装来源的发现；ADR-0033 区分 skills.sh 目录身份与实际 Git 安装身份。
- `src-tauri/src/agents/catalog.json:139`、`:255` 分别配置 Claude Code 与 Codex 的 Skill 目录。未发现原生插件注册、启停或市场管理实现。
- `src-tauri/src/lock.rs:21` 的 `plugin_name` 及 `src-tauri/src/tasks/parser.rs:242` 的分组只是 Skill 发现/提取元数据，不能视为原生 Plugin 生命周期。

## 参考项目已实现能力

下列路径均相对于 `/Users/sunven/github/package-manager`。实现事实不代表已验证本机最新版工具具有相同运行行为。

| 维度 | Codex | Claude Code |
| --- | --- | --- |
| 原生配置范围 | `CODEX_HOME`，默认 `~/.codex` | `CLAUDE_CONFIG_DIR`，默认 `~/.claude` |
| 清单 | `plugin list --available --json` | `plugin list --json --available` |
| 安装/卸载 | `plugin add/remove` | `plugin install/uninstall --scope user` |
| 启停 | 编辑用户配置的插件 enabled 值 | `plugin enable/disable --scope user` |
| 更新入口 | 重新安装，文案明确会启用且不保证最新版 | `plugin update --scope user` |
| 市场操作 | `marketplace add/upgrade/remove` | `marketplace add/update/remove` |
| 版本限制 | 按命令与响应能力检查 | 写入硬性限定 `2.1.300` |

关键证据：

- UI 与命令已接入：`src/components/Shell.tsx:35`、`src/App.tsx:451`、`src/components/ClaudeCodePage.tsx:19`、`src-tauri/src/lib.rs:1165`。
- Codex 生命周期：`src-tauri/src/codex_plugins.rs:428`；启停编辑：同文件 `:668`；重新安装后果：`src/components/CodexPluginList.tsx:160`。
- Claude 命令参数：`src-tauri/src/claude_plugins.rs:513`；严格版本限制：`src-tauri/src/claude_cli.rs:130`。
- 插件身份包括市场，不能只按名称合并：`src/codexPlugins.ts:1`、`src/claudeCode.ts:1`。Claude 后端还区分 scope 与 projectPath：`src-tauri/src/claude_plugins.rs:116`。
- Codex 同步市场的确认文案说明可能刷新已配置插件，包括禁用插件：`src/components/CodexMarketplacesPage.tsx:225`。这仍需针对采用的 CLI 版本做隔离验证。
- Claude 移除市场检查全部安装作用域；存在非 user 安装时阻止：`src-tauri/src/claude_plugins.rs:221`。确认框展示受影响插件：`src/components/ClaudeMarketplacesPage.tsx:144`。
- Codex 来源支持 Git 与本地目录：`src-tauri/src/codex_marketplaces.rs:476`；Claude 另支持 JSON URL/文件：`src-tauri/src/claude_marketplaces.rs:196`。
- 详情读取已有本地内容，不为预览下载远程插件：`src-tauri/src/codex_plugin_details.rs:64`、`src-tauri/src/claude_plugin_details.rs:129`。
- 维护前后核对状态：`src-tauri/src/codex_plugins.rs:379`、`src-tauri/src/claude_plugins.rs:397`、`src/hooks/useClaudeCode.ts:136`。

参考项目不包含公共插件目录、发布、项目配置管理、批量操作或 MCP 认证管理，见 `README.md:170`。README/ADR 引用的若干历史 `.scratch` 目录本次不可用；以上以现存代码为依据。

## 本机 CLI 帮助核查

Codex `plugin` 提供 add、list、marketplace、remove；没有独立的插件 enable、disable、update 子命令。全局 `--enable/--disable` 是 feature flags，不能当作单插件启停。`marketplace upgrade` 帮助仅承诺刷新 Git 市场快照；其与已安装插件更新的关系仍需验证。已检查命令没有 `--scope`，项目级契约未确认。

Claude Code 提供 install、list、enable、disable、uninstall、update；插件安装和市场添加支持 user/project/local，检查过的管理命令提供 JSON 输出。参考项目只允许 `2.1.300` 写入，不适配本机 `2.1.323`；照搬会令本机只能查看。

## 官方资料入口

- Codex 原插件页 `https://developers.openai.com/codex/plugins/` 本次跳转到 `https://learn.chatgpt.com/docs/plugins`，是跨产品页面；不能把全部页面能力归给本机 CLI。
- Codex 原 CLI 参考 `https://developers.openai.com/codex/cli/reference/` 本次跳转到 `https://learn.chatgpt.com/docs/developer-commands?surface=cli`。
- Codex App Server：`https://developers.openai.com/codex/app-server/`。调查中 plugin/list、read、install、uninstall 被标注开发中；不能仅因接口存在就视为生产接入契约。
- Claude Code 插件管理：`https://code.claude.com/docs/en/discover-plugins`。
- Claude Code 市场规范：`https://code.claude.com/docs/en/plugin-marketplaces`。
- Claude Agent SDK 插件：`https://platform.claude.com/docs/en/agent-sdk/plugins`；SDK 的本地插件加载不等于市场管理。

后续按用户确认的操作范围核查运行语义、作用域、外部变更与移除副作用；不得把参考项目的行为声明当作所有工具版本的保证。
