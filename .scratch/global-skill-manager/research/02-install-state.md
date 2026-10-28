# 02 全局安装的磁盘真相与异常形态

> 研究对象：`vercel-labs/skills` CLI v1.7.4（本机 `npx` 缓存中的 1.7.4；源码克隆于 commit `d8eca76d5dbe47df2d146d036e32534b74ece57a`）。
> 方法：只读盘点本机 `~/.agents`、各 agent 全局目录与 lock；对照 CLI 源码逐条验证。未修改任何 skill 目录、agent 目录或 lock 文件。
> 研究时间：2026-10-28。盘点期间观察到一次外部进程在 10:52 写入 lock（新增 `vercel-react-best-practices`），因此本机计数从票据创建时的 **61 lock / 49 list** 变为 **62 lock / 50 list**；差额与原因不变。

## 0. 结论速览

- lock 是“安装来源账本”，不是磁盘清单；`skills list` 是“磁盘扫描结果”，不是 lock 的视图。两者按不同规则计数，不能互相校验数量。
- 41 个 canonical skill 中，40 个能用 lock 的 40 位 Git tree hash 在本地重算出完全一致的 hash；唯一例外是 `vercel-react-best-practices`，原因是安装器故意跳过了源仓库的 `metadata.json`。因此 hash 可以用于本地改动检测，但必须先扣除安装器排除项，否则会误报。
- 21 条 lock-only 条目全部是旧安装残留（20 个 `mattpocock/skills` + 1 个 `vercel-labs/skills` 的 `find-skills`）。它们不是被 `list` 过滤掉的“有效条目”，而是磁盘上根本没有目录。
- `skills list -g` 额外列出 9 个 Antigravity 下由其他工具生成的 `gitnexus-*`（无 lock、source=null），所以 49 = 40 个受管 canonical + 9 个未受管目录；61 = 40 个受管 + 21 个失效 lock。61 − 49 = 21 − 9 = 12。
- `skills list --json` 的 `agents` 数组对 Gemini CLI、GitHub Copilot、OpenCode、Antigravity 会过度上报：这 4 个 agent 的全局目录实际各只有 10 个与 canonical 同名的 skill（Antigravity 另有 9 个未受管 gitnexus），但 `list` 对全部 41 个 canonical skill 都声称已挂载。判定挂载必须看物理目录，不能信这个数组。
- lock 没有记录安装模式（symlink/copy）、没有记录每个 agent 的挂载状态，也没有逐 skill 记录“实际安装到了哪些 agent”。`lastSelectedAgents` 只是上次交互式选择的偏好，不是事实。
- `skills update -g` 只比较“远端最新 hash”和 lock，不检查本地副本是否被改过；它会按“宿主 agent（无宿主时为全部检测到的 agent）+ universal agents”重新安装，可能新增链接、把 copy 安装改成 symlink，甚至把 21 条 stale lock 中仍存在于上游的 7 个 skill 重新装回来。GUI 不应无条件调用 update-all。

## 1. 磁盘上的四层对象

### 1.1 路径解析

| 对象 | 路径 | 说明 |
|---|---|---|
| 全局 lock | `~/.agents/.skill-lock.json`，或 `$XDG_STATE_HOME/skills/.skill-lock.json` | `getSkillLockPath()`，`src/skill-lock.ts:67-73`。本机未设置 `XDG_STATE_HOME`。 |
| canonical 仓库 | `~/.agents/skills/<dir>` | `getCanonicalSkillsDir(true)`，`src/installer.ts:133-136`。symlink 模式下这里是唯一内容源。 |
| agent 全局目录 | 见 1.3 表 | 每个 agent 的 `globalSkillsDir`；由 `src/agents.ts` 在模块加载时按环境变量计算。 |
| 项目 lock（对照） | `<cwd>/skills-lock.json` | v1，字段是 `computedHash`，不是 `skillFolderHash`；与全局问题无关，但不要把两者混淆（`src/local-lock.ts`）。 |

环境变量会影响路径，GUI 不能硬编码：

- `XDG_STATE_HOME` → 全局 lock 位置
- `CLAUDE_CONFIG_DIR` → Claude Code 全局目录
- `CODEX_HOME` → Codex 检测根（但 Codex 的全局 skills 固定为 `~/.agents/skills`）
- `GROK_HOME` → Grok 全局目录
- `XDG_CONFIG_HOME` → OpenCode / Amp / Universal 等 `~/.config` 路径

### 1.2 canonical 目录的语义

- symlink 模式：安装时把源 skill 复制到 `~/.agents/skills/<name>`，再给每个非 shared agent 建**相对 symlink**指向它（`src/installer.ts:283-293`、`403-406`）。
- copy 模式：跳过 canonical，直接复制到每个目标目录（`src/installer.ts:371-381`）。若目标中包含 shared agent，则该 agent 的目录就是 canonical，所以 canonical 仍可能存在。
- link 模式：canonical 本身是指向源目录的 symlink，不复制内容（`src/installer.ts:396-401`）。`add` 的常规全局安装不暴露该模式，但本地开发/恢复流程可能出现。
- 安装时会 `cleanAndCreateDirectory(canonicalDir)`，即**同名再安装是覆盖**，不保留旧版本（`src/installer.ts:198-205`、`403-405`）。

### 1.3 agent 全局目录（v1.7.4 官方表）

| agent | 全局目录 | 项目目录 | 本机状态 |
|---|---|---|---|
| `claude-code` | `~/.claude/skills`（受 `CLAUDE_CONFIG_DIR` 影响） | `.claude/skills` | 41 个：31 symlink + 10 真实副本 |
| `codex` | `~/.agents/skills`（shared） | `.agents/skills` | 共享 canonical；另有 10 条旧 `~/.codex/skills` 链接，CLI 不扫描 |
| `gemini-cli` | `~/.gemini/skills` | `.agents/skills` | 10 条 symlink（全部指向 Claude 副本） |
| `github-copilot` | `~/.copilot/skills` | `.agents/skills` | 10 条 symlink |
| `opencode` | `~/.config/opencode/skills` | `.agents/skills` | 10 条 symlink |
| `antigravity` | `~/.gemini/antigravity/skills` | `.agents/skills` | 10 条 symlink + 9 个真实 `gitnexus-*` |
| `grok` | `~/.grok/skills`（受 `GROK_HOME` 影响） | `.grok/skills` | 30 条 symlink |
| `pi` | `~/.agents/skills`（shared） | `.agents/skills` | 共享 canonical；另有 26 条旧 `~/.pi/agent/skills` 链接，CLI 不扫描 |
| `cursor` / `qoder` / `qoder-cn` / `amp` 等 | `~/.cursor/skills`、`~/.qoder/skills`、`~/.qoder-cn/skills`、`~/.config/agents/skills` | 各自 | 本机无这些目录 |

完整映射见 `src/agents.ts:80-827` 与官方 `docs/agents.md:8-60`。

### 1.4 不是事实的字段

| 字段/输出 | 真实语义 | 不能用来判断 |
|---|---|---|
| `lastSelectedAgents` | 上一次交互式 agent 选择，安装前就写入（`src/add.ts:482-528`） | 某个 skill 实际挂载到哪些 agent |
| `dismissed.findSkillsPrompt` | UI 提示是否已关闭 | 任何安装状态 |
| `skills list --json` 的 `agents` | CLI 的扫描/推断结果，存在过度上报（见 3.3） | 物理挂载真相 |
| `installedAt` / `updatedAt` | lock 写入时间；`updatedAt` 每次 add/update 都会变 | 文件内容是否被用户改过 |
| `updatedAt` vs 目录 mtime | 无固定关系；手工编辑不会改 lock | 本地改动检测 |

## 2. lock v3 schema

### 2.1 顶层结构

`src/skill-lock.ts:13-60`：

```json
{
  "version": 3,
  "skills": {
    "<installName>": { "...": "SkillLockEntry" }
  },
  "dismissed": { "findSkillsPrompt": true },
  "lastSelectedAgents": ["claude-code", "..."]
}
```

- `version`：当前为 3。`readSkillLock()` 对 `version < 3` 直接返回空 lock（`src/skill-lock.ts:92-96`）；对 `version > 3` 不拒绝，会按 v3 形状返回。GUI 应只接受 `=== 3`。
- key 是**安装名**（`skill.installName` / `skill.name`），不是目录名、也不是 frontmatter 显示名的必然同义词。特殊字符会经 `sanitizeName()` 变成目录名（如 `ce:review` → `ce-review`），但 lock key 保留原名。`list` 会用 sanitized key 做兜底匹配（`src/list.ts:118-122`）。
- 写入是 `JSON.stringify(lock, null, 2)` 直接覆盖，无临时文件、无 fsync、无文件锁（`src/skill-lock.ts:109-118`）。
- 读取遇到 JSON 解析失败会返回空 lock（`src/skill-lock.ts:99-102`）。下一次 `addSkillToLock()` 会把这个空 lock 写回，导致**整个 lock 被清空**。这是 GUI 必须防的数据丢失路径。

### 2.2 SkillLockEntry 字段

| 字段 | 类型 | 何时出现/为空 | 语义与用途 |
|---|---|---|---|
| `source` | string | 必有 | 规范化来源：GitHub 为 `owner/repo`；非 GitHub/SSH 保留 URL；local 为路径。用于分组、`remove <source>`、展示。 |
| `sourceType` | string | 必有 | `github` / `gitlab` / `git` / `local` / `well-known` / `download`（`src/types.ts:108-116`）。本机 62 条全部 `github`。 |
| `sourceUrl` | string | 必有 | 原始拉取 URL，例如 `https://github.com/owner/repo.git`；update 用它重新 fetch。generic Git 若缺它，update 会失败。 |
| `ref` | string? | 可选 | 分支/tag；缺省为默认分支。update 按 `source + ref` 分组，避免不同 ref 互相污染。本机全部无 `ref`。 |
| `skillPath` | string? | 可选 | 源仓库内 `SKILL.md` 的相对路径，例如 `skills/agent-browser/SKILL.md`。update 用 `dirname` 定位文件夹；缺失时 skip（`src/update.ts:273-275`）。 |
| `skillFolderHash` | string | 必有（可为空串） | 源 skill 文件夹指纹。40 位 = Git tree SHA；64 位 = SHA-256 内容 hash；空串 = 无可用 hash。详见第 6 节。 |
| `installedAt` | ISO string | 必有 | 首次安装时间；`addSkillToLock` 保留旧值（`src/skill-lock.ts:186-190`）。 |
| `updatedAt` | ISO string | 必有 | 本条目最近一次 lock 写入时间；每次 add/update 都刷新，即使内容未变。 |
| `pluginName` | string? | 可选 | 源仓库 plugin manifest（marketplace.json / plugin.json）声明的分组名；本机 `mattpocock-skills` 35 条、`core-skills` 9 条、其余无（`src/skills.ts:217-225`、`src/plugin-manifest.ts`）。 |
| `sourceBaseUrl` | string? | 可选 | well-known 源的 base URL。 |
| `wellKnownDigest` | string? | 可选 | well-known 内容摘要；这类条目 `skillFolderHash` 通常为空，update 用 digest 而不是 hash。 |

本机 lock 校验结果：`version=3`、62 条、所有条目 6 个必填字段齐全、无重复 key、时间戳均为 ISO 格式。`dismissed.findSkillsPrompt=true`，`lastSelectedAgents` 有 16 个（包含本机并未安装的 amp/cline/cursor/droid/fx/kilo/kimi/warp/zed，进一步说明它只是偏好）。

### 2.3 空 hash / 缺字段的含义

- `find-skills`：`skillFolderHash=""`，`skillPath` 存在，`installedAt=2026-02-12`。`update` 把它归类为 “Private or deleted repo” 并 skip（`src/update.ts:270-271`）。空 hash 不表示“未修改”，只表示“无法校验”。
- well-known：hash 为空是正常设计，用 `wellKnownDigest`。
- local：hash 是本地目录的 SHA-256，但 `sourceType=local` 的 update 直接 skip（“Local path”）。
- generic `git`：update 也直接 skip（“Git URL”），即使有 hash。

## 3. 安装模式与“是否挂载”的判定

### 3.1 模式选择

`defaultInstallMode(targets)`（`src/add.ts:399-405`）：

- 所有目标共享同一个 `skillsDir`（或全是 Eve）→ `copy`
- 目标跨多个 skills 目录 → `symlink`（交互式还会让用户选）
- `--copy` 强制 copy

对全局安装，`skillsDir` 是**项目**相对目录，不是全局目录；这导致“universal”判断与全局语义混在一起（见 3.3）。

### 3.2 各模式的磁盘形态

| 模式 | canonical | 每个非 shared agent 的全局目录 | lock |
|---|---|---|---|
| symlink | 真实目录（从源复制） | 相对 symlink → canonical | 1 条 |
| copy | 仅当目标是 shared agent 时才存在 | 真实目录，内容独立复制 | 1 条 |
| link | symlink → 源目录 | 相对 symlink → canonical | 1 条 |
| symlink 失败回退 | 真实目录 | 真实目录（copy） | 1 条，无模式标记 |

关键：**lock 不记录模式**。同一个 lock 条目可能是 symlink、copy、link，或部分 agent 回退成 copy。只能从磁盘形态推断。

### 3.3 “universal” 的实现怪癖

`isUniversalAgent(type)` 只判断 `agents[type].skillsDir === '.agents/skills'`（`src/agents.ts:914-916`），而 `getAgentBaseDir(global=true)` 只要该值为真就返回 canonical（`src/installer.ts:156-164`）。

后果：

- **真正 shared 的 agent**（`globalSkillsDir === ~/.agents/skills`）：Codex、Pi、Cline、fx、Kimi Code、Warp、Zed 等。它们确实读 canonical，`list` 报全量是合理的。
- **项目目录是 `.agents/skills`、但全局目录不同的 agent**：Gemini CLI、GitHub Copilot、OpenCode、Antigravity、Cursor、Amp、Droid、Kilo 等。代码仍把它们当 shared：
  - `list` 会把 canonical 里的每个 skill 都算到它们头上，即使其全局目录里没有。
  - `installSkillForAgent` 在 `isGlobal && isUniversalAgent` 时直接返回，不创建其全局目录的链接（`src/installer.ts:409-419`）。
- 官方 `docs/agents.md:12,36,37,55` 明确这些 agent 的全局路径是 `~/.gemini/skills`、`~/.copilot/skills`、`~/.config/opencode/skills`、`~/.gemini/antigravity/skills`，不是 canonical。实现与文档不一致。

本机证据：Gemini/Copilot/OpenCode/Antigravity 的全局目录各只有 10 个旧 hyperframes 链接，但 `skills list -g --json` 对全部 41 个 canonical skill 都列出这 4 个 agent；新安装的 `vercel-react-best-practices` 只在 Claude 和 Grok 建了链接，没有进入这 4 个目录。

**GUI 判定规则：`shared` 只能用 `resolve(agent.globalSkillsDir) === resolve(canonicalDir)` 判断，不能用 `skillsDir === '.agents/skills'`。**

### 3.4 如何判定“某个 skill 挂载到某个 agent”

对每个 agent，取 `globalSkillsDir`（按环境变量解析），检查 `<dir>/<sanitized-name>`：

| 磁盘形态 | 判定 | 说明 |
|---|---|---|
| 路径不存在 | `absent` | 未挂载 |
| agent 目录就是 canonical | `shared` | 只要 canonical 有有效 skill 即挂载 |
| symlink，realpath == canonical/<name> | `linked` | 正常 symlink 安装；比较 realpath 或 dev+ino |
| symlink，realpath == 另一个 agent 的副本 | `linked-to-copy` | 旧链/分离链；内容可能独立漂移 |
| symlink，target 不存在 | `broken` | 断链 |
| 真实目录，内容 hash == canonical | `copy` | copy 模式或 symlink 回退；当前同步 |
| 真实目录，内容 hash != canonical | `copy-drift` | 需要选择权威版本 |
| 真实目录，无 lock/canonical | `foreign` | 手工/其他工具安装 |

匹配名字时要用目录名（安装名 sanitized），并可回退到 frontmatter `name`（`list` 的 fallback 逻辑，`src/installer.ts:1334-1384`）。但**不要用 `lastSelectedAgents` 或 `list --json agents` 反推**。

### 3.5 `skills list` 的扫描范围与去重

`listInstalledSkills()`（`src/installer.ts:1182-1416`）：

1. 扫描 canonical。
2. 扫描每个**检测到的** agent 的 `globalSkillsDir`。
3. 再扫描所有 agent 的 `globalSkillsDir` 中“目录存在但 agent 未被检测到”的（`src/installer.ts:1256-1270`）。所以即使 agent 应用被卸载，只要目录还在，仍会列出。
4. 每个目录必须有 `SKILL.md` 且 frontmatter 有字符串 `name` 和 `description`，否则跳过（`src/installer.ts:1281-1295`、`src/skills.ts:80-133`）。
5. 按 `scope:frontmatterName` 去重（`src/installer.ts:1297-1298`、`1391-1408`）。同名多副本只保留一个条目，`path` 取第一次扫描到的位置（canonical 优先）。
6. `-a/--agent` 只过滤 `agentsToCheck`，不会缩小清单：第 3 步仍会把所有“目录存在”的 agent 目录加入扫描，所以 `skills list -g --json -a claude-code` 在本机仍返回 50 条，并包含 Antigravity 的 9 个 gitnexus。文档说 `-a` 是过滤，但实现是附加式（`src/installer.ts:1256-1270`）。

因此 `list` 的计数是**唯一 frontmatter 名字数**，不是目录数、不是 lock 条目数、也不是“安装次数”。

## 4. 为什么 lock 61 而 `skills list -g --json` 是 49

### 4.1 票据快照与当前快照

| 时点 | lock 条目 | list 条目 | 说明 |
|---|---:|---:|---|
| 2026-10-28 10:51（票据创建） | 61 | 49 | 本报告要解释的基线 |
| 2026-10-28 10:52 起（外部进程安装 `vercel-react-best-practices`） | 62 | 50 | 差额仍为 12；不是本研究写入 |

`vercel-react-best-practices` 的 lock `installedAt=2026-10-28T02:52:37.643Z`，lock mtime 10:52，canonical 目录同时出现；它只被链接到 Claude 和 Grok（非 shared 且已检测），没有进入 Gemini/Copilot/OpenCode/Antigravity。

### 4.2 精确分解（以 61/49 为准）

| 桶 | 数量 | 在 lock | 在 `list -g` | 说明 |
|---|---:|---|---|---|
| 受管 canonical skill（lock + 目录 + 有效 SKILL.md） | 40 | 是 | 是 | 每个都产生一个 list 条目 |
| 失效 lock（lock 有，canonical 和所有 agent 目录都没有） | 21 | 是 | 否 | 旧安装残留 |
| 未受管 agent-only skill（目录有，canonical/lock 都没有） | 9 | 否 | 是 | Antigravity 的 `gitnexus-*`，source=null |
| 合计 | lock 61 | list 49 | | |

另有 10 个与 canonical 同名的物理副本（hyperframes 族 + faceless + media-use，位于 Claude 目录）；它们不新增 lock 条目，也被 list 按名字合并，因此不参与上面的加减。

公式：

```
list = 受管 canonical + 未受管 agent-only = 40 + 9 = 49
lock = 受管 canonical + 失效 lock      = 40 + 21 = 61
61 - 49 = 21 - 9 = 12
```

当前（62/50）：

```
list = 41 + 9 = 50
lock = 41 + 21 = 62
```

同样相差 12。

### 4.3 21 条失效 lock 的具体构成

| hash 形态 | 数量 | 条目 |
|---|---:|---|
| 64 位 SHA-256 | 18 | caveman, design-an-interface, diagnose, edit-article, git-guardrails-claude-code, migrate-to-shoehorn, obsidian-vault, qa, request-refactor-plan, review, scaffold-exercises, setup-pre-commit, ubiquitous-language, write-a-skill, writing-beats, writing-fragments, writing-shape, zoom-out |
| 40 位 Git tree | 2 | to-issues, to-prd |
| 空串 | 1 | find-skills |

- 来源：20 条 `mattpocock/skills`，1 条 `vercel-labs/skills`（find-skills）。
- `pluginName`：6 条 `mattpocock-skills`，其余 15 条无 pluginName（多为 `deprecated/`、`personal/`、`in-progress/` 路径）。
- 时间：18 条 `installedAt=2026-06-11`、`updatedAt=2026-06-15`；2 条 `updatedAt=2026-07-01`；find-skills `2026-02-12`。
- 为什么目录没了：`skills remove` 会同时删除目录和 lock（`src/remove.ts:303-334`）；CLI 没有“只删目录、保留 lock”的正常路径。因此最可能是**用户在 CLI 之外删除/清理了这 20 个目录**（或旧版本工具行为），而不是当前 CLI 的过滤。无法从现有证据确定具体执行者。
- 这 21 条并非无害：20 条 mattpocock 中，**7 条的上游 `skillPath` 仍存在且当前 hash 已变化**（git-guardrails-claude-code, migrate-to-shoehorn, scaffold-exercises, setup-pre-commit, writing-beats, writing-fragments, writing-shape）。按 `update` 代码，`skills update -g` 会把它们当成可更新条目重新安装回 canonical，并链接到当前检测到的 agent。另外 13 条上游路径已删除，update 会走“deleted upstream”提示（交互式可清理，非交互式跳过）。find-skills 空 hash，update 直接 skip（但上游仍存在，重新 `add` 可装回）。

### 4.4 9 条未受管 agent-only

全部位于 `~/.gemini/antigravity/skills/`：

`gitnexus-cli`, `gitnexus-debugging`, `gitnexus-exploring`, `gitnexus-guide`, `gitnexus-impact-analysis`, `gitnexus-pdg-query`, `gitnexus-pr-review`, `gitnexus-refactoring`, `gitnexus-taint-analysis`

- 都是单文件 skill（只有 `SKILL.md`），创建于 2026-06-09 / 07-03。
- lock 无条目，canonical 无副本；`list` 的 `source/sourceUrl/sourceType` 全为 `null`，`agents` 只有 Antigravity。
- 内容提到 `node .gitnexus/run.cjs`、`npx gitnexus analyze`，判断为 GitNexus 工具生成/手工放入，不是 `skills` CLI 管理。
- 它们使 list 比“受管 skill 数”多 9，是 61 vs 49 差额从 21 缩小到 12 的原因。

### 4.5 为什么不是其他原因

| 猜测 | 是否成立 | 证据 |
|---|---|---|
| `list` 过滤了没有 source 的 lock 条目 | 否 | 21 条都有完整 source/sourceUrl；它们没有目录，磁盘扫描自然看不到。 |
| `list` 过滤了 plugin skill | 否 | 9 个 core-skills、29 个 mattpocock-skills 都在 list 中；pluginName 不是过滤条件。 |
| 同名去重导致少了 12 | 否 | 去重只影响 10 个物理副本（同名），不改变 40 个 canonical 名字；去掉副本后仍是 40 + 9。 |
| lock key 与目录名不一致 | 否 | 本机 62 个 key 全部等于 sanitize 后的目录名；21 条缺失是目录真的不存在。 |
| `SKILL.md` 无效被跳过 | 否 | 所有在盘 canonical 目录（当前 41/41）frontmatter 完整；没有“有目录但无 SKILL.md”的 canonical 条目。 |
| `.DS_Store` 占了条目 | 否 | `list`/`remove` 都要求目录 + `SKILL.md`；`.DS_Store` 是文件，`.system` 没有根 `SKILL.md`。 |
| `list` 只统计“已挂载”的 skill | 否 | 它扫描 canonical 和所有存在的 agent 目录；未挂载的 canonical 也会列出（`agents: []`）。 |

### 4.6 同名副本为什么让“目录数”更大但 list 仍少

本机实际扫描到的有效目录条目约 161 个（canonical 41 + Claude 41 + Grok 30 + Gemini 10 + OpenCode 10 + Copilot 10 + Antigravity 19；Codex/Pi 的全局目录就是 canonical，另有旧目录不被 CLI 扫描）。去重后只有 50 个名字，因为：

- Claude 的 10 个真实副本与 canonical 同名（hyperframes 族 + faceless + media-use）；
- Claude/Grok 的 symlink 指向 canonical；Gemini/OpenCode/Copilot/Antigravity 的 symlink 指向 Claude 副本，但名字相同；
- 9 个 gitnexus 是唯一新增名字。

所以不能拿 `find ~/.agents/skills -maxdepth 1` 的目录数（41）去解释 49；49 还包含 9 个 agent-only 名字。

## 5. 状态判定表

### 5.1 正常

| 状态 | 判定规则 | 本机证据 | GUI 处理 |
|---|---|---|---|
| 受管·symlink 挂载 | lock 有；canonical 有有效 `SKILL.md`；agent 路径是 symlink，realpath 或 dev+ino == canonical | Claude 31 条、Grok 30 条 | 正常；可更新/卸载；展示来源与 agent |
| 受管·shared | `globalSkillsDir === canonical` | Codex、Pi（canonical 41/41） | 正常；不需要 per-agent 链接 |
| 受管·copy 同步 | canonical 有；agent 路径是真实目录；内容指纹 == canonical | Claude 10 条（hyperframes 族 + faceless + media-use） | 正常但标记“copy/drift-prone”；更新前备份 |
| 受管·未挂载 | lock + canonical，但所有 agent 路径都不存在 | 0（每个 canonical 至少有 Claude 的 link/copy） | 正常；显示“未挂载”，允许挂载 |
| 受管·旧链但可达 | symlink 指向另一个 agent 的副本，最终内容 == canonical | Codex/Gemini/OpenCode/Copilot/Antigravity 各 10 条指向 Claude 副本 | 可用；标记 legacy/detached，漂移风险 |

### 5.2 异常

| 状态 | 判定规则 | 本机证据 | GUI 处理 |
|---|---|---|---|
| 失效 lock（lock-only） | lock 有；canonical 与所有 agent 目录都没有该名字 | 21 条（18×64、2×40、1 空） | 主列表隐藏或标“stale”；提供“遗忘 lock 条目”（备份后只删 key），不要在 update-all 中带上 |
| 孤儿 canonical | canonical 有有效 `SKILL.md`；lock 无 | 0 | 标 unmanaged；source=null；不可 update；可删除或“收养”（需用户指定来源） |
| 外来 agent-only | agent 目录有有效 `SKILL.md`；canonical 和 lock 都没有 | 9 个 gitnexus（Antigravity） | 标 foreign/unmanaged；只在该 agent 下展示；不要自动 update |
| 断链 | symlink 的 target 不存在 / ELOOP | 0 | 标 broken；允许删除链接或按 canonical 重建 |
| 复制漂移 | 同名多副本内容指纹不同 | 0（10 个副本当前完全一致） | 标 drift；展示各副本路径/hash；让用户选择权威版本；备份后覆盖 |
| 来源冲突/同名覆盖 | 同名但 `lock.source` 与新来源不同；或两个 agent 副本来自不同来源 | 0 | 安装前拦截：显示“将用 B 覆盖 A”，要求确认并备份 |
| 无效 skill 目录 | 目录存在但 `SKILL.md` 缺失，或缺 `name`/`description`，或 YAML 错 | 0 | 不计入 skill；标 invalid/quarantine；不要删除用户文件 |
| 噪声 | `.DS_Store`、`.system`、`node_modules`、`.git` 等 | 4 个 `.DS_Store`（`~/.agents`、`~/.agents/skills`、`~/.claude/skills`、`~/.codex/skills`）+ Codex `.system` | 过滤；不要计数、不要当 skill |
| lock 完整性 | JSON 无效、version != 3、缺必填字段、raw 重复 key | 0（v3、62 条、字段齐全、无重复） | 进入只读/修复模式；先备份；禁止直接调用 CLI 写操作 |
| 旧链/分离链 | agent symlink 指向另一个 agent 的副本而非 canonical | 50 条（5 agents × 10 个副本） | 标 legacy；可保留；更新/修复时收敛到 canonical |

### 5.3 不可判定

| 状态 | 判定规则 | 本机证据 | GUI 处理 |
|---|---|---|---|
| CLI 过度上报 agent | `list --json.agents` 有，但该 agent 的 `globalSkillsDir` 无路径，且该目录 != canonical | Gemini/Copilot/OpenCode/Antigravity：41 报全，物理各 10；每 agent 31 个不实 | 忽略 `list.agents`；用物理扫描；把该差异标为“CLI 报告/未验证” |
| 本地改动 vs 安装器排除 | 重算 hash 与 lock 不同，但源目录含 `metadata.json`/`__pycache__`/`__pypackages__`/越界 symlink | `vercel-react-best-practices` 1 例（源含 `metadata.json`） | 先按安装器排除规则构造期望指纹，再判断；或使用 GUI 自己的基线 |
| 空 hash | `skillFolderHash=""` 且不是 well-known | find-skills | 标“无法校验上游/本地”；不要当作已修改 |
| 复制权威 | 无 canonical，且多个 agent 副本 hash 不同 | 0 | 没有权威副本；展示所有版本，比较上游后让用户选 |
| 手工 skill 来源 | 无 lock、无 source，frontmatter 也不含来源 | 9 个 gitnexus | 标“来源未知”；只允许删除/保留，不提供 update |
| agent 是否还读 canonical | 文档说全局路径是 agent 专属目录，但 CLI 实现按 shared 处理 | Gemini/Copilot/OpenCode/Antigravity | 以官方 `docs/agents.md` 的全局路径为准做物理展示；如要依赖 canonical 兼容路径，需逐个实测 agent 运行时 |

## 6. 本地改动检测与 copy 漂移

### 6.1 `skillFolderHash` 的三种形态

| 形态 | 产生条件 | 算法 | 能否直接本地重算 | 备注 |
|---|---|---|---|---|
| 40 位 hex | GitHub 源且 tree API/`git rev-parse` 可用 | 源文件夹的 Git tree SHA（`src/blob.ts:281-301`、`src/git.ts:390-418`） | 能，见 6.2 | 包含文件名、内容、mode、子树；是**源目录**指纹 |
| 64 位 hex | 非 GitHub、local、或 tree 获取失败时回退 | `computeSkillFolderHash()`：递归文件，跳过 `.git`/`node_modules`，按相对路径 `localeCompare` 排序，`sha256(path + content)`（`src/local-lock.ts:192-231`） | 能 | 不含 mode；symlink 被忽略；包含 `metadata.json`（如果源里有） |
| 空串 | well-known（用 digest）；老条目/获取失败 | 无 | 不能 | `update` 按“Private or deleted repo” skip（`src/update.ts:270-271`） |

本机当前 62 条：43 条 40 位、18 条 64 位、1 条空串。18 条 64 位全部是失效的旧 mattpocock 条目；41 个在盘 canonical 条目全部是 40 位。

### 6.2 40 位 hash 的本地重算（实测）

不修改 skill 目录，用临时 Git index：

```bash
tmp=$(mktemp -d)
git init -q "$tmp"
GIT_DIR="$tmp/.git" GIT_WORK_TREE="$HOME/.agents/skills/<name>" git add -A -f .
GIT_DIR="$tmp/.git" git write-tree
rm -rf "$tmp"
```

结果：41 个 canonical skill 中 **40 个与 lock 完全一致**。这说明 40 位 hash 确实可以从已安装目录重算，可用于本地改动检测。

注意：

- 必须 `-f`，否则 skill 内的 `.gitignore` 会让 `git add` 漏文件；GitHub tree 不按 `.gitignore` 排除。
- mode 参与 tree hash；安装器会 `chmod` 保留源文件 mode（`src/installer.ts:595-596`），所以通常一致。若用户 `chmod` 过文件，会误报为改动。
- 空目录不参与 Git tree；安装器也不会复制空目录，通常无影响。
- 安装器会排除 `metadata.json`、`.git`、`__pycache__`、`__pypackages__`，并跳过越界 symlink（`src/installer.ts:475-482`、`526-614`）。**源目录含这些内容时，重算必然与 lock 不一致，但这不是用户改动。**

### 6.3 唯一例外：`vercel-react-best-practices`

- lock hash：`343db721...`（上游 `HEAD:skills/react-best-practices` 的 Git tree）。
- 本地重算：`51ed8872...`，不一致。
- 原因：上游该文件夹含 `metadata.json`，安装器按 `EXCLUDE_FILES` 跳过；本地目录没有它。
- 把上游 `metadata.json` 临时复制进副本后重算，得到 `343db721...`，与 lock 完全一致。
- 结论：这是**安装器排除规则造成的假阳性**，不是本地修改。

因此，GUI 若用 hash 判本地改动，必须先判断源目录是否含安装器排除项；更稳妥的是在首次扫描时记录自己的基线。

### 6.4 64 位 hash 的本地重算

按 `computeSkillFolderHash` 的规则实现即可（Rust 侧同样）：只统计普通文件，跳过 `.git`/`node_modules`，相对路径统一 `/`，按 `localeCompare` 等价排序，依次 update path bytes 和 content bytes，最后 SHA-256。

同样有排除项问题：`computeSkillFolderHash` 不排除 `metadata.json`，而安装器会排除；如果 64 位 hash 是从源目录算的，已安装副本可能永远对不上。对本机 18 条 64 位条目来说没有目录，无法验证；规则上要按“期望安装指纹”而不是“源指纹”比较。

### 6.5 推荐的本改动检测模型

不要只依赖 lock hash。建议 GUI 维护自己的只读基线（存 app 数据，不写 skill 目录）：

```
CopyFingerprint {
  path, realpath, dev, ino,
  treeHash,      // 40 位算法，便于和 lock 对比
  contentHash,   // 64 位算法，便于和 64 位 lock 对比
  fileCount, totalBytes, maxMtime
}
```

首次扫描记录；之后每次扫描重算并比较。这样可以：

- 检测任意本地编辑/新增/删除（即使 lock 是空的、64 位的、或源含排除项）；
- 区分“本地改动”和“上游更新”：local hash vs 基线 = 本地；upstream latest vs lock = 上游；
- 检测 copy 漂移：同名多副本的指纹是否一致；
- 不受 `updatedAt`、目录 mtime、`.DS_Store` 噪声影响（可在指纹中显式忽略或单独标记）。

### 6.6 区分本地改动 / 上游更新

| local vs lock | upstream vs lock | 结论 |
|---|---|---|
| 相同 | 相同 | 最新，未改 |
| 相同 | 不同 | 上游有更新，本地未改；可安全 update |
| 不同 | 相同 | 本地被改；update 会覆盖，先备份/确认 |
| 不同 | 不同 | 两边都变；必须 diff/备份，不能自动合并 |
| 无法计算 | 任意 | 标 uncheckable；不要猜 |

### 6.7 copy 模式漂移检测

1. 以 skill 名为 key，收集 canonical + 每个 agent 的物理路径。
2. 解析 symlink：realpath + dev/ino；同一 inode 只算一个副本。
3. 对每个不同的 realpath 算 treeHash/contentHash。
4. 若只有一个 hash：同步；再和上游/lock 比，判断是否落后。
5. 若有多个 hash：
   - canonical 存在且匹配其中一个 → 其他标 drift；
   - canonical 不存在 → 没有权威副本；全部展示，和上游 fresh clone 比较，让用户选择；
   - symlink 指向另一个 agent 的副本 → 漂移归因到终端 target，所有 link 共享该状态。
6. 本机 10 个 copy 副本（Claude）与 canonical 的 SHA-256 全部相同，当前无漂移；但它们是独立 inode，canonical 更新不会自动同步，属于结构性风险。

### 6.8 CLI 的 update 不会保护本地改动

- 全局 update 只做 `latestHash !== entry.skillFolderHash` 比较（`src/update.ts:731-747`），从不读取已安装目录的 hash。
- 更新成功后直接 `addSkillToLock(..., skillFolderHash)`（`src/update.ts:810-821`），不检查本地是否被编辑。
- 目标 agent 由 `nonInteractiveTargetAgents()` 决定：若检测到宿主 agent，则为宿主 + 所有 universal agents；否则为全部检测到的 agent + universal；若一个都没检测到，则为所有 agent（`src/add.ts:384-392`、`src/update.ts:72-89`）。**没有 `--agent` 过滤**；文档也只列 `-g/-p/-y`（`docs/commands.md:194-199`）。
- 模式由 `defaultInstallMode(targets)` 重新决定。若原来是最初的 copy 安装、现在检测到多个目录，会选 symlink：canonical 被重建，agent 的真实副本被 `rm` 后替换成 symlink（`src/installer.ts:259-293`）。这会删除独立副本（包括用户在副本上的修改）。
- 对本机 21 条 stale lock：7 条上游仍存在且 hash 已变，`skills update -g` 会重新安装它们；13 条上游已删，交互式会提示清理，非交互式跳过；find-skills 空 hash 跳过。**update-all 不是安全的“只更新当前清单”操作。**

### 6.9 `remove` 的边界

- `remove --all` 会移除 `installedSkills`（所有扫描到的磁盘目录）∪ `lockSkillsKeys`（所有 lock key）（`src/remove.ts:150-160`）。它会删掉 9 个未受管 gitnexus 和 21 条 stale lock，不适合作为 GUI 的“清理失效项”。
- `remove <source>` 会删除该 source 下所有 skill（`resolveSkillsToRemove` 的 source 匹配，`src/remove.ts:45-75`）。
- 安全做法：逐名删除；对 stale 条目用精确名字；对未受管条目单独确认；删除前备份 canonical 和 agent 副本。

## 7. 多来源、多 agent、plugin、subagent 边界

### 7.1 同名不同来源

- lock 是 `Record<name, entry>`，同名第二次安装会覆盖第一条 entry，并 `cleanAndCreateDirectory(canonicalDir)` 覆盖内容。**无法同时保留两个来源的同名 skill。**
- 已有的 agent symlink 都指向同一个 canonical 路径，所以覆盖后所有 agent 同时看到新内容；copy 模式的旧副本则保持旧内容（漂移）。
- `add` 只会按 agent 提示 “overwrites”，不会提示“来源 A 被来源 B 替换”（`src/add.ts:2270-2330`）。
- GUI 规则：安装前读 `lock[name].source`；若不同，必须显式确认“替换来源”，备份 canonical + 所有物理副本；安装后刷新所有 agent 指纹。

### 7.2 同一来源多个 skill / plugin

- 每个 skill 一条 lock entry；`source`/`sourceUrl` 相同，`skillPath` 不同。
- `pluginName` 来自源仓库 manifest，仅用于展示分组；JSON list 不输出 `pluginName`（`src/list.ts:125-136`）。
- 不是所有同源 skill 都有 `pluginName`：本机 49 条 mattpocock 中 35 条有、14 条无（deprecated/personal/misc 路径）。所以**按 source 分组，不要按 pluginName 分组做完整性判断**。
- plugin 不是独立安装/更新单元；update 仍是 per-skill，但按 source+ref 批量 fetch。plugin 上游删 skill 时，lock 可能留下 stale 条目（除非走 remove 流程）。

### 7.3 subagent

- 只有 Eve 支持 subagent；路径是项目内 `agent/subagents/<name>/skills`，`sourceType` 仍可能是 github。
- 全局 lock 没有 `subagents` 字段；项目 lock 才有（`src/local-lock.ts:45-52`）。
- 全局 GUI 可以忽略 subagent；若扫描项目目录，需单独建模。

### 7.4 多 agent 的“同一 skill 多形态”

同一 skill 可以同时存在：canonical 真实目录、Claude 的 symlink、Claude 的 copy、Gemini 的 symlink、Codex 的旧 symlink、Antigravity 的 foreign copy。它们可能内容一致，也可能漂移。GUI 的数据模型应以 **skill 名 + 物理副本列表** 为中心，而不是单一 `path`。

## 8. 给 GUI 的判定/操作清单

### 8.1 清单构建

1. 读 lock（按 `XDG_STATE_HOME`），校验 `version===3`、`skills` 对象、每条必填字段；失败则只读模式。
2. 枚举 canonical：目录 + 有效 `SKILL.md` → `canonicalSkill`。
3. 枚举每个 agent 的 `globalSkillsDir`（按环境变量）：目录/symlink + 有效 `SKILL.md` → 物理副本。
4. 对每个物理副本解析 realpath/dev+ino，按名字归并；计算指纹。
5. 生成状态：
   - `managed`：lock 有且 canonical 有效；
   - `stale-lock`：lock 有且所有位置无；
   - `orphan-canonical`：canonical 有且 lock 无；
   - `foreign`：只有 agent 目录有且 lock 无；
   - `broken`：symlink target 不存在；
   - `copy-drift`：同名多副本指纹不同；
   - `invalid`：缺/坏 `SKILL.md`；
   - `noise`：`.DS_Store`/`.system` 等。
6. agent 附着状态按 3.4 生成；不要用 `list --json.agents`。

### 8.2 安全操作

| 操作 | 建议 |
|---|---|
| 安装/更新/删除 | 优先调用 CLI，但一次只允许一个进程；调用前备份 lock + canonical + 受影响 agent 目录；调用后重读并 diff。 |
| 写 lock | 不要直接覆盖。若必须写：先校验 v3 + 备份 + 临时文件 + `rename` 原子替换；保留 `installedAt`，只更新目标 entry；拒绝 version !=3 和非法 JSON。 |
| update-all | 不要直接用 `skills update -g`。GUI 自己枚举“当前受管且用户选中的 skill 名”，逐个 `skills update <name> -g` 可避开 stale 条目；但 CLI 仍会改变 agent 集合/模式，若要保留挂载关系，需自己实现更新或更新后重建。 |
| remove-all | 禁止。逐名删除；排除 foreign/stale，除非用户明确选择。 |
| 清理 stale | 逐名 `skills remove <name> -g -y`（会删 lock key；目录已不存在），或 GUI 备份后只删 entry；不要用 `--all`。 |
| 修复断链 | 若 canonical 有效，重建 symlink；否则标 broken，不要删除目标。 |
| copy 漂移 | 展示每个副本的路径/hash/mtime；默认不自动覆盖；提供“以 canonical 为准同步”“保留当前”“用上游覆盖”三种选择，并备份。 |

### 8.3 不可判定时的 UI

- 明确区分“已确认”“推测”“无法判定”，不要把 `list.agents` 的过度上报显示成已安装。
- 对空 hash、或含安装器排除项且没有基线的条目，显示“无法校验”而不是“已修改”；64 位 hash 仍可用内容指纹校验。
- 对无 lock 的 foreign skill，显示“来源未知”，不提供 update/回滚。
- 对 copy 副本，显示“独立副本”，不要显示成 symlink。

## 9. 未决问题

1. Gemini CLI / GitHub Copilot / OpenCode / Antigravity 在运行时是否还会读取 `~/.agents/skills` 作为兼容路径？官方 `docs/agents.md` 只给各自全局目录，没有声明会读 canonical；CLI 实现却按 shared 处理。本机新安装只给 Claude/Grok 建链，说明 CLI 不会写这四个目录；但 agent 运行时行为未实测。
2. 20 个 mattpocock 目录被删除的具体执行者（用户手工 / 旧版工具 / 其他管理器）无法从现存文件证明；只能确定当前 CLI 的 remove 会同时清 lock。
3. `skills update -g` 对 7 个 stale 条目的“重新安装”是根据源码 + 当前上游 hash 差异推断的，未实际执行（只读约束）。建议在隔离的临时 HOME 中验证。
4. copy 模式下“哪个副本是权威”没有 lock 字段可依据；本机当前全部一致，规则按 6.7 处理。
5. version > 3 的 lock 行为未定义；GUI 应拒绝而不是按 v3 解释。

## 10. 主要来源

- CLI 源码（v1.7.4，commit `d8eca76d5dbe47df2d146d036e32534b74ece57a`）：
  - `src/skill-lock.ts`：lock 路径、schema、读写、版本处理
  - `src/installer.ts`：canonical/agent 路径、symlink/copy、list 扫描与去重、排除项
  - `src/list.ts`：JSON 输出、source 查找
  - `src/add.ts`：安装模式、hash 写入、目标 agent、覆盖提示
  - `src/update.ts`：更新检查、stale 跳过、删除提示、placement
  - `src/remove.ts`：remove 扫描、canonical 保留、lock 清理
  - `src/agents.ts`：agent 全局目录映射、`isUniversalAgent`
  - `src/local-lock.ts`：`computeSkillFolderHash` 算法（64 位）
  - `src/blob.ts` / `src/git.ts`：40 位 Git tree hash
  - `src/skills.ts` / `src/plugin-manifest.ts`：frontmatter 校验、pluginName
- 官方文档：`docs/agents.md:8-60`（全局路径表）、`docs/commands.md:122-123,174-199`（安装方式、update 选项）。
- 本机只读盘点：`~/.agents/.skill-lock.json`、`~/.agents/skills`、`~/.claude/skills`、`~/.codex/skills`、`~/.gemini/skills`、`~/.config/opencode/skills`、`~/.copilot/skills`、`~/.gemini/antigravity/skills`、`~/.grok/skills`、`~/.pi/agent/skills`。
- 本机 CLI：`~/.npm/_npx/0757c8f0298e564a/node_modules/skills`（1.7.4）；`skills list -g --json` 实测 50 条（票据时点 49）。
- 本地 hash 复算：临时 Git index / SHA-256，41 个 canonical 中 40 个与 lock 匹配，1 个因 `metadata.json` 排除而不同。
