# 05: 项目更新检查（内容指纹）

**Parent:** [spec.md](../spec.md)

**What to build:** 为项目 scope 实现只读更新检查。解析项目锁中 `sourceType=github`、`computedHash` 为 64 位十六进制、`skillPath` 有效的条目，按 (source, ref) 分组，解析最新 commit，按 commit 下载一次来源归档并缓存，使用与 pinned CLI 完全一致的内容指纹算法计算每个 skill 文件夹 SHA-256，与 `computedHash` 比较；非 GitHub 与无法解析的条目标记「不支持检测」。更新检查状态升级为 v2（`scopeId -> UpdateCheckState`），v1 迁移为 global。自动检查沿用 6 小时阈值并按 scope 独立：启动检查当前 scope，切换时对超过阈值的 scope 检查，手动刷新只作用于当前 scope。报告、来源失败计数、徽章均按 scope；一个 scope 失败不影响其他 scope。下载设大小与超时上限，超限转为来源级失败。

**Blocked by:** 02

**Notes:** 来源/ref 解析与安全校验复用 `update_target`（ADR 0009）；`git_tree.rs` 不变；失败状态通过统一 `ErrorCode`/AppError 暴露（ADR 0006）。

**Status:** resolved

- [x] 项目锁条目分组与最新 commit 解析正确，同一 commit 只下载一次
- [x] 指纹算法有 golden vectors，与 pinned CLI 对同一目录的计算结果一致
- [x] 候选/无更新/不支持/来源失败的判定与报告字段完整
- [x] `update-check.json` v2 读写与 v1 迁移有测试；遗忘/重新定位清除对应状态
- [x] 启动与切换的 6 小时阈值行为符合 spec §4.6
- [x] 侧栏徽章数据按 scope 提供，未检查不显示，失败显示错误
- [x] 一个 scope 的检查错误被隔离，其他 scope 正常
- [x] 归档缓存命中、大小/超时上限与错误映射有测试

## Comments

- 2026-10-30: Resolved on `feat/project-scope` via merge `9986dfc` (ticket commits `94e3158`, `0b3386a`, `6bff97d`, `4ab53f8`, `fba7ef4`). Content-hash golden vectors, archive caching/staleness, project lock grouping, unsupported entries, v1→v2 per-scope state and per-scope auto-check are covered; `use-auto-update-check.test.tsx` and the fake backend cover scope isolation. Code-review fix for the launch check landed in `bbac48c`. Final gate: `cargo test` 483 passed / 3 ignored, `pnpm test` 203 passed.
