# 07: Extra agent 的项目目录与管理守卫

**Parent:** [spec.md](../spec.md)

**What to build:** 为 extra agent 增加可选 `projectSkillsDir`（`agents.json`，相对项目根），校验非空、相对、无 `..` 逃逸、规范化后位于项目根内、不等于 `.agents/skills`、在 extra 间唯一；内置与现有条目不预填。管理 agent 对话框增加该字段。已配置的 extra 参与项目的安装/挂载目标集合（目录不存在也可选，挂载时创建），仅当目录存在时参与清单扫描。实现守卫：删除 extra 时，若任一已注册项目路径不可用则阻止并列出项目；否则先卸载全局与所有已注册项目中的挂载再删除定义。修改/清空 `projectSkillsDir` 时，若任一已注册项目可能仍存在挂载（含不可用项目）则阻止，要求先卸载或先处理不可用项目。

**Blocked by:** 02, 04

**Notes:** extra 的项目目录作为 `attachment_target` 门控的新目标路径来源（ADR 0005），不另写一套资格判定。

**Status:** resolved

- [x] `agents.json` 可选字段向后兼容；既有文件无需迁移
- [x] 校验覆盖绝对路径、`..` 逃逸、空值、等于 canonical、重复目录
- [x] 管理对话框可设置/显示项目目录并展示校验错误
- [x] 已配置 extra 出现在项目安装/挂载目标；目录存在时参与清单
- [x] 删除守卫：存在不可用项目时阻止并列出；否则清理全局与所有项目挂载后删除
- [x] 修改/清空目录守卫：存在挂载或不可用项目时阻止并说明原因
- [x] 单元测试覆盖校验、目标集合、清单参与与两类守卫

## Comments

- 2026-10-30: Resolved on `feat/project-scope` via merge `a6f1513` (ticket commits `ec9bed4`, `587f9e7`). `extra_agent_guard.rs` and `agents/extras.rs` tests cover project-directory validation/uniqueness, target-set inclusion with missing dirs, inventory participation, removal/edit guards including unavailable projects, and physical containment; `App.agents.test.tsx` covers the dialog field and guard errors. Final gate: `cargo test` 483 passed / 3 ignored, `pnpm test` 203 passed.
