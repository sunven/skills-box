# Skills Box v1.1 — 自动检查更新

Status: Implemented (2026-10-29)
Date: 2026-10-29

> 本规格只覆盖只读的自动更新检查；skill 的自动更新仍不在范围内。
>
> - v1 规格：[../global-skill-manager/spec.md](../global-skill-manager/spec.md)
> - 术语表：[../../GLOSSARY.md](../../GLOSSARY.md)
> - 实现票据：[issues/01-auto-update-check.md](issues/01-auto-update-check.md)

## 1. 目标

打开应用后无需手动点击，就能看到每个 managed skill 的上游更新状态，并在重启后仍能看到上次检查的时间。手动检查保留，且与自动检查共用同一份报告。

## 2. 行为

### 2.1 触发

- 启动后，当 inventory 成功加载、lock 可读、至少有一个 managed skill、且无变更任务进行中时，立即执行一次全量更新检查。
- 运行期间，距最近一次全量检查 6 小时后再执行一次；手动全量检查会把下一次自动检查推迟到 6 小时后。
- 不做窗口聚焦触发；窗口关闭即退出应用，因此不检查「关窗/托盘后台」。

### 2.2 让路

自动检查在以下情况跳过本轮，等下一个 6 小时周期或用户手动触发：

- inventory 未成功加载或 lock 错误；
- 没有 managed skill；
- 安装/更新/卸载任务正在进行。

失败（离线、限流、仓库不可达）不自动重试；更新「上次检查」时间并保留「上次上游成功」时间与上次 per-skill 结果。

### 2.3 范围与缓存

- 自动检查与页头手动检查都是全量检查（`skill_ids = null`）。
- 详情面板的单 skill 检查是 scoped 检查：只更新该行/详情，不写全局检查状态，不重置自动检查计时。
- 自动与手动都遵守现有 6 小时 per-source tree 缓存；缓存新鲜时不发网络请求。缓存命中仍算一次 check attempt：推进「上次检查」，不推进「上次上游成功」。
- 不调用 `skills check` / `skills update`；检查保持只读，不修改 lock 或 skill 文件。

### 2.4 并发

同一时刻最多一个检查。自动检查进行中时手动按钮禁用并显示进度；手动检查进行中时定时器跳过本轮。React StrictMode 下启动检查只能触发一次。

## 3. 持久化检查状态

文件：app data 根目录 `update-check.json`（不是 `cache/`）。只在全量检查完成后原子写入；scoped 检查不写。时间均为 epoch 毫秒。

```json
{
  "version": 1,
  "lastAttemptAt": 1793200000000,
  "lastSuccessAt": null,
  "lastOutcome": "failed",
  "failedSourceCount": 2,
  "cachedSourceCount": 0,
  "skippedSourceCount": 0
}
```

- `lastAttemptAt`：`UpdateCheckReport.checkedAtEpochMs`。
- `lastSuccessAt`：`UpdateCheckReport.lastSuccessAtEpochMs`，可能为 `null`；缓存命中不推进。
- `lastOutcome`：有 error 且无 ok/cached → `failed`；有 error 或 skipped 且非 failed → `partial`；否则 `ok`。
- 写入失败只在报告 warnings 中追加提示，不影响检查结果。
- 文件损坏、版本未知或读取失败按「无记录」处理。
- 新命令 `get_update_check_state` 返回 `UpdateCheckState | null`，供启动时在自动检查完成前显示历史。

## 4. UI

- 页头状态（`data-testid="last-update-check"`）：
  - 检查中：「正在检查…」；手动按钮 spinner + disabled。
  - 命令级失败：现有「更新检查失败：<message>」。
  - 有全量检查记录：「上次检查：<本地时间>」，失败/部分失败/有跳过时追加简短后缀。
  - 无记录：「尚未检查更新」。
- Tooltip：上次上游成功时间、失败/缓存/跳过来源数、warnings，以及「启动和每 6 小时自动检查，也可手动触发」。
- 页头时间和来源计数只取全量检查状态；scoped 检查的失败只在对应行/详情体现。
- `formatEpochDateTime` 改为本地时间；详情与行内时间同步。

## 5. 决策

- **前端调度**：复用现有 `check_updates` 命令与 React Query 报告；Rust 只持久化状态并提供读取命令。应用关闭窗口即退出，后端常驻调度没有收益。
- **只读**：自动检查绝不自动更新 skill；更新动作仍由 [07-update](../skills-box-v1/issues/07-update.md) 的手动流程执行。
- **缓存**：自动与手动都遵守 6h TTL，不在本期提供强制刷新；未鉴权 GitHub API 的 60 次/小时限额下，绕过缓存会放大请求。
- **scoped 检查不推进全局时间**：页头「上次检查」没有范围限定，单个 skill 的检查不应让它看起来像全量检查。
- **无 ADR**：决策可逆，且本节已记录取舍。

## 6. 非目标

- skill 自动更新文件；应用自身自动更新。
- 自动检查开关/频率设置。
- 系统通知。
- 关窗/托盘后继续检查。
- 强制绕过 6h 缓存。
- 持久化完整报告或检查历史。
- 单 skill 检查影响全局计时。

## 7. 验收

- [x] 满足条件时启动恰好触发一次全量自动检查；任一让路条件命中时不触发。
- [x] StrictMode 下不重复请求；自动检查进行中手动按钮禁用；手动进行中定时器不重复触发。
- [x] 距上次全量检查 6 小时再触发；手动全量检查重置计时；scoped 检查不重置。
- [x] 缓存全命中时无网络请求，`lastAttemptAt` 推进、`lastSuccessAt` 不变。
- [x] 重启后、新检查完成前，页头显示持久化的上次检查时间与结果摘要。
- [x] 失败保留上次成功时间，不弹窗/横幅；不因自动检查 spawn CLI 或修改 lock。
- [x] 页头与详情时间按本地时区显示。
- [x] `pnpm test`、`cargo test`、`pnpm build` 通过。

## 8. 已知限制

- 窗口最小化时 WebView 定时器可能被节流；启动检查是确定发生的路径，周期检查是尽力而为。
- 若变更任务在自动检查已开始后启动，本次报告可能基于旧 lock；下一周期或手动检查会纠正。
