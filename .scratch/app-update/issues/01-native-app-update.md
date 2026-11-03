# 01: 原生应用更新生命周期

Status: resolved
Execution: Completed locally (2026-11-03)
Parent: [spec.md](../spec.md)
Blocked by: —

## What to build

用 Rust 管理独立 App update 生命周期，接入 Tauri 2 updater 原生 API 和 typed IPC。检查/下载不占 Skill 写入许可；验签成功后安装再取得全局变更许可，忙时保留已下载包，用户重试时复用。状态、候选和进度由后台拥有，设置页面卸载及请求 future 丢弃不能终止正在执行的安装或提前释放许可。

- 检查去重、候选有效性和阶段化错误；下载大小未知与验签前 Finished 回调正确处理。
- 安装后台 worker 持有许可，安装成功后完成 discovery/子进程停机与受控重启。
- 安装状态接入 CloseRequested、ExitRequested、quit_app 及所有写入口；不直接授予 renderer 无保护的重启权限。
- 检查/下载/安装驱动具备可验证的边界，使用确定性暂停测试覆盖原生写入与规划阶段的竞态。
- 已安装状态不能重复安装；只读目录和失败的手动恢复信息符合规格。
- 不改 Skill lock、baseline、TaskEvent、MutationKind 或中断 marker 语义。

## Acceptance

- 原生检查/下载/安装/忙时重试与阶段错误测试通过。
- 非已登记写操作同样阻止安装；worker 在请求 future 被丢弃后仍阻止新的写操作。
- 安装期间关闭、Cmd+Q、确认退出和新写操作均不能绕过保护。
- 安装与重启交接不重复安装，不进入普通强制退出逻辑。

## Comments

- 2026-11-03：Q1–Q11 已确认；完整规格共同理解确认后可开始执行。

- 2026-11-03：用户已确认完整规格并授权继续，实现和本机验收已开展。

## Answer

原生应用更新由 Rust 后台 worker 拥有，下载不占写许可，安装与重启保留全局变更许可和配置/退出保护。已加入同步 IPC 线程无 Tokio 上下文的回归测试，8 项原生应用更新测试通过。忙时、失败时复用已验签包，已安装后重启重试不重复安装。

验收：[verification.md](../verification.md)。
