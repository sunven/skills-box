# 更新检测机制与成本

- 票据：`.scratch/global-skill-manager/issues/03-update-detection.md`
- 研究对象：`skills` CLI `v1.7.4`（tag `a80e4c9d12d607631249d4d7d059f835d4b2f13a`，2026-10-19；本机 `npx` 缓存包版本 `1.7.4`）
- 采样时间：2026-10-28，macOS，Node `v24.15.0`
- 方法：CLI 源码逐段阅读、GitHub REST API 官方文档、对本机 5 个公开仓库各做一次只读 Trees API 请求、本机 lock 与目录只读盘点
- 约束：**没有执行 `skills update` 或 `skills check`**（原因见 1.1：本版本的 `check` 会实际更新），没有修改任何用户 skill 文件或 lock 文件

## 结论先行（TL;DR）

1. **`skills check` 在 v1.7.4 不是只读检查**：它与 `update`/`upgrade` 在 CLI 路由层走同一个 `runUpdate`，会直接重新安装有更新的 skill。文档写的 “without reinstalling” 与实现不一致。GUI 绝不能把 `skills check` 当状态查询接口。
2. **`check`/`update` 没有 `--json`、没有 dry-run、没有“仅检查”模式**；`--json`、`--dry-run` 会被静默忽略。退出码也不表达“有更新”：只有更新执行失败（重装失败、CLI 入口缺失等）才返回 `1`，网络/仓库检查失败仍可能返回 `0` 并打印 “All global skills are up to date”。
3. **更新状态可以由 GUI 自己低成本计算**：按 `(source, ref)` 分组，每个来源只请求一次 GitHub 递归 Trees API，然后在内存中比较 `skillFolderHash` 与目标文件夹的 tree SHA。50–100 个 skill 通常只需要 5–20 个请求，而不是 50–100 个。
4. **`skillFolderHash` 是文件夹 tree SHA，不是 commit SHA**；lock 里没有记录 source 的 commit SHA，也没有记录本地内容哈希。因此不能只用 `git ls-remote` 判断更新，也不能只靠 lock 判断“本地是否被改过”。
5. **`skills update` 不保留 copy 模式、不按原 agent 列表更新、不检测本地改动、无回滚**。更新执行应由「生命周期安全」票据（07）决定是否改为逐 skill 调 `skills add`；检测层应保持纯只读。
6. **推荐策略**：v1 默认“手动刷新 + 缓存即时展示”；可选“启动时一次性后台检查”，仅在缓存超过 6 小时时触发，不做定时轮询。单 skill 检查在网络上等价于刷新该来源，必须合并/去重。

## 1. `skills check` 的真实行为

### 1.1 它就是 `update`

`src/cli.ts` 的命令分发：

```ts
case 'check':
case 'update':
case 'upgrade':
  await runUpdate(restArgs);
  break;
```

来源：[`src/cli.ts` L420-L423](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/cli.ts#L420-L423)；已安装的 `dist/cli.mjs` 同样是这个分支。

`runUpdate` 没有任何 `checkOnly`/`dryRun` 分支：它先发现更新，然后对每个有更新的 skill 调用 `skills add ... -g -y` 重新安装。见 [`src/update.ts` L496-L748](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/update.ts#L496-L748)。

CLI 文档却写着：

> `skills check` — Check installed skills for available updates without reinstalling them.

来源：[`docs/commands.md` L166-L171](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/docs/commands.md#L166-L171)。README 也同样列出 `npx skills check`。**以实现为准：这是文档与实现的偏差，不应依赖。**

### 1.2 参数、JSON 与退出码

`parseUpdateOptions` 只识别 `-g/--global`、`-p/--project`、`-y/--yes` 和位置参数；其他以 `-` 开头的参数被忽略：

来源：[`src/update.ts` L66-L84](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/update.ts#L66-L84)。

因此：

- `skills check --json`：`--json` 被忽略，命令仍会更新。
- `skills check --dry-run`：同样被忽略，仍会更新。
- 没有机器可读输出；输出是带 ANSI 颜色的人类文本。
- 没有“仅检查”的退出码。`runUpdate` 只在 `totalFail > 0` 时设置 `process.exitCode = 1`；发现更新但全部更新成功、没有更新、被跳过、网络检查失败，都可能是 `0`。见 [`src/update.ts` L1068-L1075](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/update.ts#L1068-L1075)。

### 1.3 最危险的失败路径：检查失败被当成“已是最新”

`updateGlobalSkills` 在单个来源检查失败时只打印一行错误，不增加 `failCount`：

```ts
} catch (error) {
  console.log(`  ${DIM}✗ Failed to check skills from ${source}${RESET}`);
}
```

来源：[`src/update.ts` L658-L663](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/update.ts#L658-L663)。

循环结束后，如果 `updates.length === 0`，它会打印：

```
✓ All global skills are up to date
```

来源：[`src/update.ts` L690-L695](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/update.ts#L690-L695)。

于是“离线/限流/仓库删除”可能得到退出码 `0` + “全部最新”的错误结论。**GUI 不能解析这段文本作为状态。**

### 1.4 范围选择也需要注意

如果没有显式 `-g` 或 `-p`，并且 stdin 不是 TTY，`resolveUpdateScope` 会根据当前目录是否存在项目级 skill 选择 `project` 或 `global`：

来源：[`src/update.ts` L123-L142](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/update.ts#L123-L142)。

GUI 即使将来要调用（不推荐用于检查），也必须显式传 `-g`，否则可能更新当前工作目录的项目 skill。

## 2. `skills update` 的实现与副作用

### 2.1 检测阶段

全局更新的主流程（[`src/update.ts` L496-L748](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/update.ts#L496-L748)）：

1. 读取 `~/.agents/.skill-lock.json`（v3）。
2. 跳过不可检测项：
   - `sourceType === 'local'`：本地路径；
   - `sourceType === 'git'`：通用 Git URL；
   - `sourceType === 'well-known'`：走另一套 digest 逻辑；
   - `!skillFolderHash` 或 `!skillPath`：跳过，理由显示为 “Private or deleted repo” / “No skill path recorded” / “No version tracking”。
3. 对可检测项按 **`source + ref`** 分组（不是只按 source），每组取第一个 entry 的 `ref`。
4. 对 GitHub 来源调用一次 `fetchRepoTree(source, ref, getGitHubToken)`，拿到整个仓库的递归 tree。
5. 如果该组所有 locked `skillPath` 都还能在 tree 中找到，则逐个 skill 取文件夹 tree SHA 与 lock 比较；不同则加入更新列表。
6. 如果任意 locked `skillPath` 找不到，或 API 不可用，则 **回退到完整 `git clone`**（`cloneRepo`，默认 shallow `--depth 1`，超时 5 分钟），再发现/重定位/删除判断，并对每个 skill 计算 hash。
7. 对每个更新项串行 `spawnSync` 调 `skills add <source>/<folder>#<ref> --skill <name> -g -y`。

关键源码：

- 分组键 `source + '\n' + ref`：[`src/update.ts` L556-L566](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/update.ts#L556-L566)
- Trees API 快速路径与路径缺失回退：[`src/update.ts` L576-L662](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/update.ts#L576-L662)
- 逐 skill 更新：[`src/update.ts` L700-L744](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/update.ts#L700-L744)

**成本含义**：正常情况下每个来源 1 个 API 请求；一旦某个 skill 路径在 tree 中消失（改名/删除/仓库重构），整个来源会退化为一次完整 clone。对于大仓库，这可能是数十 MB 和数秒到数分钟。GUI 自己的检查器不应默认走这条回退。

### 2.2 应用阶段

每个更新项执行的是 `add`，不是原地 patch：

- `add` 会重新下载该 skill 的内容（skills.sh blob 快速路径，失败则 clone）。
- `installer.ts` 的 `cleanAndCreateDirectory()` 先 `rm -rf` 目标目录再 `mkdir`，然后写入新内容；安装失败没有回滚。见 [`src/installer.ts` L198-L205](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/installer.ts#L198-L205)。
- 多 skill 更新是串行的；部分成功会保留，失败项返回非零退出码，但已成功的不会回滚。
- 更新成功后 `addSkillToLock` 保留原 `installedAt`，刷新 `updatedAt`。

### 2.3 copy 模式与 agent 挂载

全局 lock 的 schema **没有** install mode，也没有原 agent 列表：

来源：[`src/skill-lock.ts` L20-L36](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/skill-lock.ts#L20-L36)。

而 update 调 `add` 时只传 `-g -y`，没有 `--copy`，也没有 `--agent`：

来源：[`src/update.ts` L720-L735](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/update.ts#L720-L735)。

`add` 在 `-y` 下会重新探测本机已安装 agent；安装模式在多个不同目标目录时默认 `symlink`，只有单一目标目录才默认 `copy`：

来源：[`src/add.ts` L760-L810](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/add.ts#L760-L810)、[`src/add.ts` L859-L886](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/add.ts#L859-L886)。

`symlink` 模式在目标位置已有真实目录时会 `rm -rf` 后重建符号链接：

来源：[`src/installer.ts` L255-L295](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/installer.ts#L255-L295)。

因此一般情况：

- **copy 模式不会被保留**：更新后可能变成“`~/.agents/skills` 规范副本 + 各 agent 符号链接”。
- **agent 集合会按“当前检测到的 agent”重算**：可能新增挂载；不再被检测到的 agent 的旧副本不会被清理，可能留下旧版本。
- 如果原先是 copy-only（没有规范副本），更新会创建规范副本并重新挂载，改变磁盘形态。

### 2.4 删除上游已消失的 skill

如果 clone 回退后发现某个 locked skill 在来源中不存在，`resolveSkillLocations` 会把它归入 `deletedSkills`，然后 `promptDeletions` 询问是否删除本地副本：

来源：[`src/update.ts` L262-L325](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/update.ts#L262-L325)、[`src/skill-relocation.ts` L35-L85](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/skill-relocation.ts#L35-L85)。

- 交互式 TTY 下会询问，确认后调用 `removeCommand` 真正删除。
- `-y` 或非 TTY 下会跳过删除，只打印警告。

这也是 `skills check` 不能作为只读命令的原因之一：它可能引导用户删除本地 skill。

## 3. `skillFolderHash` 的比较语义

### 3.1 两种格式

`~/.agents/.skill-lock.json` v3 的 `skillFolderHash` 可能是：

| 形式 | 含义 | 何时产生 | GUI 如何比较 |
|---|---|---|---|
| 40 位 hex | 该 skill 文件夹的 **Git tree object SHA-1**（GitHub Trees API 的 `tree[].sha`） | GitHub API 可用时，`add` 从 repo tree 中取 | 请求 repo tree，找 `type=tree` 且 `path=<skill folder>` 的 `sha`，直接与 lock 比较 |
| 64 位 hex | 该 skill 文件夹内容的 **SHA-256**，按相对路径排序后拼接路径和内容计算 | GitHub API 不可用、走 clone 安装时；或旧版本/旧记录 | 需要拿到远端完整内容后重算，不能与 GitHub tree SHA 混比 |
| 空字符串 | 没有版本跟踪 | 旧 lock / 安装时未取到 | 跳过，标记 unknown |

40 位来源：[`src/blob.ts` L281-L301](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/blob.ts#L281-L301)；64 位算法：[`src/local-lock.ts` L110-L150](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/local-lock.ts#L110-L150)。64 位算法用 JavaScript 的 `localeCompare` 排序相对路径；如果 GUI 用 Rust 的字节序重实现，非 ASCII/大小写文件名可能得到不同结果。除非完全复刻 Node 语义，否则不建议 GUI 自行重算 64 位 hash，宁可标 `legacy-unknown` 或复用 CLI 结果。

### 3.2 它不是 commit SHA

lock 里没有保存 source 的 commit SHA。`skillFolderHash` 只对 skill 文件夹做 tree hash：

- 40 位 Git tree SHA 对文件夹内任意文件内容、文件名、层级、文件模式（含可执行位）变化敏感；64 位 SHA-256 只覆盖相对路径和文件字节，不包含文件模式；
- 仓库其他目录变化、commit message、作者、时间不会变；
- 仓库根目录的 SKILL.md（root-level skill）会退化为整个仓库 root tree SHA，任何仓库文件变化都会被误判为“有更新”；
- skill 文件夹之外的共享脚本/父目录文档变化不会触发更新。

因此 `git ls-remote` 只能告诉你“来源 HEAD 变了”，不能告诉你“具体哪个 skill 变了”。要做精确比较，必须拿到 tree。

### 3.3 本地改动无法由 lock 单独判断

`update.ts` 只比较 `entry.skillFolderHash` 与远端 hash；它不读取本地规范目录，也不计算本地内容哈希。更新应用时 `add` 直接覆盖，所以：

- 远端没变、本地被改：`skills check` 会认为“已是最新”，本地改动保留；
- 远端变了、本地也被改：`skills update` 会直接覆盖本地改动，没有警告；
- lock 里的 40 位 hash 是远端 tree SHA，不是“本地当前内容”的 hash。

GUI 若要在更新前提示“本地已修改”，必须自己建立本地基线或计算本地指纹；不能只读 lock。

> 本机验证：对 41 个本地 skill 计算 Git tree SHA，40 个与 lock 的 40 位 hash 完全一致；唯一不一致的 `vercel-react-best-practices` 是因为远端文件夹包含 `metadata.json`，而安装器会排除它（本地 75 个文件 vs 远端 76 个 blob）。说明“本地 Git tree SHA == lock hash”对大多数 skill 成立，但会被安装器的排除规则破坏。排除规则见 [`src/installer.ts` L475-L482](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/installer.ts#L475-L482)。

## 4. GUI 能否不调用 `check` 自行计算状态

可以。推荐把更新检测做成 **Tauri 后端的纯只读检查器**，不经过 `skills check/update`。

### 4.1 输入

1. `~/.agents/.skill-lock.json`（v3）：`source`、`sourceType`、`sourceUrl`、`ref`、`skillPath`、`skillFolderHash`、`updatedAt`。
2. 本地规范目录是否存在：`~/.agents/skills/<sanitizeName(name)>`（copy-only skill 可能没有规范目录，需要从 `skills list -g --json` 的 `path` 获取实际位置）。
3. GitHub Trees API：`GET https://api.github.com/repos/{owner}/{repo}/git/trees/{ref or HEAD}?recursive=1`。
4. 可选：`GITHUB_TOKEN` / `GH_TOKEN`，或 GUI 自己保存的 token（v1 公开仓库可不要求）。

`skills list -g --json` 只返回 `name/path/scope/agents/source/sourceUrl/sourceType`，不含 `ref`、`skillPath`、`skillFolderHash` 或更新状态（[`src/list.ts` L113-L127](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/list.ts#L113-L127)）。它适合把 lock key 映射到真实本地路径，但不能替代读取 lock；而且它只列出实际存在的目录，ghost lock entry 不会出现。

### 4.2 算法

```text
entries = lock.skills
  .filter(sourceType == "github")
  .filter(skillFolderHash != "" && skillPath != "")
  .filter(local path exists)          // 无本地目录的 ghost 不显示更新徽章

groups = groupBy(entries, key = source + "\n" + ref)

for each group:
  tree = GET /repos/{source}/git/trees/{ref or "HEAD"}?recursive=1
  if 304: use cached tree
  if 403/429/404/5xx/timeout: mark group error, keep last known status
  if tree.truncated: mark group unknown (or use partial clone/per-subtree fallback)

  for each entry in group:
    if hash is 40-hex:
      folder = dirname(skillPath)
      node = tree.tree.find(type == "tree" && path == folder)
      if !node: status = source-path-missing
      else if node.sha == hash: status = up-to-date
      else: status = update-available
    else if hash is 64-hex:
      status = legacy-unknown         // 或单独走 clone 重算，不建议默认做
    else:
      status = unknown
```

关键点：

- **按 `(source, ref)` 分组，一次请求覆盖该来源下所有 skill**；50–100 个 skill 不等于 50–100 个请求。
- **不要按 skill 调 GitHub Contents API**；那会线性放大请求数和延迟。
- 对 `path-missing` 不要直接说“有更新”。它可能是改名、删除、仓库重构或默认分支变化。先标为“来源中找不到”，需要时再按 07 票据的修复流程处理。
- `tree.truncated === true` 时不能相信“找不到 = 删除”。GitHub 文档规定递归 tree 最多 100,000 项 / 7 MB，超出会截断。来源：[GitHub Trees API](https://docs.github.com/en/rest/git/trees)。
- `ref` 不存在（分支被删/改名）会 404；如果 lock 没有 `ref`，用 `HEAD`，GitHub 会解析默认分支，默认分支变化可自动跟上。

### 4.3 状态模型（建议）

| 条件 | 状态 | UI 语义 |
|---|---|---|
| 本地目录不存在 | `orphan-lock` | 不显示更新；在异常区提示“lock 记录存在但本地目录缺失” |
| `skillFolderHash` 为空 | `unknown` | “无法检测（无版本记录）” |
| `sourceType != github` | `unknown` | v1 只支持公开 GitHub 来源 |
| `ref` 是 40 位 commit SHA | `pinned` | “已锁定到 commit，不跟随新版本” |
| 40 位 hash + 路径存在 + 远端 SHA 相同 | `up-to-date` | “已是最新” + 检查时间 |
| 40 位 hash + 路径存在 + 远端 SHA 不同 | `update-available` | “可更新” |
| 40 位 hash + 路径在 tree 中不存在 | `source-path-missing` | “来源中找不到该 skill（可能已删除/移动）” |
| 64 位 hash | `legacy-unknown` | “旧版哈希，更新一次后恢复检测”（或提供“重新检查”走 clone） |
| tree 被截断 / API 失败 / 限流 | `error` | “检查失败”，保留上次成功结果并标 stale |
| 本地指纹与基线不同 | `local-modified` | “本地已修改”；与 `update-available` 叠加为冲突 |
| 本地指纹不同 + 远端不同 | `local-modified + update-available` | 更新前必须提示覆盖风险 |

状态应分层：**远端状态**（up-to-date/update-available/path-missing/unknown/error）× **本地状态**（clean/modified/unknown）。不要用一个枚举把两者混在一起。

### 4.4 本地改动检测

lock 的 40 位 hash 是远端 tree SHA，不能直接代表本地内容。可选方案：

1. **GUI 自有基线（推荐）**：首次扫描时对每个本地 skill 计算内容指纹（排序后的相对路径 + 文件字节，排除 `.git`、`node_modules` 和安装器排除项），存入 app 数据目录；之后每次扫描重算并比较。安装/更新成功后重置基线。首次扫描前无法判断历史改动，显示“未建立基线”而不是“未修改”。
2. **Git tree SHA 对照**：对 40 位 entry，用临时 Git index 计算本地文件夹的 tree SHA，与 lock 比较。大多数情况可行（本机 40/41 命中），但安装器会排除 `metadata.json`、`__pycache__`、`__pypackages__`、`.git`，远端 tree 仍包含它们；有这些文件时会误报。只应作为启发式，不能作为唯一依据。
3. **mtime 启发式**：任一文件 mtime 晚于 lock `updatedAt` 时提示“可能被修改”。实现简单，但会被 `touch`、解压、同步工具误触发；适合作为弱提示。

copy 模式的额外问题：agent 目录里是独立副本，规范目录可能不存在。需要分别计算每个 attachment 的指纹，与基线/其他副本比较，才能判断“某个 agent 的副本是否落后”。这属于 02/08 票据的挂载状态模型，更新检测只消费其结果。

### 4.5 缓存

CLI 自身没有跨进程缓存：没有 ETag/If-None-Match，没有磁盘缓存，只有进程内的 `_rateLimitedThisSession`。源码中没有 `etag` / `if-none-match` 相关逻辑；[`src/blob.ts` L94-L101](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/blob.ts#L94-L101) 只是进程级限流标记。

GUI 应自己缓存：

- 每个 `(source, ref)`：`rootTreeSha`、`etag`、`fetchedAt`、`lastError`、`rateLimitReset`。
- 每个 skill：`status`、`latestHash`、`checkedAt`、`localFingerprint`。
- 每个来源的 HTTP 响应体（tree JSON）可选；如果不想存大 JSON，只存每个 path→sha 的映射即可。
- 304 只在带 `Authorization` 且正确认证时不计入主限额；未认证的 304 仍可能计入。来源：[GitHub 条件请求文档](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api#use-conditional-requests)。因此没有 token 时，TTL 比 ETag 更实际。
- 阶段 2 可加一层“来源 HEAD 未变则跳过 tree”：用 `git ls-remote <url> HEAD`（或目标 ref）拿 commit SHA，与缓存的 source commit 比较；未变则直接复用上次 tree/状态。这样稳定仓库的检查不消耗 GitHub API 配额，只付一次 git 网络请求。pinned commit、API-only 环境仍走 tree 请求。

### 4.6 认证、限额与离线

GitHub 官方限额：

- 未认证：**60 请求/小时/IP**。
- 已认证（PAT/OAuth/GitHub App）：**5,000 请求/小时**（部分企业应用 15,000）。
- 次级限额：并发不超过 100；REST 约 900 points/分钟；超限返回 403 或 429，可能带 `retry-after`。
- `x-ratelimit-remaining` / `x-ratelimit-reset` 是权威状态。

来源：[GitHub REST API rate limits](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api)。

CLI 的处理：

- 先匿名请求；遇到 `403 + x-ratelimit-remaining: 0` 才认为限流，然后尝试 `GITHUB_TOKEN`/`GH_TOKEN`，再尝试 `gh api`；私有仓库的 `401/404` 也会触发认证重试；都失败后回退 clone。
- 不把 `429` 当成限流；没有 `retry-after` 处理；没有持久化限流状态。

来源：[`src/blob.ts` L127-L153](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/blob.ts#L127-L153)、[`src/blob.ts` L229-L275](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/blob.ts#L229-L275)、[`docs/commands.md` L60-L62](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/docs/commands.md#L60-L62)。

GUI 应：

- 解析 `x-ratelimit-*` 和 `retry-after`；持久化 `rateLimitReset`；在 reset 前不再请求。
- 403 但 remaining > 0 可能是权限/次级限流，不要立刻重试。
- 404 对公开仓库可能是删除/改名/私有；不要把它当成“无更新”。
- 离线时保留上次成功报告，显示“上次检查于 …，当前离线/失败”；绝不显示“已是最新”。

## 5. 成本估算：50–100 个 skill

### 5.1 公式

```text
请求数/次完整检查 ≈ unique(source, ref) 的数量
                  + 失败重试/分支回退
                  + 路径缺失导致的 clone 回退
```

正常情况（所有路径存在、API 可用）下，**请求数与 skill 数无关，只与来源仓库数有关**；每个来源 1 个递归 tree 请求。

### 5.2 本机实测的 tree 响应

| 仓库 | tree 条目 | 响应大小 | truncated |
|---|---:|---:|---|
| `vercel-labs/skills` | 171 | 49 KB | false |
| `mattpocock/skills` | 291 | 86 KB | false |
| `vercel-labs/agent-browser` | 607 | 184 KB | false |
| `vercel-labs/agent-skills` | 477 | 158 KB | false |
| `heygen-com/hyperframes` | 10,883 | 3.46 MB | false |

单次请求在本机网络约 0.3–3 秒；大仓库明显更慢。GitHub 上限是 100,000 项 / 7 MB（递归模式），超出会 `truncated: true`。

### 5.3 场景表

| 场景 | 唯一来源数 | 请求/次 | 未认证 60/hr 可完整检查次数 | 6h TTL 平均请求/小时 | 评价 |
|---|---:|---:|---:|---:|---|
| 单 monorepo，60–100 skill | 1 | 1 | 60 | 0.17 | 最省；但单次 payload 可能很大 |
| 5 个来源，50–100 skill | 5 | 5 | 12 | 0.83 | 本机形态，推荐 |
| 10 个来源，50–100 skill | 10 | 10 | 6 | 1.67 | 舒适 |
| 20 个来源，50–100 skill | 20 | 20 | 3 | 3.33 | 可接受；启动检查会占 1/3 小时配额 |
| 30 个来源 | 30 | 30 | 2 | 5.0 | 建议分批/加 token |
| 50 个来源 | 50 | 50 | 1 | 8.33 | 接近上限；安装也会耗配额 |
| 100 个来源 | 100 | 100 | 0（超过 60） | — | 未认证不可一次完成；需 token 或分 2 小时 |

注意：

- 60/hr 是 **按 IP** 共享的；公司 NAT、VPN、其他工具会占用同一配额。不要把 60 当可用预算，建议按 30–40 规划。
- `skills add` / 安装也会调用 Trees API 和 raw.githubusercontent；更新检查要给安装留余量。
- 如果所有 skill 都来自少数几个大仓库，请求数很少，但单次 tree 响应可能达到数 MB；用缓存避免每次启动都拉。
- 如果 100 个 skill 分散在 100 个仓库，未认证下不可能在 1 小时内完整检查；必须使用 token（5,000/hr）或按来源分批。

## 6. 刷新策略建议

| 策略 | 请求成本 | 体验 | 建议 |
|---|---|---|---|
| 仅手动“检查更新” | 每次 N 个来源 | 用户等待 2–15 秒；状态明确 | **v1 默认**；缓存结果即时展示 |
| 启动时一次性后台检查（缓存过期才跑） | 每天最多 4 次（6h TTL） | 不阻塞 UI；打开即可看到上次结果 | **推荐可选开关**；不是轮询 |
| 定时轮询（如 15 分钟） | 每天 96 次 × N | 状态新鲜但浪费配额 | **不做**；与 map 的“无后台轮询”一致 |
| 单 skill 检查 | 仍要 1 个来源 tree | 看似轻量，实际与刷新该来源同价 | 合并到来源级；UI 可只更新该行，但缓存整组 |
| 按来源检查 | 1 个请求 | 详情页/来源分组的“检查此来源” | 推荐作为手动补充 |
| token + ETag 条件请求 | 未变化时 0 配额（认证 304） | 最省，但需要 token | v2/高级选项 |

具体建议：

1. **TTL 6 小时**（可配置 1–24h）。启动时先渲染缓存；只有 `now - lastCheckedAt > TTL` 才发一次后台检查。
2. **手动刷新始终可用**，绕过 TTL；同一个来源 60 秒内防抖。
3. **并发 3–4**，按来源串行或小并发；不要 20 个并发（GitHub 建议避免并发，次级限额 900 points/min）。
4. **一次最多检查 20–25 个来源**；超过时分批并在 UI 显示“已检查 x/y”，或提示配置 token。
5. **失败退避**：网络错误 15 分钟，限流按 `x-ratelimit-reset`，仓库 404 至少 6 小时（避免反复打不存在的仓库）。
6. **安装/更新后**把受影响来源标为 stale，不要立刻重新请求；等下一次手动/启动检查。
7. **不自动更新**：检测只更新徽章，更新动作仍由用户确认（符合 map 的安全取向）。

## 7. 边界情况

| 边界 | CLI 行为 | GUI 建议 |
|---|---|---|
| 来源仓库改名 | Trees API 若返回 301，Node `fetch` 默认跟随；CLI 不记录新地址，lock 的 `source` 仍是旧值 | 记录 `response.url`，显示“来源已重命名”；仍按旧 source 检查，必要时提供“迁移来源” |
| 来源删除 / 转私有 | 匿名 404 → token/gh → 仍失败 → clone 失败；只打印错误，可能仍显示“全部最新”且退出 0 | 标记 `error/source-unavailable`；保留上次状态；不要显示“无更新” |
| 默认分支变化 | 无 `ref` 时依次尝试 `HEAD`、`main`、`master`；`HEAD` 跟随默认分支 | 无 `ref` 正常；有 `ref` 且分支不存在 → `ref-missing` |
| tag / commit 锁定 | lock 的 `ref` 原样用于 tree/clone；commit SHA 也支持（clone 失败会 `cloneAtSha`） | 显示“已锁定”；不提示跨 ref 的新版本 |
| 私有仓库 | CLI 可用 token/gh/SSH；v1 不在产品范围 | 标记 `unknown/private`，不要请求；提示“v1 仅支持公开来源” |
| 本地改动 | update 不检查、不警告，直接覆盖 | GUI 自建基线；有本地改动时更新前确认/备份（07 票据） |
| 上游 skill 改名/移动 | API 快速路径发现路径缺失 → clone → 按 name 唯一定位；唯一匹配则迁移，多个匹配则跳过，找不到则提示删除 | 默认只标 `source-path-missing`；用户点“修复”再走 clone/重定位 |
| 上游 skill 删除 | clone 后 `deletedSkills`；交互式询问删除，`-y` 跳过 | 不自动删除；详情页提示“来源已删除”，保留本地副本 |
| copy 模式 | lock 不记录模式；update 用 `add -g -y` 重算 agent/模式，通常转 symlink | 更新前记录附件形态；执行层显式传 `--copy`/`--agent` 或接受形态变化 |
| 64 位旧 hash | update 会 clone 后按 SHA-256 比较；API 无法单独比较 | 标 `legacy-unknown`；或按需 clone 重算；建议更新一次归一化为 40 位 |
| tree 截断 | CLI 不检查 `truncated`；路径找不到会回退 clone | 检查 `truncated`，标 unknown，必要时 partial clone |
| API 限流 / 离线 | 回退 clone；失败只打印，退出码可能 0 | 缓存 + 错误态 + 退避；绝不把失败当最新 |
| 空 hash / 无 skillPath | CLI 跳过，理由可能显示“Private or deleted repo” | 标 `unknown`，提供“重新安装以恢复跟踪”指引 |
| 同一来源多个 ref | CLI 按 source+ref 分组，分别检查 | GUI 必须同样按 ref 分组，不能只按 source |

## 8. 推荐设计（可直接交给实现）

### 8.1 命令边界

新增只读 Tauri command（名称可随 09 票据调整）：

```text
check_skill_updates({ force?: bool, sources?: string[], skillNames?: string[] })
  -> UpdateReport
```

- 不写 `~/.agents/.skill-lock.json`，不写 skill 目录。
- 不调用 `skills check/update`；只读 lock + 本地指纹 + GitHub API。
- 返回按来源分组的报告，前端可以只渲染某个 skill 的状态。
- 报告带 `checkedAt`、`stale`、`errors[]`、`rateLimit`。

### 8.2 后端模块

- `lock_reader`：解析 v3 lock，校验字段，按 `(source, ref)` 分组。
- `github_tree`：`reqwest` + rustls，10s 超时，`User-Agent: skills-box/<version>`；可选 token；处理 200/304/403/404/409/429；解析 `x-ratelimit-*`、`retry-after`、`etag`。
- `hash_compare`：40 位 → tree entry SHA；64 位 → `legacy`（或独立 fallback）。
- `local_fingerprint`：SHA-256 over sorted relative paths + bytes，排除 `.git`、`node_modules`；基线存 app 数据目录。
- `update_cache`：JSON 或 SQLite；schema version；TTL；按来源存 `etag/rootTreeSha/lastError`。
- `orchestrator`：并发 3–4，按来源去重，支持取消；失败隔离，不影响其他来源。

### 8.3 前端状态与文案

- 列表行徽章：`可更新` / `已是最新` / `本地已修改` / `无法检测` / `检查失败`。
- 顶部：`上次检查：x 分钟前` + `检查更新` 按钮；stale 时显示“结果可能已过期”。
- 详情页：来源、ref、当前 hash 前 8 位、远端 hash 前 8 位、检查时间、错误原因。
- `path-missing` 文案：“来源中已找不到该 skill，可能已被删除或移动。”
- `legacy-unknown` 文案：“旧版哈希无法直接比较；更新一次后将恢复检测。”
- 限流文案：“GitHub 检查额度已用尽，预计 HH:mm 恢复。”
- 离线文案：“无法连接 GitHub；显示上次检查结果。”

### 8.4 与更新执行（07 票据）的边界

本票据只负责“检测”。但检测结论直接影响执行建议：

- 当前 `skills update` 不适合作为 GUI 的唯一执行入口：它不保留 copy 模式、不按原 agent 列表、不警告本地改动、无回滚、可能删除上游已删除的 skill。
- 更可控的执行方式是 GUI 逐 skill 调 `skills add <exact source/folder>#<ref> --skill <name> -g -a <agents> [--copy] -y --json`，在更新前记录附件形态并预览；具体由 07/09 决定。
- 无论采用哪种执行方式，**检测和更新必须是两个独立动作**；检测永远只读。

## 9. 本机验证与限制

### 9.1 本机采样

采样时 `~/.agents/.skill-lock.json` 有 62 条、5 个来源；`~/.agents/skills` 下有 41 个目录。用上述 tree 比较得到：

| 状态 | 数量 |
|---|---:|
| 已是最新 | 28 |
| 可更新 | 5 |
| 来源路径缺失（tree 中找不到） | 8 |
| lock 有记录但本地无目录（ghost） | 21 |

其中 8 个 path-missing 包括 `writing-great-skills`、`resolving-merge-conflicts`（`mattpocock/skills`）和 6 个 `heygen-com/hyperframes` 子 skill；在 tree 中按同名目录搜索不到，按 CLI 的“同名唯一定位”规则会被判为 deleted（除非是改成了不同名字的迁移）。若直接跑 `skills update`，这两个来源会回退到完整 clone，并可能提示删除本地副本。这正是 GUI 不应把 `check/update` 当状态查询的原因。

### 9.2 本地哈希验证

- 对 41 个本地 skill 计算 Git tree SHA：40 个与 lock 的 40 位 hash 一致，1 个（`vercel-react-best-practices`）不一致。
- 该 skill 的远端文件夹有 76 个 blob，本地只有 75 个文件；缺少的是安装器排除的 `metadata.json`。因此不一致不代表用户改动。
- 结论：Git tree SHA 对照可作为“本地改动”的启发式，但不能作为唯一真相；GUI 自有基线更可靠。

### 9.3 限制

- 本机采样会随外部安装/更新变化；计数只代表 2026-10-28 的瞬间。
- 没有执行真实的 `skills update`（符合约束），应用阶段结论来自源码，不是端到端实验。
- 没有测试私有仓库、GitHub Enterprise、超大仓库截断、代理/离线等场景；这些按源码和官方文档推断。
- 没有评估 GUI 侧 Rust 哈希实现的性能；41 个 skill 的指纹计算应在毫秒到百毫秒级，可在扫描时异步完成。

## 10. 来源

- CLI 源码 `v1.7.4`：<https://github.com/vercel-labs/skills/tree/v1.7.4>
- `check`/`update` 路由：[`src/cli.ts` L420-L423](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/cli.ts#L420-L423)
- 更新检测与应用：[`src/update.ts` L496-L748](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/update.ts#L496-L748)
- Trees API 封装与 hash 提取：[`src/blob.ts` L235-L301](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/blob.ts#L235-L301)
- 本地 SHA-256 算法：[`src/local-lock.ts` L110-L150](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/local-lock.ts#L110-L150)
- lock schema：[`src/skill-lock.ts` L20-L36](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/skill-lock.ts#L20-L36)
- 安装/覆盖/排除规则：[`src/installer.ts` L198-L205](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/installer.ts#L198-L205)、[`src/installer.ts` L475-L482](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/src/installer.ts#L475-L482)
- 文档中的 `check` 声明：[`docs/commands.md` L166-L171](https://github.com/vercel-labs/skills/blob/a80e4c9d12d607631249d4d7d059f835d4b2f13a/docs/commands.md#L166-L171)
- GitHub REST API rate limits：<https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api>
- GitHub Git trees API：<https://docs.github.com/en/rest/git/trees>
- GitHub 条件请求与缓存最佳实践：<https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api#use-conditional-requests>
