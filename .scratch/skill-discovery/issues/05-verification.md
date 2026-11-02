# 05 — 验证与交付

Status: resolved
Type: task
Parent: [spec.md](../spec.md)

## 工作

按规格验收 Source discovery 的完整用户行为，并更新 README/必要使用说明。前序票据包含各自的接口测试，本票据集中跨层场景和交付检查，不重复实现细节测试。

- 以 temporary repo/ScriptedExecutor 和 Fake backend 模拟来源 60→61、失败恢复、同源多 Scope、记录异常、Source 停用/恢复与 Project revision 变化；不向真实上游提交测试 skill。
- 浏览器验证发现入口、搜索筛选、未读计数、详情/全部已读、忽略恢复、Scope 切换和安装预填；检查常用与窄窗口布局、键盘和状态反馈。
- 验证正常关闭取消只读进程，已完成来源结果可重启恢复；现有更新检查及安装路径仍通过回归。
- 文档明确默认分支、六小时/手动调度、首次不算新增、历史保留、Node/Git 需求、原生 API token 范围及第一版不含后台/自动安装。

## 检查

- `npm test`
- `cargo test --manifest-path src-tauri/Cargo.toml`
- `npm run build`
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- `git diff --check`

首次实现中需要的有意义失败/并发测试由对应票据完成；通过后仅在新更改、失败或未解决风险出现时扩大或重复测试。

## Comments

- 2026-11-02：交付检查已列明；本轮仅完成设计文档，未执行实现测试。

- 2026-11-02：用户确认完整规格，已可实施。

## Answer

已完成前后端回归、构建、格式/空白检查、浏览器流程和使用说明。前端 255 项、Rust 577 项通过；4 项需要真实网络/CLI 的显式 opt-in 测试未运行。完整证据与检查边界见验收记录。

验收：[verification.md](../verification.md)。
