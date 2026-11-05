# 02: 恢复技能与批量任务

Status: resolved
Blocked by: 01
Parent: [spec.md](../spec.md)

## What to build

为 Clean stale records 与 Restore skill 扩展现有批量预览、确认和注册任务。单项和批量都持有整批的 writer permit 与 Scope；目标固定，逐项串行，普通失败继续、备份失败和不可安全继续时停止，支持在当前项完成后停止后续项。

Restore 使用原 Lock Source/ref，在原来源按同名 Skill 匹配，允许目录移动，拒绝改名、删除、不支持来源和歧义；默认写 canonical，用户手选的 Agent targets 统一作用于本次目标。复用 `skills@latest add`、来源预览、Source/ref 解析和安装验证，但必须有专用的 stale-only 授权，不能用 InstallRequest.retry 覆盖已有 Managed、Copy-only 或 Unmanaged Skill。

两种动作都记录逐项结果、备份和中断状态。重试只针对原失败或未执行项，重新预览确认。验证后的成功项刷新基线与对应检查结果，部分写入失败展示实际状态，不声称已自动回滚。

## Validation

Fake executor/transport 覆盖 ref、默认分支、同名移动、改名/删除/不可访问、名称歧义、已有技能冲突和 CLI 成功但验证失败。暂停式任务测试覆盖整批许可、外部状态变化、单项失败继续、备份失败停止、项间取消和重试不重放成功项。

## Comments

- 2026-11-05: 沿用 ADR-0021 与 ADR-0016 的任务和 Scope 所有权；不通过前端循环单项 IPC 实现批量。

- 2026-11-05: 实现完成并通过对应验证。
