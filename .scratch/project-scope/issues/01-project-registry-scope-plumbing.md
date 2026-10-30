# 01: 项目注册表与 scope 管线

**Parent:** [spec.md](../spec.md)

**What to build:** 建立项目作用域的基础设施。应用数据新增带版本的 `projects.json`（项目条目 + `lastActiveScope`），条目字段为 id、root、displayName、addedAt、lastOpenedAt；实现 `list_projects`、`add_project`、`relocate_project`、`forget_project`、`set_active_scope`。校验规则：目录存在且可读、canonicalize 后去重、拒绝 `/`、home、全局 canonical `~/.agents/skills` 及其内部；允许嵌套/重叠；displayName 取根目录名且不可重命名；只记录路径，不创建任何目录。引入 `ScopeId = global | project:<uuid>` 类型并贯穿所有读取/变更命令的签名；全局行为保持不变。注册表损坏时不崩溃：保留原文件、回退为空列表并在环境层暴露警告。遗忘只删注册条目；重新定位保留项目 id、更新 root/displayName/lastOpenedAt；激活 scope 更新 lastOpenedAt 并持久化 lastActiveScope。

**Blocked by:** None (can start immediately)

**Notes:** 新错误码必须进入 `ErrorCode` 注册表与 `ErrorCode::ALL` 测试（ADR 0006），沿用统一 `AppError` 构造器；不得新增裸字符串错误码。

**Status:** resolved

- [x] `projects.json` 有版本号、原子写入、可注入路径（测试用临时 AppPaths）
- [x] 添加/重新定位校验覆盖：不存在、非目录、不可读、`/`、home、全局 canonical 内部、重复规范化路径；嵌套与重叠允许
- [x] `displayName` 始终来自根目录名；按 `lastOpenedAt` 倒序返回
- [x] `ScopeId` 贯穿现有命令签名；全局路径与行为不变
- [x] 启动时恢复 `lastActiveScope`；项目被遗忘或不可用时回退 global
- [x] 遗忘只移除注册条目与该 scope 的检查状态，不删除任何磁盘文件
- [x] 注册表 JSON 损坏时应用不崩溃、不静默覆盖原文件，全局作用域仍可用并显示警告
- [x] Rust 单测覆盖校验、排序、持久化、损坏恢复与 lastActive 回退

## Comments

- 2026-10-30: Resolved on `feat/project-scope` via merge `2436eb8` (ticket commit `0ac6929`). Registry validation, atomic persistence, corrupt-registry recovery, scope ids and last-active fallback are covered by `src-tauri/src/projects.rs` tests plus the App scope suites. Final gate: `cargo test` 483 passed / 3 ignored, `pnpm test` 203 passed, `pnpm build` and `cargo fmt --check` clean.
