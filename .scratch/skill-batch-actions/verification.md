# Skills 多选与批量操作验证

Date: 2026-11-04
Implementation: complete
Spec: [spec.md](spec.md)
Decision: [ADR-0021](../../docs/adr/0021-batch-operation-lifecycle.md), [ADR-0022](../../docs/adr/0022-unmanaged-uninstall.md), [ADR-0023](../../docs/adr/0023-uninstall-dependency-boundary.md)

## 自动化检查

| 检查 | 结果 |
| --- | --- |
| `pnpm test` | 33 个文件，296 项通过 |
| `cargo test --manifest-path src-tauri/Cargo.toml --quiet` | 614 项通过；4 项外部网络测试按仓库设置跳过 |
| `pnpm build` | TypeScript 与 Vite 生产构建通过；Vite 有大于 500 kB 的包体积提示 |
| `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | 通过 |
| `git diff --check` | 通过 |
| 批量预览弹窗 axe 检查 | 19 条通过，0 个违规，0 个待人工核查结果 |

新增前端测试在 [App.batch.test.tsx](../../src/App.batch.test.tsx)、[batch.test.ts](../../src/lib/batch.test.ts)，覆盖筛选全选与裁剪、排序保留、Scope 切换、混合更新资格、删除确认失效、Agent 默认未选、副本删除确认、停止与重试、成功选择收尾、项目身份及项目独有 Agent 的解除挂载。

新增后端测试在 [manager/batch/tests.rs](../../src-tauri/src/tasks/manager/batch/tests.rs)，共 17 项，覆盖只读预览与跳过原因、本地 modified / unknown / current / 失效版本、删除确认和失效预览拒绝、整批写许可、目标间停止、普通失败继续、备份失败停止且保留结果、退出等待当前写入、后续内容变化跳过、备份后再校验、多 Agent 顺序、缺失项目 Agent 目录的创建与全局隔离、copy Detach 备份后变化、重试目标限制，原先跳过的更新目标不自动加入执行，以及来源 / 引用 / 技能路径变化使旧更新证据失效。追加覆盖源路径缺失及同一 Skill 的 copy/link 混合卸载，以及选中范围外依赖 Skill 仍不能绕过依赖检查。

追加的 Unmanaged 卸载验证覆盖：单项和批量的删除确认、Managed 与 Unmanaged 混合执行、无需 CLI runtime、canonical 与 Agent-only 副本备份、无 lock 时不创建 lock、存在 lock 时不改写其他条目、原始名称与 CLI 规范化名称的隔离、canonical 符号链接卸载后保留源目录、备份失败不删除、备份后新增安装记录或改变链接目标使确认失效、副本依赖拒绝，以及同名全局／项目隔离。实际文件删除均在 Rust 临时目录中完成。

同一 Skill 的副本引用回归先复现 `agent.dependency_conflict`，修正后允许 Managed、Copy-only、Unmanaged 的计划内链接和副本一起卸载，继续拒绝范围外引用。单项执行测试复现 canonical 实目录、Claude copy、Gemini link、Codex 旧目录 link 的组合，校验备份清单、所有实际路径消失、无悬空链接、其他 Skill 保留；备份后新建外部引用仍阻止 Managed 与 Unmanaged 写入。库存测试验证 Codex 旧目录仅在 skills 目录存在时可见。前端使用截图中的六个名称及 `path_missing` 状态，验证六项均能在展示实际目录并确认删除后卸载。

另用本机已缓存的 `skills@1.7.4` 在隔离的临时 HOME / XDG 目录执行六次真实 `remove -g -s <name> -y`，验证源路径已失效也能删除 canonical、Claude copy、Gemini link 和对应 lock，保留其他 Skill；该 CLI 留下的六个 Codex 旧目录链接由上述 Rust 原生清理测试覆盖。证据：[copy-links-cli-check.json](copy-links-cli-check.json)。未对用户真实 HOME 执行 CLI 删除。Rust 全量构建最初因磁盘空间不足中断，清理本项目的生成缓存后重跑通过。

## 浏览器验证

使用现有 `preview.html` 的 FakeBackend，未对真实全局或项目技能执行删除。通过真实浏览器交互完成：

- 勾选两个 Skill；表头显示半选，勾选不打开详情，四个批量按钮出现。
- 批量更新两项，结果逐目标显示成功、备份与日志入口，成功项自动取消勾选。
- 两个 Skill 同时挂载到 Qoder、WorkBuddy，预览和结果显示 4 个 Skill × Agent 目标。
- 两个 Skill 批量解除 WorkBuddy 挂载。
- 批量卸载在未勾选删除确认时无法提交；确认后显示两项卸载结果。
- 筛选两个 Unmanaged skill 后批量卸载，预览显示可执行 2、跳过 0，明确提示本地改动未知和符号链接仅删除链接；删除确认后两项均显示已卸载，刷新移除相应行。截图：[未受管卸载预览](unmanaged-uninstall-preview.png)、[未受管卸载结果](unmanaged-uninstall-result.png)。
- Unmanaged 的行菜单「卸载」可用；单项弹窗显示实际路径、「无安装记录」和链接删除说明，提交前仍要求确认。截图：[未受管单项预览](unmanaged-single-preview.png)。
- 520px × 760px 窗口的工具栏换行，弹窗与结果可用，页面无横向溢出。
- 原生复选框支持键盘空格操作。

截图：[桌面选择](desktop.png)、[更新预览](preview.png)、[更新结果](result.png)、[窄屏选择](narrow-selected.png)、[窄屏结果](narrow-result.png)、[多 Agent 预览](agents.png)、[挂载结果](attach-result.png)、[卸载确认](uninstall-preview.png)、[卸载结果](uninstall-result.png)。自动无障碍检查原始结果：[a11y-preview.json](a11y-preview.json)。

## 验证边界

真实 pinned CLI 的网络集成测试沿用仓库的 ignored 设置；此次没有启动真实 Tauri 窗口进行操作。额外的真实 CLI 卸载检查仅使用本机缓存包和临时 fixture，不访问上游。批量执行、停止、退出等待和备份边界由使用临时目录及可控制执行器的 Rust 测试验证；界面与 IPC 使用现有预览后端验证。批量任务不支持重启后恢复结果或自动续跑，与确认规格一致。
