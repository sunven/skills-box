# 03: 签名与双架构发布流程

Status: resolved
Execution: Completed locally (2026-11-03)
Parent: [spec.md](../spec.md)
Blocked by: —

## What to build

生成本项目独立 updater key pair，私钥保存在仓库外；配置 Tauri 2 公钥、稳定版 HTTPS endpoint 和签名产物。补齐 `v*` 标签触发的 macOS Apple Silicon/Intel 构建、草稿产物汇总校验及稳定版发布流程。

- 保留 macOS 12.0+、ad-hoc 签名且不公证；不复用 fanyifanyi 密钥或旧版配置脚本。
- 版本标签与 package.json、Cargo.toml、tauri.conf.json 一致，预发布不替换稳定清单。
- 完整 `latest.json` 含两种架构的 HTTPS URL 和签名；验证本地对应产物、签名、公钥匹配后才发布。
- 私钥/密码仅通过仓库外文件与 Actions Secrets 输入，不写仓库或日志。
- README/发布文档提供本机签名构建及仓库/Secrets 前提。当前 GitHub 目标不可访问，不能将线上发布记为已完成。

## Acceptance

- 两架构齐全与缺失、版本错误、签名/公钥不匹配均有实际发布校验。
- 本机生成签名更新产物且验证通过；代码和产物不包含私钥。
- 工作流只有在完整验证后公开稳定版。

## Comments

- 2026-11-03：Q3、Q4、Q7、Q8、Q11 已确认；远程仓库仍返回 404，Actions Secrets 状态无法核实。

- 2026-11-03：用户已确认完整规格并授权继续，实现和本机验收已开展。

## Answer

独立密钥已存于仓库外，Tauri updater 公钥、HTTPS 端点、签名版本约束和双架构草稿发布流程已实现。两架构最终 .app、DMG、更新包和签名已生成，真实签名/版本/架构校验通过；篡改、错误密钥、签名版本缺失/不一致、缺失架构和错误应用版本均被拒绝。GitHub 仓库仍不可访问，远程 Secrets 与发布不在已完成结果内。

验收：[verification.md](../verification.md)。
