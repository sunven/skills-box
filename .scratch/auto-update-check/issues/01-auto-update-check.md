# 01: 自动检查更新与检查时间持久化

**Parent:** [spec.md](../spec.md)

**What to build:** 应用启动后自动执行只读的上游更新检查，并把最近一次全量检查的时间与结果摘要持久化；页头显示「上次检查」，手动检查保留。自动检查遵守 6h per-source 缓存，在 lock/inventory 未就绪、无 managed skill 或变更任务进行时让路；不自动更新文件。实现分三层：Rust 状态文件与读取命令、前端调度与 gating、页头展示与本地时间。行为与验收以 [spec.md](../spec.md) 为准。

**Blocked by:** —（v1 已合并）

**Status:** resolved

### Rust

- [x] `AppPaths` 增加 `update_check_state_path()` → `app_data/update-check.json`。
- [x] 新增 `update/state.rs`：`UpdateCheckState`（`version`、`lastAttemptAt`、`lastSuccessAt`、`lastOutcome`、`failedSourceCount`、`cachedSourceCount`、`skippedSourceCount`），camelCase serde；原子写入；读取时校验版本，损坏/未知版本返回 `None`；写失败返回 `io::Error`。
- [x] `UpdateService::check` 在全量检查（`skill_ids.is_none()`）完成后写状态；写失败向报告 `warnings` 追加提示；scoped 检查不写。
- [x] `lastOutcome` 推导：有 error 且无 ok/cached → `failed`；有 error 或 skipped 且非 failed → `partial`；否则 `ok`。
- [x] `UpdateService::read_state()`；`commands::get_update_check_state` 并注册到 `lib.rs`。
- [x] 测试：状态 round-trip/损坏/未知版本；全量写、scoped 不写；写失败只产生 warning；outcome 推导；`TestClock` 验证时间；报告与状态的前端契约序列化。

### 前端

- [x] `types.ts` 增加 `UpdateCheckState`；`api.ts` 增加 `getUpdateCheckState()`；修正 `checkUpdates` 的「No automatic/background callers」注释。
- [x] `use-updates.ts`：保留报告 query/mutation；修正「Manual-only」注释；全量检查成功后同步状态缓存；scoped 检查只 merge 报告，不推进页头时间/计数。
- [x] 新增自动检查 hook（建议 `use-auto-update-check.ts`）：
  - 等 inventory 成功、lock 可读、至少一个 managed skill；
  - 用 `get_active_mutation` 检查变更任务；
  - 启动只跑一次（StrictMode 单飞）；
  - 全量检查完成后 6h 调度下一次；手动全量重置；scoped 不影响；
  - 同一时刻最多一个检查；失败不额外重试。
- [x] `App.tsx` 接线；`app-header.tsx` 展示检查中/持久化/实时状态与 tooltip；页头时间与计数只取全量状态。
- [x] `update-view.ts` 的 `formatEpochDateTime` 改为本地时间；详情/行内同步。
- [x] 测试：假定时器启动一次/6h/手动重置/scoped 不重置/让路/StrictMode 单飞；缓存命中只推进 attempt；重启后先显示持久化状态；失败 tooltip；本地时区（固定 TZ）。

### 验收

- [x] `pnpm test`、`cargo test`、`pnpm build` 通过。
- [x] 对照 [spec.md](../spec.md) §7 逐项验证。

## Comments

- 2026-10-29: 决策与文档完成（GLOSSARY、spec、v1 注记）。本票据覆盖实现与测试，状态 `ready-for-agent`。
- 2026-10-29: Implemented and verified. Rust: `cargo test` 323 + integration passed; `cargo clippy --all-targets` clean. Frontend: `pnpm test` 135 passed; `pnpm build` passed. Real `pnpm tauri dev` launch: the automatic check ran without user action, the header showed `上次检查：2026-10-29 13:18`, and 5 update candidates appeared. The app-data `update-check.json` contained `version: 1`, `lastOutcome: "ok"`, `cachedSourceCount: 5`, with `lastAttemptAt` newer than `lastSuccessAt` (cache hits did not advance the upstream success time). Scoped checks, StrictMode single-flight, gating and persisted-state display are covered by `App.updates.test.tsx` and `use-auto-update-check.test.tsx`.
