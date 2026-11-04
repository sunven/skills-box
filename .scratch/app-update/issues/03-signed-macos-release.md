# 03: 签名与参考项目发布流程

Status: resolved
Execution: Reference implementation complete (2026-11-04); release signing and live delivery remain unverified
Parent: [spec.md](../spec.md)
Blocked by: —

## What to build

生成本项目独立 updater key pair，私钥保存在仓库外；配置 Tauri 2 公钥、稳定版 HTTPS endpoint 和签名产物。自动发布严格按照 `/Users/sunven/github/fanyifanyi` 当前实现：`v*` 标签触发单个 Apple Silicon 任务，由 `tauri-apps/tauri-action@v1` 直接发布并生成更新清单。

- 保留 macOS 12.0+、ad-hoc 签名且不公证；不复用 fanyifanyi 密钥或旧版配置脚本。
- 版本标签与 package.json、Cargo.toml、tauri.conf.json 一致，预发布不替换稳定清单。
- `latest.json` 由官方 Action 生成，含 Apple Silicon 的 HTTPS URL 与签名；发布后按参考实现检查元数据。
- 私钥/密码仅通过仓库外文件与 Actions Secrets 输入，不写仓库或日志。
- README/发布文档提供本机签名构建及仓库/Secrets 前提。当前 GitHub 目标不可访问，不能将线上发布记为已完成。

## Acceptance

- 官方 Action 参数与参考项目一致；本地脚本覆盖版本、CHANGELOG、附注标签和确认推送。
- 本机生成签名更新产物且验证通过；代码和产物不包含私钥。
- 按参考项目直接发布稳定版，随后检查 Apple Silicon 更新元数据；不保留自定义双架构草稿发布流程。

## Comments

- 2026-11-03：Q3、Q4、Q7、Q8、Q11 已确认；远程仓库仍返回 404，Actions Secrets 状态无法核实。

- 2026-11-03：用户已确认完整规格并授权继续，实现和本机验收已开展。

## Answer

2026-11-03 历史结果：独立密钥已存于仓库外，Tauri updater 公钥、HTTPS 端点、签名版本约束和双架构草稿发布流程已实现。两架构最终 .app、DMG、更新包和签名已生成，真实签名/版本/架构校验通过；篡改、错误密钥、签名版本缺失/不一致、缺失架构和错误应用版本均被拒绝。GitHub 仓库仍不可访问，远程 Secrets 与发布不在已完成结果内。

验收：[verification.md](../verification.md)。

2026-11-04：根据用户“严格按照 fanyifanyi 实现”的新指令，现采用参考项目的官方 Action 单架构直接发布及 git-cliff 发布准备流程；原双架构草稿门槛已被替代。密钥目录检查同时解析真实路径，避免符号链接将私钥写入仓库。
