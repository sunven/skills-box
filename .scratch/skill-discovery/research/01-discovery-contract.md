# 自动发现：现有来源与扫描契约

日期：2026-11-02。仅记录只读调查事实；产品决策仍在 `grill-with-docs` 访谈中，本文不是已批准的实现方案。

## 结论

现有 Update check 只比较 Lock entry 中记录的 skills，不生成来源内的全部可安装项。Install preview 已能列出来源中的名称和描述，但不返回源路径，也不保存跨次发现记录。因此，单独增加一个定时器不能完成“上游新增 skill”的发现与提醒。

可复用的能力有：公开 HTTPS Source 规范化、GitHub token 认证、GitHub 和通用 Git 的 Source archive 获取、按 commit 的归档缓存，以及现有 Install 向导。扫描规则、候选身份、首次基线、提醒生命周期和来源范围仍需明确。

## 当前应用契约

| 能力 | 已有行为 | 依据 |
|---|---|---|
| Source | GitHub 保留 `owner/repo`；通用 Git 保留完整 HTTPS URL 并补 `.git`；GitHub 身份忽略大小写，通用 Git 保留路径大小写 | `src-tauri/src/tasks/source.rs` |
| Lock 来源 | 全局 lock v3 与项目 lock v1 归一为 Lock entry；可含 source、sourceUrl、ref、skillPath | `src-tauri/src/lock.rs`、`src-tauri/src/project_lock.rs` |
| Update target | 来源解析与安全 ref 校验分开；Update 保留 lock 的 ref，Install 使用默认分支 | `src-tauri/src/update_target.rs`、ADR 0009 |
| Install preview | pinned `skills@1.7.4 add <source> -l [-g]`；只读、可取消；输出解析为 name、description、group、目标 scope 的冲突 | `src-tauri/src/cli/scoped.rs`、`src-tauri/src/tasks/manager.rs`、`src-tauri/src/tasks/parser.rs` |
| Install | 以 source 和 skill 名称调用 CLI；当前不提供按源路径选择同名项或 `--full-depth` 的能力 | `src-tauri/src/cli/scoped.rs` |
| Update check 调度 | 本次启动首次评估立即检查 active Scope；之后按 Scope/revision 的六小时间隔调度，切换到近期检查过的 Scope 不重复检查 | `src/hooks/use-auto-update-check.ts` |
| Update check 预算 | 每次最多 20 个来源，并发 4，整体读取期限 30 秒；tree 缓存六小时 | `src-tauri/src/update/mod.rs`、`src-tauri/src/timeouts.rs` |
| 生命周期 | 关闭应用后无后台扫描；项目 Relocate/Forget 会使旧 Update check 结果失效 | `.scratch/auto-update-check/spec.md`、ADR 0015 |
| GitHub 全局比较 | Trees API 路径与 tree SHA 可比较已安装目录，但不含名称、描述正文 | `src-tauri/src/update/github.rs` |
| Source archive | GitHub 使用 API/归档，通用 Git 使用临时浅 checkout；包含相对路径与文件内容，可读取 `SKILL.md` 和声明文件 | `src-tauri/src/update/archive.rs`、`src-tauri/src/update/git.rs` |
| 获取去重 | 获取器在一次 Check attempt 内复用同 commit/获取方式的归档；磁盘归档缓存按 commit 与获取方式隔离 | `src-tauri/src/update/archive/acquisition.rs` |
| 凭据 | 页面保存 token 优先，其次启动环境 `GITHUB_TOKEN`，再匿名；CLI 子进程环境不转发 token | `src-tauri/src/settings.rs`、`src-tauri/src/update/api.rs`、`src-tauri/src/env/child_env.rs` |

归档解析会去掉目录项、symlink、`.git` 与 `node_modules`，并限制压缩大小 64 MiB、解压大小 256 MiB、文件数 20,000。它可以提供扫描材料，但不能未经核对就称为与 CLI clone 完全等价。

## pinned CLI 的实际扫描规则

直接检查本机 npx 缓存中的 npm 包 `skills@1.7.4`：`~/.npm/_npx/0757c8f0298e564a/node_modules/skills/dist/cli.mjs`。未运行联网命令或 Install/Update。

### Clone 扫描

`parseSkillMd`、`discoverSkills`、`findSkillDirs`（bundle 约 1250–1385 行）：

- 必须有非空、字符串类型的 frontmatter `name` 和 `description`；不合法 YAML 或缺字段会跳过。CLI 支持 BOM 和完整 YAML，应用的 inventory frontmatter reader 目前只是有限标量解析器。
- `metadata.internal: true` 默认隐藏；CLI 另有显式选项/环境开关，但应用当前不暴露它们。
- 名称和描述会移除终端转义、控制字符，将换行折叠为空格，再 trim。
- 来源根存在有效 `SKILL.md` 时，默认只返回根 skill；`--full-depth` 可改变该行为，应用当前未使用此 flag。
- 优先查找来源根的子目录、`skills`、`skills/.curated`、`skills/.experimental`、`skills/.system` 与 CLI 的 agent project skills 目录。
- 常用 skills 容器向下查找最多三层；来源根与插件声明的搜索目录默认只查一层。遇到含 `SKILL.md` 的目录后停止向下遍历该分支，即使该文件解析无效或名称重复。
- 优先扫描没有找到任何 skill 时，才递归回退到根下最多五层；`--full-depth` 也会触发回退。
- 递归跳过 `node_modules`、`.git`、`dist`、`build`、`__pycache__`；这些并不是简单的全路径统一排除规则，优先遍历会先检查子目录本身有无 `SKILL.md`。
- 名称按扫描顺序去重。安装的 `--skill` 过滤忽略名称大小写，当前应用没有按路径选择同名项的入口。
- 仓库自身若有 `skills-lock.json`，CLI 会从 agent skills 目录中排除被该 lock 记录的项目安装副本，避免当成仓库发布的 skills。

### 插件声明

`getPluginSkillPaths`、`getPluginGroupings`（bundle 约 1036–1095 行）读取 `.claude-plugin/marketplace.json` 和 `.claude-plugin/plugin.json`。声明可补充常规目录之外的搜索位置和分组。只接受以 `./` 开头、解析后仍位于来源根内的相对路径。

### GitHub 特殊路径

`resolveSourceSkills`（bundle 约 5065–5145 行）对部分 allowlist 的 GitHub 来源先尝试 blob 下载路径，失败再 clone。该路径的 `findSkillMdPaths` 与 clone walker 不完全相同，例如文件名大小写、根 skill、扫描回退与插件声明。因此，“沿用 CLI 规则”需要说明以哪条契约为准，不能只按路径后缀搜索所有 `SKILL.md`。

## 设计需要覆盖的具体场景

- 来源有 60 个 skills，用户只装了 20 个：首次扫描的其余 40 个是否展示、是否算“新增”。
- 一个来源同时出现在 Global 和多个 Project：来源获取是否去重、列表与“已安装”按哪个 Scope 判断。
- 上游只移动目录、只修改名称、新增同名项：候选身份与新增标记不能混为同一问题。
- 用户卸载、忽略或已读某项后再次扫描：是否重复提醒。
- 来源使用 pinned ref，而默认分支增加了 skill：发现追踪哪个版本。
- 一次获取失败、超时或超过预算：不能将其当成成功的空目录，也不能由失败建立或覆盖发现基线。
- 新 skill 位于根 skill 内、非标准深层目录或插件声明路径：发现列表与后续 Install 能力需要一致。

以上场景是待决问题，不代表已经选择某个行为。

## 补充核对：安装兼容范围与凭据边界

只读子代理已核对同一本机 npm 包与应用代码，无编辑、联网或读取真实凭据。

| 差异 | CLI clone | CLI GitHub blob |
|---|---|---|
| 根 skill | 有有效根项时提前返回 | 不提前返回，仍可能包含其他优先路径 |
| 文件名 | 精确 `SKILL.md` | 后缀忽略大小写 |
| 插件声明 | 搜索位置与分组均可受声明影响 | 不读取声明，也不生成插件分组 |
| 同名项 | 按原始名称和扫描顺序去重，大小写变体可共存 | 无名称去重 |
| 扫描回退 | 没有有效项才递归回退 | 没有优先路径才回退 |
| 获取失败 | clone/解析失败 | 任一 snapshot 下载失败即可整体回退 clone |

Blob 特殊路径用于 `vercel`、`vercel-labs`、`heygen-com`、`remotion-dev` 和 `zapier/connectors`；有 ref 或 `--full-depth` 时不使用。其请求包含 Trees、各项 raw Markdown 和各项 snapshot；实际结果不只由目录路径决定。

原生 `SourceArchive.files` 可以一次取得元数据所需文件，但 codeload 的 `export-ignore`、被移除的 symlink 以及 CLI 的两套扫描规则使其无法透明替代实际安装预览。通用 Git checkout 打包保留 `export-ignore` 文件，已有测试覆盖；CLI clone 的 `stat/readFile` 可以读取 symlink `SKILL.md`，原生归档会移除这类条目。

预览/安装 CLI 子进程环境清除 `GITHUB_TOKEN` 和 `GH_TOKEN`。页面设置值只由原生 GitHubApiClient 发给 `api.github.com`；复用现有预览不会使 CLI 自动获得设置页凭据。CLI 自身的 Trees 路径先匿名请求，必要时才尝试它自己的凭据来源；raw Markdown 和 snapshot 不带 token。

为落实用户已选的「现有安装范围」，设计草稿采用原生认证版本查询与现有只读预览相结合的方式。初次或版本变化时，实际预览提供 catalog；版本未变时复用已有 catalog，避免重复预览。Node/npx/Git 需求及 CLI 自行请求的凭据边界需在最终计划说明，不能把该方式描述成完全原生扫描或全部请求均经过设置页 token。

主要引用：`src-tauri/src/cli/scoped.rs:63`、`tasks/parser.rs:1`、`env/child_env.rs:1`、`update/api.rs:62`、`update/archive.rs:88`、`update/archive/acquisition.rs:21`、`update/git.rs:375`；npm bundle 约 1250、1302、3928、4000、5084 行。
