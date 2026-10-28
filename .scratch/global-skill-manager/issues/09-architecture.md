# 技术架构与命令边界

Type: grilling
Status: resolved
Blocked by: 01, 04

已知事实（来自「更新检测机制与成本」）：不得用 `skills check` 做只读检查；架构需包含 GUI 自建的按来源分组检查器、缓存与 GitHub 速率限制处理。

已知事实（来自「全局安装的真相与异常形态」）：读取模型必须合并 lock 来源账本与物理目录；不要信任 `list --json.agents`；lock 写入非原子且损坏时会被读成空，架构需包含备份、串行化与拒绝 version != 3 的保护。

已知事实（来自「Tauri 执行外部 CLI 与打包」）：推荐 `tokio::process::Command` + `ProcessExecutor`；不把 `tauri-plugin-shell` 暴露给前端；Finder 启动只有最小 PATH，必须解析 Node 并将 `<node_dir>` 加入子进程 PATH；长任务用 per-task `Channel`、进程组 SIGTERM→SIGKILL、`Semaphore(1)` 单写者与 mutation 前 lock 快照；打包 `.app` + ad-hoc，依赖系统 Node/npx/git。

已知事实（来自「CLI 能力与 JSON 契约」）：固定 `skills@1.7.4`；spawn env 设 `DISABLE_TELEMETRY=1`/`DO_NOT_TRACK=1`，清理 agent 检测变量，只解析 `--json`；`add` 必须显式 `-a` 并建议追加 `-a universal`；断链走 GUI unlink，卸载走 CLI + 事后校验；更新用 `add` 重建而非 `update`。

已知事实（来自「安装向导的来源预览与选择契约」）：安装命令固定为每来源一次 `add <source> -g -s <names> -a <ids> -a universal -y --json`；预览解析 CLI `-l`；写入阶段禁止取消；完成后逐 skill 校验；失败项单独重试。

已知事实（来自「更新、断链与卸载的安全语义」）：更新逐 skill 重建 `add`，不调用 `update/check`；更新/卸载前强制备份 lock + canonical + copy attachments + manifest，并写 JSONL 日志；断链由 GUI unlink，跨 agent copy 依赖则阻止；卸载用 `remove -g -s` + 事后校验；失败不自动回滚。

已知事实（来自「清单与详情的信息规则」）：读取模型要输出正交状态（更新 / 管理 / 警告）、来源 / 时间 / 路径 / hash、物理挂载；描述来自 SKILL.md frontmatter；lock 损坏或非 v3 必须全屏错误；手动刷新、无 watcher；搜索只覆盖 name/description/source。

## Question

在 CLI 契约与 Tauri 执行方案的事实基础上，锁定 v1 的工程架构：

- 变更入口：以 `npx skills` 为唯一变更入口，还是对断链等缺口做少量原生文件操作；读取用 `list -g --json` + lock，还是完全原生扫描；两者如何保持一致性。
- Tauri 命令边界：列出命令清单（如 `list_skills`、`get_skill_detail`、`install_skills`、`check_updates`、`update_skills`、`detach_agent`、`uninstall_skill`、`detect_environment`），每个命令的输入 / 输出 / 错误模型。
- 错误模型：如何把 CLI stderr / 退出码归一化为稳定的前端错误码；网络、权限、lock 冲突、部分失败的表示。
- 并发与状态：安装 / 更新任务的队列、取消、进度事件；是否允许并发；前端状态管理选型（TanStack Query / Zustand / 仅 hooks）。
- 安全：参数转义、路径校验、渲染 `SKILL.md` 的 XSS 处理、外部链接与 opener 的使用边界。
- 测试缝：CLI 执行器抽象、fixture、Rust 单测、前端组件测试、手工验收。
- 打包与前提：Node / CLI 检测、PATH 探测、缺失引导；v1 只保证 `pnpm tauri build` 的本机 .app。

输出：一页架构决策（含命令表与错误码表），写入 `## Answer`；如满足 ADR 三条件，另建 `docs/adr/0001-*.md` 并在答案中链接。只闭合本票据。

## Answer

用户确认（2026-10-28）。架构决策已记录在 [ADR 0001](../../../docs/adr/0001-cli-mutations-native-reads.md)。

### 读写边界

- **读取**：Rust 原生读 model。读 `~/.agents/.skill-lock.json`（严格 v3）、扫描 canonical 与各 agent `globalSkillsDir`、`lstat` 判断挂载、解析 `SKILL.md` frontmatter、维护本地 baseline。`list -g --json` 只做诊断交叉校验，不作为真相。
- **变更**：安装 / 更新 / 卸载调 `npx -y skills@1.7.4`；更新重新构造 `add`，不调用 `update` / `check`；断链由 Rust 直接 unlink；不把 shell/fs 插件暴露给前端。
- **只读可用**：Node/CLI 缺失时仍可展示 inventory；变更按钮禁用并显示环境错误。
- **Agent catalog**：从固定版本 CLI 生成并 vendored 79 个 agent 的 ID / 显示名 / 目录 / universal 标志 / 检测规则，不在运行时解析 CLI 文档。

### 模块

`env`（PATH/Node/npx/git/CLI/agent 检测）、`agents`（catalog）、`lock`（读取/校验/快照）、`inventory`（扫描/分类/正交状态）、`baseline`（本地指纹）、`update_cache`（GitHub checker）、`backup`（快照/日志）、`tasks`（单写者队列/进程执行）、`commands`（Tauri 边界）。

### Tauri 命令表

| 命令 | 输入 | 输出 | 说明 |
|---|---|---|---|
| `get_environment` | `refresh?`（默认 false） | `EnvReport` | 只读；Node/npx/git/CLI/agent/PATH/lock health；`refresh=true` 跳过探测缓存 |
| `get_inventory` | — | `InventorySnapshot` | 只读；lock 损坏/非 v3 时返回 `lock.*` 错误 |
| `get_skill_detail` | `skillId` | `SkillDetail` | 只读；按需读 SKILL.md，限制 1 MiB |
| `start_preview` | `source`, `Channel<TaskEvent>` | `TaskId` | 只读任务；可取消；60s 超时；解析 `add -l`，失败返回 raw text |
| `start_install` | `InstallRequest`, `Channel<TaskEvent>` | `TaskId` | 变更任务；写入阶段不可取消 |
| `check_updates` | `skillIds?` | `UpdateCheckReport` | 只读；6h per-source 缓存；并发 3–4；最多 20–25 来源 |
| `start_update` | `skillIds[]`, `Channel<TaskEvent>` | `TaskId` | 变更任务；逐 skill 串行；可在 skill 之间取消 |
| `detach_agent` | `skillId`, `agentId` | `DetachResult` | 变更；快速；走单写者队列；copy 也先备份 |
| `start_uninstall` | `skillId`, `Channel<TaskEvent>` | `TaskId` | 变更任务；备份后调 `remove -g -s` |
| `cancel_task` | `taskId` | `CancelAck` | preview 随时；install/uninstall 仅 spawn 前；update 另允许 skill 之间 |
| `get_active_mutation` | — | `ActiveMutationInfo \| null` | 退出确认；`null` 表示没有进行中的变更 |
| `dismiss_interrupted_mutation` | — | `()` | 确认启动时的中断警告；保留 backup 快照 |
| `open_skill_folder` | `skillId` | `()` | 仅打开服务端解析的已知路径 |
| `open_skill_md` | `skillId` | `()` | 默认编辑器打开 SKILL.md |
| `open_external_url` | `url` | `()` | 仅绝对 https 且带主机名；Rust 重新校验 |
| `reveal_app_data` | `kind: backups \| logs \| cache \| baselines` | `()` | 打开 app data 子目录 |
| `quit_app` | — | `()` | 确认退出；保留中断标记并杀掉 CLI 进程组 |

- `InstallRequest = { source, skills[], agents[], retry? }`；Rust 校验来源/名称/agent，后台始终追加 `-a universal`，不接受 `--copy`、`--all`、`-a '*'`。`retry` 只由结果页的显式重试按钮设置，允许覆盖同来源、上一次部分安装留下的 managed/stale 条目；普通安装仍拒绝同名冲突。
- `TaskEvent = { taskId, phase, message, skill?, completed?, total?, result?, error? }`；`TaskResult = { action, overall: success | partial | failed | cancelled, skills[], backupPath?, logPath?, startedAt, finishedAtEpochMs }`。`action` 为 `preview | install | update | uninstall`；`startedAt` 在任务注册时写入。
- 任务阶段：preview `preparing → cloning → parsing → done`；install `preparing → running_cli → verifying → done`；update/uninstall `preparing → backup → running_cli → verifying → done`。CLI 不暴露克隆/写入边界，因此 install 的取消窗口是 spawn 前；preview 是独立只读任务，可取消。**这是对「安装向导」票据中“克隆阶段可取消”的架构细化：安装 CLI 一旦启动不可取消。**
- 任务返回 TaskId 后通过 Channel 推送事件；前端在完成/失败后 invalidation inventory/detail。单写者队列 `Semaphore(1)`；single-instance 插件防双开；变更期间允许读，但界面标注任务进行中。

### 错误模型

`AppError { code, message, detail?, hint?, retryable }`；`Warning { code, message, detail? }` 挂在成功/部分成功的 `TaskResult` 上。CLI 的 exit code 不足以判定成功；以 JSON per-skill 状态 + 事后校验为准。

| 域 | 错误码 |
|---|---|
| Env | `env.node_missing`, `env.npx_missing`, `env.git_missing`, `env.cli_unavailable`, `env.version_unsupported` |
| Lock | `lock.corrupt`, `lock.unsupported_version`, `lock.snapshot_failed`, `lock.write_failed`, `lock.verify_failed` |
| Source | `source.invalid`, `source.not_github`, `source.fetch_failed`, `source.no_skills`, `source.preview_failed` |
| Skill | `skill.not_found`, `skill.ambiguous`, `skill.unmanaged`, `skill.copy_only`, `skill.read_failed`, `skill.source_unresolvable`, `skill.conflict` |
| Agent | `agent.invalid`, `agent.not_detected`, `agent.not_attached`, `agent.native`, `agent.dependency_conflict` |
| CLI | `cli.spawn_failed`, `cli.timeout`, `cli.exit_nonzero`, `cli.invalid_json`, `cli.partial_failure`, `cli.cancelled`, `cli.no_matching_skill` |
| Network | `network.offline`, `network.rate_limited`, `network.error` |
| Task | `task.busy`, `task.cannot_cancel`, `task.cancelled`, `task.internal` |
| Backup | `backup.failed` |
| Path/Open | `path.invalid`, `path.outside_allowed_root`, `open.failed` |
| Verify | `verify.canonical_missing`, `verify.attachment_missing`, `verify.lock_missing`, `verify.lock_mismatch`；`verify.copy_fallback` 作为 warning |

- 部分失败不抛 Task 级错误，返回 `overall=partial` + 每 skill 结果；fatal 错误才抛 `AppError`。
- CLI stderr 放 `detail`，前端默认只展示归一化 message；详细输出可展开。

### 并发、取消与状态

- 单写者队列；安装/更新/卸载/断链串行；preview 只读任务不占写锁。
- 每个任务一个 `Channel<TaskEvent>`；按行/50–100ms 批量；进度以阶段 + `completed/total` 表示，不伪造百分比。
- 取消：preview 随时；install/uninstall 仅 spawn 前；update 另允许在两个 skill 之间；其余阶段 `task.cannot_cancel`。进程组 SIGTERM → 3s → SIGKILL；mutating 前快照 lock，结束后校验。
- 超时：环境探测 5s、读取 30s、写入 15min 硬上限。
- 应用退出时若有变更任务：提示风险；退出则杀进程组并保留 snapshot，下次启动校验。
- 前端：TanStack Query 管理 `environment` / `inventory` / `skill` / `updateReport`；本地 `useState/useReducer` 管理筛选、选中、弹窗。任务完成后 invalidate；不引入 Zustand。
- 外部 CLI 并发修改不做实时协调；单实例只防 GUI 双开；外部改动需手动刷新，操作前校验、操作后验证。

### 安全

- 前端只调用类型化命令；不传 path / argv / env / executable。来源字符串、skillId、agentId 均由 Rust 校验。
- 路径：skillId 从服务端 inventory 解析；canonical 根 `~/.agents/skills`；agent 目录来自 vendored catalog；`realpath` + 前缀校验；断开目标必须在 agent 目录内且不得是 canonical；拒绝把 agent skills 目录自身 symlink 的情况。
- spawn：`env_clear` + 最小 allowlist；`DISABLE_TELEMETRY=1`、`DO_NOT_TRACK=1`；清理 agent 检测变量；stdin null；cwd 为 app data 临时目录；固定 `<node_dir>/npx -y skills@1.7.4`；不设 `XDG_STATE_HOME`。
- 前端渲染：`react-markdown` + `rehype-sanitize`，`skipHtml`；frontmatter 在渲染视图隐藏；远程图片不加载（渲染为链接）；链接仅允许 https，经 opener 打开；原文视图转义为纯文本。
- Tauri：不启用 `shell:allow-execute/spawn`；opener 仅允许 https 与已知本地路径；限制性 CSP；无远程脚本/图片；无遥测。
- 所有备份/日志/cache/baseline 写 app data；日志不含密钥；备份路径不回传任意可执行路径。

### 测试缝

- Rust 单测：`EnvResolver`、`AgentCatalog`、`LockStore`、`InventoryReader`、`Baseline`、`UpdateChecker`（mock HTTP）、`BackupStore`、`TaskManager`、`CliInvocation`、`Verifier`、`PathGuard`、错误映射。
- `ProcessExecutor` trait + Tokio 实现 + Scripted fake；fixture：lock v3 / corrupt / v2、canonical、symlink/copy、stale、copy-only、unmanaged、SKILL.md 缺失。
- 前端 Vitest + RTL：筛选、详情渲染、安装向导、危险确认、错误状态；`mockIPC`；不直接散落 `invoke`，统一 `api.ts`。
- 不做 Tauri E2E；`SKILLS_BOX_FAKE_CLI=1`（debug only）模拟慢/失败/取消；手工验收矩阵由「v1 验收标准与测试矩阵」提供。

### 打包与前提

- `pnpm tauri build --bundles app`；ad-hoc 签名；`minimumSystemVersion: 12.0`；不公证；本机自用。
- 依赖系统 Node >= 22.20、npx、git；不捆绑 Node sidecar；固定 `skills@1.7.4`，不跟随 latest。
- app data：`~/Library/Application Support/com.sunven.skills-box/` 下 `backups/`、`logs/`、`cache/`、`baselines/`；v1 不自动清理备份。
- 更新检查仅手动触发；6h per-source 缓存；不在启动/焦点时自动检查；失败保留上次结果并标「检查失败」。
- 不做应用自身自动更新。

> 验收细化（来自「v1 验收标准与测试矩阵」）：测试需要可注入 `AppPaths`（canonical / agent 根）与仅 debug 生效的 `SKILLS_BOX_ROOT`；生产构建忽略。sandbox 使用临时 `HOME` / `XDG_STATE_HOME`，破坏性测试不碰真实 HOME。
