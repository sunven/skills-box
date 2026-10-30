# 08: 验收、真实 CLI 沙箱与安全硬化

**Parent:** [spec.md](../spec.md)

**What to build:** 完成项目作用域的验收与硬化。补齐 Rust 集成 fixtures（临时项目根、项目锁、catalog 项目目录、extra 项目目录）；前端 App 套件覆盖 scope 切换、安装目标、徽章、不可用/错误、守卫；新增 ignored 真实 CLI 沙箱测试：临时 HOME + 临时项目根，执行项目 `add` 与 `remove`，断言 `.agents/skills`、`skills-lock.json`、agent 项目目录链接与清理，且不触碰真实 HOME；指纹 golden vectors 与 CLI 输出对齐；安全检查（capabilities/CSP/白名单）扩展项目根断言；性能检查覆盖清单读取与单来源一次下载；按 spec §10.3 逐项验收。

**Blocked by:** 03, 04, 05, 06, 07

**Status:** resolved

- [x] Rust fixtures 覆盖项目锁、清单、extra 项目目录与 scope 错误隔离
- [x] 前端 App 套件覆盖全部项目作用域流程与守卫
- [x] ignored 沙箱测试在临时 HOME/项目上通过真实 CLI 的 add/remove
- [x] 指纹 golden vectors 与 pinned CLI 一致
- [x] `ErrorCode::ALL` 注册表测试包含全部新项目错误码；前端统一 `AppError` 消费
- [x] `API_COMMANDS` 覆盖测试包含全部新增 scope 命令，preview 与 App 测试共用 fake backend
- [x] 安全审计断言项目白名单、路径归属、capabilities 与 CSP
- [x] 性能检查覆盖有界目录探测、归档缓存与大小/超时上限
- [x] spec §10.3 验收矩阵全部勾选并记录证据
- [x] 全局回归套件全部通过，无行为降级

## Comments

- 2026-10-30: Resolved on `feat/project-scope` via merge `3f6af3a` (ticket commits `241a4a7`, `61e9f3f`). Added the ignored real-CLI project add/remove sandbox (temp HOME/root, canonical, agent link, v1 lock, hash agreement, project/global isolation), a project security audit, and a 100-skill project performance fixture; spec §10.3 is fully ticked with evidence. Post-review fixes (`be2b25b`..`78b42e3`) merged in `bbac48c`; deferred architectural refactors are tracked in issue 09. Final gate: `cargo test` 483 passed / 3 ignored, `pnpm test` 203 passed, `pnpm build` and `cargo fmt --check` clean.
