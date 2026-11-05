# 失效安装记录：实现与验收记录

Date: 2026-11-05
Status: resolved
Spec: [spec.md](spec.md)

## 交付行为

异常提示中的「失效 lock 条目 → 处理」打开当前 Scope 的完整清单，默认未选。用户选择记录后，分别进入「清理记录」或「恢复技能」预览，再按可执行数量整批确认。Global 和 Project 共用已有的批量任务、写入许可、备份、逐项结果、停止与重新预览重试流程。

清理通过受限的原生写入删除选中失效键及其基线；恢复通过 `skills@latest add` 使用原 Source/ref，默认只写 canonical，可选择追加 Agent 挂载。未知 JSON 字段及未选中条目得到保留。详情见 [ADR-0027](../../docs/adr/0027-stale-record-cleanup-boundary.md)。

本次所有变更验证均使用临时 AppPaths、隔离 HOME 或共享 Fake backend，没有清理或恢复用户真实的 21 条记录。

## 自动化验证

| 命令 | 结果与证据 |
| --- | --- |
| `pnpm exec vitest run` | 38 个文件、328 项通过；[完整日志](artifacts/ui-tests.log)。新增清单/动作/发现关联集成测试 6 项。 |
| `pnpm exec vitest run src/App.batch.test.tsx src/App.stale-records.test.tsx` | 最后的滚动区键盘修正后，相关 21 项通过；[日志](artifacts/modal-tests.log)。 |
| `pnpm build` | TypeScript 与 Vite 生产构建通过；[日志](artifacts/build.log)。保留现有大于 500 kB 的 chunk 提示。 |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 638 项库测试和 33 项集成测试通过，5 项网络测试默认忽略；[完整日志](artifacts/rust-tests.log)。 |
| `cargo test --manifest-path src-tauri/Cargo.toml tasks::manager::batch::tests -- --nocapture` | 38 项批量执行测试通过；[日志](artifacts/batch-tests.log)。 |
| `cargo test --manifest-path src-tauri/Cargo.toml tasks::stale_records::tests -- --nocapture` | 6 项原生资格、备份、原子写入及警告测试通过；[日志](artifacts/native-tests.log)。 |
| `cargo test --manifest-path src-tauri/Cargo.toml --test stale_restore_real_cli -- --ignored --nocapture` | 真实 CLI 隔离恢复通过，本次 `skills@latest --version` 解析为 **1.7.6**；[日志](artifacts/cli-restore.log)。 |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`、`git diff --check` | 通过。 |

真实 CLI 测试在一次性 HOME/npm cache 下，从 `mattpocock/skills` 恢复 `code-review`，原记录使用旧目录路径。验证了新的 `SKILL.md`、只恢复 canonical、未创建 Claude 挂载、未选中 Lock entry 原值不变，以及 Lock 原文备份。测试结束后临时目录自动删除，生产执行选择器仍为 `skills@latest`。

## 验收映射

| Spec 项 | 验证方式与结果 |
| --- | --- |
| 1. 完整清单与数量 | App 测试读取全部 21 条、默认未选、全选并确认「清理 21 条记录」；不可执行条目保留并正确计算数量。浏览器完成同一流程。 |
| 2. Scope 隔离 | `cleanup_isolates_global_and_two_project_locks_and_baselines` 验证 Project 清理不影响 Global/另一个 Project；`project_restore_uses_its_own_lock_baseline_and_selected_agent` 验证 Project 恢复使用自己的 Lock、基线、canonical 与挂载，其他 Scope 的文件和记录不变。更新报告继续按 Scope 缓存。 |
| 3. 定向删除与保留 | 原生测试逐项清理、保留顶层及未选中条目内未知字段、Skill 文件与原文件权限，验证备份中的原始字节。 |
| 4. 工具依赖 | `MissingRuntime` 下整批清理成功且没有执行 CLI；App 测试在 `mutationsEnabled: false` 时完成清理。恢复预览沿用工具资格与来源错误。 |
| 5. 占用与不完整扫描 | 原生测试覆盖普通文件、目录、悬空链接与未检测到的 Codex 目录；资格检查扫描全部认可的 Agent 目录，目录读取/解析失败直接阻止变更。既有项目路径测试覆盖项目根边界。 |
| 6. 备份和原子失败 | 比较 Lock/基线备份原文，覆盖备份失败、最终检查失败，以及通过父目录权限分别注入临时文件创建失败和原子替换失败；原 Lock 字节保持不变。 |
| 7. 预览失效 | 原生测试覆盖外部 Lock 修改；App 测试覆盖预览后新增同名文件；既有批量测试覆盖项目身份及备份期间状态变化。两条连续清理/恢复测试确认本任务上一项写入不会使下一项误失效。 |
| 8. 原来源/ref/移动/挂载 | 批量测试覆盖 `release/stable`、默认分支、同名移动路径、canonical 默认值、手选 Claude 挂载；真实 CLI 另行验证移动路径与 canonical-only。 |
| 9. 不可恢复原因 | 恢复预览测试覆盖改名/删除及同名歧义，均不开始写入；Source/ref 的合法性与错误继续使用既有 `update_target` 和只读来源预览边界，不更换来源。 |
| 10. 专用授权及核实 | `RestoreAction` 使用严格 stale-only 验证，`InstallRequest.retry` 为 false；占用测试拒绝覆盖。补充测试模拟 CLI 退出成功但只留下空目录，修正后必须存在可检查的 `SKILL.md` 才能成功。 |
| 11. 任务/停止/重试 | 暂停式恢复测试确认整批持有许可，停止等待当前写入完成，后续项未执行；通用批量测试覆盖失败分类和重试目标集合。部分恢复测试确认失败后继续其他项，重试不会重放成功项或覆盖部分文件。 |
| 12. 部分写入与警告 | 基线清理失败准确返回成功加警告；空目录恢复失败保留备份与原基线，并显示核实后的 canonical/Lock 状态，不声称回滚。 |
| 13. 刷新与发现历史 | App 测试确认清理移出完整清单、恢复加入库存，保留无关更新结果；发现测试确认当前 Scope 安装状态变化、其他 Scope 保留、最后一个来源停用后读/忽略历史可在重新激活时保留。生产发现投影继续以真实 Scope 记录为准。 |
| 14. 隔离与验证 | 前端 Fake backend、后端临时 AppPaths、真实 CLI 临时 HOME 均通过；本目录保存日志和界面证据。 |

空目录恢复的回归测试先得到 `[Success, Success]`，修正后得到预期的 `[Failed, Success]`。原始失败见 [回归前日志](artifacts/incomplete-restore-red.log)，当前通过结果见批量及全套 Rust 日志。

## 浏览器检查

使用 `agent-browser` 检查现有预览页 `preview.html?staleRecords`，数据来自 Fake backend。

- 桌面 1280×800：21 条完整清单、未选默认值、清理和恢复分别预览；恢复默认 0 个 Agent。
- 窄窗口 390×844：页面 `scrollWidth = 390`、弹窗宽 342，无横向溢出；预览渲染全部 21 个目标，提交按钮和结果操作可达。
- 清理后异常数量从 27 降为 6，对应移除了 21 条测试记录；恢复结果正常显示备份入口。
- 清单打开后焦点在关闭按钮，Shift+Tab 保持在弹窗内；清理预览滚动区可通过 Tab 聚焦，PageDown 将 `scrollTop` 从 0 移至 557。
- axe 检查清理预览、清理结果及恢复预览均为 **0 violations**。滚动/覆盖区域的颜色对比度仍有 `incomplete` 项，已结合截图人工查看；不把这些项记作自动通过。报告见 [清理预览](artifacts/clean-preview-a11y.json)、[清理结果](artifacts/clean-result-a11y.json)、[恢复预览](artifacts/restore-preview-a11y.json)。

截图：[完整清单](artifacts/list-desktop.png)、[窄窗口清理预览](artifacts/clean-preview-mobile.png)、[窄窗口清理结果](artifacts/clean-result-mobile.png)、[恢复预览](artifacts/restore-preview-desktop.png)、[恢复结果](artifacts/restore-result-desktop.png)。

## 已接受的限制

- 真实网络 CLI 验证覆盖 Global/canonical-only；Project、ref、追加 Agent、停止和故障注入使用可控执行器验证。
- 未记录 ref 时恢复来源当前默认分支；即使有 ref，也不承诺恢复到原安装时的精确提交。
- 应用写入许可、预期文档检查与原子替换不提供对外部 CLI 的跨进程事务保证。
- 完成项保留；备份用于人工恢复，本轮没有一键撤销或自动回滚。
