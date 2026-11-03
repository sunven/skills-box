# 04: 应用更新完整验证与验收记录

Status: resolved
Execution: Completed locally (2026-11-03)
Parent: [spec.md](../spec.md)
Blocked by: 01, 02, 03

## What to verify

按完整规格验收并记录实际结果，覆盖全流程交互、Rust 并发与退出保护以及发布产物签名。运行必要的 `pnpm test`、`pnpm build`、`cargo test`，本机构建 release `.app` 并实际检查设置界面。

- 网络/清单/签名错误与未知大小进度；已下载包 busy 重试不重新下载。
- 原生写入与阻塞规划许可、后台 worker 生命周期、安装阶段新写操作和退出保护。
- StrictMode 自动检查、手动检查、提醒导航/关闭、页面切换和分阶段错误。
- 双架构完整性、版本/签名验证及稳定版发布门槛。
- 记录本机签名产物验证；线上两版本更新仅在真实分发链可访问时验收，不将 fake/backend 或本机测试等同于线上验证。

## Acceptance

- 规格验收项逐一写明通过证据或真实限制。
- 必要测试与构建通过，发布前提和已知限制写入最终结果。

## Comments

- 2026-11-03：完整规格共同理解确认后实施；不提前声称实现或线上升级已通过。

- 2026-11-03：用户已确认完整规格并授权继续，实现和本机验收已开展。

## Answer

本机验收完成，测试、构建、原生设置与实际产物验证证据已写入 verification.md。线上两版本升级因目标 GitHub 仓库不可访问而未验收；本机 Intel 交叉构建不等同于 Intel 真机运行。

验收：[verification.md](../verification.md)。
