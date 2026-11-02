# 02 — 复用只读 Source preview

Status: resolved
Type: task
Parent: [spec.md](../spec.md)

## 工作

从现有 TaskManager 预览路径提取可复用的执行/解析能力，供 Install preview 和 Source discovery 共用。来源目录只使用实际预览的名称、描述和分组；目标 Scope 的冲突由当前手动预览流程另外添加。

- 保留固定 `skills@1.7.4 add <source> -l [-g]`、ScopedCli 的执行范围、运行环境和 60 秒命令上限。
- 共用 ProcessExecutor、取消、错误分类和 parser，不新建另一套 CLI 目录枚举器。
- 手动预览继续支持现有 raw-output fallback；Source discovery 必须把解析失败视为失败，不能发布空列表。
- 只在明确的有效列表或明确的无有效 skills 输出下生成目录结果；超时/取消和失败输出不得误当完整列表。
- 不继承 settings token 或 GITHUB_TOKEN，不进入 Mutation session，不写 snapshots/安装 lock。
- 目录不携带某个 Scope 的 conflict。对大小写归并后的重名保存必要诊断，不以路径承诺能区分安装目标。

## 验证

复用既有 ScriptedExecutor 和预览输出 fixtures，验证正常/分组/空列表、输出失败、缺运行环境、取消和 timeout。现有 Global/Project 安装预览行为及字段契约必须通过回归。

## Comments

- 2026-11-02：原生归档不能完全模拟 CLI clone/blob 两套预览；以实际只读预览落实用户选择的安装范围。

- 2026-11-02：用户确认完整规格，已可实施。

## Answer

Install preview 与 Source discovery 已共用固定 CLI 的只读执行/解析 seam。明确空列表才建立空基线，非零退出和解析失败保留旧目录；CLI 环境移除 GITHUB_TOKEN/GH_TOKEN，保留手动预览 fallback。预览、取消和解析回归通过。

验收：[verification.md](../verification.md)。
