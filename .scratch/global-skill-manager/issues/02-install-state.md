# 全局安装的真相与异常形态

Type: research
Status: resolved
Blocked by: —

## Question

查清全局 skill 在磁盘上的真实状态模型，为 GUI 的清单、详情、异常提示和安全操作提供判定规则。需要覆盖：

1. `~/.agents/.skill-lock.json` v3 的 schema 与字段语义：`source`、`sourceType`、`sourceUrl`、`skillPath`、`skillFolderHash`、`installedAt`、`updatedAt`、`pluginName`、`dismissed`、`lastSelectedAgents`；hash 何时为空、如何计算。
2. 中心仓库 `~/.agents/skills/<name>` 与各 agent 目录（`~/.claude/skills`、`~/.codex/skills`、`~/.gemini/skills`、`~/.config/opencode/skills`、`~/.copilot/skills` 等）的关系：CLI 何时用符号链接、何时复制、相对/绝对链接、如何判定某个 agent 已挂载。
3. 为什么本机 lock 有 61 条而 `skills list -g --json` 只有 49 条：过滤、去重、失效、来源缺失、非 skill 条目等具体原因。
4. 异常形态的判定：孤儿目录（有目录无 lock）、失效 lock 条目（有 lock 无目录）、断链、复制的旧版本、同名 skill 来自不同来源、手工放入的 skill、`.DS_Store` 等噪声。
5. 本地改动检测：能否用 `skillFolderHash` 判断中心副本是否被用户改过；目录哈希的算法与忽略规则；copy 模式下如何判断 agent 侧是否落后。
6. 多来源 / 多 agent 的边界：同一 skill 名来自两个来源、一个来源含多个 skill、插件型 skill、subagent 安装等是否需要特殊处理。

方法：以本机真实目录为准做只读盘点，结合 CLI 源码与 lock 文件结构；**不要修改任何 skill 目录或 lock 文件**。把结论写成 `.scratch/global-skill-manager/research/02-install-state.md`，附一个“状态判定表”（正常 / 异常 / 不可判定）与 49/61 的具体解释。在本票据下追加 `## Answer`，把 `Status` 改为 `resolved`。不要编辑 `map.md` 或其他票据。

## Answer

完整报告（含 schema 字段表、状态判定表、hash 复算方法与源码行号）：[../research/02-install-state.md](../research/02-install-state.md)。

- **49 vs 61 的准确解释**：`list` 不读 lock，只扫描磁盘。61 = 40 个受管 canonical + 21 条失效 lock；49 = 40 个受管 canonical + 9 个 Antigravity 下由其他工具生成的 `gitnexus-*`（无 lock、source=null）。差额 12 = 21 − 9。票据创建后 10:52 有外部进程新增 `vercel-react-best-practices`，现为 62/50，差额不变。21 条失效 lock = 18×64 位 hash + 2×40 位 + 1 空 hash（find-skills），其中 7 条上游仍存在且 hash 已变，`skills update -g` 会把它们重新装回。
- **lock v3 是“来源账本”，不是磁盘清单**：按安装名索引，字段为 `source/sourceType/sourceUrl/ref/skillPath/skillFolderHash/installedAt/updatedAt/pluginName(+sourceBaseUrl/wellKnownDigest)`。它不记录安装模式，也不记录每个 agent 的挂载状态；`lastSelectedAgents` 只是上次交互偏好，本机包含 9 个未安装的 agent。
- **symlink/copy**：多目录目标默认 symlink（canonical 复制 + agent 相对链接）；单目录/`--copy` 为独立复制；symlink 失败会静默回退 copy。lock 无模式标记，必须从磁盘形态推断。
- **挂载判定必须看物理目录**：只有 `agent.globalSkillsDir === ~/.agents/skills` 才算 shared（Codex/Pi）。Gemini CLI、GitHub Copilot、OpenCode、Antigravity 的全局目录是各自路径，但 CLI 的 `isUniversalAgent` 按项目目录把它们误判为 shared，导致 `list.agents` 对全部 41 个 skill 都报已挂载，实际各只有 10 个；新安装也没有写这四个目录。不要用 `list --json.agents` 或 `-a` 做挂载/清单真相。
- **本地改动检测可行，但要扣除安装器排除项**：40 位是源文件夹 Git tree SHA，可用临时 Git index 从已安装目录重算（41 个中 40 个与 lock 完全一致）；唯一例外 `vercel-react-best-practices` 因源含 `metadata.json` 被安装器跳过而“假阳性”。64 位是 SHA-256 路径+内容 hash；空 hash 不可校验。建议 GUI 维护自己的 per-copy 基线指纹，并同时比较上游 hash 以区分“本地改动/上游更新”。
- **安全边界**：`skills update -g` 只比远端 hash、不检查本地副本，且会按“宿主 agent（无宿主时为全部检测到的 agent）+ universal”重装，可能新增链接、把 copy 改成 symlink、复活 7 条 stale 条目；`remove --all` 会连 9 个未受管 skill 和 21 条 stale 一起删。lock 写入非原子，JSON 损坏会被读成空并在下次写入时清空全部条目。GUI 应备份、串行化、逐名操作，并拒绝 version != 3 或损坏的 lock。
