# 原生 Plugin 管理实现与验收

日期：2026-11-20。状态：已实现原生插件管理；同日按用户决定移除精确版本白名单。默认能力按宿主提供，来源、作用域和实际异常限制保留；初次验收与后续修订分别记录。

## 已实现

- 新增独立「插件」入口，可切换 Codex / Claude Code。固定用户级，不随技能项目选择变化。
- 读取宿主配置与安装记录，浏览配置的市场、搜索、筛选、查看本地 README 和组件声明。保留市场缺失时的安装或待核对记录，单个目录失败不会清空其他市场。
- 市场添加、同步、移除；插件安装、启停、卸载；Codex 重新安装与 Claude 更新分开。普通重新读取不调用变更命令。
- 确认页展示目标、来源、宿主、配置目录、实际动作及副作用。执行前重查 CLI、配置层、原生记录和目录；本地新增来源内容变化也使旧确认失效。
- Codex 开关使用官方 `config/value/write`，包含转义后的插件键与用户配置版本，未直接改写原生配置。非用户配置覆盖来源不明时只读。
- 写入 worker 持有公共 mutation permit 到原生命令和核验结束，IPC 丢弃不释放写入权；与 Skill 写入、应用更新互斥。正常退出等待完成。
- 独立的插件操作记录，不进入 Skill 快照或恢复协议。部分写入保留实际结果；核验失败标记待核对，无自动回滚、重复写入或自动批准。
- 默认能力不依赖 CLI 版本名单：Codex 提供本地/Git 市场来源，Claude 提供本地/Git/JSON 地址。版本仍用于展示、异常缓存和确认指纹；运行中返回或写后状态不符合预期时停用相应动作。
- 可选隔离兼容性检查更新实际覆盖的基本操作，成功可恢复对应动作，失败可停用对应动作；保留来源格式及同步、移除市场的既有状态，包括异常禁用记录。

主要代码：`src-tauri/src/native_plugins/`、`src/components/native-plugins-page.tsx`、`src/lib/native-plugins.ts`。与 TaskManager、App 导航、退出提示及既有 IPC/Fake backend 集成。

## 原生验收环境与边界

- Codex CLI **0.170.0**；Claude Code **2.1.323**。这些是实际验证记录，不是最低版本或未来版本兼容承诺。
- 最终 Rust 原生适配测试均使用临时 HOME、CODEX_HOME / CLAUDE_CONFIG_DIR、工作目录和静态插件；清空子进程环境，再设置白名单变量。禁用 Git 全局/系统配置、交互认证、CLI 自动更新。
- Git 和 JSON URL 来源测试仅连接临时回环 HTTP 服务；网络失败连接关闭的回环端口。未使用真实私有仓库、账号或用户凭据。
- 批准需求测试只含 `/usr/bin/false`，未传入任何自动接受参数，确认原生 CLI 拒绝执行。Codex 覆盖测试使用临时包装脚本提供配置覆盖，没有修改系统级配置。
- 浏览器只访问 `preview.html` 的 Fake backend，验证后关闭专用浏览器会话和开发服务器。桌面与窄窗口截图保存在 `verification/plugins-desktop.png`、`verification/plugins-narrow.png`。未通过界面测试写入真实插件配置。

## 初次验收结果

| 项目 | 结果与证据 |
| --- | --- |
| 两端基本生命周期 | 通过：原生添加、安装、禁用、启用、更新/重装、卸载、移除；验证记录、内容和启用设置 |
| Git 市场同步 | 通过：ref/sparse、提交 1.0.0 → 2.0.0、禁用状态保持；Codex 同步改变插件内容，Claude 单独更新取得新内容 |
| 来源格式 | 通过：Git 仓库、本地目录、Claude JSON 文件与回环 JSON URL；Codex `.agents/plugins/marketplace.json` 与兼容 Claude 目录 |
| Codex 配置写入 | 通过：缺省键、带点插件身份转义、expectedVersion 冲突；真实 CLI 覆盖响应 `okOverridden` 与非用户归属阻止 |
| Claude 移除作用域 | 通过：同市场 user/project/local 安装全部可见，预览阻止移除；直接在隔离环境验证原生 `--scope user` 仍会连带删除其他安装 |
| 市场缺失与配置残留 | 通过：Codex 移除市场后保留配置记录并标为安装待核对；目录错误与已安装状态分离 |
| 批准和网络失败 | 通过：Claude command source 非交互拒绝、适配器预先阻止；两端 Git 网络失败没有报告成功 |
| 并发、恢复和不确定结果 | 通过：IPC 丢弃仍持有写入许可、退出等待核验、外部变更使确认失效、部分写入、重读失败、中断记录、未知响应只停用受影响动作 |
| 本地详情与敏感数据 | 通过：拒绝目录外符号链接/父路径；README 有大小限制；只返回组件名称与安全元数据；URL 用户信息、查询和片段脱敏 |
| 兼容性检测 | 初次验收通过：两端隔离基本生命周期；当时未知构建的市场同步、远程来源和连带移除禁用。此版本限制已由后续用户决定移除 |
| 前端 | 全量 **50 文件 / 523 测试通过**；最终插件专项 **5 测试通过**；生产构建通过（保留已有 bundle 超过 500 KB 提示） |
| 浏览器 | 通过：宿主切换、搜索、插件详情、重新安装确认及结果、添加来源/分支/sparse 表单；1280×800 与 820×720；插件页 axe WCAG A/AA **0 violations / 0 incomplete**，无浏览器错误 |
| Rust | 新模块 **16 个常规测试通过**；**7 个真实 CLI 隔离测试通过**。全量运行的唯一失败是已有 ZCode 环境检测断言，见下文 |

可复现命令（真实 CLI 路径在 opt-in 测试中明确指定；其他机器需调整测试路径）：

```sh
pnpm test
pnpm build
pnpm exec vitest run src/App.native-plugins.test.tsx
cargo test --manifest-path src-tauri/Cargo.toml native_plugins --lib
cargo test --manifest-path src-tauri/Cargo.toml native_plugins --lib -- --include-ignored
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

原始证据保存在 `verification/`。`native-tests.txt` 包含 7 个原生测试及 16 个常规测试；`native-unit-tests.txt` 单独记录常规测试。此前探测脚本与 JSON 保留原生响应形状，生产能力以 Rust 适配器测试为准。

## 移除版本白名单后的回归验收

2026-11-20：移除 `2.1.323` / `0.170.0` 的精确匹配门槛，保留历史原生验收证据。以下回归使用 Fake backend 和临时目录中的模拟 CLI，没有对真实用户插件执行写入，也未重跑 7 个需要真实 CLI 的 opt-in 测试。

- `cargo test --manifest-path src-tauri/Cargo.toml native_plugins --lib`：**25 通过、7 忽略、0 失败**。覆盖两端旧版、当前示例版本及其他版本的默认动作和来源格式；兼容性检测的恢复/停用与未覆盖市场状态保留；CLI 版本变化使旧确认失效；跨作用域限制和未知响应回归。
- `pnpm exec vitest run src/App.native-plugins.test.tsx`：**8 测试通过**。覆盖 Claude `2.1.325` / `99.1.0` 与 Codex `0.171.0` 无需兼容性检测即可打开市场操作预览；Claude 项目安装仍阻止移除。
- `pnpm build`：通过；保留已有主 bundle 超过 500 KB 的提示。

## 保持禁用或未交付的能力

- command / headersHelper 安装、带依赖声明、Node `package.json`、npm/archive/远程独立插件等未验证安装来源：只读展示，安装/更新/重装被阻止；交给宿主完成。当前已验证安装的是市场内部本地插件目录，包括 Git 市场内的目录。
- 未知 CLI 构建不因版本号被禁用；默认可操作不等于所有版本均已通过验收。基本兼容性检查不覆盖同步及连带移除，无法清除这些动作的实际异常记录。
- 非用户来源、跨作用域共享安装、不完整安装清单或无法确定移除影响时不写入。Codex 可观测项目有配置时保守阻止市场移除。
- Codex 本地市场无原生同步契约，使用重新读取；不将其显示为同步成功。
- 登录、授权、信任、自动接受安装命令、组件编辑、项目级管理、批量操作、插件自动更新和完整备份/回滚不在此次交付范围。
- 界面测试使用 Fake backend；没有在用户真实配置中进行发布或桌面端写入试验。

## 已有全量测试失败

`agents::tests::detects_home_roots_in_catalog_order` 预期 `[amp, claude-code, gemini-cli, qoder]`，当前机器因 `/Applications/ZCode.app` 存在而额外检测到 `zcode`。该测试和 `src-tauri/src/agents/catalog.json` 的绝对路径检测规则均与 HEAD 相同，本次未修改。记录于 `verification/rust-tests.txt`（817 passed、1 failed、18 ignored；后续新增测试单独验收）。没有删除或更改用户应用来迁就测试。
