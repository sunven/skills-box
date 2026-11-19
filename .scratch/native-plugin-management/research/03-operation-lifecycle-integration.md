# 原生插件操作与现有任务生命周期

日期：2026-11-19。只读调查当前 Skills Box 代码，未运行测试、未修改用户配置。

## 可复用边界

1. Skill mutation、Project Relocate/Forget、App data 清理和 App update 安装共用 mutation permit；并非应用所有写操作都通过它，部分配置操作使用 configuration guard。证据：`src-tauri/src/tasks/manager.rs:462`、`src-tauri/src/commands.rs:202`、`src-tauri/src/app_update.rs:170`。
2. permit 必须由实际 worker 持有到写入与验证结束，不能随 IPC future 被取消而提前释放；注册任务还要在锁内复查 shutdown。证据：`src-tauri/src/tasks/manager/scoped_mutation.rs:99`、`src-tauri/src/tasks/manager.rs:717`。
3. 共用 permit 不自动提供运行中任务身份或退出确认。当前 close/quit 观察 active_mutation；原生 Attach/Detach 的退出等待与已注册任务不同。证据：`src-tauri/src/lib.rs:214`、`src-tauri/src/tasks/manager.rs:489`、`:584`。
4. App update 下载可与写操作并行，安装与重启通过同一 mutation permit 和 configuration guard 排他。独立 Plugin 锁不能替代它。证据：`src-tauri/src/app_update.rs:313`、`:344`；ADR-0020。

## 不能直接继承的 Skill 假设

- `useMutation` 可复用进度、终态与完成后刷新，但现有 action、结果和 query keys 面向 Skill。需要 Plugin 专属身份与刷新映射，不能把 Plugin 伪装为 Skill。证据：`src/lib/mutation.ts:470`、`src/lib/mutation-refresh.ts:69`。
- 现有 interruption marker 会按 Global v3 / Project Skill lock 规则检查路径。原生 Plugin 配置不能直接填入这种 marker。证据：`src-tauri/src/interrupted.rs:171`。
- BackupStore 只覆盖 Skill lock、canonical store 和 attachments；不能保证还原 Plugin 的原生配置、安装缓存、数据与凭据。现有界面也没有自动回滚，恢复 Skill 与恢复快照是不同能力。证据：`src-tauri/src/backup.rs:3`、`src/components/interrupted-banner.tsx:67`；ADR-0028。
- Skill MutationSession 在快照后封闭普通取消窗口，但普通单任务的 confirmed quit 会取消 token、终止进程组并最多等待六秒；Batch 则等待当前目标完成。不能笼统宣称 Plugin 退出完全沿用现有取消。证据：`src-tauri/src/tasks/session.rs:352`、`src-tauri/src/tasks/manager.rs:794`、`:564`、`src-tauri/src/tasks/manager/batch.rs:311`。

## 对应已确认产品决策

Q18 接受保留原生实际结果、写后核对、重试前重新确认，首版不提供完整 Plugin 备份或自动回滚。Q19 选择 Plugin 写入开始后等待当前操作和核验结束再正常退出，不暴露强制中断；异常结束后标记待核对。

这需要独立的 Plugin 操作协议适配公共任务/互斥/UI 基础设施，而不是复用要求 Skill 快照的 MutationSession 全部步骤。有关例外及取舍见 ADR-0039。原有 Skill 取消、备份和退出行为保持其原协议。
