# 07: 备份与更新执行

**Parent:** [spec.md](../../global-skill-manager/spec.md)

**What to build:** 用户可更新单个 skill 或全部可更新项。更新前强制生成 Backup snapshot（lock、canonical、copy 挂载目录、attachment manifest → app data `backups/<timestamp>-<action>-<skill>/`）并追加 JSONL 操作日志；备份失败中止。单个更新确认页展示来源、当前/目标 tree hash、受影响 agent、本地改动与 copy 转换警告；存在本地改动或「本地改动?」需勾选确认才能开始；带 copy 挂载的更新明确警告更新后将转为 symlink 并先备份。执行用 lock 构造 `skills add <source> -g -s <skill> -a <当前物理挂载的非 universal agents> -a universal -y --json`（无挂载时仅 `-a universal`），保留 detach 状态；更新全部只处理 update-available，跳过 modified / unknown / path_missing / error，串行执行，失败继续，完成后汇总并允许逐项重试。更新后逐项校验 canonical、attachments、lock 并重建 baseline；不自动回滚，结果展示备份与日志路径；`reveal_app_data` 打开 backups/logs/cache/baselines。不调用 `skills update` / `skills check`。

**Blocked by:** 05（安装执行与校验）、06（更新检查）

**Status:** resolved

- [x] 单个更新：备份 snapshot 存在且完整，更新后 canonical/attachments/lock 校验通过，baseline 刷新（对齐 M08）
- [x] 更新全部：跳过 modified / unknown / path_missing / error，继续其余项，汇总失败并支持逐项重试（对齐 M09）
- [x] 本地改动/无法判断必须勾选确认；未勾选不能开始更新
- [x] copy 挂载更新有警告与备份，更新后 attachment 为 symlink，其他 agent 正确重链（对齐 M10）
- [x] 已 detach 的 agent 不被重挂；无挂载时仅 `-a universal`
- [x] 备份失败中止（`backup.failed`）；无自动回滚
- [x] 结果展示备份路径与日志路径；JSONL 含时间、动作、skill、agents、结果、备份路径、错误
- [x] fake 日志不含 `update`、`check`、`remove --all`、`--all`、`--copy`、`-a '*'`
- [x] 更新后 inventory/detail/updateReport 失效刷新；失败项重试后状态正确

## Comments

- 2026-10-28: Resolved on integration/skills-box-v1. All tickets merged; code-review fixes merged in 3e9d2ad.
- Evidence: 真实 CLI sandbox 的 copy→symlink 更新保留 `installedAt`/`skillFolderHash`（acceptance.md §2 步骤 5、§5 M08–M10）；`tasks::update::*` 与 `App.update-exec.test.tsx` 覆盖备份、跳过、重试、取消矩阵（acceptance.md §1）。
