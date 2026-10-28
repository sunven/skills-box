# 03: Skill 详情与 SKILL.md

**Parent:** [spec.md](../../global-skill-manager/spec.md)

**What to build:** 用户点击列表行，右侧持久详情面板展示：名称、正交徽章、描述与动作区；概览（来源、ref、skillPath、安装/更新时间、canonical 路径（copy-only 显示 agent 路径）、hash、本地改动、更新状态、管理状态）；agent attachment 列表（名称、目录、模式 link/copy/native/broken、无挂载显示「未挂载」，本票据只读）；`SKILL.md` 默认渲染、可切原文：隐藏 frontmatter、不渲染原始 HTML、不加载远程图片（渲染为链接）、链接仅 https 经 opener 打开、按需读取且 1 MiB 上限、缺失/损坏显示警告。Finder 与默认编辑器入口只接受服务端 inventory 解析的 skillId，前端不传路径。首次扫描记录 Baseline fingerprint 到 app data，本地改动判定为 clean / modified / unknown（无 baseline 且 lock hash 不一致标「本地改动?」），并显示在列表徽章与详情。异常弹窗扩展到 modified 类别。

**Blocked by:** 02（库存读取模型与平铺列表）

**Status:** resolved

- [x] `get_skill_detail` 返回元数据、attachments、SKILL.md 内容或读取错误；1 MiB 上限生效
- [x] Markdown 安全子集：`skipHtml` + sanitize；恶意 HTML 不执行；远程图片不加载；链接仅 https 经 opener（对齐 M04）
- [x] frontmatter 在渲染视图隐藏；原文视图完整；缺失/损坏 SKILL.md 有明确警告
- [x] `open_skill_folder` / `open_skill_md` 仅接受服务端解析的已知路径；路径守卫拒绝越界与 agent 目录自身为 symlink 的情况
- [x] Baseline fingerprint 在首次扫描写入 app data；modified / unknown 判定符合规格判定表
- [x] 本地改动警告同时出现在列表与详情；文案与后续危险动作一致
- [x] 详情动作按管理状态限制：unmanaged 只保留 Finder；copy-only 不提供更新/断开入口（卸载入口随 09 接入）；不内置编辑
- [x] 异常弹窗 v1 覆盖 stale / unmanaged / broken / modified，按类别显示计数与示例
- [x] 更新状态字段在本票据未接入 checker 时显示「未知」，不误报「已是最新」

## Comments

- 2026-10-28: Resolved on integration/skills-box-v1. All tickets merged; code-review fixes merged in 3e9d2ad.
- Evidence: `detail::tests::*`、`markdown.test.ts`、`App.detail.test.tsx` 通过（acceptance.md §1、§5 M04）；恶意 HTML/远程图片/opener 白名单由 `skipHtml` + `rehype-sanitize` + CSP 测试覆盖。
