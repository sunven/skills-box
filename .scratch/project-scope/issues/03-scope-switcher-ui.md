# 03: Scope 切换器、项目视图与前端状态

**Parent:** [spec.md](../spec.md)

**What to build:** 左侧栏顶部加入「全局」入口与「项目」分组：项目行显示名称、路径副标题、更新徽章、不可用/错误徽章，底部「+ 添加项目」；当前 scope 高亮，按最近打开排序。接入 Tauri dialog 插件实现文件夹选择（最小 capability）。实现添加、重新定位、遗忘对话框，以及项目视图头部（名称、路径、canonical 路径、锁状态、上次检查、手动刷新）。三种空状态：无 skill、路径不可用、scope 错误。切换 scope 时清空选择、重置筛选与搜索。React Query 键全部包含 scope；启动恢复上次激活 scope，不可用时回退全局并保留条目标记。扩展 fake backend 以支持内存项目注册表与按 scope 的清单/详情/状态。

**Blocked by:** 02

**Notes:** 派生状态继续集中在 `buildWorkspaceView`（ADR 0007），不要把 scope 逻辑散回 App memo；mutation 刷新键复用统一 mutation surface（ADR 0008）并携带 scope；fake backend 必须遵守 `API_COMMANDS`/`FakeHandlers` 契约与覆盖测试（ADR 0010）。

**Status:** resolved

- [x] 侧栏展示全局与项目列表、徽章、不可用/错误状态和添加入口
- [x] 文件夹选择、添加、重新定位、遗忘流程可用；遗忘确认文案明确不删文件
- [x] 项目视图头部与三种空状态完整
- [x] 切换 scope 重置选择/筛选/搜索，查询键按 scope 隔离
- [x] 启动恢复上次 scope；不可用回退全局
- [x] fake backend 覆盖项目注册表、按 scope 快照与切换场景
- [x] App 测试覆盖切换、徽章、不可用/错误、添加与遗忘

## Comments

- 2026-10-30: Resolved on `feat/project-scope` via merge `e88f7ed` (ticket commit `9623ac9`). `src/App.scope.test.tsx` covers last-scope restore/fallback, switching with selection/filter reset, per-scope badges, unavailable/error states, add/relocate/forget through the native picker, and empty states; the fake backend owns the in-memory registry. Final gate: `cargo test` 483 passed / 3 ignored, `pnpm test` 203 passed.
