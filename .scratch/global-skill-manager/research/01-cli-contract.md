# `skills` CLI v1.7.4 全局管理契约

- **对象**：npm `skills@1.7.4`（`github.com/vercel-labs/skills`，tag `v1.7.4`，commit `a80e4c9`）。
- **本机形态**：macOS 27.2 arm64；CLI **没有全局安装**，只存在于 npx cache（`~/.npm/_npx/*/node_modules/skills`，研究时 3 份均为 1.7.4）；`which skills` 为空。
- **Node**：交互 shell 的 `node` 为 v24.15.0（nvm），Homebrew 为 v26.12.1；包声明 `engines.node >=22.20.0`（`package.json:145`）。票据中的 “Node v26.3.0” 与本机不符。
- **只读约束**：研究只执行了 `--help`、`--version`、`list -g --json` 与源码检查；没有执行任何 `add/remove/update/check`。本机 skill 目录与 lock 未被本研究的命令修改。
- **快照漂移**：研究期间有外部进程安装了 `vercel-react-best-practices`（lock `installedAt=2026-10-28T02:52:37Z`），使计数从票据基线的 **lock 61 / list 49** 变为 **lock 62 / list 50**。下文先解释票据基线，同时给出当前值。
- **源码基准**：`/tmp/skills-research-01` 已 checkout 到 `v1.7.4`；所有行号均指该 tag（与已安装 npm 包对应）。

## 结论速览（决策用）

| 领域 | 结论 | GUI 应采用的做法 |
|---|---|---|
| 读取 | 只有 `list -g --json` 是稳定机器接口；字段少（无 description、时间、hash、mount 模式），且 `agents` 把“原生共享目录”误报成挂载 | 以 `list -g --json` 为索引，再读 lock + `lstat` 补全；不要相信 `-a` 过滤 |
| 安装 | `add --json -y` 可解析；但**单个非 universal agent 默认 copy，且不写 canonical**；省略 `-a` 时自动加入的 universal 含无全局目录的 `promptscript`，会让每个 skill 在 JSON 中 failed；`--all`/`-a '*'` 在全局同样失败 | 显式 `-a <ids>`（绝不可省略）；若只有一个非 universal agent，再加 `-a universal` 强制 symlink；禁止 `--all` |
| 预览 | `add -l` 只有 name+description 的人类文本，**不能与 `--json` 组合**，无 path | 自己用 GitHub API/克隆做结构化预览，或解析文本仅作展示 |
| 删除/断链 | `remove -g -s X -y` 是“彻底卸载”；`-a` 的 per-agent 删除**可能连带删除 canonical + lock**，且无 JSON、部分失败仍 exit 0 | 彻底卸载交给 CLI 并做事后校验；**单 agent 断链由 GUI 自己 unlink**，绝不改 canonical/lock |
| 检查更新 | **`skills check` 是 `update` 的别名，会直接安装更新**；v1.7.4 没有 check-only、没有 `--json`、没有 dry-run | GUI 自己用 lock + GitHub Trees API 做只读检测；不要调用 `check` |
| 更新应用 | `update` 无 JSON，重装到“所有检测到的 agent + universal”，不保留 copy 模式与原挂载集，覆盖本地改动 | 如需保留断链状态，GUI 用 lock 构造 `add` 并显式传原挂载 agents |
| 运行时 | Node ≥22.20.0；需 git；npx 未固定版本会漂移；`NO_COLOR` 在 v1.7.4 无效 | 固定 `skills@1.7.4`（最好随应用打包），显式解析 node 路径，设置 `DISABLE_TELEMETRY=1` |

## 调用矩阵（速查）

| 命令 | 写盘 | 机器输出 | 非交互方式 | 退出码 | GUI 可用性 |
|---|---|---|---|---|---|
| `skills --version` | 否 | 版本字符串 | 天然 | 0 | 可用（探测版本） |
| `skills --help` / `<cmd> --help` | 否 | 人类文本 | 天然 | 0 | 仅展示 |
| `skills list -g --json` | 否 | JSON 数组 | 天然 | 0；非法 `-a` 为 1（文本） | **可用**（主读取接口） |
| `skills list -g` | 否 | 人类文本（含 ANSI） | 天然 | 0 | 仅兜底 |
| `skills add <src> -g -s <names> -a <ids> -a universal -y --json` | 是 | JSON 数组 | `-y` + 显式 `-a` | 0 全成功；1 任一 failed/skipped | **可用**（推荐安装） |
| `skills add <src> -g -y --json`（无 `-a`） | 是 | JSON，但每 skill failed | `-y` | 1 | **不可用**（promptscript 必然失败，且已写盘） |
| `skills add <src> -g --all --json` / `-g -a '*' -y --json` | 是（含大量无用 agent 目录） | JSON（每 skill failed） | 隐含 `-y` | 1 | **禁用**（Eve + PromptScript 必然失败） |
| `skills add <src> -l` | 否（临时 clone） | 人类文本 | 天然 | 0 | 仅展示；无 path、不能配 `--json` |
| `skills remove -g -s <name> -y` | 是（全 agent + canonical + lock） | 无 | `-y` | 0（部分失败/未匹配也 0） | 仅用于彻底卸载，需事后校验 |
| `skills remove -g -s <name> -a <agent> -y` | 是（可能含 canonical/lock） | 无 | `-y` | 0（部分失败/未匹配也 0） | **不用于断链**；语义依赖环境 |
| `skills update <name> -g -y` | 是（重装 + 重挂） | 无 | `-g -y` | 0 最新/无匹配；1 失败 | 仅在接受“全 agent 重挂 + 覆盖本地改动”时使用 |
| `skills check ...` | **是（等同 update）** | 无 | — | 同 update | **禁用** |
| `skills update --json` / `check --json` | 是 | 无（flag 被忽略） | — | 同 update | **禁用** |



## 1. 状态模型与写入位置

### 1.1 路径

| 内容 | 路径 | 依据 |
|---|---|---|
| canonical skill 根 | `~/.agents/skills/<sanitizeName(name)>/SKILL.md` | `installer.ts:133-136` |
| 全局 lock | `~/.agents/.skill-lock.json`；若设置 `XDG_STATE_HOME` 则为 `$XDG_STATE_HOME/skills/.skill-lock.json` | `skill-lock.ts:67-73` |
| 非 universal agent 挂载 | 各 agent 的 `globalSkillsDir`（如 `~/.claude/skills`、`~/.grok/skills`） | `agents.ts:80-827` |
| universal agent | 不创建独立目录，直接读 canonical（部分 agent 仍会扫描其原生目录，见 §2.4） | `agents.ts:914-916`、`installer.ts:412-419` |
| 项目级 | `./.agents/skills` + `./<agent>/skills`，lock 为 `./skills-lock.json` | `installer.ts:133-136`、`local-lock.ts` |

目录名不是 frontmatter name：`sanitizeName()` 做 lower-case、非 `[a-z0-9._]` 变 `-`、去掉首尾 `.`/`-`（`installer.ts:66-81`）。例如 `ce:review` → `ce-review`。

### 1.2 全局 lock（version 3）

顶层：`version`、`skills`、`dismissed?`、`lastSelectedAgents?`（`skill-lock.ts:8,51-60`）。

```json
{
  "version": 3,
  "skills": {
    "<skill name>": {
      "source": "owner/repo 或完整 URL",
      "sourceType": "github | gitlab | git | local | well-known",
      "sourceUrl": "https://github.com/owner/repo.git",
      "ref": "tag/branch/SHA（可选）",
      "skillPath": "skills/foo/SKILL.md（可选）",
      "skillFolderHash": "40 位 GitHub tree SHA；可能为空",
      "installedAt": "ISO-8601",
      "updatedAt": "ISO-8601",
      "pluginName": "可选",
      "sourceBaseUrl": "well-known 可选",
      "wellKnownDigest": "well-known 可选"
    }
  },
  "dismissed": { "findSkillsPrompt": true },
  "lastSelectedAgents": ["claude-code", "codex"]
}
```

- `skillFolderHash` 对 GitHub 是 **skill 文件夹的 git tree SHA**（root skill 用 repo root tree SHA），不是 commit、不是本地内容哈希（`blob.ts:288-318`、`add.ts:2224-2277`）。
- 本机 62 条全部 `sourceType=github`；唯一空 hash 是 `find-skills`（因此永远无法自动更新，见 §5.2）。
- `lastSelectedAgents` 只由交互式 add 写入；`update` **不读取**它（`update.ts` 中无引用）。

### 1.3 关键：lock 与磁盘是两套独立事实

- lock 有而目录没有 → **stale lock**，`list` 完全不显示。
- 目录有而 lock 没有 → `list` 显示，但 `source/sourceUrl/sourceType` 全为 `null`。
- lock 损坏或 `version < 3`：`readSkillLock()` **静默返回空 lock**，不告警（`skill-lock.ts:80-102`）。下一次成功 add/remove 会以空 lock 为基底写回，**丢失全部旧条目**。本机沙箱验证：坏 JSON / version=2 时 `list -g --json` exit 0，所有 source 变 null。
- lock 读改写无文件锁、无原子 rename；并发 CLI 会丢条目。

## 2. 读取契约

### 2.1 `skills list -g --json`

退出码：成功（含空数组）0；`-a` 非法 agent 时 **stdout 输出人类文本并非 JSON**，exit 1（`list.ts:84-95`）。全局 lock 损坏仍 0。

Schema（`list.ts:113-128`）：

```json
[
  {
    "name": "agent-browser",
    "path": "/Users/<user>/.agents/skills/agent-browser",
    "scope": "global",
    "agents": ["Antigravity", "Claude Code", "Codex", "Gemini CLI", "GitHub Copilot", "Grok Build", "OpenCode", "Pi"],
    "source": "vercel-labs/agent-browser",
    "sourceUrl": "https://github.com/vercel-labs/agent-browser.git",
    "sourceType": "github"
  }
]
```

| 字段 | 语义与坑 |
|---|---|
| `name` | SKILL.md frontmatter `name`（经 sanitizeMetadata），不是目录名；同名会被合并 |
| `path` | canonical 条目为 `~/.agents/skills/<dir>`；仅存在于 agent 目录的条目为该 agent 目录；copy 模式可能没有 canonical；**不是 realpath** |
| `scope` | `-g` 时恒为 `"global"` |
| `agents` | **display name 数组**（不是 CLI `-a` 的 ID）；对 canonical 条目，universal agent 即使没有任何挂载也会出现（见 §2.4） |
| `source/sourceUrl/sourceType` | 来自 lock；未托管条目为 `null` |
| 缺失 | 无 `description`、`installedAt/updatedAt/hash/ref/skillPath/pluginName`、无 mount 类型、无 symlink target、无“stale/untracked”标记 |

输出是 pretty-print JSON，stdout 只有 JSON；但被跳过的非法 SKILL.md 会向 stderr 打 `⚠ Skipped ...`（`skills.ts:76-78`）。

### 2.2 排序/分组

- JSON：**没有显式排序**。顺序来自 `listInstalledSkills()` 的 Map 插入顺序：先 canonical 的 `readdir` 顺序，再按 agent 扫描顺序追加 agent-only 条目（`installer.ts:1186-1219`）。不要依赖稳定排序。
- 文本：按 lock 的 `pluginName` 分组，组名排序，未分组进 `General`；组内仍为插入顺序（`list.ts:177-256`）。JSON 不做分组。

### 2.3 为什么 lock 61 而 list 49

票据基线（外部安装前）：

| 类别 | 数量 | 是否出现在 list | 解释 |
|---|---:|---|---|
| canonical 目录且 lock 有记录 | 40 | 是 | `~/.agents/skills` 的 40 个真实目录（无 symlink），与 lock 名字合并 |
| lock 有、canonical 目录已不存在 | 21 | **否** | stale lock；list 只枚举磁盘，不补幽灵条目 |
| 仅存在于 agent 目录、lock 无记录 | 9 | 是 | `~/.gemini/antigravity/skills/gitnexus-*`，`source=null`，`agents=["Antigravity"]` |
| **list 合计** | **49** | | 40 + 9 |
| **lock 合计** | **61** | | 40 + 21 |

21 条 stale：`find-skills, caveman, diagnose, to-issues, to-prd, write-a-skill, zoom-out, design-an-interface, edit-article, git-guardrails-claude-code, migrate-to-shoehorn, obsidian-vault, qa, request-refactor-plan, review, scaffold-exercises, setup-pre-commit, ubiquitous-language, writing-beats, writing-fragments, writing-shape`。

9 条外部条目：`gitnexus-cli, gitnexus-debugging, gitnexus-exploring, gitnexus-guide, gitnexus-impact-analysis, gitnexus-pdg-query, gitnexus-pr-review, gitnexus-refactoring, gitnexus-taint-analysis`。

外部安装后：canonical 41、lock 62、list 50，公式不变（41 + 9；62 - 41 = 21 stale）。其他会减少 list 数量的因素：无 `SKILL.md` 的目录、frontmatter 缺 name/description、`metadata.internal: true`、同名 skill 合并（`skills.ts:100-124`、`installer.ts:1297-1407`）。

### 2.4 `agents` 字段不是挂载真相

`listInstalledSkills()` 对 canonical 条目的 agent 归属逻辑（`installer.ts:1275-1407`）：

- 对每个“检测到”的 agent，`getAgentBaseDir()` 对 universal agent **直接返回 canonical 路径**（`installer.ts:156-184`）。因此只要 canonical 中存在该 skill，所有被检测到的 universal agent 都会被标记为 `agents`，即使它们没有任何独立挂载。
- 非 universal agent 才检查自己的 `globalSkillsDir`。
- `-a` 不是过滤器：它只限制 canonical 归属循环；随后仍会扫描所有“存在目录但未被选中”的 agent，并把其中的 skill 归属给该 agent（`installer.ts:1250-1269`）。实测 `list -g --json -a claude-code` 仍返回 `Grok Build`；`-a codex` 仍返回 50 条并带 Claude/Grok。

本机检测结果（由该逻辑反推并人工核对）：Claude Code、Grok Build 为非 universal；Antigravity、Codex、Gemini CLI、GitHub Copilot、OpenCode、Pi 为 universal。故 41 个 canonical skill 中 30 个显示 8 个 agent，11 个显示 7 个（10 个 HyperFrames + `writing-great-skills`，均缺 Grok 挂载），9 个 Antigravity-only 条目显示 1 个。

### 2.5 本机真实挂载拓扑（read-only `lstat`）

| 位置 | 结果 |
|---|---|
| `~/.agents/skills`（41 项） | 全部真实目录，无 symlink |
| `~/.claude/skills` | 31 个 symlink → `../../.agents/skills/<name>`；10 个真实 copy（HyperFrames） |
| `~/.grok/skills` | 30 个 symlink → `../../.agents/skills/<name>` |
| `~/.gemini/skills`、`~/.copilot/skills`、`~/.config/opencode/skills`、`~/.gemini/antigravity/skills` | 各 10 个 symlink，但目标为 `../../.claude/skills/<name>`（**指向 Claude 的 copy，而非 canonical**） |
| `~/.codex/skills`、`~/.pi` | universal，原生读 canonical |

这带来一个 GUI 必须处理的依赖：删除 `~/.claude/skills/<HyperFrame>` 的 copy 会让另外 4 个 agent 的 symlink 悬空。CLI 的 per-agent remove 不会检查原生目录 symlink。

### 2.6 如何获得 agent 列表与挂载状态

CLI **没有** `agents` 子命令，也没有 JSON 形式的“支持/已检测 agent”接口：

- 支持列表：源码 `src/agents.ts:80-827`（79 个 ID），包内 `docs/agents.md` 有生成的 Markdown 表（可直接 vendored/解析）。`skills list -a __invalid__` 的 stdout 会打印 “Valid agents: ...” 并 exit 1，可作为运行时 hack，但不是契约。
- 已检测 agent：`detectInstalledAgents()` 只是对每个 agent 的 `detectInstalled()` 做 `existsSync`（`agents.ts:829-837`）。GUI 应复制同一套探测规则，或只展示“有挂载/有目录”的 agent。
- 挂载状态：对每个 agent 的 `globalSkillsDir` 执行 `lstat`：
  - symlink 且 `realpath` == canonical → `linked`
  - symlink 且指向别处 → `linked-elsewhere`（本机 Gemini 等即此）
  - 真实目录 → `copied`
  - 不存在 → `missing`
  - universal agent → 额外标 `native`（读 canonical，不可断链）；若其原生目录有 legacy 条目，单独展示。

## 3. `add` 契约

### 3.1 参数与非交互

`parseAddOptions`（`add.ts:2581-2647`）支持：`-g/--global`、`-y/--yes`、`-l/--list`、`--all`、`-a/--agent`（多值）、`-s/--skill`（多值）、`--metadata`、`--full-depth`、`--json`、`--copy`、`--subagent`。**未知 flag 被静默忽略**。

- `--json` 必须与 `-y`（或 `--all`，其隐含 `-y`）一起使用，否则 exit 1（`add.ts:1503-1507`）。
- `--json` 与 `-l` 互斥，exit 1（`add.ts:1509-1511`）。
- `--json` 不支持 well-known source，exit 1（`add.ts:1590`）。
- `--json` 时 stdout 被临时重定向到 stderr，**stdout 最终只有一个 JSON 数组**（`add.ts:1403-1450`）。
- `-y` 且未给 `-a`（`add.ts:1825-1897`）：
  - 若运行环境被识别为某个 AI agent：目标 = 该 agent + 所有 universal（除非显式 `-a`）。
  - 否则若检测到 Eve：目标 = **仅 Eve**（全局安装会失败）。
  - 否则若没有任何已检测 agent：目标 = **全部 79 个 agent**。
  - 否则：目标 = 已检测 agents + 所有 universal（22 个，含 `promptscript`，见附录 A）。
  - **重要**：`ensureUniversalAgents()` 加入的 universal 列表包含 `promptscript`，而它的 `globalSkillsDir` 是 `undefined`。因此 `add -g -y`（以及 `add -g -y --json`）只要省略 `-a`，每个 skill 都会因 PromptScript 目标失败：JSON 会报 `status="failed"` 并 exit 1，文本模式只打印失败却 exit 0，而 canonical/其他 agent 以及 lock 可能已经写入。**不要省略 `-a`。**
- 非 TTY 下任何交互提示都会取消：非交互取消 exit 1，交互取消 exit 0（`add.ts:458-474`）。

### 3.2 安装模式：single-agent 默认 copy 的坑

模式决策（`add.ts:1987-2023`）：

1. `--copy` → copy。
2. 否则默认 symlink；但当目标只有一个唯一 `skillsDir`（或全部是 Eve）→ **copy**。
3. 非 `-y` 且多目录时才会询问 symlink/copy。

安装器语义（`installer.ts:371-463`）：

| 模式 | canonical `~/.agents/skills/<name>` | 非 universal agent 目录 |
|---|---|---|
| symlink | `rm -rf` 后完整复制 | 相对 symlink → canonical |
| copy | **不写 canonical**（除非目标本身是 universal，此时 agentBase 就是 canonical） | `rm -rf` 后完整复制到每个 agent |
| link | canonical 本身 symlink 到源 | symlink → canonical（add 的远程流程不用） |

因此：

- `skills add <src> -g -a claude-code -y` **只写 `~/.claude/skills`，不写 canonical**，JSON `mode="copy"`、`path` 为 Claude 目录。
- `skills add <src> -g -y`（本机检测到 Claude/Grok + universal）在构造上会走 symlink：写 canonical，链接 Claude/Grok，universal 直接共享；但如 §3.1 所述，自动 universal 含 PromptScript，会额外制造失败（JSON 全 failed）。因此它**不是**可用的 GUI 调用。
- 只有一个非 universal 目标时，追加 `-a universal`（或任一 universal ID，如 `codex`）可把唯一目录数变为 2，从而强制 symlink；`universal` 不会产生额外目录（`installer.ts:156-184,412-419`）。
- `--copy` + 多 agent：每个 agent 一份 copy，canonical 不写。

### 3.3 `add --json` 输出

Schema（`add.ts:588-606`、`add.ts:2312-2346`）：

```json
[
  {
    "name": "research",
    "status": "installed",
    "source": "mattpocock/skills",
    "ref": null,
    "hash": "0a6796c5667e95ed2301ba7381c123b4acb2ae1a",
    "path": "/Users/<user>/.agents/skills/research",
    "scope": "global",
    "agents": ["Claude Code", "Grok Build", "Universal"],
    "mode": "symlink",
    "security": null
  }
]
```

- 每 skill 一条；任一目标失败 → 该 skill `status="failed"`，只带 `name/status/error`。`agents` 为 **display name**，排除 skipped 目标。
- `path` = 第一个结果的 `canonicalPath ?? path`；copy 模式下是某个 agent 目录，不是 canonical。
- `mode` 是整体模式；symlink 失败回退 copy 时 **仍报 `mode="symlink"`**，JSON 不暴露 `symlinkFailed`（`installer.ts:443-464`、`add.ts:2338`）。
- `security` 仅对“GitHub 确认公开”的仓库请求 skills.sh audit；否则 null（`add.ts:608-656`、`telemetry.ts:99-126`）。
- 请求的 skill 名未匹配 → `status="skipped", reason="No matching skill found in source"`；有 skipped 也 exit 1（`add.ts:1698-1734`、`2343-2345`）。
- 致命错误（clone 失败、无有效 skill、local path 不存在、invalid agent、well-known+JSON）→ 单个 `{status:"failed", error}`，exit 1；参数解析错误（如 `--metadata` 非法）→ stdout `[]`、stderr 错误、exit 1（`cli.ts:387-396`）。

### 3.4 `add -l` 预览

- 先克隆/下载，再打印 `Available Skills`，按 plugin 分组，每项只有 **name + description**，没有 path；最后提示 `Use --skill <name>`，exit 0（`add.ts:1651-1699`）。
- 不能与 `--json` 同用；没有机器格式。
- 若想给安装向导提供结构化清单（name/description/path），只能 GUI 自己做 GitHub API 发现或临时 clone；`-l` 文本只适合兜底展示。

### 3.5 Source 接受格式与归一化

`parseSource`（`source-parser.ts:356-572`）：

| 输入 | 解析 | 说明 |
|---|---|---|
| `owner/repo` | github | 补 `https://github.com/owner/repo.git` |
| `owner/repo/sub/path` | github + subpath | 只在子目录内发现 |
| `owner/repo@skill` | github + skillFilter | 自动把 `skill` 加入 `-s` |
| `owner/repo#ref` | github + ref | ref 可为 branch/tag/SHA |
| `owner/repo#ref@skill` | 两者 | fragment 解析 |
| `github:owner/repo...` / `gitlab:owner/repo` | 前缀简写 | |
| `https://github.com/owner/repo`、`/tree/<ref>`、`/tree/<ref>/<subpath>` | github | |
| GitLab `/‑/tree/...`、`https://gitlab.com/group/sub/repo` | gitlab | 支持 subgroup |
| Azure `.../_git/repo?path=&version=` | git | GB/GT 前缀转为 ref |
| `git@host:owner/repo.git`、`ssh://...`、任意 `*.git` | git（通常） | **GitHub SSH 不会被识别为 github**，因此 update 会跳过（§5.2） |
| `./x`、`../x`、`/abs/x`、`.` | local | |
| `raw.githubusercontent.com/...`、GitHub archive/release、GitLab raw/archive | download | 直接下载 SKILL.md/zip/tar，限额 10 MiB 等 |
| `https://notion.so/<page>` / Notion pack | Notion | 特殊解析；依赖 `ntn`；不属于 v1 公开 GitHub 范围 |
| 其他 `https://...` | well-known → 失败再 download | Notion 页面/pack 有特殊处理；`add --json` 不支持 well-known |
| npm 包名（`npm:foo` 或不存在的包） | 无 | **add 不支持 npm 包**；包同步属于 `experimental_sync` |

别名：`coinbase/agentWallet`、`vercel-labs/vercel-skills`（`source-parser.ts:217-220`）。

全局 lock 归一化（`add.ts:2224-2277`）：

- GitHub：`source=owner/repo`（SSH 保留原 URL），`sourceUrl=https://github.com/owner/repo.git`，`sourceType=github`；`ref`、`skillPath` 原样记录；`skillFolderHash` 取 tree SHA。
- GitLab/通用 git：`source` 为完整 URL（或简写），`sourceType=gitlab|git`。
- local：绝对路径，`sourceType=local`。
- well-known：host/path 标识 + digest。
- 直接 download / Notion：不写全局 lock（`normalizedSource` 为空且不是 local），因此永远不会被 update 追踪。
- SSH/`git`、local、well-known、download、Notion 的更新能力见 §5.2。

### 3.6 重装/覆盖行为

- symlink 模式：`cleanAndCreateDirectory(canonical)` 先 `rm -rf` 再复制；agent symlink 若已指向同目标则保留，否则删除重建（`installer.ts:198-205,232-298,402-407`）。
- copy 模式：`cleanAndCreateDirectory(agentDir)` 后复制；canonical 保持不变（可能是旧内容）。
- **没有备份、没有内容 diff、没有“已存在则跳过”**；`-y` 下不确认。文本模式会显示 `overwrites:` 摘要，但 JSON 无此信息。
- 覆盖对象是目录级：agent 目录中多出的文件也会被删除。若目标路径是无关真实目录，同样会被 `rm -rf`。
- lock 在安装成功后写；写失败被吞掉（`add.ts:2242-2277` 的 `catch {}`），JSON 仍可能报 installed。

### 3.7 `add` 退出码

| 情况 | 文本模式 | `--json` 模式 |
|---|---:|---:|
| 全部成功 | 0 | 0（数组全部 `installed`） |
| 部分 agent/目标失败 | **0**（只打印 `Failed to install N`） | 1（受影响 skill `failed`） |
| 请求的 skill 名未匹配 | 1 | 1（`skipped` 条目） |
| 无有效 skill / local path 不存在 / clone 失败 / 非法 agent / 缺 source | 1 | 1（failed 对象） |
| `--json` 无 `-y`、`--json` + `-l`、well-known + `--json` | — | 1 |
| 交互取消且 stdin 是 TTY | 0 | — |
| 交互取消且 stdin 非 TTY | 1 | —（`--json` 已要求 `-y`，不会进入提示） |
| 参数解析错误（如 `--metadata` 非 JSON） | 1 | 1（stdout `[]`，错误在 stderr） |

## 4. `remove` 与 per-agent 断链

### 4.1 行为

`remove` 没有 `--json`；`parseRemoveOptions` 静默忽略未知 flag（`remove.ts:414-453`）。`-y` 跳过确认；非 TTY 无 `-y` 会在确认处取消并 exit 0。

算法（`remove.ts:70-404`）：

1. 扫描 canonical + **所有** agent 的 globalSkillsDir（不只是检测到的），收集含 `SKILL.md` 的目录名；同时读 lock keys（含 stale）。
2. 解析请求名：按 sanitizeName 匹配目录名或 lock key；若都不匹配，则把参数当作 **lock source**，删除该 source 下所有 skill（`remove.ts:40-68`）。
3. `targetAgents`：显式 `-a` 用给定列表；否则 = **全部 79 个 agent**。
4. 对每个 skill：
   - 删除每个 target agent 的路径（universal 的 `getInstallPath` 返回 canonical，但被跳过，稍后单独处理）。
   - `remainingAgents = detectInstalledAgents() - targetAgents`，逐一检查其路径是否存在；任一存在 → `isStillUsed=true`。
   - 若 `!isStillUsed`：`rm -rf canonical` + 删除 lock 条目。否则保留 canonical 与 lock。
5. 输出仅人类文本；部分失败只打印 `Failed to remove N`，**不设置 exit code**。

### 4.2 场景矩阵

| 命令 | canonical | lock | 其他 agent | 退出码 |
| --- | --- | --- | --- | --- |
| `remove -g -s X -y`（无 `-a`） | 删除 | 删除 | 清除所有 agent 的 X | 0（即使部分 rm 失败） |
| `remove -g -s X -a claude-code -y`，还有其他**已检测** agent 仍能看到 X（本机 universal 即满足） | 保留 | 保留 | 仅删除 Claude 路径 | 0 |
| 同上，但 Claude 是唯一已检测 agent / 没有其他检测 agent | **删除** | **删除** | — | 0 |
| 同上，但 Claude 根本没有 X，且没有其他检测 agent 能看到 X | **删除** | **删除** | 用户只想“断开一个未挂载的 agent”，却触发彻底卸载 | 0 |
| `remove -g -s X -a cursor -y`，X 只在 Claude，Claude 仍被检测到 | 保留（Claude 是 remaining 且路径存在） | 保留 | Claude 的 X 保留；cursor 无变化 | 0 |
| `remove -g -s X -a claude-code -y`，X 只在 Claude，另有一个**未被检测**的 agent 仍指向 X | 删除 | 删除 | 未检测 agent 的 symlink 变悬空 | 0 |
| `remove -g -s 不存在 -y` | 不变 | 不变 | 不变 | **0** |
| `remove -g --skill '*' -a cursor -y` | 若 cursor 是唯一/最后使用则删除 | 同左 | 只清 cursor | 0 |
| `remove -g --all` | 删除全部 | 删除全部 | 清全部 | 0 |
| `remove --agent '*' ...` | — | — | — | **1**（`*` 不是合法 AgentType；文档写法有误） |
| `remove --all X -y` | — | — | — | 1（拒绝 `--all` + 命名） |

结论：CLI 没有“仅断链、保证保留 canonical/lock”的契约。它把“是否保留 canonical”绑定到 **剩余已检测 agent 是否还能看到 X**；universal agent 因为直接读 canonical，几乎总会让 `isStillUsed=true`（本机就是如此），但这只是环境巧合，不是用户意图。反过来，如果用户只想断开一个未挂载的 agent 且没有其他检测 agent，CLI 会把整个 skill 卸载。

### 4.3 GUI 应自己实现的断链

推荐边界：

1. **单 agent 断链**：只操作该 agent 目录中的条目，绝不触碰 canonical/lock。
   - `lstat` 目标：symlink → `unlink`（不要 `rm -rf` 跟随）；真实目录 → 先检查是否被其他 agent 的 symlink 指向，再移到废纸篓/删除。
   - 删除前 `realpath(dirname(target))` 必须 ≠ canonical 目录；若 agent 的 skills 目录本身 symlink 到 `~/.agents/skills`，直接拒绝。
   - 扫描所有 agent 目录的 symlink，若 `realpath` 指向待删目录，先提示/改指 canonical，避免本机 HyperFrames 那种跨 agent 依赖断裂。
   - universal agent 没有可断的挂载：标记为 `native`，禁用“断开”或解释为“所有 universal agent 共享 canonical”。
2. **恢复挂载**：创建相对 symlink，目标为 canonical，链接名用 canonical 目录 basename：
   - 例：`~/.claude/skills/<name> -> ../../.agents/skills/<name>`；`~/.gemini/antigravity/skills/<name> -> ../../../.agents/skills/<name>`。
   - 原子创建、已存在则拒绝；先确保父目录存在；不覆盖真实目录。
3. **彻底卸载**：调用 `skills remove -g -s <name> -y`（不带 `-a`），然后校验 canonical、所有 agent 路径、lock key 均已消失；若 exit 0 但状态未变（no match），按失败处理。
4. 断链后必须让 GUI 自己的“更新应用”尊重当前挂载集；若调用 `skills update`，它会把断开的 agent 重新挂上（§5.4）。

## 5. `check` / `update`

### 5.1 最重要的事实：`check` 会更新

`cli.ts:420-423`：

```ts
case 'check':
case 'update':
case 'upgrade':
  await runUpdate(restArgs);
```

`runUpdate` 没有任何 check-only/dry-run 分支；检测到差异后立即 spawn `add ... -g -y` 重装（`update.ts:697-744`）。因此：

- **`skills check` = `skills update`**，会写盘。
- 包内 `docs/commands.md:166-172` 声称 check “without reinstalling”，与 v1.7.4 实现矛盾，是文档 bug。
- `check --help` 只打印通用 help（`cli.ts:344-362` 的短路），安全；但无参数的 `check` 危险。
- `update`/`check` 不解析 `--json`（`parseUpdateOptions` 只认 `-g/-p/-y` 与位置参数，`update.ts:66-84`）；传了也会照常更新。
- `-g` 单独就足以跳过 scope 提示并立即更新；`-y` 只影响 scope 自动判断与“上游删除”提示，**不是执行确认开关**。

### 5.2 检测机制

仅全局部分（`update.ts:496-748`）：

1. 读 lock，取所有 skill（可用位置参数做大小写不敏感的精确过滤）。
2. 跳过：
   - `sourceType=local` → `Local path`
   - `sourceType=git` → `Git URL`（GitHub SSH 也在此列）
   - `sourceType=well-known` 且无 digest → `Well-known skill`
   - `skillFolderHash` 为空 → `Private or deleted repo`
   - `skillPath` 为空 → `No skill path recorded`
3. 按 `source + ref` 分组。
4. GitHub：`GET https://api.github.com/repos/{owner}/{repo}/git/trees/{ref|HEAD|main|master}?recursive=1`，取 `skillPath` 对应文件夹的 tree SHA，与 lock 比较；若 lock 中的路径在 tree 中不存在 → 回退 clone 以重定位；API 失败 → clone。
5. 非 GitHub：直接 clone，比较计算出的 folder hash。
6. 发现差异后逐个 skill spawn：
   ```text
   node <cli> add <source>/<skillFolder>#<ref> --skill <name> -g -y
   ```
   （`update.ts:700-744`；`buildUpdateInstallSource` 见 `update-source.ts:102-132`）
7. 子进程 exit 0 → 记 success；非 0 → failure。没有解析子进程 stdout/stderr。
8. 上游删除的 skill：交互 TTY 且无 `-y` 时会询问是否删除本地；非交互/`-y` 只打印警告，**保留本地与 lock**。

### 5.3 输出与退出码

无 JSON，输出为人类文本（节选）：

- `Checking for skill updates…`
- `Checking skills from source: <source>`
- `Found N global update(s)`
- `Updating <name>…` / `✓ Updated <name>`
- `✓ All global skills are up to date`
- `N skill(s) cannot be checked automatically:` + 每项 `(reason)` + `To update: npx skills add ...`
- `Failed to update N skill(s)`
- `No installed skills found matching: <name>`

退出码（`update.ts:1026-1086`）：

| 情况 | exit |
|---|---:|
| 全部最新 / 无 lock / 名称不匹配 / 只有 skipped | 0 |
| 任一更新失败 | 1 |
| 上游有更新（即使成功应用） | 0 |
| `--json` | 被忽略，不改变行为 |

**没有“有可用更新”这一信号**；GUI 无法用 exit code 或文本稳定判断。

### 5.4 应用更新时保留什么

| 维度 | 是否保留 | 依据 |
|---|---|---|
| source/ref/skillPath | 是（由 lock 构造 URL） | `update-source.ts:102-132` |
| installedAt | 是（`addSkillToLock` 保留原值） | `skill-lock.ts:177-193` |
| `skillFolderHash` | 更新为新 tree SHA | 子进程 add 重写 |
| copy 模式 | **否** | 子进程 `add -y` 无 `--copy`，模式由目标目录数重新决定；通常变 symlink |
| 原 agent 挂载集 | **否** | 子进程不带 `-a`，`add` 用“已检测 + universal”或 host agent；可能新增挂载 |
| universal 的 legacy 原生目录 | 不处理 | 只写 canonical |
| 本地改动 | **不检测、不备份、直接覆盖** | `cleanAndCreateDirectory` |
| 断链状态 | **不保留**（断开的 agent 会被重新挂上） | 同上 |
| 上游删除的 skill | 非交互保留 | `promptDeletions` |

另有两个可靠性缺口：

- 子进程 add 在**文本模式**下部分目标失败仍 exit 0（§3.7），于是 update 会误记 success。由于子进程同样不带 `-a`，自动 universal 里的 `promptscript` 每次都会产生一个失败目标，但这个失败被文本模式的 exit 0 和丢弃的 stderr 完全吞掉。
- 子进程 stdout/stderr 被 `pipe` 后丢弃，失败细节不可见。

### 5.5 GUI 的更新策略

- **检测**：GUI 自己读 lock v3，对 GitHub 条目调用同样的 Trees API 比较 folder SHA；处理 60 req/h 匿名限流（可用 `GITHUB_TOKEN`/`GH_TOKEN`）。不调用 `skills check`。
- **应用**：
  - 方案 A（推荐，保留挂载语义）：用 lock 的 `source/ref/skillPath` 构造 `add` 命令，显式传“当前仍挂载的 agent IDs + `universal` 标记”，`-g -y --json`；成功后 lock 由 add 重写。
  - 方案 B（简单，放弃挂载/本地改动保真）：`skills update <name> -g -y`，并接受它重挂所有检测 agent。
- 对 `sourceType=git/local`、空 hash、`find-skills` 这类条目，直接标“不可自动更新”，不要调用 CLI。

## 6. 失败模式与错误分类

| 场景 | 命令 | 输出/退出码 | GUI 可识别性 |
|---|---|---|---|
| 未知命令 | 任意 | stdout `Unknown command: X`，exit 1 | 高 |
| 非法 agent（list） | `list -a bad` | stdout 文本 + valid list，exit 1，**非 JSON** | 中 |
| 非法 agent（add --json） | `add ... -a bad` | JSON failed 对象，exit 1 | 高 |
| 缺 source | `add --json` | JSON failed `Missing required argument: source`，exit 1 | 高 |
| 仓库不存在/网络失败 | `add` | clone 错误文本；JSON failed（含 `Failed to clone repository`），exit 1 | 高（JSON） |
| 超时 | clone | `Clone timed out after Ns...`，exit 1 | 高 |
| 鉴权失败 | clone | `Authentication failed for ...`，exit 1 | 高 |
| 无有效 SKILL.md | add | exit 1；JSON failed `No valid skills found...` | 高 |
| skill 名不匹配 | add | JSON skipped + exit 1 | 高 |
| 本地路径不存在 | add | JSON failed，exit 1 | 高 |
| well-known + JSON | add | exit 1，`--json is not yet supported...` | 高 |
| 写入权限/部分目标失败 | add 文本 | 打印失败但 **exit 0** | **低** |
| 同上 | add --json | 该 skill failed，exit 1 | 高 |
| 删除名不匹配 | remove | 仅文本，**exit 0** | **低** |
| 删除部分失败 | remove | 文本，**exit 0** | **低** |
| 删除非法 agent / --all 冲突 | remove | exit 1 | 中 |
| 更新失败 | update/check | 文本 + exit 1 | 中 |
| 无更新/名称不匹配 | update/check | 文本 + exit 0 | 低 |
| lock 损坏/旧版本 | list/add/remove/update | list 静默 null；后续写入覆盖 lock | **极低** |
| 并发写 lock | 任意变更 | 后写覆盖先写，丢条目 | 极低 |

建议 GUI 的错误分类：

- `add`：唯一可以完全依赖 JSON 的变更命令；解析数组 + exit code，并把 `status=failed/skipped` 映射为 UI 错误。
- `remove`/`update`：**不要解析文本或只看 exit code**；执行前后读取文件系统与 lock 做状态 diff，以 diff 作为成功判据。
- 所有变更前校验 lock：JSON 可解析、`version === 3`、`skills` 是对象；不满足则拒绝或先备份。

## 7. 运行时前提与环境变量

- **Node**：`>=22.20.0`。本机实际 v24.15.0（PATH）与 v26.12.1（Homebrew）；GUI 从 Finder 启动时 PATH 很短，必须显式解析 node。
- **CLI 安装形态**：本机无 `skills` 全局 bin，只有 npx cache；`npx skills` 会按 spec 解析并可能联网。研究时 npm latest = 1.7.4，但仍应固定 `skills@1.7.4` 或随应用打包 `node_modules/skills`，避免未来漂移与 npx 首次安装失败。
- **外部依赖**：`git`（必需）；`gh`（可选，clone 鉴权与 API fallback）；`ssh`（可选）；`ntn`（仅 Notion）。
- **网络**：github.com、api.github.com、skills.sh（telemetry/audit、find/download）。
- **环境变量**：
  - `DISABLE_TELEMETRY=1` 或 `DO_NOT_TRACK=1`：关闭 telemetry 与 audit（`telemetry.ts:86-87`）。建议 GUI 始终设置，否则进程退出前最多等待 5s flush（`telemetry.ts:187-191`）。
  - `GITHUB_TOKEN` / `GH_TOKEN`：GitHub API；`gh api` fallback。
  - `GH_HOST`：改变 shorthand 的 GitHub host；公开 GitHub 场景应清空。
  - `XDG_STATE_HOME`：**改变全局 lock 路径**；`XDG_CONFIG_HOME`、`CLAUDE_CONFIG_DIR`、`CODEX_HOME`、`GROK_HOME`、`AUTOHAND_HOME`、`HERMES_HOME`、`VIBE_HOME`、`SARVAM_HOME`：改变 agent 目录/检测。
  - `SKILLS_CLONE_TIMEOUT_MS`：clone 超时。
  - `INSTALL_INTERNAL_SKILLS=1`：显示/安装 `metadata.internal` skill。
  - `SKILLS_API_URL`、`SKILLS_DOWNLOAD_URL`、`SKILLS_DEBUG`：find/download/Notion 调试用途（bundle 中存在）。
  - `NO_COLOR`：**实测 v1.7.4 不生效**（`NO_COLOR=1` 下 list 文本仍有 ANSI）；只有 `--json` 保证无 ANSI。GUI 应使用 JSON，或在展示前剥离 ANSI。
  - agent 检测环境：`CURSOR_AGENT`、`CURSOR_EXTENSION_HOST_ROLE` 等会触发“运行在 agent 内”的非交互分支，改变 `add -y` 的默认 agent 集；GUI 应清理这些变量并始终显式 `-a`。
- **认证/私有仓库**：文档声称支持私有仓库（git credential/gh/SSH）；但 v1 范围只做公开 GitHub。私有仓库常见结果是 API 拿不到 tree → `skillFolderHash=""` → update 永远 skipped；GitHub SSH 又被归类为 `git` → 同样 skipped。不要向用户承诺私有仓库可更新。
- **遥测内容**：install 会发送 source、skills、agents、global、skillFiles、metadata；remove 发送 source/skills/agents/global/sourceType；update 发送计数（`telemetry.ts:6-62,132-168`）。公开仓库才会发 audit 请求。

## 8. 缺口与风险（按严重度）

### 严重（会破坏用户状态或产生错误结论）

1. **`check` 是 update 别名**：GUI 若把 `skills check` 当只读检测，会在用户不知情时重装/覆盖。缓解：禁用该命令，自己实现检测。
2. **per-agent `remove -a` 不可靠**：可能删 canonical + lock，也可能在目标 agent 未挂载时误删；取决于“剩余已检测 agent”。缓解：GUI 自己 unlink；彻底卸载用无 `-a` 的 remove 并校验。
3. **update 重挂所有检测 agent、覆盖本地改动、不保留 copy/断链**：用户的“只给 Claude 用”会在 update 后变成所有 agent 都有。缓解：GUI 自己构造 add 并显式传挂载集。
4. **单 agent add 默认 copy 且不写 canonical**：GUI 若按 canonical 建模会看不到新装 skill，且 JSON `path` 指向 agent 目录。缓解：显式加 universal 标记强制 symlink，或安装后校验 `path == canonical`。
5. **`--all` / `-a '*'` / 省略 `-a` 的 `-y` + `-g` 会制造失败**：目标包含 Eve 与 PromptScript（`globalSkillsDir=undefined`），且 `ensureUniversalAgents()` 自动加入的 22 个 universal 就含 PromptScript。每个 skill 都因至少一个目标失败而 `status=failed`、exit 1（文本模式仍 exit 0）；`-a '*'` 还会尝试为几十个未安装 agent 建目录。缓解：始终显式 `-a`；禁止 `--all`/`-a '*'`。
6. **lock 损坏/version<3 静默清空**：下一次写入覆盖全部 provenance。缓解：变更前校验并备份；版本不符拒绝执行。

### 高（会让 UI 显示或判定错误）

7. **`list` 的 `agents` 把 universal 原生共享误报为挂载**：本机每个 canonical skill 至少显示 6 个 universal agent，即使没有任何独立链接。缓解：结合 `lstat` 与 universal 规则重新计算。
8. **`list -a` 不是过滤器**：仍会扫描并归属其他 agent 的目录。缓解：拉全量在 GUI 内过滤。
9. **JSON schema 不足以渲染详情**：无 description、installedAt/updatedAt/hash/ref、mount 模式、symlink 目标。缓解：读 lock + SKILL.md + lstat。
10. **文本模式部分失败 exit 0**（add/remove）：脚本化会误判成功。缓解：add 用 `--json`；remove 用状态 diff。
11. **symlink 失败回退 copy 不暴露**：JSON `mode` 仍为 symlink；GUI 若相信会误判挂载类型。缓解：安装后 lstat。
12. **update 子进程输出被丢弃，且子 add 可能 exit 0 部分失败**：update 可能误报成功。缓解：不依赖 update 结果，更新后核对 hash/文件。
13. **锁无并发保护/非原子写**：并行安装会丢 entry，崩溃可能截断 JSON。缓解：GUI 串行化变更，写前备份。
14. **npx/Node PATH 漂移**：未固定版本会随 npm latest 变化；Finder 启动无 shell PATH。缓解：打包固定 CLI + 解析 node 绝对路径。
15. **`NO_COLOR` 无效**：文本输出带 ANSI；`--json` 才干净。缓解：只解析 JSON。

### 中（边界行为/体验）

16. **universal agent 无法真正断链**：它们直接读 canonical；只能“移除 legacy 原生目录”，不影响其可见性。UI 应标为共享而非可断开。
17. **跨 agent symlink 依赖**：本机 Gemini/Copilot/OpenCode/Antigravity 的 10 个链接指向 Claude 的 copy；删除 Claude copy 会制造悬空链接。断链前必须做依赖扫描。
18. **copy 与 canonical 可能内容分叉**：单 agent copy 安装不更新已存在的 canonical；list 仍显示 canonical 路径。缓解：统一用 symlink 模式；安装后比对。
19. **同名 skill 冲突**：list 按 frontmatter name 去重合并，lock 以 name 为 key；第二个同名 skill 会覆盖第一个。UI 需在安装前检测 canonical/lock 同名。
20. **不可更新条目**：`sourceType=git/local`、空 hash（如 `find-skills`）、无 skillPath、well-known 无 digest 都会被 update 跳过；上游删除的 skill 在非交互下不会清理。UI 应显示“不可自动更新/需手动处理”。
21. **`remove --agent '*'` 不可用**（`*` 不合法），与文档示例矛盾；`remove` 无 JSON。UI 不要照抄文档。
22. **`update` 无批量 JSON/无 dry-run**；每个 skill 单独 spawn add，N 个更新会重复 clone/请求，慢且失败面大。
23. **`add -l` 无 path/JSON**：无法直接支撑“选择仓库内 skill”的结构化向导；需要自建发现。

## 9. 推荐的 GUI 命令边界（v1）

**只读**

- `skills list -g --json`：仅作为磁盘索引；失败时（exit 1/非 JSON）降级为直接扫描 `~/.agents/skills` + agent 目录。
- 直接读 `~/.agents/.skill-lock.json`（尊重 `XDG_STATE_HOME`）与各 `SKILL.md`。
- 挂载状态：GUI 自己 `lstat`，不要用 `agents` 字段。

**变更**

- 安装：`skills add <source> -g -s <names...> -a <selected ids...> -a universal -y --json`
  - 至少两个不同 `skillsDir` 组以强制 symlink；`universal` 作为不产生额外 agent 目录的 canonical 标记。
  - 禁止 `--all`、`-a '*'`、`-g` 下的 Eve/PromptScript。
  - 校验每条 `status=installed`、`path` 在 `~/.agents/skills` 下；否则按失败处理并核对 lstat。
- 彻底卸载：`skills remove -g -s <name> -y`，执行前后 diff canonical/agent/lock；exit 0 不足以保证成功。
- 单 agent 断链/恢复：GUI 直接操作 symlink（§4.3），不调用 CLI。
- 检查更新：GUI 自己用 lock + GitHub Trees API；不用 `skills check`。
- 应用更新：优先 GUI 构造 `skills add <lock-derived-url> -g -s <name> -a <current-mount ids...> -a universal -y --json`；若接受“全 agent 重挂 + 覆盖本地改动”，才用 `skills update <name> -g -y`。

**运行环境**

- 解析绝对 node 路径；固定 CLI 版本（`skills@1.7.4` 或打包）。
- spawn env：`DISABLE_TELEMETRY=1`、`DO_NOT_TRACK=1`、`GIT_TERMINAL_PROMPT=0`；清理 `CURSOR_AGENT`、`CURSOR_EXTENSION_HOST_ROLE` 等 agent 检测变量；公开 GitHub 时清空 `GH_HOST`；保留用户 `HOME`/`XDG_*`（否则路径与 GUI 读取不一致）。
- 串行化所有变更命令；每次变更前备份 lock；变更后重建内存索引。

## 附录 A：agent 规则与关键目录

- 注册表：79 个 ID（`agents.ts:80-827`）。
- `isUniversalAgent()` 仅看 `skillsDir === '.agents/skills'`（`agents.ts:914-916`）。24 个 ID 属于此类；`getUniversalAgents()` 排除 `showInUniversalList:false` 的 `replit`、`universal`，因此 `-y` 会自动加入 **22 个** universal，其中包含全局不支持的 `promptscript`（`agents.ts:878-884,795-806`）。这是 `add -g -y` 省略 `-a` 时必然部分失败的原因。
- 全局不支持的 ID：`eve`、`promptscript`（`globalSkillsDir=undefined`）。
- 关键 agent：

| ID | Display | CLI 全局写入位置 | universal |
|---|---|---|---|
| `claude-code` | Claude Code | `~/.claude/skills` | 否 |
| `grok` | Grok Build | `~/.grok/skills` | 否 |
| `codex` | Codex | `~/.agents/skills` | 是 |
| `gemini-cli` | Gemini CLI | canonical（legacy `~/.gemini/skills` 仍被扫描） | 是 |
| `github-copilot` | GitHub Copilot | canonical（legacy `~/.copilot/skills`） | 是 |
| `opencode` | OpenCode | canonical（legacy `~/.config/opencode/skills`） | 是 |
| `antigravity` | Antigravity | canonical（legacy `~/.gemini/antigravity/skills`） | 是 |
| `pi` | Pi | canonical | 是 |
| `cursor` | Cursor | canonical（legacy `~/.cursor/skills`） | 是 |
| `amp` | Amp | canonical（docs 的 `~/.config/agents/skills` 不是 CLI 实际写入点） | 是 |
| `universal` | Universal | canonical | 是（伪 agent，适合做 symlink 标记；被 `getUniversalAgents` 排除） |
| `promptscript` | PromptScript | 不支持全局 | 是（`globalSkillsDir=undefined`，但会被 `-y` 自动加入 → 全局失败） |
| `eve` | Eve | 不支持全局 | 项目专用 |

> 注意：`docs/agents.md` 的 “Global Path” 是 agent 的原生目录，不等于 CLI 对 universal agent 的实际写入位置；以 `agents.ts` 的 `isUniversalAgent` 规则为准。

## 附录 B：证据与复现

只读命令（均未改变用户状态）：

```bash
node ~/.npm/_npx/5606f1555d02ef53/node_modules/skills/bin/cli.mjs --version
node .../cli.mjs list -g --json
node .../cli.mjs list -g --json -a claude-code
node .../cli.mjs list -g --json -a not-an-agent
node .../cli.mjs list -g
node .../cli.mjs --help
node .../cli.mjs remove --help
node .../cli.mjs check --help
```

补充沙箱（仅写 `/tmp`）：用 `XDG_STATE_HOME=/tmp/badstate` 放入坏 JSON，`list -g --json` 仍 exit 0 且全部 `source=null`；version=2 同样被当作空 lock。

源码检查：`/tmp/skills-research-01`（tag v1.7.4）。

## 来源

- [src/cli.ts:420-423（check/update 别名）](https://github.com/vercel-labs/skills/blob/v1.7.4/src/cli.ts#L420-L423)
- [src/list.ts:55-128（list 参数与 JSON schema）](https://github.com/vercel-labs/skills/blob/v1.7.4/src/list.ts#L55-L128)
- [src/installer.ts:1186-1219（扫描范围）](https://github.com/vercel-labs/skills/blob/v1.7.4/src/installer.ts#L1186-L1219)
- [src/installer.ts:1275-1407（agent 归属与去重）](https://github.com/vercel-labs/skills/blob/v1.7.4/src/installer.ts#L1275-L1407)
- [src/installer.ts:371-463（symlink/copy 写入）](https://github.com/vercel-labs/skills/blob/v1.7.4/src/installer.ts#L371-L463)
- [src/agents.ts:80-827（79 个 agent 注册表）](https://github.com/vercel-labs/skills/blob/v1.7.4/src/agents.ts#L80-L827)
- [src/agents.ts:878-916（universal 规则）](https://github.com/vercel-labs/skills/blob/v1.7.4/src/agents.ts#L878-L916)
- [src/add.ts:588-606（add JSON schema）](https://github.com/vercel-labs/skills/blob/v1.7.4/src/add.ts#L588-L606)
- [src/add.ts:1503-1511（--json 约束）](https://github.com/vercel-labs/skills/blob/v1.7.4/src/add.ts#L1503-L1511)
- [src/add.ts:1651-1699（-l 预览）](https://github.com/vercel-labs/skills/blob/v1.7.4/src/add.ts#L1651-L1699)
- [src/add.ts:1987-2023（安装模式决策）](https://github.com/vercel-labs/skills/blob/v1.7.4/src/add.ts#L1987-L2023)
- [src/add.ts:2224-2277（全局 lock 写入）](https://github.com/vercel-labs/skills/blob/v1.7.4/src/add.ts#L2224-L2277)
- [src/add.ts:2312-2346（add JSON 输出）](https://github.com/vercel-labs/skills/blob/v1.7.4/src/add.ts#L2312-L2346)
- [src/remove.ts:303-343（canonical 保留判定）](https://github.com/vercel-labs/skills/blob/v1.7.4/src/remove.ts#L303-L343)
- [src/remove.ts:391-403（remove 退出行为）](https://github.com/vercel-labs/skills/blob/v1.7.4/src/remove.ts#L391-L403)
- [src/update.ts:496-748（全局检测与应用）](https://github.com/vercel-labs/skills/blob/v1.7.4/src/update.ts#L496-L748)
- [src/update.ts:1026-1086（runUpdate 退出码）](https://github.com/vercel-labs/skills/blob/v1.7.4/src/update.ts#L1026-L1086)
- [src/update-source.ts:102-132（更新 URL 构造）](https://github.com/vercel-labs/skills/blob/v1.7.4/src/update-source.ts#L102-L132)
- [src/skill-lock.ts:8-102（lock version/path/静默清空）](https://github.com/vercel-labs/skills/blob/v1.7.4/src/skill-lock.ts#L8-L102)
- [src/source-parser.ts:356-572（source 格式）](https://github.com/vercel-labs/skills/blob/v1.7.4/src/source-parser.ts#L356-L572)
- [src/telemetry.ts:86-87（telemetry 开关）](https://github.com/vercel-labs/skills/blob/v1.7.4/src/telemetry.ts#L86-L87)
- [src/blob.ts:235-318（GitHub Trees API 与 folder hash）](https://github.com/vercel-labs/skills/blob/v1.7.4/src/blob.ts#L235-L318)
- [docs/commands.md:166-172（check 文档与实现矛盾）](https://github.com/vercel-labs/skills/blob/v1.7.4/docs/commands.md#L166-L172)
- [package.json:145-147（Node engines）](https://github.com/vercel-labs/skills/blob/v1.7.4/package.json#L145-L147)
