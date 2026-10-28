# CLI 能力与 JSON 契约

Type: research
Status: resolved
Blocked by: —

## Question

查清 `skills` CLI（本机 v1.7.4，源码 `github.com/vercel-labs/skills`）在“全局 skill 管理”场景下的真实契约，产出 GUI 可直接依赖的调用矩阵。需要覆盖：

1. 读取
   - `skills list -g --json` 的完整 schema、字段含义、排序/分组规则；为什么本机 lock 有 61 条而 list 只返回 49 条（哪些被过滤、合并或遗漏）。
   - 如何获取全部受支持 agent 的机器可读列表、本机已检测到的 agent、每个 skill 的挂载状态（符号链接 / 副本 / 缺失）。
2. 安装
   - `skills add <source> -g -s <skills> -a <agents> -y --json` 的完整参数、输出 JSON、退出码与非交互行为。
   - `-l/--list` 预览仓库内 skill 时的输出格式；能否与 `--json` 组合；是否返回每个 skill 的 name / description / path。
   - source 的接受格式与归一化规则（owner/repo、GitHub URL、子目录、tag/commit、package、文档链接）。
   - 默认符号链接 vs `--copy` 的差异；全局安装的写入位置；对已有 skill 的重装/覆盖行为。
3. 删除与断链
   - `skills remove -g -s <skill> -a <agent> -y` 是否只移除指定 agent 的挂载，还是同时删除中心副本与 lock 条目；移除最后一个 agent 时会发生什么。
   - 如何仅断开单个 agent 而保留中心副本；如果没有现成命令，说明 GUI 需要自己操作文件系统的边界与风险。
   - 删除时的交互确认、退出码、部分失败行为。
4. 更新
   - `skills check` 的检测机制、输出、是否支持 `--json`、退出码。
   - `skills update <skill> -g -y` 的非交互行为、输出、退出码；是否保留 copy 模式与 agent 挂载；本地改动如何处理。
5. 运行时前提
   - Node / CLI 最低版本、`npx` 缓存与版本漂移、环境变量（`DISABLE_TELEMETRY`、`NO_COLOR` 等）、认证与私有仓库边界。
6. 失败模式
   - 常见错误的 stderr / 退出码（网络、仓库不存在、skill 不存在、权限、lock 冲突），以及 GUI 可稳定识别的错误分类。

方法：优先进源码（克隆到 /tmp）与 `--help`，用本机真实安装做低风险只读验证；**不要执行删除或更新类命令改动用户现有 skill**。把结论写成 `.scratch/global-skill-manager/research/01-cli-contract.md`，并在本票据下追加 `## Answer`（含矩阵式结论与文件指针），把 `Status` 改为 `resolved`。不要编辑 `map.md` 或其他票据。

## Answer

完整调用矩阵、JSON schema、61→49 对账与风险清单见 [`../research/01-cli-contract.md`](../research/01-cli-contract.md)（基于 v1.7.4 tag 源码 + 只读实测）。

**读取**
- 唯一稳定机器接口是 `skills list -g --json`：`[{name,path,scope,agents[],source,sourceUrl,sourceType}]`；无 description/时间/hash/mount 模式，`agents` 是 display name，且会把 universal agent 的原生共享误报成挂载。GUI 必须再读 lock + `lstat` 补全。
- `lock 61 / list 49` 的算术：40 个 canonical 目录 + 9 个仅存在于 Antigravity 目录的外部 skill = 49；其余 21 条是目录已删的 stale lock，list 不显示。研究期间外部安装了 `vercel-react-best-practices`，当前为 lock 62 / list 50，公式不变。lock 损坏或 `version<3` 会被静默当空，下一次写回会丢全部条目。
- CLI 无 agent 枚举接口；支持列表见 `agents.ts`（79 个）或包内 `docs/agents.md`，检测规则需 GUI 复制。

**安装**
- 变更类命令中只有 `add` 支持 `--json`，且必须 `-y`；`--json` 与 `-l` 互斥，`-l` 只有 name+description 人类文本、无 path。
- **必须显式传 `-a`**：省略 `-a` 时 `ensureUniversalAgents()` 会加入含 `promptscript`（无全局目录）的 22 个 universal，导致每个 skill 在 `-g --json` 下都 failed；`--all`/`-a '*'` 还含 Eve，同样失败。
- 单个非 universal agent（如只传 `-a claude-code`）默认 **copy 且不写 `~/.agents/skills`**；追加 `-a universal`（不产生额外 agent 目录的 canonical 标记）可强制 symlink。推荐：`add <src> -g -s <names> -a <ids> -a universal -y --json`。
- 文本模式部分目标失败仍 exit 0；symlink 失败回退 copy 不暴露。`--copy` 只写各 agent 目录，不写 canonical。

**删除/断链**
- `remove -g -s X -y`（无 `-a`）是彻底卸载；`remove -a` 的 per-agent 删除是否保留 canonical/lock 取决于“剩余已检测 agent 是否还能看到 X”，不是用户意图；在没有其他已检测 agent 能看到 X 时，目标 agent 未挂载也会触发彻底卸载，未检测 agent 可能留下悬空链接。无 JSON，部分失败/未匹配均 exit 0。
- 因此 **单 agent 断链必须由 GUI 直接 unlink**，不动 canonical/lock；删除前检查父目录 realpath 与跨 agent symlink 依赖（本机 10 个 HyperFrames 的 Gemini/Copilot/OpenCode/Antigravity 链接指向 Claude copy）。universal agent 标记为 native，不可断链。彻底卸载用 CLI 并做事后状态校验。

**检查/更新**
- **`skills check` 是 `update` 的别名，会直接重装**（`cli.ts:420-423`），与包内文档“check without reinstalling”矛盾；无 check-only、无 `--json`（flag 被忽略）、无 dry-run、无“有更新”退出码。
- `update` 检测 GitHub folder tree SHA，发现差异后 spawn `add ... -g -y`；不保留 copy 模式/原挂载集，会重挂所有检测 agent 并覆盖本地改动；子进程部分失败可能 exit 0，被误记成功。
- GUI 应自己用 lock + GitHub Trees API 做只读检测；应用更新用 lock 构造 `add` 并显式传当前挂载 agents，才能尊重断链与本地状态。

**运行时**
- Node `>=22.20.0`；本机 PATH 为 v24.15.0、Homebrew v26.12.1，CLI 仅在 npx cache（无全局 bin）。固定 `skills@1.7.4` 或打包，显式解析 node 路径。
- 建议 spawn env：`DISABLE_TELEMETRY=1`、`DO_NOT_TRACK=1`；清理 `CURSOR_AGENT` 等 agent 检测变量；`NO_COLOR` 实测无效，只解析 `--json`。所有变更串行化，变更前备份并校验 lock。
