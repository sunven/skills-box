# Skills Box v1 — 实现规格

Status: Draft for review
Date: 2026-10-28

> 本规格由 wayfinder 地图上的决策票据合成。实现者应把本文件当作入口，但不要跳过被链接的票据与研究：细节与证据都在那里。
>
> - 地图：[map.md](map.md)
> - 架构 ADR：[0001-cli-mutations-native-reads.md](../../docs/adr/0001-cli-mutations-native-reads.md)
> - 术语表：[GLOSSARY.md](../../GLOSSARY.md)
> - 可点击原型：[prototype/index.html](prototype/index.html)
> - 验收矩阵：[issues/10-acceptance.md](issues/10-acceptance.md)
> - 研究文件：[research/](research/)

---

## 1. 目标与成功标准

### 1.1 目标

为 macOS 个人用户提供一个全局 Agent Skill 管理器，覆盖：

- 查看所有全局 skill 及其来源、agent 挂载、更新状态、本地改动与异常。
- 从公开 GitHub 仓库安装单个或多个 skill，并选择挂载到哪些 agent。
- 手动检查更新；按单个或全部更新，同时尊重断链状态与本地改动。
- 从单个 agent 断开 skill；彻底卸载 skill。
- 在异常状态下给出明确、只读的提示，不扩大破坏面。

### 1.2 成功标准

- 完整实现第 10 节引用的 P0/P1 验收矩阵；P0 全过，P1 无阻塞问题。
- Finder 启动的 `.app` 可完成核心闭环；真实 HOME 不被破坏性测试触碰。
- 每次变更后，`npx skills list -g --json` 仍能识别预期 skill；现有 agent 目录不被误删。
- 不出现数据丢失、权限越界、命令注入、XSS、远程资源加载或 CLI 生态回归。

### 1.3 非目标（Out of scope）

- 项目级 skill 管理。
- 内置 SKILL.md 编辑 / 新建 / init。
- skills.sh 搜索发现与市场浏览。
- 私有仓库、鉴权、GitLab、SSH、local、Notion、subdir、tag/commit 锁定。
- Windows / Linux。
- skill 自动更新、后台轮询、启动自动检查。
- 遥测。
- npm `experimental_sync`、`skills use` 启动 agent。
- 备份恢复 UI、历史操作面板、诊断面板。
- 断链修复 / 重定向、异常一键修复。
- 批量卸载、`remove --all`。
- 「仅卸载、保留目录」动作。
- copy-only skill 的更新。
- 自包含 Node sidecar / 纯 Rust 安装器。
- Tauri E2E / Playwright。
- 完整 WCAG 审计、减少动效打磨。
- DMG / 开发者签名 / 公证、应用自身自动更新。
- 真实 HOME 上的破坏性测试。

完整范围边界见地图的 **Out of scope**。

---

## 2. 用户与场景

### 2.1 用户

一个在多个 coding agent（Claude Code、Codex、Gemini CLI、GitHub Copilot、OpenCode、Antigravity 等）之间共享 skill 的开发者。本机已有 Node、npx、git 与 `~/.agents/skills` 中心仓库；不一定记得哪些 skill 来自哪个仓库、挂载到了哪些 agent、是否有本地改动。

### 2.2 核心场景

1. **盘点**：打开 app，看到所有全局 skill；按来源、agent、更新状态、异常筛选；查看某个 skill 的来源、挂载、路径、SKILL.md 与本地改动。
2. **安装**：粘贴公开 GitHub 仓库，预览仓库内 skill；选择 skill 与 agent；安装后确认 canonical 与挂载正确。
3. **更新**：手动检查更新；更新单个或全部；本地改动/未知/异常被跳过或要求确认；断链状态被保留。
4. **断开**：把某个 skill 从单个 agent 移除，但保留 canonical 与 lock。
5. **卸载**：彻底删除共享副本、所有挂载与 lock 条目；先备份。
6. **排障**：理解未受管、copy-only、stale lock、断链、路径缺失与网络失败，不被误导为「没有 skill / 已是最新」。

---

## 3. 领域模型

术语定义见 [GLOSSARY.md](../../GLOSSARY.md)。以下为规格使用的状态模型。

### 3.1 身份与磁盘布局

- Skill 的身份是 **name**（lock key / 目录名）。一行一个 name，不因多来源产生重复行。
- 中心仓库（canonical store）：`~/.agents/skills/<name>/`。
- Lock：`~/.agents/.skill-lock.json`，严格 v3。GUI 只读、校验、快照；不直接写。
- Agent 目录：vendored catalog 中每个 agent 的 `globalSkillsDir`；挂载模式由 `lstat` 判定。
- App 数据：`~/Library/Application Support/com.sunven.skills-box/` 下的 `backups/`、`logs/`、`cache/`、`baselines/`。

### 3.2 管理状态

| 状态 | 判定 | 是否入主列表 | 动作 |
|---|---|---|---|
| Managed | 有 lock，canonical 目录存在 | 是 | 全套 |
| Copy-only | 有 lock，无 canonical，存在 agent copy | 是 | 卸载、Finder；不可更新/断开 |
| Unmanaged | 无 lock，存在目录 | 是 | 只读、Finder |
| Stale lock | 有 lock，任何位置都无目录 | 否（异常弹窗） | 只读提示 |
| Agent-only unmanaged | 无 lock，仅 agent 目录 | 是 | 只读、Finder |

### 3.3 更新状态

| 状态 | 判定 | 色调 |
|---|---|---|
| 已是最新 | 远端 tree hash 与基线/lock 一致，最近一次检查成功 | 绿 |
| 可更新 | 远端 tree hash 不同 | 琥珀 |
| 未知 | 无 hash / 64 位或空 hash / 无法比较 | 灰 |
| 检查失败 | 网络、限流、仓库不可达 | 红；保留上次结果 |
| 源路径缺失 | 上游仓库存在但记录的 skillPath 不存在 | 红 |

### 3.4 Agent 挂载

| 模式 | 判定 | 可断开 |
|---|---|---|
| link | symlink，`realpath` 指向 canonical | 是 |
| copy | 真实目录，非 canonical | 是（需依赖检查 + 备份） |
| native / shared | universal agent，直接读 canonical | 否 |
| broken | symlink 目标不存在 | 否（只读异常） |
| missing | 该 agent 未挂载 | — |

挂载真相一律来自物理目录与 `lstat`；**不使用 `skills list -g --json.agents`**（会高报 universal agent）。

### 3.5 警告与本地改动

- **本地改动**：当前文件与 baseline 或 lock hash 不一致。
- **本地改动?**：无 baseline 且 lock hash 不一致，无法判断（可能是安装器排除项造成的假阳性）。
- **未挂载**：没有任何 agent attachment；合法状态，不计异常。
- **断链**：attachment symlink 目标缺失；agent 行只读警告。
- **仅副本**：无 canonical 的 copy-only skill；管理状态，不计入「可更新」。

### 3.6 更新候选与基线

- 更新候选：更新状态为「可更新」的 managed skill。
- Baseline fingerprint：GUI 在安装、首次扫描、更新后记录；用于后续本地改动检测。
- 首次扫描若无 baseline，且 lock hash 与当前目录不一致：标为「本地改动?」，要求用户确认，不自动视为干净。
- 所有状态判定不依赖 CLI 的 `list --json.agents` / `mode`。

---

## 4. 信息架构与关键流程

原型：[prototype/index.html](prototype/index.html)。用户已选定 **A 三栏工作台**。

### 4.1 主界面

- 左侧：来源、agent、状态筛选。
- 中间：平铺 skill 列表 + 搜索 + 排序 + 结果计数 + 异常横幅。
- 右侧：持久详情面板。
- 页头：安装、检查更新、刷新、环境状态。
- 状态用正交徽章：更新状态 / 管理状态 / 警告，互不覆盖。

### 4.2 列表

- 默认按 `lock.updatedAt` 倒序；可切名称、来源。
- 搜索：name / description / source；不搜 SKILL.md 正文与 path。
- 状态筛选：全部 / 可更新 / 已是最新 / 未知 / 异常 / 未受管。
- 「异常」包含 path-missing、检查失败、本地改动、断链、仅副本；未挂载只提示。
- 行内：名称、单行描述、来源、agent 头像（最多 5 + n）、徽章、更新时间。
- 未受管时间显示「—」；详情另示目录 mtime。
- 计数基于完整库存；选中项被过滤掉时自动选首条。
- 不分组、不虚拟列表、不分页；目标 100 条以内。

### 4.3 详情

- 头部：名称、状态徽章、描述、动作。
- 概览：来源、ref、skillPath、安装/更新时间、canonical 路径（copy-only 显示 agent 路径）、hash、本地改动、更新状态、管理状态。
- Agent 挂载：名称、目录、模式、断开入口；无挂载显示「未挂载」。
- `SKILL.md`：默认渲染，原文切换；渲染隐藏 frontmatter、不加载远程图片、不渲染原始 HTML；原文完整显示；按需读取，1 MiB 上限；缺失/损坏显示警告。
- 动作：检查更新、更新（仅可更新）、彻底卸载（仅 managed）、Finder / 默认编辑器；不内置编辑。
- 未受管只保留 Finder；copy-only 只保留卸载 / Finder；path-missing / error 可检查与卸载，更新禁用。

### 4.4 安装向导

1. **来源与预览**：公开 GitHub 根仓库；`owner/repo` 或仓库 URL；默认分支。运行 `add -l` 解析 name + description；解析失败展示 raw text，允许手动输入 skill 名。
2. **选择 skill**：多选；已安装/未受管同名禁用；单 skill 仓库跳过选择。
3. **选择 agent**：只展示检测到的；默认勾选已含受管 skill 的；未受管目录默认不勾选；universal 标为共享 canonical；后台始终追加 `-a universal`。
4. **确认**：来源、skill、agent、目标路径、「写入阶段不可取消」。
5. **执行**：每来源一次 CLI 调用；阶段进度；逐 skill 校验；失败项重试。

### 4.5 更新

- 单个：确认页展示来源、当前/目标 tree hash、受影响 agent、本地改动、copy 转换警告。
- 全部：只处理 update-available；跳过 modified/unknown/path-missing/error；串行；失败继续；汇总 + 逐项重试。
- 更新后刷新 inventory/detail；不自动回滚。

### 4.6 断链 / 卸载

- 断链：从 agent 行发起；显示 agent、模式、保留 canonical/lock、其他 agent；copy 依赖时阻止。
- 卸载：从详情发起；显示删除范围；checkbox 解锁；备份后执行；事后校验。

### 4.7 异常与错误状态

- 异常横幅 → 只读弹窗：stale lock、未受管、断链、本地改动、源路径缺失。
- lock 损坏 / 非 v3：全屏错误，禁用变更，不显示空列表。
- CLI 缺失：环境横幅 + 只读 inventory（若可读）；变更禁用。
- 网络失败：保留上次更新徽章，标「检查失败」；不显示「已是最新」。
- 空库存 / 无搜索结果 / 无选中：各自空态。
- 加载：骨架/局部 spinner；不阻塞整窗。
- 刷新：手动 + 变更后；无 watcher / 轮询。

### 4.8 交互质量

- 中文界面，技术名词保留英文。
- 基础键盘：Tab 顺序、Esc 关闭、modal focus trap、关闭后焦点返回。
- 不做完整 WCAG 审计；减少动效为 P2。

---

## 5. CLI 调用矩阵

固定使用 `npx -y skills@1.7.4`；系统全局 `skills` 版本不影响。完整证据见 [research/01-cli-contract.md](research/01-cli-contract.md) 与 [research/04-tauri-cli.md](research/04-tauri-cli.md)。

### 5.1 允许的调用

| 用途 | 命令 | 说明 |
|---|---|---|
| 诊断读取 | `skills list -g --json` | 只做交叉校验；不作为挂载/清单真相 |
| 预览 | `skills add <source> -l -g` | 人类文本，解析 name + description；可取消 |
| 安装 | `skills add <source> -g -s <names> -a <ids> -a universal -y --json` | 每来源一次；显式 agent；禁止省略 `-a` |
| 更新 | 同安装，按 skill 逐个执行 | 用 lock 构造 source；显式当前物理挂载 agent + universal |
| 卸载 | `skills remove -g -s <name> -y` | 不带 `-a`；事后逐项校验 |
| 断链 | 无 CLI 调用 | Rust 直接 unlink / 删除 copy（依赖检查 + 备份） |

### 5.2 禁止的调用

- `skills update` / `skills upgrade` / `skills check`：会重挂所有 agent、覆盖本地改动、`check` 实际执行更新。
- `skills remove -a`：语义依赖环境，可能彻底卸载或留下悬空链接。
- `skills remove --all`。
- `skills add --all`、`-a '*'`、`--copy`。
- 任何把 frontend 提供的 argv / path / env / executable 直接拼入命令的调用。

### 5.3 来源

- 仅公开 GitHub 根仓库：`owner/repo` 或 `https://github.com/owner/repo`。
- 只装默认分支；拒绝 subdir、`#ref`、GitLab、SSH、local、Notion。
- 归一化后展示 `owner/repo`；非法输入明确报错。

### 5.4 Agent

- Vendored 79-agent catalog：ID、显示名、`globalSkillsDir`、universal 标志、检测规则。
- 只展示检测到的 agent；默认勾选已含受管 skill 的；未受管目录默认不勾选。
- Universal agent 标为共享 canonical；不产生独立目录。
- 后台始终追加 `-a universal`，避免单非 universal agent 默认 copy 且不写 canonical。

### 5.5 环境

- 要求 Node >= 22.20、npx、git。
- Finder 启动时解析 PATH；按「用户覆盖 → Volta → fnm → nvm → Homebrew → 系统 → 有界登录 shell 回退」探测。
- 子进程：`env_clear` + 最小 allowlist；`DISABLE_TELEMETRY=1`、`DO_NOT_TRACK=1`；清理 agent 检测变量；stdin null；cwd app data 临时目录；把 `<node_dir>` 加入 PATH。
- 不设 `XDG_STATE_HOME`；固定 `skills@1.7.4`。

### 5.6 Lock

- 严格读取 v3；损坏或非 v3：拒绝操作，不静默当空。
- 所有 mutation 前快照 `.skill-lock.json`；CLI 是唯一写 lock 的组件。
- 操作后逐项校验 lock key、canonical、agent 挂载；CLI exit 0 不足以判定成功。
- `list --json` 的 `agents` / `mode` 不作为真相；用物理目录与 `lstat`。

---

## 6. 命令契约与错误模型

完整命令表、TaskEvent、错误码表见 [issues/09-architecture.md](issues/09-architecture.md) 与 [ADR 0001](../../docs/adr/0001-cli-mutations-native-reads.md)。实现时以该票据为准；本节给出摘要。

### 6.1 读写边界

- **Rust 原生读取**：lock v3、canonical、agent 目录、`lstat`、frontmatter、baseline。
- **CLI 只做变更**：install / update / uninstall；断链原生 unlink。
- 只读 inventory 不依赖 Node；变更依赖 Node/npx/git。

### 6.2 Tauri 命令

| 命令 | 输入 | 输出 |
|---|---|---|
| `get_environment` | — | `EnvReport` |
| `get_inventory` | — | `InventorySnapshot` |
| `get_skill_detail` | `skillId` | `SkillDetail` |
| `start_preview` | `source`, `Channel<TaskEvent>` | `TaskId` |
| `start_install` | `InstallRequest`, `Channel<TaskEvent>` | `TaskId` |
| `check_updates` | `skillIds?` | `UpdateCheckReport` |
| `start_update` | `skillIds[]`, `Channel<TaskEvent>` | `TaskId` |
| `detach_agent` | `skillId`, `agentId` | `DetachResult` |
| `start_uninstall` | `skillId`, `Channel<TaskEvent>` | `TaskId` |
| `cancel_task` | `taskId` | `CancelAck` |
| `open_skill_folder` | `skillId` | `()` |
| `open_skill_md` | `skillId` | `()` |
| `reveal_app_data` | `kind` | `()` |

- 前端不传 path / argv / env / executable。
- `InstallRequest = { source, skills[], agents[] }`；Rust 校验并追加 universal；不接受 copy / all / `*`。
- `TaskEvent` 推送阶段、message、skill、completed/total、result/error。
- `TaskResult` 包含 overall、per-skill、backupPath、logPath、时间。

### 6.3 并发与取消

- 单写者队列，安装/更新/卸载/断链串行。
- Single-instance 插件防双开。
- Preview 只读任务可取消；install/uninstall 仅 spawn 前；update 另允许 skill 之间。
- 进程组 SIGTERM → 3s → SIGKILL；超时：探测 5s、读取 30s、写入 15min。
- 变更期间允许读，UI 标注进行中；完成后刷新。
- 应用退出时若任务运行：提示风险；退出则杀进程组并保留 snapshot；下次启动校验。

### 6.4 错误模型

`AppError { code, message, detail?, hint?, retryable }`；成功/部分成功的 warnings 单独返回。主要错误码域：

- `env.*`、`lock.*`、`source.*`、`skill.*`、`agent.*`
- `cli.*`、`network.*`、`task.*`、`backup.*`、`path.*`、`open.*`
- `verify.*`（`verify.copy_fallback` 为 warning）

CLI stderr 放 `detail`；前端默认只显示归一化 message。部分失败返回 `overall=partial`，不抛 Task 级错误。

### 6.5 前端

- TanStack Query：`environment`、`inventory`、`skill`、`updateReport`。
- 本地 state：筛选、选中、弹窗、任务展示。
- 不引入 Zustand；不散落 `invoke`，统一 `api.ts`。
- React 19 + Vite + Tailwind + shadcn/ui；`react-markdown` + `rehype-sanitize`，`skipHtml`；远程图片不加载；链接仅 https 经 opener。
- 变更完成后 invalidate 相关 query。

---

## 7. 生命周期语义

完整决策表见 [issues/07-lifecycle-safety.md](issues/07-lifecycle-safety.md)；安装契约见 [issues/06-install-wizard.md](issues/06-install-wizard.md)。

### 7.1 安装

- 每个 source 一次 CLI 调用；显式 `-a <ids> -a universal -y --json`；`-g -s <names>`。
- 同名已安装/未受管禁用，后端也拒绝。
- 预览 `add -l`；解析失败 raw + 手动名称。
- 单 skill 自动跳过选择；无 SKILL.md 空态。
- 写入阶段不可取消；安装前快照 lock。
- 事后逐 skill 校验 canonical / 挂载 / lstat；copy fallback 提示；partial failure 逐项重试。

### 7.2 更新

- 单个 / 全部；检查与执行分离；不调用 `update`/`check`。
- 更新候选：update-available；更新全部跳过 modified/unknown/path-missing/error。
- 本地改动：baseline + lock hash；有改动/无法判断需 checkbox + 备份。
- 用 lock 构造 source；显式当前物理非 universal agents + universal；无挂载仅 `-a universal`。
- copy 挂载转 symlink，警告 + 备份；copy-only 不更新。
- 保留断链状态；更新后重建 baseline；逐项校验；不自动回滚。

### 7.3 断链

- 仅 managed + canonical。
- symlink：unlink；copy：无其他 agent symlink 依赖才删除，否则阻止。
- canonical/lock 不动；universal/native 和 agent-only 不可断开。
- copy 断链也备份（copy 目录 + lock 快照）。
- 断链后无挂载显示「未挂载」；后续更新尊重当前挂载。
- 断链异常只读，不修复。

### 7.4 卸载

- 仅 managed；unmanaged 禁用；copy-only 可卸载。
- 备份 lock + canonical + copy 目录 + attachment manifest；备份失败中止。
- 跨 agent copy 依赖阻止。
- `remove -g -s <name> -y`；逐项校验 canonical、所有 agent 路径、lock key 消失。
- 不提供「仅卸载、保留目录」；备份即保留；无 `remove --all`。

### 7.5 备份、日志与恢复

- 每次更新/卸载前强制备份到 app data `backups/<timestamp>-<action>-<skill>/`。
- JSONL 操作日志：时间、动作、skill、agents、结果、备份路径、错误；日志失败不阻断。
- v1 无恢复 UI；结果展示备份路径；不自动回滚；备份不自动清理。
- 所有变更串行；lock snapshot；拒绝 corrupt/non-v3。

---

## 8. 异常与降级

| 情况 | 行为 |
|---|---|
| 未受管 | 主列表只读；Finder；无更新/卸载/断开 |
| copy-only | 主列表；标「仅副本」；可卸载；不可更新/断开 |
| stale lock / ghost | 不入主列表；异常弹窗；只读 |
| 断链 | agent 行红色警告；只读；不修复 |
| 本地改动 | 警告；更新/卸载需确认；备份 |
| 本地改动? | 灰警告；视为未知，需确认 |
| path-missing | 更新禁用；检查/卸载可用；标红 |
| 检查失败 | 保留上次结果；标红；不误报最新 |
| lock 损坏 / 非 v3 | 全屏错误；禁用变更；不显示空列表 |
| CLI 缺失 | 环境横幅；只读 inventory；变更禁用 |
| 网络离线 / 限流 | 列表可用；检查失败；保留上次徽章 |
| 缓存过期 | 手动检查；6h TTL；不自动请求 |
| 外部 CLI 修改 | 无 watcher；手动刷新；操作前校验、操作后验证 |
| 权限不足 | 单行/详情错误；不崩溃 |
| 空库存 / 无结果 / 无选中 | 独立空态 |

---

## 9. 安全

威胁模型与边界：

- **命令注入**：前端不传 argv/executable；source/skill/agent 校验；不做 shell 拼接。
- **路径穿越 / 任意删除**：skillId 从服务端 inventory 解析；canonical / agent 根前缀校验；`realpath`；断链不跟随 symlink；拒绝 agent skills 目录本身是 symlink。
- **Symlink 攻击**：`lstat` 判定；copy 依赖检查；删除前 realpath；不删除 canonical 之外的未知目录。
- **XSS / 远程内容**：Markdown 安全子集；`skipHtml`；不加载远程图片；链接仅 https，经 opener；CSP 限制。
- **权限过大**：不启用 shell/fs 插件；opener 仅允许 https 与已知本地路径；类型化命令。
- **遥测 / 隐私**：CLI `DISABLE_TELEMETRY=1`、`DO_NOT_TRACK=1`；app 无遥测；日志不含密钥。
- **单实例 / 并发**：single-instance；单写者；lock snapshot + 校验。
- **测试后门**：`SKILLS_BOX_ROOT` 仅 debug；release 忽略；sandbox 标识。

---

## 10. 测试与验收

完整 P0/P1 清单、24 个手工用例、性能阈值、sandbox 环境与回归配方见 [issues/10-acceptance.md](issues/10-acceptance.md)。

### 10.1 发布门槛

- P0 全过；P1 全过或已记录已知问题；P2 尽力。
- 无 P0 数据丢失、权限越界、CLI 回归。
- Finder `.app` 主流程可用；用户签收。

### 10.2 测试分层

- Rust 单测 + 集成（temp AppPaths + fake executor + fixtures）。
- 前端 Vitest/RTL + `mockIPC`。
- 真实 CLI sandbox（临时 HOME/XDG）+ 小型公开仓库。
- Finder `.app` 手工；100-skill fixture 性能。
- 不做 Tauri E2E。

### 10.3 回归

每次 mutation 后：

```bash
HOME="$SBX_HOME" XDG_STATE_HOME="$SBX_STATE" npx -y skills@1.7.4 list -g --json > after.json
```

- 对比 skill 名称/路径；检查 canonical、agent 目录、lock v3。
- 断言 fake 日志不含 `update`、`check`、`remove --all`、`-a '*'`、`--all`、`--copy`。

### 10.4 性能目标

Release `.app`，100 条 fixture，5 次中位数：

- 启动到可交互 < 2s；warm inventory < 500ms；cold < 1.5s。
- 搜索/筛选 < 50ms；详情 < 150ms；刷新 < 1s。
- 任务首个事件 < 500ms；主线程单次卡顿 < 100ms。
- 20 个来源更新检查 < 30s 或明确报错，UI 不冻结。

### 10.5 v1 验收记录（2026-10-28，ticket 11）

完整 M01–M24 逐条证据、待签项与已知问题见
[`../skills-box-v1/acceptance.md`](../skills-box-v1/acceptance.md)。摘要：

- 自动化：`cargo test` 303 passed / 2 ignored（两个 ignored 是真实网络测试：
  `sandbox_real_cli` 的 CLI 闭环与只读更新检查）；`pnpm test` 107 passed；
  `pnpm build`、`cargo clippy --all-targets` 通过。
- 真实 CLI sandbox：临时 HOME + `XDG_STATE_HOME` + `npx -y skills@1.7.4` 对
  `mattpocock/skills` 完成单/多安装、copy、copy→symlink 更新、断链、单/ copy-only
  卸载；10 次 `list -g --json` 回归名称/路径一致，canonical/agent/lock v3 正确，
  未受管目录保留，20 次 argv 无禁用形式。证据在
  `src-tauri/target/sandbox-evidence/1793180808-90031/`（复跑 116.5s）。
- 100-skill fixture（80 managed + 20 copy-only，20 来源、4 agent）：release 测试
  cold 21.8ms、warm 9.8ms、详情 1.2ms（debug 分别为 71.2/18.3/2.5ms）；
  前端 100 条筛选+排序中位数 0.09ms（同日早前复跑 0.02ms）。
- 发布：`pnpm tauri build --bundles app` 产出
  `src-tauri/target/release/bundle/macos/skills-box.app`（6.30 MiB，HEAD 重建）；
  `Signature=adhoc`、`LSMinimumSystemVersion=12.0`、未公证（bundler 明确跳过）；
  `codesign --verify --deep --strict` 通过。LaunchServices 启动进程 ~150ms、
  窗口 ~367ms、~1.4s 截图完整 UI；环境探测 Node/npx/git/1.7.4、Lock v3 正确；
  二次启动同 pid/单窗口。M17 的主线程卡顿与 Finder 双击、M22/M23 的真实
  退出/键盘操作待用户签收。
- 已知问题：`XDG_STATE_HOME` 非默认 lock 路径不检测（owner @sunven，v1.1）；
  ad-hoc 未公证（发布范围，后续版本）；真实 20 来源公网检查未跑（fake 20 来源
  已测，v1.1 复测）。P0 未发现数据丢失、权限越界或 CLI 生态回归。

---

## 11. 实现顺序与里程碑

### Phase 0 — 脚手架与安全基线

- 依赖：`@tanstack/react-query`、`react-markdown`、`rehype-sanitize`、shadcn/ui、Vitest/RTL；Rust：tokio、serde、thiserror、reqwest、sha2、walkdir、tempfile(dev)。
- Tauri 插件：opener、single-instance；不启用 shell/fs。
- CSP、capabilities、AppPaths、AppError、任务骨架、备份/日志骨架。
- 测试环境：sandbox AppPaths、fake executor、fixtures。

### Phase 1 — 读取模型与 UI 骨架

- EnvResolver、AgentCatalog、LockStore、InventoryReader、Baseline。
- 三栏 UI、平铺列表、正交徽章、搜索/筛选/排序、详情、SKILL.md 渲染/原文。
- 未受管 / copy-only / stale / broken / local modified / path-missing 的只读展示。
- lock 损坏/非 v3 全屏错误；CLI 缺失只读降级。

### Phase 2 — 变更基础设施与安装

- ProcessExecutor（Tokio + fake）、TaskManager、CLI invocation builder、PATH 解析、Channel 事件。
- 备份/日志、lock snapshot、verifier、路径守卫。
- 安装向导（preview / selection / agents / confirm / progress / partial retry）。
- 安装后逐项校验；copy fallback 提示；同名冲突后端拒绝。

### Phase 3 — 更新

- GitHub tree checker、6h cache、source/ref 分组、并发与限流。
- 更新单个/全部；本地改动/基线；copy 转换；断链保留；备份；失败重试。
- 不调用 `update`/`check`。

### Phase 4 — 断链与卸载

- symlink unlink、copy 依赖检查、copy 备份。
- 卸载 CLI + 事后校验；unmanaged 禁用；copy-only；跨 agent 阻止。
- 操作日志与备份路径展示。

### Phase 5 — 硬化

- 安全审计：命令注入、路径穿越、symlink、XSS、CSP、opener。
- 错误态、取消、单实例、退出保护、权限错误。
- 键盘/焦点；性能优化到阈值。

### Phase 6 — 验收与交付

- sandbox 手工矩阵、真实 CLI 集成、CLI 回归、Finder `.app`、100-skill 性能。
- 用户签收；更新本规格与验收记录；生成 release `.app`。

---

## 12. 技术栈与文件布局

### 12.1 前端

- React 19 + Vite 8 + TypeScript。
- Tailwind CSS + shadcn/ui；TanStack Query；React 本地 state。
- `react-markdown` + `rehype-sanitize`；`skipHtml`；自定义 img/link。
- Vitest + React Testing Library。
- 中文 UI。

### 12.2 Rust

- Tauri 2；tokio；serde；thiserror；reqwest（rustls）；sha2；walkdir；tempfile(dev)。
- 模块：`commands`、`env`、`agents`、`lock`、`inventory`、`baseline`、`update`、`backup`、`tasks`、`security`、`error`。
- 不启用 shell/fs 插件；使用 opener + single-instance。

### 12.3 数据与配置

- 读 `~/.agents/skills` + `~/.agents/.skill-lock.json`；不直接写 lock。
- App data：backups / logs / cache / baselines。
- `tauri.conf.json`：限制性 CSP、窗口尺寸、`minimumSystemVersion: 12.0`、ad-hoc 签名。
- `package.json` scripts：`dev`、`build`、`tauri`、`test`；Rust `cargo test`。

### 12.4 关键类型

```ts
type ManagementState = 'managed' | 'copy_only' | 'unmanaged';
type UpdateState = 'current' | 'update' | 'unknown' | 'error' | 'path_missing';
type AttachmentMode = 'link' | 'copy' | 'native' | 'broken';
type LocalModification = 'clean' | 'modified' | 'unknown';

interface TaskEvent {
  taskId: string;
  phase: 'preparing' | 'cloning' | 'running_cli' | 'verifying' | 'completed' | 'failed' | 'cancelled';
  message: string;
  skill?: string;
  completed?: number;
  total?: number;
  result?: TaskResult;
  error?: AppError;
}
```

---

## 13. 风险与缓解

| 风险 | 缓解 |
|---|---|
| `add -l` 人类文本解析脆弱 | pinned 1.7.4 + fixture 测试 + raw fallback + 手动名称 |
| Lock 非原子 / 损坏 | 严格 v3、snapshot、串行、拒绝 corrupt、事后校验 |
| Agent 检测与 CLI 漂移 | vendored catalog + pinned 版本 + 物理 `lstat` 真相 |
| GitHub 限流 / 网络失败 | 6h cache、按 source 批量、并发 3–4、保留上次结果、手动检查 |
| copy 模式 / 本地改动被覆盖 | 解析物理挂载、更新前警告 + 备份、copy→symlink 明确提示 |
| Finder PATH 找不到 Node | 解析器 + `<node_dir>` 注入 + 环境错误引导 |
| Markdown XSS / 远程资源 | `skipHtml`、sanitize、禁远程图片、CSP、https opener |
| 外部 CLI 并发修改 | 单实例、单写者、手动刷新、操作前校验/操作后验证 |
| 备份增长 | v1 不自动清理；显示路径；后续版本可加保留策略 |
| 更新 source 重建错误 | 用 lock source/ref/skillPath；失败 `skill.source_unresolvable` |
| 100 skills 性能 | 只读摘要、详情按需、无 watcher、性能 fixture |

---

## 14. 未决问题与后续

未决（不阻塞实现）：

- App 名称、图标与品牌是否在 v1 内。
- 偏好持久化（主题、筛选、默认 agent）与首次启动引导。
- 菜单栏 / 托盘 / 多窗口形态。
- 前端错误边界、崩溃恢复、系统通知。
- 完整 WCAG、减少动效打磨（P2）。

后续版本候选：

- 项目级 skill、skills.sh 发现、内置编辑 / 新建。
- 私有仓库、GitLab / SSH / local / subdir / ref。
- 恢复 UI、备份保留策略、断链修复、异常一键修复。
- Windows / Linux、DMG / 公证、应用自动更新。
- Tauri E2E、完整无障碍。

---

## 15. 引用

- 地图与决策索引：[map.md](map.md)
- 研究：
  - [CLI 契约](research/01-cli-contract.md)
  - [安装状态](research/02-install-state.md)
  - [更新检测](research/03-update-detection.md)
  - [Tauri 执行器](research/04-tauri-cli.md)
- 票据：
  - [主界面与关键流程原型](issues/05-ui-prototype.md)
  - [安装向导](issues/06-install-wizard.md)
  - [生命周期安全](issues/07-lifecycle-safety.md)
  - [清单与详情规则](issues/08-inventory-detail.md)
  - [技术架构与命令边界](issues/09-architecture.md)
  - [验收标准与测试矩阵](issues/10-acceptance.md)
- [ADR 0001](../../docs/adr/0001-cli-mutations-native-reads.md)
- [GLOSSARY.md](../../GLOSSARY.md)
- [可点击原型](prototype/index.html)
