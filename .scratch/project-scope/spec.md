# Skills Box — 项目作用域规格

Status: Draft for review
Date: 2026-10-30

> 本规格由一轮 grilling 会话的决策合成。实现者应把本文件当入口，细节以这里为准；术语以词汇表为准。
>
> - 术语表：[GLOSSARY.md](../../GLOSSARY.md)
> - ADR：[0011-project-scope-registry.md](../../docs/adr/0011-project-scope-registry.md)、[0012-project-update-content-hash.md](../../docs/adr/0012-project-update-content-hash.md)
> - 实施票据：[issues/](issues/)
> - 既有全局规格：[../global-skill-manager/spec.md](../global-skill-manager/spec.md)
> - 自动检查规格：[../auto-update-check/spec.md](../auto-update-check/spec.md)

---

## 1. 目标与成功标准

### 1.1 目标

在现有全局 skill 管理之上，增加与全局完全对等的**项目作用域**：

- 注册、切换、重新定位、遗忘项目目录。
- 查看项目内 skill 的清单、详情、内容、挂载、更新状态与异常。
- 在项目内安装、挂载、断链、卸载、检查更新、更新。
- 每个 scope 有独立的 canonical、lock、挂载集合、更新检查状态与错误状态。

### 1.2 成功标准

- 全局作用域的既有行为、验收与安全约束不回归；现有全局 skill 的 ID、lock、挂载不被改写。
- 项目内的安装/更新/卸载结果与在项目根直接运行 `skills@1.7.4`（不带 `-g`）一致，且应用额外保证 canonical + symlink 模型与事后校验。
- 一个项目的锁损坏、路径不可用或权限错误不影响其他 scope。
- 在应用外由 CLI 创建的项目 skill、挂载与异常，能在项目清单中如实呈现。
- 项目更新检查对同一 lock 的判定与 CLI 的 `computedHash` 语义一致。

### 1.3 非目标（Out of scope）

- 自动扫描磁盘发现项目；项目重命名；项目排序拖拽。
- 删除项目文件、批量清理项目 skill、"移除项目并删除内容"。
- 在 scope 之间移动/复制 skill，或把全局 skill 批量迁移进项目。
- 跨 scope 合并列表、跨 scope 搜索或批量更新。
- 应用写 `skills-lock.json` 或 `~/.agents/.skill-lock.json`。
- 非 GitHub 来源的更新检查（与全局一致，只展示、不检测）。
- `experimental_sync`、`skills use`、`skills init`、`remove --all`。
- 项目级环境探测（Node/npx/git 仍用全局 EnvReport）。
- Windows / Linux。

---

## 2. 场景

1. **注册项目**：用户从侧栏「+ 添加项目」选择一个目录；应用校验后加入列表并激活。
2. **切换**：在「全局」与各项目之间切换；每个 scope 有独立的清单、筛选与更新徽章。
3. **项目安装**：在项目作用域安装 skill，默认只写 `<项目>/.agents/skills`，按需勾选 agent 挂载。
4. **项目更新**：切到项目时按 6 小时阈值自动检查；查看候选后手动更新。
5. **项目卸载**：在项目内断链或彻底卸载，不影响全局或其他项目。
6. **失效处理**：项目目录被移动或删除后，条目保留并标记「路径不可用」，可重新定位或从列表移除。
7. **排障**：项目锁损坏、挂载断链、未受管 skill、符号链接逃逸等按 scope 隔离呈现。

---

## 3. 领域模型

### 3.1 Scope、Project 与 Project root

- **Scope**：`global` 或某个已注册项目。同一时刻只有一个激活 scope。
- **Project**：应用数据中的注册条目，字段为 `id`（稳定 UUID）、`root`（绝对路径）、`displayName`（默认为根目录名，不可重命名）、`addedAt`、`lastOpenedAt`。
- **Project root**：项目条目指向的目录；重新定位改变 root，不改变项目 id。
- 注册表持久化在应用数据 `projects.json`（带版本号），并记录 `lastActiveScope`。
- 按 `lastOpenedAt` 倒序展示；每次激活都会更新它。
- 注册表损坏时不静默覆盖：保留原文件、回退为空项目列表并显示应用级警告；全局作用域仍可用。

**准入规则**：

- 必须是已存在、可读的目录；按规范化（canonicalize）后的绝对路径去重。
- 拒绝 `/`、用户 home 本身、全局 canonical `~/.agents/skills` 及其内部。
- 允许项目根互相嵌套或重叠；每个 scope 独立管理，不做特殊合并。
- 添加只记录路径，不创建 `.agents` 等任何目录；创建发生在安装时。

### 3.2 项目磁盘布局

项目作用域完全遵循 pinned CLI 的项目布局：

| 产物 | 路径 |
|---|---|
| canonical store | `<root>/.agents/skills` |
| 项目锁 | `<root>/skills-lock.json` |
| catalog agent 挂载 | `<root>/<agent.skillsDir>`（如 `.claude/skills`） |
| extra agent 挂载 | `<root>/<配置的项目相对目录>` |
| universal agent | 直接读取 `<root>/.agents/skills`，无独立挂载 |

全局布局不变：`~/.agents/skills`、`~/.agents/.skill-lock.json`、各 agent 全局目录。

### 3.3 技能身份

- 身份是 **(scope, name)**；同名 skill 在全局与多个项目中是彼此独立的 skill。
- 全局 skill ID 保持现有形态不变；项目 skill ID 由项目 id 前缀 + 名称组成，对前端不透明。
- 安装同名冲突判断只在当前 scope 内进行；全局已存在同名 skill 不影响项目安装，反之亦然。

### 3.4 项目锁

- 只读解析 `<root>/skills-lock.json`，版本 1；字段为 `source`、`sourceUrl?`、`ref?`、`sourceType`、`skillPath?`、`computedHash`。
- 缺失 lock 是正常空状态；JSON 损坏或版本不受支持是该 scope 的错误状态（见 §8）。
- 项目锁没有安装/更新时间，UI 对相关字段显示「—」。
- 项目锁不记录挂载 agent；挂载一律由文件系统推导，与应用不写 lock 的既有约束一致。

### 3.5 Agent 挂载

- 清单扫描范围内所有 catalog agent 的**项目目录**（是否存在），不要求该 agent 在此机器全局被检测到；以发现应用外创建的挂载。
- 挂载形态仍为符号链接或复制；指向 canonical、断链、复制模式、Copy-only 的判定复用全局规则。
- **安装/挂载目标集合**（向导与挂载对话框）为：全局已检测到的 catalog agent + 已配置项目目录的 extra agent。项目目录尚不存在也可以作为目标，由安装或挂载创建。
- 项目内不存在 universal 目标选择：安装始终追加 `universal`，由 CLI 写入 canonical。

### 3.6 Extra agent

- `agents.json` 的条目新增可选字段 `projectSkillsDir`：相对项目根的项目内 skills 目录。
- 校验：非空、相对路径、规范化后仍位于项目根内、不等于 `.agents/skills`、在所有 extra 之间唯一。
- 内置与现有 extra 默认未配置；未配置的 extra 只出现在全局作用域。
- 已配置的 extra：项目目录存在时参与清单扫描；无论是否存在，都出现在项目的安装/挂载目标中（挂载时创建目录）。
- 修改或清空 `projectSkillsDir` 的守卫见 §7.5。

### 3.7 更新状态与指纹

- 全局：沿用 Git tree SHA 比较（`skillFolderHash`），行为不变。
- 项目：`computedHash` 是 CLI 的 SHA-256 内容指纹（64 位十六进制）。检查按 (source, ref) 分组，解析最新 commit，按 commit 下载一次来源归档，用与 CLI 相同的算法计算每个 skill 文件夹的 SHA-256，与 lock 比较。
- 两条检查路径共用 `update_target` 的来源/ref 解析与安全校验（ADR 0009）；项目路径只替换指纹域与远端材料，`git_tree.rs` 保持不变。
- 指纹算法必须与 pinned CLI 一致：递归收集文件，排除 `.git` 与 `node_modules`，相对路径使用 `/`，按路径排序，依次 hash 路径与内容，输出 SHA-256 十六进制。
- 指纹排序依赖 CLI 的 `localeCompare`（ICU）。当前实现只覆盖可排序的 ASCII 路径；含非 ASCII 相对路径的 skill 明确报告「不支持检测」，而不是用猜测的排序冒虚假更新的风险。这类项目因此不会显示 Current/Update；这是已记录的限制（ADR 0012），不在此处实现 ICU。
- 仅 `sourceType=github` 且来源为公开 GitHub 的条目可检查；其余显示「不支持检测」，与全局一致。
- 更新状态按 scope 保存：`update-check.json` v2 为 `scopeId -> state`；v1 数据迁移为全局作用域条目。

### 3.8 异常

项目作用域复用全局异常分类：未受管、Copy-only、Stale lock entry、断链、缺失 canonical、缺失 skillPath、无效指纹等。额外增加：

- **路径不可用**：项目根不存在、不是目录或不可读；属于 scope 状态，不是清单异常。
- **逃逸链接**：项目 canonical 或 agent 项目目录解析后位于项目根之外；标记异常并禁止内容读取。

---

## 4. 信息架构与关键流程

### 4.1 侧栏与 scope 切换

- 左侧栏顶部固定「全局」入口与「项目」分组：
  - 项目行显示名称、路径副标题、更新徽章；另可显示「不可用」「错误」徽章。
  - 分组底部为「+ 添加项目」。
  - 项目按 `lastOpenedAt` 倒序；当前 scope 高亮。
- 下方的技能筛选器、搜索、列表与详情只作用于当前 scope。
- 切换 scope 时：清空选择项，筛选与搜索重置为默认；不跨 scope 继承。
- 启动时恢复 `lastActiveScope`；若该项目不可用则回退到全局，条目保留并标记。

### 4.2 添加 / 重新定位 / 遗忘项目

- **添加**：系统文件夹选择器（Tauri dialog 插件），校验后写入注册表并激活。
- **重新定位**：仅对不可用项目提供；校验新目录后保留项目 id 与 `addedAt`，更新 `root`、`displayName`、`lastOpenedAt`，清除该 scope 的更新检查状态。
- **遗忘**：仅删除注册条目与该 scope 的检查状态，绝不删除磁盘文件；若遗忘的是当前 scope，切回全局。

### 4.3 项目视图

- 头部展示项目名、项目路径、canonical 路径、锁状态、上次检查时间与手动刷新入口。
- 空状态区分三种：项目无任何 skill（引导安装）、项目路径不可用（提供重新定位/遗忘）、项目错误（展示错误与重试）。
- 详情与内容浏览沿用全局交互；「打开文件夹」指向项目内路径。

### 4.4 安装向导

- 向导在**当前激活 scope** 内运行，标题与确认页明确展示目标 scope 与 canonical 路径（全局或 `<项目>/.agents/skills`），并提供切换 scope 的快捷入口。
- 目标 agent 默认不勾选；列表为 §3.5 的目标集合；文案说明 universal 与 canonical 始终写入。
- 预览与安装的 CLI 均在目标 scope 对应的 cwd 中运行（项目根，或全局的隔离 cwd）。
- 校验、去重、同名冲突、Copy-only/Stale 的拦截规则只在当前 scope 内生效。

### 4.5 挂载 / 断链 / 卸载

- 挂载对话框列出当前 scope 的目标集合中尚未挂载该 skill 的 agent；extra agent 由应用原生创建链接。
- 断链为原生 unlink，只移除该项目 scope 内的挂载；canonical 与 lock 不变。
- 卸载只作用于当前 scope：项目内运行不带 `-g` 的 `remove`，并清理项目内 extra 链接。

### 4.6 更新检查

- 按 scope 独立检查，沿用 6 小时阈值：
  - 启动时检查当前激活 scope（恢复项目则检查该项目）。
  - 切换到某 scope 时，若其上次检查超过 6 小时则自动检查。
  - 手动刷新只重置当前 scope 的计时与状态。
- 侧栏徽章显示各 scope 最近一次检查得到的更新候选数；未检查过不显示徽章；检查失败以错误徽章呈现。
- 检查报告、来源失败计数、last check 均按 scope 展示与保存；一个 scope 的检查失败不影响其他 scope。

### 4.7 更新

- 只有当前 scope 的检查产生候选后，用户才能手动更新；不自动更新。
- 更新重建为带 cwd 的 `add`，保留该 skill 在当前 scope 的原挂载集合，并始终追加 `universal`。
- 更新后重新创建 extra agent 链接并执行与安装相同的事后校验。

### 4.8 错误与不可用状态

- 侧栏条目区分「不可用」（路径问题）与「错误」（锁损坏、权限、内容异常）。
- 不可用：不加载清单、不检查更新、禁止变更；提供重新定位与遗忘。
- 错误：展示错误与重试；禁止该 scope 的变更；不影响其他 scope；不出现全局横幅。
- 环境横幅仍为全局（Node/npx/git/CLI 不可用时所有变更入口禁用）。

### 4.9 Extra agent 管理

- 「管理 agent」对话框为 extra agent 增加可选的「项目内 skills 目录」字段，并做 §3.6 校验。
- 删除 extra agent：若任一已注册项目路径不可用，则阻止删除并列出这些项目；否则先卸载全局与所有已注册项目中的该 extra 挂载，再删除定义。
- 修改/清空 `projectSkillsDir`：若任一已注册项目（含不可用项目）可能仍存在该 extra 的挂载，则阻止修改并提示先卸载；不可用项目需先重新定位或遗忘。

---

## 5. CLI 调用矩阵

### 5.1 允许的调用

| 操作 | 全局 | 项目 |
|---|---|---|
| 预览 | `add <src> -l -g`（cwd=隔离目录） | `add <src> -l`（cwd=项目根） |
| 安装 | `add <src> -g -s … -a … -y --json` | `add <src> -s … -a … -y --json`（无 `-g`） |
| 更新 | 同上，按 lock 重建 | 同上，按 lock 重建（cwd=项目根） |
| 卸载 | `remove -g -s <name> -y` | `remove -s <name> -y`（cwd=项目根） |

安装与更新的 `-a` 列表始终包含 `universal` 与用户选中的 catalog agent；extra agent 不在 CLI 参数中，由应用原生挂载。

### 5.2 禁止的调用

- `skills update` / `skills check`（会直接安装，无 dry-run）。
- `--all`、`remove --all`、未指定 skill 的交互式删除。
- 项目操作使用 `-g`；全局操作省略 `-g`。
- 在项目根之外创建项目产物。

### 5.3 cwd 与环境

- 全局操作沿用 ADR 0001 的隔离 cwd；项目操作以项目根为 cwd（ADR 0005）。
- 子进程环境约束（`env_clear`、最小 PATH、`DISABLE_TELEMETRY=1`、stdin null）不变。
- 项目路径作为 CLI 参数时不做 shell 拼接；沿用现有参数数组调用。

### 5.4 验证

- 安装/更新后校验：canonical 目录存在、lock key 存在、每个 catalog 挂载为预期形态；extra 链接由应用创建后单独校验。
- 挂载/断链/安装链接复用统一 `attachment_target` 门控（ADR 0005）；项目根与 extra 项目目录作为目标路径上下文传入，不复制资格判定。
- 卸载后校验：canonical、lock key、该项目 scope 内所有挂载（含 extra）均已消失。
- 退出码不单独作为成功依据；部分失败按现有 mutation 结果模型呈现。

---

## 6. 命令契约

### 6.1 Scope 参数

所有清单/详情/内容/开放目录/检查/变更命令增加显式 scope：

- 读取：`get_inventory(scope)`、`get_skill_detail(scope, skill_id)`、`list_skill_dir(scope, skill_id, path)`、`read_skill_file(scope, skill_id, path)`、`open_skill_folder(scope, skill_id)`、`open_skill_file(scope, skill_id, path)`。
- 更新：`check_updates(scope, skill_ids?)`、`get_update_check_state(scope)`。
- 变更：`start_preview(source, scope)`、`start_install(request + scope)`、`start_update(scope, skill_ids)`、`start_uninstall(scope, skill_id)`、`attach_agent(scope, skill_id, agent_id)`、`detach_agent(scope, skill_id, agent_id)`。
- 环境与全局设置命令保持无 scope。

### 6.2 新增命令

- `list_projects()`：返回注册表与 `lastActiveScope`。
- `add_project(root)`：校验并加入注册表，返回项目。
- `relocate_project(id, root)`：校验并更新路径。
- `forget_project(id)`：移除注册表条目与检查状态。
- `set_active_scope(scope)`：持久化 `lastActiveScope` 并刷新 `lastOpenedAt`。

### 6.3 更新检查状态

- 状态文件升级为 v2：`{ version: 2, scopes: { "<scopeId>": UpdateCheckState } }`。
- v1 文件读取后视为 `global` 条目，下一次写入迁移为 v2。
- 遗忘项目删除其条目；重新定位清空其条目。

### 6.4 错误模型

- 项目作用域错误码通过现有 `ErrorCode` 注册表新增（ADR 0006），禁止裸字符串；示例：`PROJECT_NOT_FOUND`、`PROJECT_ROOT_UNAVAILABLE`、`PROJECT_DUPLICATE`、`PROJECT_INVALID_ROOT`、`PROJECT_LOCK_UNSUPPORTED`。
- 锁、权限、内容错误复用现有 `lock.*`、`content.*` 等错误码，并携带 scope 归属，由前端在 scope 内展示。

### 6.5 并发与任务

- 仍为单一串行 mutation 队列；每个任务携带其发起的 scope。
- 任务运行期间禁止发起其他 mutation；scope 切换不改变已在运行任务的 scope 与目标。
- 中断恢复（interrupted mutation）按 scope 记录与提示。
- 前端复用统一 mutation surface（ADR 0008）：`mutationRefreshKeys` 与查询键一样携带 scope，避免刷新到其他 scope 的缓存。

### 6.6 前端查询与 fake backend

- React Query 的所有键包含 scope；切换 scope 不共享缓存条目。
- 派生状态继续由 `buildWorkspaceView`（ADR 0007）拥有，按当前 scope 的清单与注册表投影；切换 scope 重置选择、筛选与搜索。
- fake backend 增加内存项目注册表、按 scope 的清单/详情/检查状态，并支持任务通道；命令仍由 `API_COMMANDS` 与 `FakeHandlers` 契约约束（ADR 0010），预览与 App 测试共用。

---

## 7. 生命周期语义

### 7.1 安装

- 默认只写 canonical，agent 挂载显式勾选；始终追加 `universal`。
- 安装前预览仍为只读；安装后执行 §5.4 校验。
- 记录该 skill 在当前 scope 的基线指纹（baseline），用于本地改动检测；应用外安装的 skill 无基线，按未知处理。
- 安装失败、部分成功、同名冲突、Copy-only/Stale 的处理与全局一致，只在当前 scope 内生效。

### 7.2 挂载 / 断链 / 卸载

- 挂载要求 canonical 存在；extra agent 由应用创建父目录与符号链接；资格判定走 `attachment_target` 门控。
- 断链只移除指定 agent 的项目内链接。
- 卸载复用全局流程：备份 → CLI remove（不带 `-g`）→ 原生清理 extra 链接 → 校验。

### 7.3 更新

- 仅手动；保留原挂载集合；extra 链接重建；失败按现有结果模型呈现。
- 更新前按全局规则处理本地改动/未知状态；Copy-only 不更新（与全局一致）。

### 7.4 备份、日志与中断恢复

- 备份快照覆盖项目锁、canonical 文件夹、复制型挂载与挂载清单；位置与命名按 scope 隔离。
- 日志与 mutation session 沿用全局基础设施，记录 scope。
- 中断恢复提示限定在对应 scope。

### 7.5 遗忘 / 重新定位与 extra 守卫

- 遗忘只影响注册表与检查状态；磁盘文件保留。
- 重新定位保留项目身份、重置检查状态；若原 scope 正在运行任务则禁止操作。
- extra agent 的删除与目录修改守卫见 §4.9；不可用项目一律视为"可能仍有挂载"。

---

## 8. 异常与降级

| 状态 | scope 内表现 | 其他 scope |
|---|---|---|
| 项目根不可用 | 不可用状态，提供重新定位/遗忘；不检查、不变更 | 不受影响 |
| 项目锁损坏/版本不支持 | 错误状态，提供重试/重新定位/遗忘；禁止变更 | 不受影响 |
| 锁条目指向缺失目录 | Stale lock entry（只读） | 不受影响 |
| canonical 缺失但有 lock | Copy-only / 异常，按全局规则 | 不受影响 |
| 挂载断链 | 断链异常，可卸载或重新安装 | 不受影响 |
| canonical/agent 目录逃逸项目根 | 安全异常，禁止内容读取 | 不受影响 |
| 更新检查来源失败 | 该来源标记失败，报告为 Partial/Failed | 不受影响 |

降级原则：项目问题绝不升级为全局横幅或全局阻塞；全局环境缺失（Node/npx/git）仍全局禁用变更。

---

## 9. 安全

- 内容读取白名单：全局 canonical、已检测 agent 全局目录、**已注册项目根**（及其内部 agent 项目目录）。
- 项目根被遗忘后立即从白名单移除；所有 scope 相关路径在访问前重新校验归属。
- 项目锁只读；应用不写 `skills-lock.json`。
- canonical、agent 项目目录、extra 项目目录解析后必须位于项目根内；逃逸按异常处理并拒绝读取。
- extra `projectSkillsDir` 校验：相对、无 `..` 逃逸、规范化后仍在根内、不等于 canonical、全局唯一。
- 文件夹选择器与项目路径进入 CLI 时使用参数数组，禁止 shell 拼接；归档下载设大小与超时上限。
- Tauri capabilities 仅新增文件夹选择所需的最小 dialog 权限。

---

## 10. 测试与验收

### 10.1 发布门槛

- 全局回归全部通过；现有全局验收矩阵不降级。
- 项目作用域 P0 全过；P1 无阻塞问题。
- 真实 CLI 沙箱测试证明项目安装/卸载与 CLI 直接执行一致，且不触碰真实 HOME。
- 安全审计通过：白名单、capabilities、CSP、外部命令参数化。

### 10.2 测试分层

- **Rust 单测/集成**：注册表校验与持久化；项目锁 v1 解析；项目清单 fixture（canonical、catalog 项目目录、extra 项目目录）；scope 错误隔离；路径逃逸；指纹算法 golden vectors（与 CLI 生成结果一致）。
- **前端 Vitest**：fake backend 按 scope；切换 scope 重置状态；安装目标集合；徽章与不可用/错误状态；extra 项目目录表单校验；更新检查按 scope；`API_COMMANDS` 覆盖测试仍全绿。
- **错误分类**：新增项目错误码进入 `ErrorCode::ALL` 注册表测试；前端只通过统一 `AppError` 形态消费。
- **真实 CLI 沙箱（ignored）**：临时 HOME + 临时项目根，执行项目 `add` 与 `remove`，断言 `<项目>/.agents/skills`、`skills-lock.json`、agent 项目目录链接与清理结果。
- **安全检查**：capabilities/CSP 静态审计扩展项目根白名单断言。

### 10.3 验收矩阵（P0）

- [x] 添加/去重/拒绝非法根（`/`、home、全局 canonical 及其内部）；嵌套项目允许。证据：`projects.rs::rejects_missing_non_directory_relative_and_reserved_roots`、`dedupes_canonical_paths_and_allows_nested_and_overlapping_roots`。
- [x] 项目内安装默认只写 canonical；可选挂载 catalog agent；extra 配置后可作为目标并在挂载时创建目录。证据：`sandbox_real_cli.rs::real_cli_project_scope_add_and_remove_stay_inside_the_project`、`install.rs::project_install_runs_in_the_root_without_global_and_keeps_universal`、`project_extra_link_is_created_inside_the_project_and_verified`、`scope_layout.rs::install_targets_include_detected_catalog_and_configured_extras_even_when_missing`；`App.scope.test.tsx` 项目向导。
- [x] 项目内断链/卸载只影响该项目；全局与其他项目不变。证据：`uninstall.rs::project_uninstall_only_removes_the_project_scope_and_backs_it_up`、`a_failed_project_uninstall_never_touches_the_global_scope`、`detach.rs::project_detach_unlinks_only_the_project_attachment`；`sandbox_real_cli.rs::real_cli_project_scope_add_and_remove_stay_inside_the_project`。
- [x] 应用外创建的项目 skill/挂载/异常能被清单发现。证据：`project_read_model.rs::project_inventory_covers_canonical_catalog_and_extra_dirs_with_all_management_states`、`unreadable_skill_md_degrades_only_that_row`。
- [x] 项目锁缺失=空；损坏=scope 错误且不影响全局。证据：`project_lock.rs::missing_lock_is_an_empty_project_lock`、`project_read_model.rs::scope_errors_are_isolated_and_unavailable_or_forgotten_roots_revoke_reads`、`unreadable_lock_and_unreadable_root_are_scope_states`、`inventory/mod.rs::missing_lock_reads_unmanaged_skills_without_errors`；`App.scope.test.tsx` 项目锁错误隔离。
- [x] 项目更新检查按内容指纹判定候选；非 GitHub 显示不支持；失败按来源隔离。证据：`update/project.rs::groups_by_source_and_ref_and_downloads_each_commit_once`、`source_failures_are_isolated_and_do_not_persist_over_other_scopes`、`a_cached_archive_skips_the_download_and_is_marked_cached`。
- [x] 启动恢复上次 scope；不可用回退全局；切换按 6 小时阈值检查。证据：`App.scope.test.tsx` 恢复/回退/切换，`use-auto-update-check.test.ts` 六小时锚点。
- [x] 遗忘不删文件；重新定位保留身份并重置检查状态。证据：`projects.rs::forgetting_removes_only_the_entry_and_its_check_state`、`relocate_preserves_identity_and_clears_scope_state`；`App.scope.test.tsx` 重新定位/遗忘。
- [x] extra 删除/目录修改守卫在不可用项目存在时生效。证据：`extra_agent_guard.rs::unavailable_project_blocks_removal_and_lists_the_project`、`unavailable_project_blocks_project_dir_changes_even_without_a_known_mount`；`App.agents.test.tsx` 守卫对话框。
- [x] 内容读取不能越出已注册项目根；逃逸链接被拒绝。证据：`project_read_model.rs::escaping_canonical_and_agent_paths_are_anomalies_and_content_refuses_them`、`canonical_store_symlink_outside_the_project_is_an_escape`；`security_audit.rs::project_scope_grants_only_the_registered_root_and_rejects_escapes`。
- [x] 全局作用域行为与既有一致，全局 lock 与挂载未被改写。证据：`sandbox_real_cli.rs::real_cli_project_scope_add_and_remove_stay_inside_the_project` 断言项目操作未创建全局 lock，`real_cli_sandbox_install_update_detach_uninstall_and_regression` 全局回归；`project_read_model.rs::project_cache_follows_lock_and_canonical_changes_without_global_refresh`。

### 10.4 性能目标

- 项目清单读取不随 catalog 数量线性变慢（目录探测有界）。
- 单个来源一次检查最多下载一次归档；相同 commit 命中缓存。
- 归档下载大小与时长有上限，超限转为来源级失败。

---

## 11. 实现顺序

1. 项目注册表与 scope 管线（issue 01）。
2. 项目读取模型、锁解析与安全白名单（issue 02）。
3. 侧栏 scope 切换与项目视图（issue 03）。
4. 项目安装、挂载、断链（issue 04）。
5. 项目更新检查（issue 05）。
6. 项目更新与卸载、备份与中断恢复（issue 06）。
7. extra agent 项目目录与管理守卫（issue 07）。
8. 验收、真实 CLI 沙箱与安全审计（issue 08）。

---

## 12. 决策记录

- 项目作用域模型：ADR 0011。
- 项目更新检查内容指纹：ADR 0012。
- 术语：Project、Project root、Relocate、Forget、Scope、Last check 等见 [GLOSSARY.md](../../GLOSSARY.md)。
