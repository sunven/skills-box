# 应用数据管理验证记录

日期：2026-11-09。

## 自动化验证

| 检查 | 结果 |
| --- | --- |
| `pnpm build` | TypeScript 与 Vite 生产构建通过；保留现有大包体积提示 |
| `pnpm test` | 41 个文件，373 项通过 |
| 最后前端重试范围与提示修改后的定向 Vitest | 4 个文件，29 项通过 |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 705 项通过，0 失败，5 项需联网/真实 CLI 的显式测试未运行 |

最后定向前端命令：

```sh
pnpm exec vitest run src/App.app-data.test.tsx src/App.settings.test.tsx src/lib/api.test.ts src/test/fake-backend.test.ts
```

覆盖内容：

- 前端按需读取、开关保存与保存失败回退、缓存默认勾选、取消/确认清理、保护备份禁选、选择删除、任务忙碌重试、部分失败的再次确认。
- Rust 真实临时文件分类与删除、预览单次使用、文件变化拒绝、预览后新增文件保留、引用状态重查、损坏记录保护、未知数据保留、符号链接及目录替换防护。
- 缓存活动与清理互斥，操作策略按开始时捕获，旧设置默认值、凭据与未知字段兼容。
- 成功/失败/共享快照的保留边界、取消保护、任务级安装快照、结果中已删除备份链接的移除。
- 实际卸载生命周期验证操作前存在备份、成功后按设置删除、旧备份保留、关闭日志不产生新记录。
- 日志关闭与进行中策略、轮转文件数和大小上限、超大旧日志处理、日志目录符号链接拒绝。

Rust 全量测试日志：`/tmp/skills-box-app-data-cargo-test.log`（本次会话临时文件）。

## 浏览器验证

使用 agent-browser 的独立会话 `skills-box-app-data-20261109`，访问本地 `preview.html`，由 Fake backend 提供与 IPC 相同的类型契约。

- 从设置进入「应用数据」，检查占用总览、开关、备份列表和分类清理。
- 缓存清理默认预览 90 MiB；确认后总占用从 161 MiB 变为 71 MiB，可清理占用从 140 MiB 变为 50 MiB，缓存归零。
- 引用中的快照复选框禁用，其他快照仍可选择。
- 900 × 760 窗口没有横向溢出；桌面与确认弹窗截图已人工查看。
- 应用数据区域无障碍检查：0 violations、0 incomplete。确认弹窗：0 violations，1 项文字对比度检查需人工复核，截图已检查。

会话截图为临时文件，不加入产品资源：

- `/tmp/skills-box-app-data-desktop.png`
- `/tmp/skills-box-app-data-confirm.png`
- `/tmp/skills-box-app-data-900.png`

## 验证边界

浏览器验证使用 Fake backend；真实文件删除、策略和卸载生命周期由 Rust 临时目录测试覆盖。未执行发布打包、安装更新或联网真实 CLI 测试。占用与释放值为逻辑文件大小；不包括文件系统压缩、硬链接共享和分配块差异。首期没有快照恢复，日志仍是既有的部分操作诊断记录。

## 全选功能补充验证

2026-11-09：增加「全选可删除备份」，复用现有三态复选框。浏览器预览确认：全选只勾选两份可删除备份，受保护项仍禁用；单独取消一项后显示部分选中，再次全选及取消全选均正确；删除预览仅包含所选的两份备份（40 MiB）；删除后受保护项保留，全选入口因无可删除项而禁用。

本次 `pnpm build` 通过；`pnpm exec vitest run src/App.app-data.test.tsx src/App.settings.test.tsx` 的 2 个文件、15 项测试通过。

## 普通文件误识别与错误操作不匹配修复

2026-11-09：用户截图显示 `.DS_Store` 被列成受保护备份，点击打开后失败，并出现无关的「重新预览清理」。已先用测试复现：一个真实快照加 Global/Project 目录中的三个普通文件，错误列成四个备份；预览取消后再打开目录或保存设置失败，错误出现清理重试。

修复为只把实际目录纳入备份列表，普通文件归入「其他文件」并保留；打开快照接口再次验证目录。错误提示携带本次清理选择，仅清理相关失败显示重新预览；重新打开时清除旧错误。损坏 manifest 的真实目录仍受保护。

验证：`cargo test --manifest-path src-tauri/Cargo.toml --lib app_data::tests` 的 13 项通过；`pnpm exec vitest run src/App.app-data.test.tsx src/App.settings.test.tsx` 的 17 项通过；`pnpm build` 通过。Rust 用例同时检查普通文件不可伪造为打开目标、删除真实快照后仍保留普通文件；前端用例检查打开成功清除错误与既有清理重试继续可用。
