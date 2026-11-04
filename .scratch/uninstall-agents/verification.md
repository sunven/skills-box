# 批量卸载按 Agent 移除：验收记录

日期：2026-11-04

## 已实现

- 批量卸载默认进入「按 Agent 移除」，初始不勾选 Agent，支持多选；可切换「完全卸载」。
- 全局和项目范围均支持受管、未受管、仅副本技能及清单中已有的断链挂载。
- 只删除选定挂载，保留 canonical、其他 Agent 挂载、lock entry 和 baseline。共用物理目录不能独立移除；Gemini 等拥有独立目录的 universal Agent 可以选择。
- 预览逐项展示路径、副本和最后一份内容警告。删除副本需额外勾选确认，先备份；删除最后副本后不自动清理安装记录。
- 同一技能内选定的依赖链接先于副本执行；其他引用、失败后仍存在的依赖、预览后目标变化均阻止删除副本。
- 复用批量任务的串行写入、目标间停止和仅重试失败/未执行目标的流程。

## 自动验证

| 检查 | 结果 |
| --- | --- |
| `pnpm test` | 34 个文件、304 项通过 |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib` | 602 项通过，1 项既有环境相关失败，见下方 |
| `pnpm test src/App.batch.test.tsx src/App.uninstall.test.tsx` | 最终修改后 18 项通过 |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib uninstall_agents` | rustfmt 后 12 项通过 |
| `pnpm run build` | 通过；保留既有大于 500 kB 的 chunk 提示 |
| `git diff --check` | 通过 |
| 组件设计 lint | 0 FAIL、0 WARN |

Rust 唯一失败为未修改的 `agents::tests::detects_home_roots_in_catalog_order`。本机存在 `/Applications/ZCode.app`，实际列表比固定预期 `[amp, claude-code, gemini-cli, qoder]` 多 `zcode`，独立运行同一测试也复现。此次未修改 Agent 检测实现或该测试。

新增 Rust 测试使用临时目录验证真实删除和备份，覆盖两个 Scope、受管/未受管、保留 lock/baseline、最后副本、依赖顺序、外部和断链目标、预览失效、备份失败及停止/重试。界面测试使用 Fake backend。

## 浏览器验证

使用现有 preview 入口和 Fake backend，以 Vite production build 输出至本目录下被忽略的 `dist/`；由本地 preview server 提供页面。验证入口为 `preview.html?batchUninstall=agents` 或 `preview.html?batchUninstall=all`，不涉及用户的真实技能删除。

- 1440、768、390 px 三个宽度均检查；生产 inspector 无 FAIL/WARN、无横向溢出或控制台错误，详见 [report.json](production-inspect/report.json)。
- inspector 允许 `placeholder-data`：现有 Fake backend 中的 Acme Agent；允许 `covered-text`：弹窗按预期遮住背景页面标题。没有为通过检查修改背景界面。
- 桌面两种模式的 axe 检查均为 0 violations、0 incomplete。窄窗口为 0 violations，1 条 contrast incomplete 来自已滚出弹窗可视区域的文字；分别检查顶部和底部截图，确认文本可读。
- 390 px 下通过聚焦、Space、Tab、Enter 完成两项确认并提交，两个选定 Agent 分别显示「已移除」。
- 「完全卸载」显示 canonical 和全部挂载路径，要求重新勾选确认。

## 界面复核

沿用现有弹窗、字号、颜色和控件，没有重新抽取设计风格。界面回答的问题是「从哪些 Agent 移除所选技能，具体会删除哪些路径」。

1. 模式、Agent 选择和逐项预览从上到下排列，主操作文字随模式变化。
2. 内容左右对齐，无多余空白列；窄窗口沿用弹窗内部滚动与固定底栏。
3. 蓝色表示选择，黄色提示副本删除，红色用于删除操作和最后内容警告。
4. 选择两个 Agent，两个预览目标、提交数量和结果行一致。
5. 没有可操作技能的 Agent 明确显示 0；共享目录和未检测到的 Agent 展示禁用原因。
6. 路径可换行，Agent 名称与对应结果清楚，无文字碰撞。
7. 窄窗口的确认区需要滚动，已通过键盘滚入视野并成功执行；无需要修改的阻塞性布局问题。

截图：[桌面](production-inspect/desktop-fold.jpg)、[平板](production-inspect/tablet-fold.jpg)、[窄窗口](production-inspect/mobile-fold.jpg)、[窄窗口确认](narrow-confirm.png)、[移除结果](narrow-result.png)、[完全卸载](complete-uninstall.png)。
