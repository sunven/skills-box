# 更新、断链与卸载的安全语义

Type: grilling
Status: resolved
Blocked by: 01, 02, 03

已知事实（来自「更新检测机制与成本」）：`skills check` 会实际执行更新；`skills update` 不保留 copy 模式、不按原 agent 集合更新、无回滚。需要在本票据中决定是否改为逐 skill 调用 `skills add` 并显式指定 agent/copy。

已知事实（来自「全局安装的真相与异常形态」）：`skills update -g` 会按“宿主 + universal”重装，可能复活 stale 条目、把 copy 改成 symlink；`remove --all` 会删除未受管 skill；lock 写入非原子。GUI 必须逐名操作、备份 lock、串行化写入，并拒绝 version != 3 或损坏的 lock。

已知事实（来自「CLI 能力与 JSON 契约」）：`remove -a` 的 per-agent 删除不可靠，可能彻底卸载或留下悬空链接；单 agent 断链必须由 GUI 直接 unlink，并检查跨 agent 链接依赖；彻底卸载用 `remove -g -s X -y` 并做事后校验。`update` 会重挂所有检测 agent、覆盖本地改动、丢失 copy/断链状态，应用更新应构造 `add` 并显式传当前挂载 agents。

已知事实（来自「安装向导的来源预览与选择契约」）：安装向导不做升级或换源覆盖；同名已安装禁用；更新走独立流程。

## Question

在 CLI 契约、磁盘状态模型与更新检测结论的基础上，确定三种生命周期动作的语义与安全规则：

- 更新：单 skill / 全部；检查与执行分开；本地改动与 lock 不一致时的警告与选择；更新失败 / 部分失败的回滚或保留；是否显示来源与目标版本。
- 断开：只移除某个 agent 的挂载，保留中心副本与 lock；copy 模式下“断开”是删除副本；操作后列表如何显示“未挂载任何 agent”。
- 彻底卸载：删除中心副本、所有 agent 挂载、lock 条目；是否提供“仅卸载、保留目录”的选项；二次确认与不可逆提示。
- 冲突与边界：同一 skill 多来源、手工修改、断链、CLI 不在时的降级；操作日志 / 可恢复性是否在 v1 范围内。

输出：一张“动作 × 影响对象 × 前置检查 × 确认文案 × 失败处理”的决策表，写入 `## Answer`。只闭合本票据。

## Answer

用户确认（2026-10-28）。

### 更新

- **执行方式**：GUI 自己重建 `add`，逐 skill 串行执行：
  `skills add <source> -g -s <skill> -a <当前物理挂载的非 universal agents> -a universal -y --json`
  不调用 `skills update` / `skills check`（后者会实际更新）。
- **单 skill / 更新全部**：检查与执行分离；更新全部只处理 `update-available`，跳过本地改动、unknown、path-missing、error；失败继续处理其余项，完成后汇总并允许逐项重试。
- **本地改动**：GUI 维护 per-skill baseline fingerprint（安装、首次扫描、更新后记录），并与 lock 的 `skillFolderHash` 交叉检查；有改动或无法判断时必须勾选警告，并先备份。
- **copy 模式**：带 copy 挂载的 canonical skill 更新时转为符号链接，明确警告并备份；没有 canonical 的 copy-only skill v1 不可更新，建议卸载后重装。
- **展示**：来源、当前/目标 tree hash、受影响 agent、更新后的挂载模式。
- **失败**：不自动回滚；备份保留，逐 skill 校验 canonical、挂载与 lock；校验失败按失败处理，不宣称成功；失败项单独重试。

### 断链

- 仅对受管且有 canonical 的 skill 提供；由 GUI 直接对目标 agent 的条目 `unlink`，绝不调用 `remove -a`，不动 canonical/lock。
- copy 挂载仅在没有任何其他 agent 符号链接解析到该目录时删除；存在跨 agent 依赖时阻止并解释，不自动改指。
- universal/native 与没有 canonical 的 agent-only skill 不提供断开，应走卸载。
- 断开后 canonical 保留；若已无挂载，列表显示「未挂载」；后续更新按当前物理挂载集重建，尊重断开状态。
- 断链保持只读异常，不提供修复/清理动作。

### 彻底卸载

- 仅对受管 skill（有 lock）提供；unmanaged 禁用。
- 前置：备份 lock、canonical、copy 挂载目录与 attachment manifest；存在跨 agent copy 依赖时阻止；确认按钮需勾选解锁。
- 执行 `skills remove -g -s <name> -y`（不带 `-a`），随后逐项校验 canonical、所有 agent 路径、lock key 均消失；exit 0 但状态未变按失败。
- 不提供「仅卸载、保留目录」的独立动作；备份即保留。
- copy-only 且有 lock 的 skill 可卸载；无 lock 的 unmanaged 不可卸载。
- 不使用 `remove --all`，不做批量卸载。

### 备份、日志与失败

- 每次更新/卸载前强制备份：lock 快照、canonical、copy 挂载目录、attachment manifest → `~/Library/Application Support/com.sunven.skills-box/backups/<timestamp>-<action>-<skill>/`。
- 追加 JSONL 操作日志：时间、动作、skill、agents、结果、备份路径、错误；日志写入失败不阻断操作。
- 备份失败则中止操作。v1 不提供恢复 UI，只在结果中显示备份路径；不自动回滚。
- 所有变更串行执行；变更前快照 lock；拒绝 `version != 3` 或损坏的 lock。
- CLI 缺失时禁用所有变更，展示环境错误。

### 决策表

| 动作 | 影响对象 | 前置检查 | 确认 | 失败处理 |
|---|---|---|---|---|
| 更新单个 | canonical + 当前挂载 + lock + 基线 | source/ref 可用；lock v3；本地改动检查；备份 | 来源、当前/目标 hash、agents、模式转换、本地改动勾选 | 保留备份；逐项校验；失败项重试；不自动回滚 |
| 更新全部 | 所有 update-available | 跳过 modified/unknown/path-missing/error；串行；备份 | 汇总更新数与跳过项 | 继续其余项；汇总失败；逐项重试 |
| 断开 | 单个 agent 条目 | canonical 存在；非 universal/native；无跨 agent 依赖（copy） | 显示 agent、模式、保留 canonical/lock、其他 agent | 阻止跨依赖；失败不触碰 canonical/lock |
| 彻底卸载 | canonical + 所有挂载 + lock + 基线 | 有 lock；备份；无跨 agent copy 依赖 | 删除范围 + 勾选解锁 | CLI 后逐项校验；状态未变按失败；保留备份 |
| 断链异常 | 只读 | 无 | 无动作 | 仅提示，不修复 |

> 架构细化（来自「技术架构与命令边界」）：copy 断链也走备份管线（copy 目录 + lock 快照）；断链操作同样进入单写者队列。
