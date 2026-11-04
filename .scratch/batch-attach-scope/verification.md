# 全局技能批量复制到项目验证

Date: 2026-11-04
Implementation: complete
Spec: [spec.md](spec.md)
Decision: [ADR-0024](../../docs/adr/0024-copy-global-skills-to-project.md)

## 自动化检查

| 检查 | 结果 |
| --- | --- |
| `pnpm test` | 34 个文件，300 项通过 |
| 最终界面与结果回归 | `App.copy-to-project.test.tsx`、`App.batch.test.tsx`、`lib/batch.test.ts`、`lib/mutation.test.ts`：27 项通过 |
| `pnpm build` | TypeScript 与 Vite 生产构建通过；沿用大于 500 kB 的包体积提示 |
| Rust 复制到项目专项 | 10 项通过，使用真实临时目录、安装记录和符号链接 |
| Rust 回归（下述两项环境相关测试除外） | 622 项通过；4 项外部网络 / 真实 CLI 测试沿用仓库 ignored 设置 |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --check` | 通过 |
| `git diff --check` | 通过 |
| 结果弹窗 axe 检查 | 19 条通过，0 个违规，0 个待人工核查结果 |

最终 Rust 回归命令：

```sh
cargo test --manifest-path src-tauri/Cargo.toml -- \
  --skip agents::tests::detects_home_roots_in_catalog_order \
  --skip one_hundred_skill_fixture_meets_the_inventory_budget
```

本机安装了 `/Applications/ZCode.app`，现有检测器因此多返回一个 `zcode` Agent。以下两项测试预期仅有 fixture 中的四个 Agent，单独运行均复现该预期差异；未修改检测规则或这些既有测试：

- `agents::tests::detects_home_roots_in_catalog_order`
- `one_hundred_skill_fixture_meets_the_inventory_budget`

## 新增验收覆盖

前端 [App.copy-to-project.test.tsx](../../src/App.copy-to-project.test.tsx) 覆盖项目必选、选择新目录后取消不注册、零 Agent 复制、确认后注册并保持全局、主动查看项目、复制与挂载分别计数、目录切换清空 Agent、复用同名受管技能，以及工具栏仍只有原有四个批量操作入口。

后端 [manager/batch/tests.rs](../../src-tauri/src/tasks/manager/batch/tests.rs) 新增 10 个复制到项目场景：

1. 预览及刷新不注册、不创建技能内容；确认后独立复制，保留全局来源，并生成项目安装记录、本地基线和指向项目内容的挂载。
2. 零 Agent 可复制当前本地改动；重复操作复用相同内容，不改写已有管理记录，内容不同跳过。
3. 同名未受管目的内容保持未受管；来源外部链接拒绝。
4. 确认前来源改变导致预览过期，不注册或写入。
5. 技能内部有效相对链接保留；来源根目录为链接时复制实际内容。
6. 确认后 Agent 目标被外部占用，显示变化后跳过，保留已完成副本并继续其他技能。
7. 实际挂载写入失败时保留副本，继续其他技能；重新预览后仅补失败挂载，不重复复制或改写项目安装记录。此场景通过目录权限制造真实写入失败；能绕过权限的特权测试进程不执行该场景，本机已完整执行。
8. 目标间停止保留成功内容及项目注册；整批持有写入许可，阻止其他全局 / 项目写入及项目重新定位 / 遗忘。
9. 写入基线失败恢复已有项目记录并移除本次未完成副本。
10. 可读取的中文文件名照常复制并保留来源、本地基线；项目版本算法无法可靠对照时预览提示未知，安装记录不声称一个虚构版本。

## 浏览器验证

使用 `agent-browser` 的独立会话和现有 `preview.html` / Fake backend，完成全局选择、切换项目、选择已有目录、勾选 Claude Code、确认两个目标、查看结果，再主动点击「查看项目」。复制与挂载分别显示成功，结果仍覆盖在全局列表上；主动查看后项目列表显示独立技能和项目挂载。

- [桌面预览](project-preview.png)：1280 × 900，复制与挂载共两个目标。
- [窄屏预览](project-preview-narrow.png)：390 × 844，零 Agent 复制仍可提交。
- [结果](project-result.png)：独立复制及挂载结果、完整项目目录和查看项目入口。
- [主动查看项目](project-after-view.png)：切换后显示项目内技能。
- [可访问性结果](a11y-dialog.json)：0 个违规。

## 验证边界

文件写入验证使用 Rust 临时目录，界面验证使用 Fake backend；此次未在真实 Tauri 窗口对用户技能执行批量复制。上游网络与真实 pinned CLI 集成测试沿用仓库的 ignored 设置。现有项目更新算法对非 ASCII 文件名的比较能力限制仍在，内容和本地基线可以独立管理，预览会提示更新检查未知。
