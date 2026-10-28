# 09: 彻底 Uninstall

**Parent:** [spec.md](../../global-skill-manager/spec.md)

**What to build:** 用户从详情发起 Uninstall：危险确认弹窗展示删除范围（canonical、所有 agent attachments、lock entry、baseline），需勾选 checkbox 解锁确认。仅 managed 与 copy-only（有 lock）可卸载，unmanaged 禁用；前置备份 lock + canonical + copy 挂载目录 + manifest，备份失败中止；存在跨 agent copy 依赖时阻止。备份成功后执行 `skills remove -g -s <name> -y`（不带 `-a`），随后逐项校验 canonical、所有 agent 路径、lock key 均消失；CLI exit 0 但状态未变按失败，不宣称成功。结果展示备份与日志路径；完成后刷新 inventory、详情与异常计数。不使用 `remove --all`，不提供批量卸载或「仅卸载、保留目录」动作。

**Blocked by:** 04（Mutation task 骨架与安装预览）、05（安装执行与校验）、07（备份与更新执行）

**Status:** resolved

- [x] sandbox 中卸载 managed skill：canonical、agent 路径、lock key 全部消失，备份存在，`list -g --json` 不再识别（对齐 M13）
- [x] unmanaged 禁用卸载；copy-only 可卸载并备份（对齐 M14）
- [x] 跨 agent copy 依赖时阻止；无 `remove --all`
- [x] 备份失败中止；勾选解锁后才可执行
- [x] 校验失败返回 `verify.*`，不得宣称成功；exit 0 不足为凭
- [x] 结果包含备份/日志路径；JSONL 记录完整
- [x] 完成后 inventory/详情/异常计数刷新；未受管 skill 不受影响
- [x] fake 日志不含 `remove --all`、`--all`、`--copy`、`update`、`check`

## Comments

- 2026-10-28: Resolved on integration/skills-box-v1. All tickets merged; code-review fixes merged in 3e9d2ad.
- Evidence: 真实 CLI sandbox 卸载 managed 与 copy-only skill 后 canonical/agent 路径/lock key 均消失，未受管目录保留（acceptance.md §2 步骤 7–9、§5 M13/M14）；`uninstall::execution_tests::*` 覆盖备份、依赖阻止、exit 0 无变化判失败（acceptance.md §1）。
