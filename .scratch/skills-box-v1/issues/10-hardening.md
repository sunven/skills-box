# 10: 硬化：并发、安全与错误态

**Parent:** [spec.md](../../global-skill-manager/spec.md)

**What to build:** 把 v1 的健壮性补齐：single-instance 插件（第二次启动聚焦已有窗口）；变更任务运行中退出应用时提示风险，确认后杀进程组并保留 lock snapshot，下次启动校验；超时矩阵（探测 5s、读取 30s、写入 15min）与取消矩阵（preview 随时、install/uninstall 仅 spawn 前、update 另允许 skill 之间，其余 `task.cannot_cancel`）全部落地并有测试；权限不足只影响单行/详情，不崩溃；基础键盘与焦点（Tab 顺序、Esc 关闭、modal focus trap、关闭后焦点返回）；CSP 最终版、opener 仅允许 https 与已知本地路径、确认无 shell/fs 插件权限；恶意 source / skill / agent / path 输入、路径穿越、Markdown XSS 与远程图片、命令注入均有拒绝测试；debug-only `SKILLS_BOX_FAKE_CLI` 注入慢/失败/取消；`list -g --json` 仅作诊断交叉校验，不参与状态真相。

**Blocked by:** 05（安装执行与校验）、06（更新检查）、07（备份与更新执行）、08（Detach）、09（彻底 Uninstall）

**Status:** resolved

- [x] 第二次启动聚焦已有窗口（对齐 M22）
- [x] 任务中退出有警告；确认后 snapshot 保留且下次启动校验（对齐 M22）
- [x] 取消矩阵全部符合规格；取消后 lock 无损（对齐 M20）
- [ ] 键盘/焦点：Tab、Esc、focus trap、焦点返回（对齐 M23） — pending user sign-off (real keyboard interaction; acceptance.md §5 M23、§7)
- [x] 恶意输入、路径穿越、symlink 攻击、Markdown XSS/远程图片、命令注入全部被拒绝（对齐 M15）
- [x] CSP 生效；opener 白名单生效；无 shell/fs 权限
- [x] 故障注入（timeout、invalid JSON、partial failure、backup failure、copy fallback）均有正确错误/警告与重试入口
- [x] `SKILLS_BOX_FAKE_CLI` 仅 debug 构建生效，release 忽略
- [x] 权限错误、单 skill 读取失败、CLI 缺失降级均有独立状态且不崩溃

## Comments

- 2026-10-28: Resolved on integration/skills-box-v1. All tickets merged; code-review fixes merged in 3e9d2ad.
- Evidence: 二次 `open` 仍为单进程/单窗口并前置原 pid（acceptance.md §4）；`security::tests::*`、`security_audit.rs`、取消矩阵与 `SKILLS_BOX_FAKE_CLI` 用例见 acceptance.md §1、§5 M15/M20/M22；clippy 0 warnings。
