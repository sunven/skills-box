# 专家包安装实施记录

日期：2026-11-11。
范围：[已确认规格](spec.md)、ADR-0031、ADR-0032。

## 已实现

- 技能市场增加专家包列表、搜索、分页和详情；成员链接进入现有市场技能详情。
- 安装预览解析完整成员坐标、具体版本和本地名称，展示新增/复用/补挂载，以及最终工作流和官方原文。
- 全局、项目分别保存 `.skills-box-packs.json`；其中 `packs` 保存不可随上游自动变化的快照，`workflows` 保存独立来源和基线。成员继续使用 `.skills-box-marketplaces.json`。
- 工作流 frontmatter 使用 YAML 解析器校验，保留有效 name，补齐缺失的 name/description，并追加成员 slug、完整来源、本地名称的对照。成员不改名、不改写 SKILL.md。
- 整包执行由一个后端任务持有写入许可。所有缺失成员先暂存、校验，再依次发布；全部成员及原目标挂载就绪后发布工作流。命令预览和实际调用使用同一计划。
- 普通成员失败继续后续成员；备份、作用域或关键记录失效停止。取消下载或在当前发布单元完成后停止；成功项、备份、日志和快照保留。
- 补齐只使用原快照与原 Agent 目标；保留后来独立更新的干净成员。陈旧记录、损坏内容、本地修改、名字/来源/路径冲突不被覆盖。
- 本地详情区分完整性、缺失挂载、成员版本变化和本地修改；离线仍可读取。已有清单显示包关联，详情可进入本地包快照，卸载/断开/批量移除提示受影响的全部包。
- 工作流支持现有查看、挂载、断开、原生卸载和陈旧记录清理；排除独立上游更新及更新全部。
- 忘记仅移除包分组，保留工作流/成员/挂载/来源记录。相同干净工作流可复用；内容或入口名称变化要求先处理旧入口。

## 实现位置

- `src-tauri/src/expert_packs/`：公开接口、快照和独立工作流记录、工作流适配、实际状态观察。
- `src-tauri/src/tasks/expert_packs.rs`、`tasks/manager/expert_packs.rs`：预览、任务所有权、暂存与发布、补齐、忘记。
- 现有 Lock、Backup、ReadModel、Uninstall、Stale records、Update、Batch 接入独立工作流来源。
- `src/components/expert-pack-*.tsx`、`src/lib/expert-packs.ts`：市场入口、安装确认、本地详情、包关联和影响提示。
- 现有 mutation 生命周期统一刷新当前作用域的本地包状态。

## 验证

- `pnpm exec vitest run`：前端全部 408 个测试通过。覆盖浏览/搜索、成员详情、整包单任务确认、最终工作流预览、离线快照、原目标补齐、忘记保留技能、不可安装详情，以及已有市场/单技能管理回归。
- `pnpm build`：TypeScript 与生产构建通过。构建仍报告主入口超过 500 kB 的提示。
- `cargo test --manifest-path src-tauri/Cargo.toml --quiet`：Rust 单元及默认集成测试共 744 项通过，13 项按默认配置跳过。额外边界测试覆盖全局/项目、全下载后发布、下载校验失败零发布、普通发布失败继续、备份失败停止、取消下载/发布、共享成员、独立版本偏离、忘记复用、陈旧工作流清理、记录损坏/符号链接、准备期间成员变化、缺失 SKILL.md、仅大小写不同的名称冲突，以及任务占用写入许可直到结束。修正了既有项目迁移测试在完成事件与写入许可释放之间的时序竞争，生产任务仍持有许可直到收尾完成。
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`git diff --check` 通过。
- agent-browser：1280 px 桌面视口实际操作专家包安装到 Claude Code/Codex、查看安装结果和本地快照、进入本地成员、卸载前包影响提示、卸载后未完成状态及补齐；本地详情 axe WCAG 2 A/AA 检查 0 violations。

### 真实公开协议与 CLI

运行 `skillhub_marketplace_real_cli::expert_pack_install_supplement_and_forget_keep_independent_skills`，使用此前检查过的官方 CLI 2026.8.5。所有下载、技能、Agent 目录、配置和应用记录均在自动清理的临时 HOME/data 中；没有向真实 Agent 技能目录安装。

真实安装 `hr-resume-screening`，确认并下载：

| 完整成员来源 | 本地名称 | 确认版本 |
| --- | --- | --- |
| `@clawhub_yuyang-rgb094/resume-screening` | `resume-screening` | `1.0.0` |
| `@clawhub_ayalili/resume-parser` | `resume-parser` | `1.0.1` |
| `@clawhub_wuritu/resume-screener-pro` | `resume-screener-pro` | `3.0.0` |
| `@clawhub_heyweekly/easy-recruitment` | `recruitment-assistant` | `1.0.0` |
| `@clawhub_olina1ye/applicant-screening-zh` | `applicant-screening-zh` | `1.0.1` |
| `@clawhub_mikonos/interview-designer` | `interview-designer` | `1.0.0` |

整包安装成功；移除一个 Claude Code 挂载并删除临时 CLI 后，补齐预览不含下载命令，原生补挂载成功。随后忘记包，工作流和六个成员仍在；单独卸载工作流后六个成员及其独立来源记录仍在，Git lock 未创建。

这验证了实际安装与恢复协议，没有执行成员脚本，也不将其解释为所有 Agent 上的工作流执行效果保证。真实网络用例为 opt-in，默认测试跳过其他已有网络集成用例。
