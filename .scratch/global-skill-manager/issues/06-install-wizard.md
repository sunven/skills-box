# 安装向导的来源预览与选择契约

Type: grilling
Status: resolved
Blocked by: 01

已知事实（来自「全局安装的真相与异常形态」）：lock 不记录安装模式与每个 agent 的挂载；`list --json.agents` 会高报。安装向导的“已安装 / 已挂载”状态必须来自物理目录检查；不得依赖 `remove --all` 或 `update -g` 的默认行为。

已知事实（来自「Tauri 执行外部 CLI 与打包」）：安装前需确认 Node >= 22.20、npx、git；从 Finder 启动的 app 只有最小 PATH，Node 目录需注入子进程 PATH。

已知事实（来自「CLI 能力与 JSON 契约」）：`add` 必须显式传 `-a`，推荐 `-a <ids> -a universal`；省略 `-a` 会让所有 skill 失败，单个非 universal agent 默认只 copy、不写 canonical；`--json` 与 `-l` 互斥，`-l` 只有 name+description 人类文本、无 path。安装向导必须固定目标 agent 集合并事后校验落盘形态。

## Question

在「CLI 能力与 JSON 契约」结论的基础上，确定安装向导的精确契约：

- source 输入：v1 只接受公开 GitHub 来源；接受 `owner/repo`、GitHub URL、子目录？是否接受 tag/commit？无效输入如何提示。
- 预览：用 CLI 的 `add -l/--list` 还是 GitHub API 获取仓库内 skill；预览展示 name/description/path 中的哪些；一个仓库只有一个 skill 时是否跳过选择。
- 选择：多选 skill、全选；agent 列表展示本机检测到的还是全部支持；默认勾选规则；是否记住上次选择。
- 确认与结果：安装前展示来源、skill、agent、目标路径；安装中进度与取消；部分失败如何呈现与重试。
- 边界：已安装同名 skill 的重装 / 升级、来源含插件或多 skill 包、仓库过大 / 无 `SKILL.md`。
- `--copy` 是否在 v1 暴露（当前倾向不暴露，默认符号链接）。

输出：一份可直接写入规格的安装流程与状态机，记录在本票据 `## Answer`。只闭合本票据，不改 `map.md` 的其他部分。

## Answer

用户确认（2026-10-28）。安装向导契约：

1. **来源**
   - v1 只接受公开 GitHub 根仓库：`owner/repo` 或 `https://github.com/owner/repo`。
   - 只装默认分支；拒绝 subdirectory、`#ref`（tag/commit/SHA）、GitLab/SSH/local/Notion。非法或歧义输入给出明确错误，不静默改写。
   - 确认页展示归一化后的 `owner/repo`。

2. **预览**
   - 运行 `skills add <source> -l -g`，解析人类文本中的 name + description；不显示 path。
   - 解析器隔离并针对 `skills@1.7.4` 做 fixture 测试；解析失败时展示原始输出，并允许手动输入 skill 名继续。
   - 预览只做列表；不做仓库内搜索或 plugin 折叠。单 skill 仓库自动跳过选择；无 `SKILL.md` 显示空态并禁止安装。

3. **选择**
   - 多选 skill；默认勾选所有未安装项；同名已安装（含不同来源、未受管）禁用并显示现有来源。换源必须先卸载；安装向导不做升级/换源覆盖。
   - agent 只展示本机检测到的；默认勾选已含受管 skill 的 agent；只有未受管目录的 agent（如当前 Antigravity）默认不勾选并标注；universal agent 标为「共享 canonical」。不记住上次选择。后台始终追加 `-a universal`。
   - 确认页展示：归一化来源、所选 skill、所选 agent、目标 `~/.agents/skills` + agent 链接，以及「写入阶段不可取消」提示。

4. **执行**
   - 每个 source 一次 CLI 调用：`skills add <source> -g -s <names> -a <ids> -a universal -y --json`；禁止 `--all`、`-a '*'`、`--copy`。
   - 进度按阶段展示：克隆/预览 → 写入 → 校验。CLI 不暴露克隆 / 写入边界，安装任务的取消窗口为 spawn 前；写入开始后禁止取消，避免非原子 lock 半写。
   - 完成后逐 skill 校验：canonical 是否存在、每个所选 agent 的挂载是否存在、`lstat` 判断链接/副本；不以 JSON 的 `agents` 或 `mode` 作为真相。CLI 静默回退 copy 时明确提示。
   - 部分失败：读取 per-skill JSON，按失败项单独重试；若 lock 未写入/损坏或校验失败，报告并保留快照，不宣称成功。
   - 安装前快照 `.skill-lock.json`；所有安装串行执行。

5. **模式与边界**
   - v1 不暴露 `--copy`；只走符号链接。
   - 不支持一次安装多个来源；一次向导只处理一个 source。
   - 私有仓库、GitLab、SSH、local、Notion、subdir、tag/commit 均拒绝；需要时回 CLI。
   - 单 skill 仓库跳过选择但仍展示确认页。

> 架构细化（来自「技术架构与命令边界」）：preview 是独立只读任务，可取消；安装 CLI 一旦启动不可取消。原“克隆阶段可取消”的表述收缩为 spawn 前取消。
