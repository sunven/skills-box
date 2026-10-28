# v1 验收标准与测试矩阵

Type: grilling
Status: resolved
Blocked by: 05, 06, 07, 08, 09

已知事实（来自「更新检测机制与成本」）：验收需覆盖“检查更新不会触发安装”、ghost / path-missing 不误报，以及缓存过期后的降级。

已知事实（来自「全局安装的真相与异常形态」）：验收需覆盖 `list --json.agents` 高报、stale lock 复活、copy→symlink 转换、未受管 skill 不被误删、损坏 / 非 v3 lock 的拒绝与提示。

已知事实（来自「Tauri 执行外部 CLI 与打包」）：验收必须从 Finder/Dock 启动的 .app 验证（终端启动会掩盖 PATH 问题）；覆盖 Node/npx/git 缺失、取消 / 超时、lock 快照与单写者、无 shell 权限、`.app` 产物。

已知事实（来自「CLI 能力与 JSON 契约」）：验收需覆盖显式 `-a` 安装、单个非 universal agent 的 copy 回退、`remove -a` 不被使用、单 agent 断链不动 canonical/lock、彻底卸载事后校验、更新不重挂所有 agent、`NO_COLOR` 无关且只解析 `--json`。

已知事实（来自「安装向导的来源预览与选择契约」）：验收需覆盖来源校验、`-l` 解析 / 兜底、单 skill 跳过、无 `SKILL.md`、未受管目录默认不勾选、`-a universal`、写入阶段取消失败、copy 回退提示、同名禁用、失败重试。

已知事实（来自「更新、断链与卸载的安全语义」）：验收需覆盖更新跳过本地改动 / unknown / error、copy 转 symlink 警告、断链跨 agent 阻止、卸载事后校验、备份失败中止、操作日志 / 备份路径、无自动回滚、copy-only 更新禁用。

已知事实（来自「清单与详情的信息规则」）：验收需覆盖平铺列表、正交徽章、未受管 / copy-only / 断链 / stale 展示、lock 损坏、CLI 缺失、网络失败不误报、手动刷新、详情渲染 / 原文、动作位置。

已知事实（来自「技术架构与命令边界」）：Rust 原生读取，CLI 只做变更；单写者任务队列 + Channel；结构化 AppError；安全渲染 / CSP；`.app` 依赖系统 Node；ADR 0001。验收需覆盖这些边界。

## Question

把已确定的流程转成可验收的 v1 标准：

- 主流程验收：查看清单 / 详情、安装单个与多个 skill、按 agent 挂载、检查更新、单个 / 全部更新、断开、彻底卸载，每一步的可观察结果。
- 边界验收：无 Node / CLI、无网络、空列表、同名多来源、锁与目录不一致、断链、本地改动、部分失败、取消任务、CLI 升级后的兼容。
- 安全验收：危险操作必须二次确认；卸载后 lock / 目录 / 链接不残留；复制模式与符号链接模式都不破坏现有 CLI 行为。
- 性能：50–100 个 skill 的首屏、搜索、详情切换、刷新；安装 / 更新进度可见。
- 回归：操作后 `npx skills list -g --json` 仍能识别全部 skill；现有 agent 目录不被误删。
- 测试分层：Rust 单测、CLI fake、前端组件测试、手工验收清单各覆盖什么。

输出：v1 验收清单（可勾选）与手工测试矩阵，写入 `## Answer`。只闭合本票据。

## Answer

用户确认（2026-10-28）。

### 1. 发布门槛

- **P0 全过**：数据安全、核心闭环、安全边界、CLI 回归、Finder 启动 `.app` 无明显卡顿。
- **P1 全过或已记录为已知问题**：错误态、边界、取消、性能精确阈值、基础键盘/焦点；已知问题需有 owner 与目标版本。
- **P2 尽力**：减少动效、长文本打磨、非关键视觉细节。
- 存在任何 P0 数据丢失、权限越界、CLI 生态回归，则不交付。
- 用户按手工矩阵签收；验收结果与命令写入最终规格。

### 2. 测试环境

- **Sandbox（自动化 + 破坏性手工）**：临时 `HOME` / `XDG_STATE_HOME`，在临时目录内创建 `~/.agents/skills`、lock、各 agent 目录；app 使用可注入 `AppPaths` 与仅 debug 生效的 `SKILLS_BOX_ROOT`，窗口显示 `SANDBOX` 标识。生产构建忽略该变量。
- **真实 HOME 只读巡检**：只跑 `get_environment`、`get_inventory`、`get_skill_detail`、`check_updates`、打开 Finder；绝不跑安装/更新/卸载/断链。
- **Fake executor**：自动化测试用 `ProcessExecutor` fake，覆盖 partial failure、timeout、invalid JSON、copy fallback、cancel、backup failure、网络错误；不访问真实网络。
- **真实 CLI sandbox**：真实 `npx -y skills@1.7.4` + 临时 HOME/XDG，安装/更新/卸载只动 sandbox；使用小型公开测试仓库。
- **100-skill fixture**：生成 100 个最小 `SKILL.md` + 多来源/多 agent 挂载，用于性能与回归；release `.app` 测。
- **CLI 版本夹具**：系统全局 `skills` 为其他版本，确认 app 仍用 pinned 1.7.4；pinned 包不可用时报错，不回落 latest。

### 3. 测试分层

| 层 | 工具 | 覆盖 | 不覆盖 |
|---|---|---|---|
| Rust 单测 | `cargo test` | Source/Lock/Inventory/Baseline/UpdateChecker/Backup/TaskManager/CLI invocation/Verifier/PathGuard/错误映射 | 真实 CLI、真实网络 |
| Rust 集成 | `cargo test` + temp `AppPaths` + fake executor | 命令表、安装/更新/断链/卸载、部分失败、取消、lock 损坏、备份失败 | 真实 Tauri 窗口 |
| 前端组件/hooks | Vitest + RTL + `mockIPC` | 列表/详情/筛选/向导/危险确认/错误态/徽章/键盘焦点 | 真实 Tauri 进程 |
| 真实 CLI sandbox | 手工或脚本 | `add`/`remove`/`add -l`、copy/symlink、lock 写入、回归 | GUI 行为 |
| Finder `.app` 手工 | release 构建 | PATH、环境探测、完整闭环、性能、单实例、安全 | 自动化 E2E |
| `list -g --json` 回归 | 真实 CLI sandbox | 操作后 skill 名称/路径、agent 目录、lock v3 | agents 字段真实性（已知高报） |

- 不引入 Tauri E2E / Playwright；自动化测试必须离线、确定性、每次独立 temp root。
- 测试命令：`cargo test`、`pnpm test`、`pnpm tauri build --bundles app`。

### 4. P0 验收清单（release blocking）

**读取 / 清单 / 详情**

- [ ] 从 Finder/Dock 启动 `.app`，环境探测正确解析 Node/npx/git；终端启动不参与通过判定。
- [ ] 只读 inventory 在 Node/CLI 缺失时仍可展示；变更按钮禁用并显示环境错误。
- [ ] Fixture 同时包含 managed / unmanaged / copy-only / broken link / local modified / path-missing；stale/ghost 不入主列表，只在异常弹窗。
- [ ] 正交徽章正确：更新状态、管理状态、警告互不覆盖；未挂载不计异常；`list --json.agents` 高报不影响挂载显示。
- [ ] 平铺列表、搜索、来源/agent/状态筛选、排序、选中重置、结果计数正确。
- [ ] 详情元数据、agent 挂载模式、SKILL.md 渲染/原文、frontmatter 隐藏、远程图片不加载、缺失 SKILL.md 警告。
- [ ] lock 损坏或 version != 3：全屏错误、变更禁用、lock 不变、不显示空列表。

**安装**

- [ ] Sandbox 真实 CLI 安装单 skill 与多 skill；显式 `-a <ids> -a universal`；canonical 与所选 agent 挂载事后校验通过。
- [ ] 同名已安装/未受管在预览禁用，后端也拒绝冲突；不发生覆盖。
- [ ] 预览解析成功与 fallback 均可用；单 skill 跳过选择；无 SKILL.md 空态；来源非法/私有/无网络有明确错误。
- [ ] CLI 静默回退 copy 被识别为 warning；partial failure 按 skill 展示并可重试。

**更新**

- [ ] 单 skill 更新前备份；本地改动警告 + checkbox；更新后校验 canonical/挂载/lock/基线。
- [ ] 更新全部跳过 local modified / unknown / path-missing / error，继续其余项，汇总失败并允许逐项重试。
- [ ] 不调用 `skills update` / `skills check`；copy 挂载更新时明确警告并备份，转换为 symlink 后校验。
- [ ] 断开状态被保留；更新不会重挂已断开的 agent。

**断链 / 卸载**

- [ ] 断链只 unlink 目标 agent 条目；canonical 与 lock 不变；其他 agent 挂载不受影响。
- [ ] copy 断链存在跨 agent 依赖时被阻止；universal/native 与无 canonical 的 agent-only skill 不提供断开。
- [ ] 彻底卸载仅对受管 skill；备份成功后才执行 `remove -g -s`；canonical、agent 路径、lock key 全部消失；unmanaged 禁用。
- [ ] copy-only 可卸载并备份；跨 agent copy 依赖时阻止；无 `remove --all`。

**安全 / 任务 / 交付**

- [ ] 破坏性操作必须勾选确认；备份失败则中止。
- [ ] 单写者队列：并发变更被拒绝/排队；不会有两个 CLI 同时写 lock。
- [ ] 来源、skillId、agentId、路径均服务端校验；恶意 source/skill 名不能注入命令或越界写。
- [ ] 前端无 shell/fs 权限；Markdown 原始 HTML/远程图片不执行；外部链接仅 https 经 opener；CSP 生效。
- [ ] 每次变更后 `npx skills list -g --json` 仍识别预期 skill；现有 agent 目录未被误删；fake executor 日志不含 `update`、`check`、`remove --all`、`-a '*'`、`--all`、`--copy`。
- [ ] Finder 启动的 release `.app` 可完成上述闭环；不要求 DMG/公证。

### 5. P1 验收清单（全过或记录已知问题）

- [ ] CLI 缺失：只读 inventory + 环境横幅；所有变更禁用。
- [ ] 网络离线 / GitHub 限流：列表可读；更新检查标「检查失败」，不显示「已是最新」；保留上次结果。
- [ ] 更新缓存：6 小时 TTL；只手动检查；启动/焦点切换不自动请求。
- [ ] CLI 版本：全局其他版本被忽略；pinned 1.7.4 不可用时报错，不回落 latest。
- [ ] 取消：preview 随时；install/uninstall 仅 spawn 前；update 可在 skill 之间；取消后 lock 无损，下次启动校验。
- [ ] 故障注入：CLI timeout、invalid JSON、partial failure、backup failure、copy fallback 均有正确错误/警告与重试入口。
- [ ] 空库存、无搜索结果、无选中、单个 skill 读取失败、权限不足均有独立状态。
- [ ] 同名多来源、copy-only、broken link、未挂载、stale lock 的展示与动作限制正确。
- [ ] 单实例：第二次启动聚焦已有窗口；退出时若有任务给出警告并保留 snapshot。
- [ ] 基础键盘/焦点：Tab 顺序、Esc 关闭、modal focus trap、关闭后焦点返回；不做完整 WCAG 审计。
- [ ] 性能达到第 7 节阈值。
- [ ] 50 与 100 条 fixture 均可用；真实 CLI sandbox 的 install/update/uninstall 闭环通过。
- [ ] 应用从 Finder 启动到可交互 < 2s；无主线程 >100ms 卡顿。

### 6. P2（尽力）

- 减少动效（`prefers-reduced-motion`）、长名称/长描述截断打磨、非关键动画、空态插画、app 图标/品牌。
- 完整 WCAG 审计、Tauri E2E、DMG/公证、真实 HOME 破坏性测试不在 v1 范围。

### 7. 性能阈值

Release `.app`，100 条 fixture，同一台目标 Mac，取 5 次运行中位数：

| 指标 | 目标 |
|---|---|
| 启动到可交互 | < 2s |
| Warm inventory | < 500ms |
| Cold inventory | < 1.5s |
| 搜索 / 筛选 | < 50ms |
| 详情切换（含 SKILL.md 读取） | < 150ms |
| 手动刷新 | < 1s |
| 任务首个事件 | < 500ms |
| 主线程单次卡顿 | < 100ms |
| 20 个来源的更新检查 | < 30s 完成或明确报错；UI 不冻结 |

- 进度以阶段 + `completed/total` 展示，不伪造百分比；任务进行中按钮禁用但界面可滚动/搜索。
- 不引入虚拟列表；超过 100 条的性能作为后续版本目标。

### 8. 手工测试矩阵

| ID | P | 场景 | 步骤 / 预期 | 环境 |
|---|---|---|---|---|
| M01 | P0 | Finder 启动与环境 | 从 Finder 启动 release `.app`；环境显示 Node/npx/git 与 PATH 来源；inventory 可读 | 真实 HOME 只读 |
| M02 | P0 | 库存状态 | 加载含 managed/unmanaged/copy-only/broken/local-modified/path-missing 的 fixture；徽章/筛选/异常弹窗正确；stale 不入列表 | sandbox |
| M03 | P0 | 列表交互 | 搜索、来源/agent/状态筛选、排序、清空筛选；计数与选中重置正确 | sandbox |
| M04 | P0 | 详情与 Markdown | 打开详情；渲染/原文切换；frontmatter 隐藏；恶意 HTML/远程图片不执行；缺失 SKILL.md 有警告 | sandbox |
| M05 | P0 | 安装单个 | 预览 → 选 1 skill + 1 agent → 安装；canonical + 链接存在；`list -g --json` 识别 | sandbox + 真实 CLI |
| M06 | P0 | 安装多个 | 选 2 skill + 多 agent；显式 `-a` + universal；全部挂载校验；无同名覆盖 | sandbox + 真实 CLI |
| M07 | P0 | 安装边界 | 同名禁用；单 skill 跳过选择；无 SKILL.md 空态；`-l` 解析 fallback；非法来源报错 | sandbox |
| M08 | P0 | 更新单个 | 本地改动 fixture；警告 + 勾选；备份生成；更新后校验 canonical/挂载/lock/基线 | sandbox + 真实 CLI |
| M09 | P0 | 更新全部 | modified/unknown/error 被跳过；其余继续；失败项重试；未调用 `update/check` | sandbox + fake / 真实 CLI |
| M10 | P0 | copy 转换 | copy 挂载更新；警告 + 备份；更新后为 symlink；其他 agent 正确重链 | sandbox |
| M11 | P0 | 断链 | 对 symlink agent 断开；canonical/lock 不变；其他 agent 不受影响；列表显示未挂载 | sandbox |
| M12 | P0 | 断链依赖 | copy 被其他 agent symlink 引用；断开被阻止并解释；状态不变 | sandbox |
| M13 | P0 | 卸载 | 受管 skill 卸载；备份存在；canonical/agent/lock 无残留；`list -g --json` 消失；unmanaged 禁用 | sandbox + 真实 CLI |
| M14 | P0 | 卸载边界 | copy-only 可卸载；跨 agent 依赖被阻止；无 `remove --all` | sandbox |
| M15 | P0 | 安全 | 恶意 source/skill/agent 输入；路径穿越；Markdown XSS/远程图片；shell 注入均被拒绝 | sandbox |
| M16 | P0 | 回归 | M05–M14 后对比 `npx skills list -g --json` 名称/路径；agent 目录未被误删；lock v3；fake 日志无禁用 flag | sandbox |
| M17 | P0 | 性能 | 100 条 fixture release `.app`；达到第 7 节阈值；任务进度可见 | release + sandbox |
| M18 | P1 | CLI 缺失 | 隐藏 Node/CLI；只读 inventory + 环境横幅；变更禁用；不崩溃 | sandbox |
| M19 | P1 | 网络失败 | 断网/限流；更新检查标失败；不误报最新；列表可用 | sandbox |
| M20 | P1 | 取消与部分失败 | fake 慢 CLI；preview 取消；install 取消仅 spawn 前；update 在 skill 间取消；partial failure 可重试 | fake |
| M21 | P1 | CLI 版本 | 全局 skills 改为其他版本；确认使用 1.7.4；pinned 不可用时报错不回落 latest | sandbox |
| M22 | P1 | 单实例 / 退出 | 二次启动聚焦已有窗口；任务中退出有警告；snapshot 保留；下次启动校验 | release |
| M23 | P1 | 键盘 / 焦点 | Tab 顺序、Esc 关闭、modal focus trap、关闭后焦点返回 | release |
| M24 | P1 | 更新检查成本 | 20 个来源；30s 内完成或报错；UI 可操作；缓存 6h 生效 | sandbox + 网络 |

### 9. 回归配方

在 sandbox 中，每次 install / update / detach / uninstall 后：

```bash
HOME="$SBX_HOME" XDG_STATE_HOME="$SBX_STATE" npx -y skills@1.7.4 list -g --json > after.json
```

- 对比变更前后的 skill 名称 / path 集合，预期增删符合操作。
- 检查 canonical、各 agent 目录、lock v3 key；确认未受管目录未被删除。
- 断言 fake executor 日志不含 `update`、`check`、`remove --all`、`-a '*'`、`--all`、`--copy`。
- 记录操作日志、备份路径与校验结果。

### 10. 签收

- P0 全勾选；P1 全勾选或列出已知问题（owner / 目标版本）；P2 记录待办。
- 无 P0 数据丢失、权限越界、CLI 生态回归。
- 用户确认 Finder `.app` 主流程可用；验收记录与命令写入最终规格。
