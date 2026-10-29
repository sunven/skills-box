# 06: 更新检查

**Parent:** [spec.md](../../global-skill-manager/spec.md)

**What to build:** 用户在页头或详情手动触发「检查更新」，得到每个 skill 的更新状态：已是最新 / 可更新 / 未知 / 检查失败 / 源路径缺失。`check_updates(skillIds?)` 只读：从 lock 的 source/ref/skillPath 按 (source, ref) 分组，每个来源一次 GitHub Trees API（recursive），以 40 位 `skillFolderHash` 对比目标文件夹 tree SHA；空 hash、64 位 hash 或无法比较判 unknown；上游存在但 skillPath 缺失判 path_missing；网络、限流、仓库不可达判 error 并保留上次结果，绝不上报「已是最新」。按来源缓存 6 小时，并发 3–4，单次最多 20–25 个来源；不自动检查（无启动/焦点触发；v1.1 已取代，见 Comments）。UI 在列表与详情显示正交徽章、上次成功检查时间（页头或 tooltip），异常弹窗扩展 path_missing 与检查失败；「更新全部」入口展示候选与跳过项摘要（执行在 07）。不调用 `skills check` / `skills update`。

**Blocked by:** 02（库存读取模型与平铺列表）、03（Skill 详情与 SKILL.md）

**Status:** resolved

- [x] mock HTTP 下单测覆盖 current / update / unknown / path_missing / error 五态映射
- [x] 仅比较 40 位 tree hash；64 位、空 hash 判 unknown，不与 Git tree SHA 混比
- [x] 6 小时 per-source 缓存命中不发起请求；v1 无启动/焦点自动请求（v1.1 起启动/周期自动检查已取代此约束，见 Comments）
- [ ] 并发上限 3–4、来源上限生效；20 个来源 30s 内完成或明确报错，UI 不冻结 — pending user sign-off (real 20-source network run deferred to v1.1; acceptance.md §5 M24、§7)
- [x] 离线/限流/仓库不可达标「检查失败」并保留上次徽章；详情显示上次成功检查时间
- [x] 源路径缺失标红且不计为「可更新」；异常弹窗展示对应计数
- [x] 状态筛选（可更新 / 已是最新 / 未知 / 异常）与默认徽章联动正确
- [x] fake 日志断言未出现 `check`、`update` 命令
- [x] 检查失败后再次手动检查可恢复；缓存过期后按新结果刷新

## Comments

- 2026-10-28: Resolved on integration/skills-box-v1. All tickets merged; code-review fixes merged in 3e9d2ad.
- Evidence: `update::tests::*`（五态映射、6h TTL、40 位 hash、并发上限、失败保留上次结果）与 `App.updates.test.tsx` 通过（acceptance.md §1、§5 M19/M24）；真实 GitHub 只读检查显示 `state=Current` 且 lock 字节不变（acceptance.md §2）。
- 2026-10-29: v1.1 引入只读的启动/每 6 小时自动检查；本票据「不自动检查（无启动/焦点触发）」与「无启动/焦点自动请求」约束被取代。6h TTL、来源上限、并发上限与只读约束继续有效。见 [auto-update-check/spec.md](../../auto-update-check/spec.md)。
