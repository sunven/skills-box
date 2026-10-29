# 12: Skill 内容浏览与详情双 tab

**Parent:** [spec.md](../../global-skill-manager/spec.md)

**What to build:** 详情抽屉拆为「内容 / 操作」两个 tab。身份头（标题、正交徽章、描述）固定在 tab 之上；全局状态告警（未受管、仅副本、本地改动、详情 warnings）固定在 tab 栏上方。「内容」tab 提供只读的 skill 目录树 + 文件预览（不只 SKILL.md：references、scripts、assets 等都可浏览），支持懒加载、内部相对链接/图片导航、外部打开与刷新；「操作」tab 承载 Agent 挂载（断开）、更新分区、瘦身后的概览与彻底卸载。抽屉加宽以容纳左树右预览。

**Blocked by:** 03（skill 详情）— resolved

**Status:** resolved

## 验收清单

- [x] 抽屉结构：身份头 → 全局告警 → 内容/操作 tab 栏；默认「内容」，换 skill 重置到内容并清空文件选中；同一 skill 内切 tab 保留树、预览与滚动状态（两面板常驻挂载）。
- [x] 抽屉宽度为 `min(680px, calc(100% - 48px))`；窄屏（≤860px）仍为全宽；关闭、遮罩与 Escape 行为不变。
- [x] 全局告警固定在 tab 之上：未受管、仅副本、本地改动 modified/unknown、详情 warnings、详情读取失败/重试。更新检查类 notice 只在操作 tab；打开文件错误在内容 tab 工具栏附近；断开结果在操作 tab。
- [x] 内容 tab：左侧目录树 + 右侧预览；目录优先、大小写不敏感字母序；文件显示大小；symlink 有链接标记；默认只展开根层；SKILL.md 默认选中；选中行高亮。
- [x] 内容 tab 工具栏：刷新（重列目录 + 重读文件 + 刷新详情）、在 Finder 中查看、用默认应用打开选中文件。预览头显示相对路径、复制路径、文件大小。
- [x] 渲染矩阵：`.md` 渲染/原文切换；图片（png/jpg/jpeg/gif/webp/svg）以 data URL 预览；其余有效 UTF-8 文本用等宽纯文本（不换行、无行号）；二进制、非 UTF-8、超限只显示元信息 + 外部打开。无语法高亮、无音视频播放。
- [x] Markdown 内部规则：相对链接指向 skill 内文件时在预览中选中该文件（不做历史栈）；相对图片按上限加载并可点击定位；`#锚点` 当前文档内滚动；https 外链仍走 opener；绝对路径、`//`、mailto、file、javascript、data 保持惰性。
- [x] 懒加载与上限：每目录 500 条、扫描上限 2000、最大深度 12、文本 1 MiB、图片 5 MiB；隐藏 `.git/`、`node_modules/`、`.DS_Store`；目录内 symlink 可展开/预览，逃逸或断链标灰不可点。
- [x] 状态：缺失 SKILL.md 不默认选中；空目录提示；截断提示；不可读/非 UTF-8/二进制/超限有明确状态；copy-only 多副本歧义沿用 `skill.ambiguous`；managed / copy-only / unmanaged 全部可只读浏览。
- [x] 操作 tab 顺序：Agent 挂载（列表 + 断开）→ 更新（状态、检查更新、更新、上次检查、远端 tree SHA、检查错误）→ 概览（来源/ref/skillPath/时间/路径/版本指纹/本地改动/管理状态）→ 危险操作（彻底卸载）。不新增挂载写操作。
- [x] 后端新增 `list_skill_dir(skillId, path)`、`read_skill_file(skillId, path)`、`open_skill_file(skillId, path)`；移除 `open_skill_md`。前端只传 skillId + 相对路径；Rust 复用服务端 skill 根解析与 allowed roots，拒绝绝对路径、`..`、`.`、空段、反斜杠、NUL 与被隐藏的目录段；canonicalize 后必须仍在 skill 根内。
- [x] `SkillDetail` 不再内嵌 `skillMd` 内容；描述与 SKILL.md 警告仍由详情读取产生，预览统一走 `read_skill_file`。
- [x] 测试：Rust 覆盖遍历/越界/隐藏目录/深度/截断/符号链接（内、外、断）/二进制/非 UTF-8/大小上限/图片 data URL/目录误读/copy-only 歧义/unmanaged；前端覆盖 tab 默认与状态、树懒加载、各预览类型、内部链接与图片、损坏态、操作区重组、告警位置、刷新与打开命令。
- [x] 门禁：`cargo test`、`cargo clippy --all-targets`、`pnpm test`、`pnpm build` 通过；预览环境走查两个 tab 与文件浏览。

## 明确不做

编辑写盘、语法高亮、音视频播放、文件搜索、文件系统 watcher、挂载写操作、第三个 tab、分页（用上限 + 截断提示）。

## Comments

- 2026-10-29: Scope settled through a grilling session; glossary updated with Skill content, Content browser, File preview, and the 「挂载」mapping.

- 2026-10-29: Implemented. Evidence: `cargo test` (316 lib + 9 security + integration), `cargo clippy --all-targets` clean, `pnpm test` 121 passed, `pnpm build` passed. Preview harness verified the content/actions tabs, lazy tree, internal link navigation, markdown/text/image previews, and state preservation across tabs.
