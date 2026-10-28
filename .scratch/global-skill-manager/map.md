# Skills Box：全局 Skill 管理器 v1

Type: wayfinder:map
Status: done

## Destination

产出一份可直接交给实现会话的 v1 规格与关键决策记录：在现有 Tauri 2 + React 项目中实现 macOS 优先、个人使用的全局 skill 管理器。v1 覆盖列表/详情、从公开 GitHub 来源安装、手动检查与更新、彻底卸载、按 agent 断开/恢复链接。领域上以 `~/.agents/skills` 与 `~/.agents/.skill-lock.json` 为唯一真相，各 agent 目录只是挂载。

## Notes

- 领域：Agent Skill 的全局安装与生命周期管理；本机形态是中心仓库 + lock 文件 + 各 agent 目录的符号链接/副本。
- 每个会话先读仓库根目录的 `GLOSSARY.md`；按票据类型调用 `research`、`grilling`、`domain-modeling`、`prototype`。
- 产品取向：macOS 优先、个人使用；v1 界面使用中文，技术名词保留英文。
- UI 取向：React 19 + Vite + Tailwind CSS + shadcn/ui；主界面倾向三栏，安装走向导。
- 依赖取向：v1 允许依赖本机 Node 22.20+ 与 `npx skills`；启动时检测并给出缺失指引。
- 变更入口尚未定案：候选是“以 `npx skills` 为唯一变更入口，读取优先 `list -g --json` + lock”，由「技术架构与命令边界」票据决定。
- 安全取向：更新、删除、断链必须预览与确认；不自动更新；检测本地改动与 lock 不一致时警告。
- 已知陷阱（来自「更新检测机制与成本」）：v1.7.4 的 `skills check` 实际执行更新，不是只读检查；GUI 不得调用它做状态检查，需自建按来源分组的只读检查器。详见 [更新检测机制与成本](issues/03-update-detection.md)。
- 已知风险（来自「更新检测机制与成本」）：`skills update` 不保留 copy 模式与原始 agent 集合、无回滚；逐 skill 更新策略由「更新、断链与卸载的安全语义」决定。
- 已知陷阱（来自「全局安装的真相与异常形态」）：lock v3 是来源账本，不记录安装模式与每个 agent 的挂载；`skills list --json.agents` 会高报（本机 41 vs 实际 10），挂载必须按各 agent 物理目录判定。详见 [全局安装的真相与异常形态](issues/02-install-state.md)。
- 已知风险（来自「全局安装的真相与异常形态」）：`skills update -g` 可能复活 stale 条目、把 copy 改成 symlink；`remove --all` 会删除未受管 skill；lock 写入非原子、损坏时会被读成空。GUI 必须逐名操作、备份、串行化，并拒绝 version != 3 或损坏的 lock。
- 已知事实（来自「Tauri 执行外部 CLI 与打包」）：Finder/Dock 启动的 .app 只有最小 PATH；必须解析 Node 并把 `<node_dir>` 加入子进程 PATH，`npx` 还会二次找 node。推荐 `tokio::process::Command` + `ProcessExecutor`、per-task `Channel`、进程组取消、单写者队列与 mutation 前 lock 快照；打包 `.app` + ad-hoc，依赖系统 Node/npx/git。
- 已知陷阱（来自「CLI 能力与 JSON 契约」）：`add` 必须显式传 `-a` 并推荐追加 `-a universal`；省略 `-a` 会让所有 skill 失败，单个非 universal agent 默认只 copy、不写 canonical。`remove -a` 不能可靠断链，单 agent 断链必须由 GUI 直接 unlink；彻底卸载用 `remove -g -s` 并事后校验。固定 `skills@1.7.4`，设置 `DISABLE_TELEMETRY=1`/`DO_NOT_TRACK=1`，`NO_COLOR` 无效，只解析 `--json`。
- 本 effort 只产出决策与规格，不实施生产代码；唯一例外是「主界面与关键流程原型」的可点击原型。

## Decisions so far

- [更新检测机制与成本](issues/03-update-detection.md): `skills check` 实为更新别名且会静默失败；GUI 需自建按来源分组的只读更新检查（GitHub tree SHA vs `skillFolderHash`），50–100 个 skill 约 5–20 请求，6 小时缓存足够。
- [全局安装的真相与异常形态](issues/02-install-state.md): lock v3 只是来源账本，不含安装模式与 agent 映射；49/61 差异来自 21 条 stale lock 与 9 个未受管目录；挂载必须看物理目录，`list --json.agents` 会高报；更新/删除存在复活 stale 与误删未受管 skill 的风险。
- [Tauri 执行外部 CLI 与打包](issues/04-tauri-cli.md): Finder 启动的 app 只有最小 PATH，必须解析 Node 并注入 `<node_dir>`；推荐 Tokio 执行器 + Channel 流式进度 + 进程组取消 + 单写者队列 + lock 快照；`.app` + ad-hoc，依赖系统 Node/npx/git。
- [CLI 能力与 JSON 契约](issues/01-cli-contract.md): 唯一稳定读取接口是 `list -g --json`，但缺 description/时间/hash/挂载且会高报 agent；`add` 必须显式 `-a`（推荐再加 `-a universal`），`remove -a` 不可靠、单 agent 断链由 GUI unlink；`update` 会重挂所有 agent 并覆盖本地改动；固定 `skills@1.7.4`。
- [主界面与关键流程原型](issues/05-ui-prototype.md): 选 A 三栏工作台；未受管 skill 留在主列表只读；stale/ghost 只在异常弹窗；安装与危险操作都用 modal，破坏性操作需 checkbox 解锁。
- [安装向导的来源预览与选择契约](issues/06-install-wizard.md): v1 只接受公开 GitHub 根仓库；预览解析 `add -l` 的 name+description；agent 只展示检测到的并默认不碰未受管目录；每来源一次 `add ... -a ... -a universal -g -y --json`，写入阶段禁止取消，完成后逐 skill 校验。
- [更新、断链与卸载的安全语义](issues/07-lifecycle-safety.md): 更新逐 skill 重建 `add`、不调用 `update/check`；更新/卸载前强制备份 lock + canonical + copy 目录 + manifest；断链由 GUI unlink 并在跨 agent 依赖时阻止；卸载用 `remove -g -s` + 事后校验；不自动回滚。
- [清单与详情的信息规则](issues/08-inventory-detail.md): 平铺列表 + 正交徽章（更新 / 管理 / 警告）；未受管与仅副本进主列表但只读 / 受限；stale/ghost 只在异常弹窗；lock 损坏全屏错误；手动刷新；动作只放页头、详情与 agent 行。
- [技术架构与命令边界](issues/09-architecture.md): Rust 原生读取 lock / 目录 / frontmatter，CLI 只做 install / update / uninstall，断链原生 unlink；单写者任务队列 + Channel；结构化 AppError；安全渲染 / CSP；`.app` + 系统 Node；ADR 0001。
- [v1 验收标准与测试矩阵](issues/10-acceptance.md): P0 全过 + P1 无阻塞；sandbox 破坏性测试；100 skills 性能阈值；CLI 回归 + 禁止命令断言；Finder `.app` 签收。
- [v1 规格成稿与评审](issues/11-spec-synthesis.md): 规格通过评审，入口 [spec.md](spec.md)；范围、Phase 0–6、ADR 0001、验收矩阵与未决项已记录。

## Not yet specified

> v1 规格已交付。以下为明确延后的非阻塞问题，详见 [spec.md](spec.md) §14。

- 偏好持久化（主题、筛选、默认 agent）与首次启动引导。
- 单窗口之外的菜单栏 / 托盘 / 多窗口形态；当前默认单窗口，尚未定案。
- app 名称、图标与品牌是否进入 v1 规格。
- 前端错误边界、崩溃恢复与安装/更新完成后的系统通知。

## Out of scope

- 生产代码实施：本 effort 到可实施规格为止，实现另起 effort；可点击原型除外。
- 项目级 skill 管理：用户明确 v1 只做全局。
- 内置 SKILL.md 编辑、新建 / init：v1 只读展示，编辑回退到外部编辑器。
- skills.sh 搜索 / 发现与市场浏览：v1 只按已知来源安装。
- 私有仓库与鉴权：v1 只支持公开 GitHub 来源。
- 安装来源的 subdirectory / tag/commit 锁定：v1 只装默认分支的根仓库，需要锁定时回 CLI。
- `--copy` 复制安装模式：v1 只用符号链接。
- 安装预览内的仓库搜索与 plugin 折叠。
- 安装时覆盖同名已安装 / 未受管 skill：v1 禁用，先卸载再装。
- 一次安装多个来源。
- 记住安装向导的上次 agent 选择。
- 在安装向导里启用 / 管理全部 79 个受支持 agent：v1 只展示本机检测到的。
- 按来源分组 / 折叠列表与画廊主视图：v1 只用三栏平铺列表。
- 文件系统监听 / 后台轮询：v1 手动刷新 + 变更后刷新。
- 搜索 SKILL.md 正文与路径：v1 只搜名称 / 描述 / 来源。
- 列表虚拟化 / 分页：v1 直接渲染，目标规模 100 条以内。
- 自包含 Node sidecar 或纯 Rust 安装器：v1 依赖系统 Node + pinned npx CLI。
- Tauri E2E / Playwright：v1 用 Rust fake executor + 前端组件测试 + 手工验收。
- 启动时自动检查更新 / 后台轮询：v1 只手动检查，6 小时缓存。
- 备份自动清理：v1 不自动删除备份。
- 外部 CLI 并发修改的实时协调：v1 单实例只防 GUI 双开，外部改动需手动刷新并在操作后校验。
- skills CLI 自动升级 / 跟随 latest：v1 固定 `skills@1.7.4`。
- Windows / Linux：macOS 优先的个人工具。
- skill 自动更新与后台轮询：更新永远手动触发。
- 遥测。
- npm `experimental_sync`。
- `skills use` 启动 agent。
- 备份、同步、多设备。
- 自定义 skill 目录。
- 异常一键修复：v1 只做只读提示，修复留给后续版本。
- 断链修复 / 重定向：v1 只读提示。
- 自动回滚与恢复 UI：v1 强制备份并写 JSONL 日志，但只在结果中显示备份路径，不提供恢复界面、历史操作面板或诊断面板。
- 「仅卸载、保留目录」的独立动作：v1 不提供，备份即保留。
- copy-only skill 的更新：v1 不支持，需卸载后重装。
- 批量卸载与 `remove --all`：v1 不做。
- 本地目录 / 拖拽安装：v1 来源仅限公开 GitHub。
- 多语言 UI：v1 只做中文界面。
- 开发者签名与公证：v1 只用 ad-hoc 本机签名。
- 完整 WCAG 无障碍审计与减少动效打磨：v1 只保证基础键盘 / 焦点，减少动效为 P2。
- 应用自身的自动更新。
- skill 的发布 / 分享 / 市场发布。
