# 06: 项目更新与卸载生命周期

**Parent:** [spec.md](../spec.md)

**What to build:** 为项目 scope 实现更新与卸载。更新仅在检查产生候选后由用户手动触发：按 lock 重建 `add`，cwd=项目根、不带 `-g`，保留原挂载集合并追加 `universal`，更新后重建 extra 链接并执行安装同级校验；本地改动/未知状态与 Copy-only 的处理与全局一致。卸载复用全局安全流程：备份项目锁、canonical、复制型挂载与挂载清单，执行不带 `-g` 的 `remove -s <name> -y`，原生清理 extra 链接，校验 canonical、lock key 与该 scope 内所有挂载消失。备份快照、mutation session、日志与中断恢复按 scope 归属；中断提示只在对应 scope 出现。

**Blocked by:** 04, 05

**Notes:** 复用统一 mutation surface（ADR 0008）与 `attachment_target` 门控（ADR 0005）；刷新键与备份/日志/中断记录都携带 scope。

**Status:** resolved

- [x] 项目更新重建 `add` 时 cwd/无 `-g`/挂载集合/universal 正确
- [x] 更新后校验 canonical、lock key、catalog 链接与 extra 链接
- [x] 本地改动与 Copy-only 的处理与全局语义一致
- [x] 项目卸载只删除当前 scope 的 canonical、lock key 与挂载
- [x] 卸载前备份覆盖项目锁、canonical、复制挂载与清单
- [x] extra 链接在更新与卸载后被正确重建/清理
- [x] 中断恢复、任务结果与日志携带 scope，且只影响对应 scope
- [x] 沙箱集成测试覆盖项目更新与卸载的真实 CLI 路径

## Comments

- 2026-10-30: Resolved on `feat/project-scope` via merge `d8d376d` (ticket commit `2234c8a`). Backend tests cover project update command building and attachment preservation, scoped uninstall with extra-link cleanup, backup snapshots, interrupted/session scope and per-scope isolation; the fake backend applies update/uninstall to the selected scope. The real-CLI project sandbox is covered by ticket 08. Code-review fixes (relocate persistence/guard, scoped backup) landed in `bbac48c`. Final gate: `cargo test` 483 passed / 3 ignored, `pnpm test` 203 passed.
