# skills.sh 市场来源实施与验收

日期：2026-11-14。实现范围遵循已确认的 [规格](spec.md) 和 [ADR-0033](../../docs/adr/0033-marketplace-catalog-and-git-installation-identity.md)。

## 实现

- 技能页内切换 SkillHub / skills.sh；首次默认 SkillHub，持久化最后选择。会话内各来源独立保留输入、查询、榜单／筛选、页码和滚动位置，专家包仍归属 SkillHub。
- Rust 适配匿名榜单、搜索和快照接口，限制响应大小并校验身份、页码和文件路径。目录身份独立于 Git 安装身份；独立网站条目只提供浏览和官网入口。
- skills.sh 提供累计榜、24 小时趋势榜、热榜和最多 200 项的搜索。保留上游顺序，失败时注明旧查询，不将其他来源或迟到查询的结果显示为当前结果。
- 详情显示 SKILL.md、只读文件树、原文及相对链接。快照整体刷新，缺失／失败／不完整有明确状态；快照 hash 不作为安装版本。
- 安装复用现有 Git 向导、命令确认和任务执行。实际预览按名字唯一匹配，只允许当前技能，当前 Scope 固定；失败不使 Discovery source 失效。安装后按现有 Git 来源刷新状态，不生成 skills.sh 市场安装记录。
- 测试与浏览器预览共用 Fake backend 的 skills.sh 目录、快照及多技能 Git 预览；Fake backend 安装记录补充 Git sourceType，以便检验安装后识别。

## 验证

| 检查 | 结果 |
| --- | --- |
| `pnpm build`（含 TypeScript） | 通过；Vite 保留现有大 chunk 提示 |
| `pnpm test:release` | 14 项通过 |
| `pnpm test` 最终全量复跑 | 47 个测试文件、486 项全部通过 |
| 前端新增 `src/App.skills-sh.test.tsx` | 19 项，覆盖来源恢复、独立状态、翻页／搜索、迟到响应、旧结果、快照刷新／缺失、无 CLI 浏览、非 GitHub 限制、名称歧义、冲突、大小写、Copy-only、全局／项目单技能安装及刷新 |
| Rust 目录／快照单元测试 | 通过，覆盖身份分离、域名来源、异常响应、重复／越界／歧义路径和超大文本 |
| 实时只读接口检查 `skills_sh_live_catalog_and_snapshot` | 三种榜单、搜索及 `find-skills` 快照全部通过；首次热榜请求发生网络错误，重试通过 |
| `real_skills_sh_selection_installs_in_global_and_project` | 通过：从真实 skills.sh 搜索选择 `vercel-labs/skills/find-skills`，Git 预览唯一匹配，隔离全局与项目分别只安装该技能；核验 SKILL.md、Git source/sourceType、唯一 lock 条目及 Claude Code 符号链接 |
| `real_cli_project_scope_add_and_remove_stay_inside_the_project` | 通过：项目安装、v2 lock 内容哈希、挂载、卸载及项目／全局隔离 |
| Rust 常规回归（排除下述两个既有环境失败） | 776 项通过，15 项网络／真实 CLI 测试默认忽略，2 项过滤 |
| 浏览器检查 | 完成来源切换、详情、相对文件链接、单技能确认及安装后状态；关闭向导焦点返回安装按钮，无浏览器错误；检查 1280、1100 和 768 宽度布局 |
| `git diff --check` | 通过 |

真实安装证据保存在 `src-tauri/target/sandbox-evidence/1794664361-79734/`，包含执行输出及全局／项目 lock。项目安装回归证据位于 `src-tauri/target/sandbox-evidence/1794662844-71598/`。测试使用临时 HOME、状态目录及项目，结束后清理临时安装。

浏览器使用 `preview.html?skillsSh` 的 Fake backend，截图展示模拟内容；真实接口和文件写入由上述 Rust／CLI 检查分别核验。截图：[详情最小窗口](detail-min-window.png)、[文件桌面窗口](files-desktop.png)、[文件窄窗口](files-narrow.png)、[安装命令确认](install-confirmation.png)。

## 既有测试限制

完整 `cargo test` 不能报告全绿，以下两项检测到宿主机器的 `/Applications/ZCode.app`，比 fixture 预期多一个 Agent：

- `agents::tests::detects_home_roots_in_catalog_order`
- `one_hundred_skill_fixture_meets_the_inventory_budget`

这两个测试、检测实现及 Agent catalog 未在本次修改；此前 [skill-publication 验收](../skill-publication/implementation.md) 已记录同样问题。本次排除这两项后运行其余完整 Rust 套件，命令为：

```sh
cd src-tauri
cargo test -- --skip agents::tests::detects_home_roots_in_catalog_order --skip one_hundred_skill_fixture_meets_the_inventory_budget
```

额外运行旧的 `real_cli_sandbox_install_update_detach_uninstall_and_regression` 时，全局单技能安装、挂载及列表读取成功，但其原有 `assert_lock` 强制要求 40 位 hash，当前 CLI 的 v3 lock 返回 64 位，因此该旧回归未完成。未修改旧断言或 Git 更新实现；新增本功能的实时安装测试直接检查真实 Git 身份、安装文件、挂载和应用 LockFile 读取，已通过。旧测试证据位于 `src-tauri/target/sandbox-evidence/1794663353-72305/`。

一次前端全量运行中，既有 `App.app-data.test.tsx` 的取消按钮焦点断言失败；该文件与应用数据对话框没有修改，单独复跑 8 项全部通过，随后最终全量复跑 486 项全部通过。

## 已接受的运行边界

匿名官网接口没有稳定版本承诺，失效时需要适配。内容快照用于浏览，安装仍取 Git 默认分支；确认页明确提示内容可能不同。非 GitHub 条目暂不提供应用内安装。
