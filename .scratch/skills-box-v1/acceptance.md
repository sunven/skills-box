# Skills Box v1 验收记录（ticket 11）

- 日期：2026-10-28（UTC；最终复跑与 release 重建同日完成）
- 分支：`worktree-agent-general-purpose-207e32c3`（基于 integration `52bfb05`）
- 环境：macOS 27.2 / arm64；Node v24.15.0；npx 11.12.1；git 2.54.0；Rust 1.98.1；pnpm 11.10.0
- 发布门槛来源：[`../global-skill-manager/issues/10-acceptance.md`](../global-skill-manager/issues/10-acceptance.md) M01–M24、P0/P1、阈值与回归配方
- 规格：[`../global-skill-manager/spec.md`](../global-skill-manager/spec.md) §10

本记录只声明实际执行过的证据。GUI 视觉、Finder 双击与主线程卡顿无法在无头会话中完成的部分均标为「待用户签收」，没有代填。

## 1. 自动化门禁

| 命令 | 结果 |
|---|---|
| `cargo test`（`src-tauri/`） | 303 passed，0 failed，2 ignored |
| `cargo clippy --all-targets` | 0 warnings（见 §6 命令记录） |
| `pnpm test` | 107 passed（14 files） |
| `pnpm build` | 通过（tsc + vite build） |
| `pnpm tauri build --bundles app` | 通过，产出 ad-hoc 签名的 `.app` |

两个 `ignored` 测试是显式网络测试，必须手动运行：

```bash
cd src-tauri
cargo test --test sandbox_real_cli -- --ignored --nocapture
```

- `real_cli_sandbox_install_update_detach_uninstall_and_regression`：真实 `npx -y skills@1.7.4` sandbox 闭环。
- `real_update_check_is_read_only`：真实 GitHub tree API 的只读更新检查。

## 2. 真实 CLI sandbox（M05–M16）

仓库：`mattpocock/skills`（38 个 Markdown skill，公开、小型）。
sandbox：临时 `HOME` + `XDG_STATE_HOME`，npm 缓存也在 sandbox 内；真实 `~` 未写入。
证据目录：`src-tauri/target/sandbox-evidence/1793180808-90031/`（`commands.log`、每一步的 JSON/stdout/stderr、lock 快照、`summary.txt`；2026-10-28 复跑，临时目录已清理）。

流程与结果（每次变更后都执行 `list -g --json` 回归）：

1. 基线：只有未受管的 `unmanaged-keep`（canonical）与 `unmanaged-agent`（Claude 目录）。
2. 单个安装 `code-review`，显式 `-a claude-code -a universal`：symlink、canonical、lock v3 正确，`list` 名称/路径一致。
3. 多个安装 `implement` + `grill-with-docs`，显式 `-a claude-code -a qoder -a universal`：两个 skill 的 canonical、Claude/Qoder 链接、lock key 全部校验。
4. copy 安装 `ask-matt`（仅 `-a claude-code`）：`mode=copy`，无 canonical，路径指向 Claude 副本。
5. 更新语义（app 的 update = 重新 `add`）：`ask-matt` 加 `-a universal` 后 copy→symlink；`installedAt` 与 `skillFolderHash` 保持不变，`mode=symlink`、canonical 出现。
6. 断链 `code-review`：只 unlink Claude 条目；canonical 与 lock 字节不变，其他挂载不受影响；`list.agents=[]` 但仍列出。
7. 卸载 `code-review`：canonical、Claude 链接、lock key 全消失；其他 skill 与未受管目录保留。
8. copy-only 安装 `diagnosing-bugs` 后卸载：无 canonical 残留，Claude 副本与 lock key 删除。
9. 卸载已转 symlink 的 `ask-matt` 后，最终只保留 `implement`、`grill-with-docs` 与两个未受管目录。

其他断言：

- 20 次真实 CLI 调用（`--version` + 5 add + 3 remove + 11 list），`commands.log` 中无 `update`、`check`、`remove --all`、`--all`、`--copy`、`-a '*'`。
- lock 始终为 `version: 3`，key 集合与预期完全一致，hash 为 40 位 hex。
- 所有安装名（`code-review`、`implement`、`grill-with-docs`、`ask-matt`、`diagnosing-bugs`）都通过 ticket 10 的 `validate_skill_names`（真实名称不被元字符规则误杀）。
- 本次复跑总耗时 116.5 s（20 次 npx 调用，含 sandbox 内 npm 包首次解析）。

更新检查只读证据（`real_update_check_is_read_only`）：真实 GitHub tree API 返回
`state=Current`、`remote_hash=fee7bb…ac05`，lock 文件字节不变、未创建 canonical，6h 缓存写入 app-data。

## 3. 100-skill 性能 fixture（M17）

测试：`src-tauri/tests/performance_fixture.rs`，临时 HOME 中生成
80 个 managed symlink + 20 个 copy-only skill，20 个来源、4 个检测到的 agent，每个 skill 2 个挂载。

| 指标 | 阈值（release .app） | debug 测试 | release 测试 |
|---|---|---|---|
| Cold inventory | < 1500 ms | 71.2 ms | 21.8 ms |
| Warm inventory（5 次中位数） | < 500 ms | 18.3 ms | 9.8 ms |
| 详情读取（5 次中位数） | < 150 ms | 2.5 ms | 1.2 ms |
| 100 条搜索 + 筛选 + 排序（Vitest，7 次中位数） | < 50 ms | 0.09 ms（本次复跑；此前 0.02 ms） | 同左（JS 测试） |

方法说明：Rust 测试在刚生成的 fixture 上取第一次读取为 cold、后续 5 次中位数为 warm；基线指纹首次写入计入 cold。表内为 2026-10-28 最终复跑值（同日早前一轮为 debug 32.8/17.6/2.3 ms、release 25.1/9.1/1.2 ms，波动来自机器负载）。release 数字用
`cargo test --release --test performance_fixture --config 'profile.release.panic="unwind"'`
复跑（产品 release profile 仍是 `panic=abort`，覆盖仅用于测试 harness）。合成 lock hash 与真实 git tree hash 不匹配，因此 fixture 首次扫描把 100 行标为「本地改动未知」——这正好覆盖异常分组路径，但不用于验证改动语义。前端数字是纯函数耗时，不含 React 渲染；启动到可交互与主线程卡顿需 release `.app` 人工确认（见 §4、§7）。

## 4. Release `.app`（M01 / M17 / M22 部分）

- 产物：`src-tauri/target/release/bundle/macos/skills-box.app`（6.30 MiB，2026-10-28 于 ticket 11 HEAD 重新构建）
- 构建：`pnpm tauri build --bundles app`，构建前清空 `APPLE_*` 凭据；bundler 明确打印
  `skipping app notarization`。
- 签名：`codesign -dv` → `Signature=adhoc`、`flags=0x10002(adhoc,runtime)`；
  `codesign --verify --deep --strict` → valid on disk / satisfies DR。
- `Info.plist`：`LSMinimumSystemVersion=12.0`、`CFBundleIdentifier=com.sunven.skills-box`、版本 0.1.0。
- `spctl -a` 对 ad-hoc 未公证包返回 rejected（v1 明确不要求公证/DMG）。
- 通过 LaunchServices `open` 启动：进程 ~150 ms 出现，窗口 ~367 ms 可见，~1.4 s 截图显示完整三栏 UI 与库存。
- 环境面板（截图 `acceptance-evidence/launch-2.png`）：`PATH · nvm`、Node v24.15.0、npx 11.12.1、
  git 2.54.0、skills 1.7.4、Agents 11、Lock v3 · 62；50 条真实库存只读加载。
- 单实例：运行中再次 `open`，进程数仍为 1、窗口数 1、原 pid 被前置。
- 证据：`src-tauri/target/acceptance-evidence/`（`launch-check.txt`、`launch-2.png`、`launch-3.png`）。

无法无头验证：真正的 Finder 双击（本次 `open` 继承了开发会话 PATH，含 nvm；真实 Finder 使用 GUI 会话 PATH）、
主线程单次卡顿 <100 ms、退出保护对话框的真实点击。均列入待签。

## 5. M01–M24 矩阵

状态含义：`自动` = 自动化证据通过；`自动+待签` = 自动化通过，但该条的 GUI/真实手工部分仍需用户确认；`待签` = 只能人工。

> 2026-10-29：v1.1 引入只读的启动/周期自动更新检查（见 [auto-update-check/spec.md](../auto-update-check/spec.md)）。M24 的 6h TTL、来源上限与并发上限继续适用；本矩阵记录的是 v1 验收结果。

| ID | P | 场景 | 结果 | 证据 / 命令 |
|---|---|---|---|---|
| M01 | P0 | Finder 启动与环境 | 自动+待签 | release `.app` LaunchServices 启动截图；环境 Node/npx/git/skills/lock 正确；Finder 双击待签 |
| M02 | P0 | 库存状态 | 自动+待签 | `cargo test` inventory/detail/baseline 用例；100-skill fixture；徽章视觉待签 |
| M03 | P0 | 列表交互 | 自动 | `pnpm test` inventory-view（搜索/来源/agent/状态/排序/计数/选中）；100 条 <50 ms |
| M04 | P0 | 详情与 Markdown | 自动+待签 | `detail::tests::*`、`markdown.test.ts`、`inventory-view`；恶意 HTML/远程图片由 `skipHtml`+`rehype-sanitize`+CSP 测试覆盖 |
| M05 | P0 | 安装单个 | 自动 | sandbox `add-single` + 回归；fake executor install 用例 |
| M06 | P0 | 安装多个 | 自动 | sandbox `add-multiple`（2 skill / 3 agent 目标）+ 回归 |
| M07 | P0 | 安装边界 | 自动+待签 | `tasks::install::validation_*`、`App.install.test.tsx`（同名/单 skill/空态/fallback/非法源）；真实源边界视觉待签 |
| M08 | P0 | 更新单个 | 自动 | sandbox `update-convert-copy`（installedAt/hash 保留）；`tasks::update::update_success_*`、备份/改动警告前端用例 |
| M09 | P0 | 更新全部 | 自动 | `tasks::update::update_all_continues_after_failures_and_skips_ineligible_skills`、`retryable_names_*`、`update_cannot_be_cancelled_*`；所有命令断言无 `update/check` |
| M10 | P0 | copy 转换 | 自动 | sandbox copy→symlink；`tasks::update::builds_add_with_current_physical_agents_and_universal_only` |
| M11 | P0 | 断链 | 自动 | sandbox detach（lock 字节不变、其他挂载不动）；`detach::tests::symlink_detach_*` |
| M12 | P0 | 断链依赖 | 自动 | `detach::tests::copy_detach_with_a_cross_agent_symlink_dependency_is_blocked_without_changes`、`uninstall` 依赖用例 |
| M13 | P0 | 卸载 | 自动 | sandbox `remove-code-review`；`uninstall::execution_tests::*`、备份用例 |
| M14 | P0 | 卸载边界 | 自动 | sandbox copy-only 安装/卸载；`cross_agent_copy_dependencies_block_*`、`uninstall_command` 无 `--all` |
| M15 | P0 | 安全 | 自动 | `security::tests::*`、`source::rejects_shell_metacharacters_*`、`install::validation_rejects_*`、`security_audit.rs`、CSP 测试 |
| M16 | P0 | 回归 | 自动 | sandbox 10 次 `list -g --json` 对比 + canonical/agent/lock v3/未受管目录 + 20 次 argv 扫描 |
| M17 | P0 | 性能 | 自动+待签 | 100-skill fixture 数字见 §3；release 启动截图 ~1.4 s 显示完整 UI（可交互性未仪器测量）；主线程卡顿待签 |
| M18 | P1 | CLI 缺失 | 自动+待签 | `resolver`/`App.test` 环境错误横幅、变更禁用；真实隐藏 Node/CLI 视觉待签 |
| M19 | P1 | 网络失败 | 自动 | `update::tests::maps_rate_limits_*`、`failures_keep_the_previous_result_*`、`App.updates.test.tsx` |
| M20 | P1 | 取消与部分失败 | 自动 | `manager` cancel/partial/timeout 用例、`App.install/update` 取消与重试用例 |
| M21 | P1 | CLI 版本 | 自动 | `cli::preview_command_is_pinned_and_read_only`、`resolver` pinned/缺失用例；sandbox 固定 `skills@1.7.4` |
| M22 | P1 | 单实例 / 退出 | 自动+待签 | `open` 二次启动同 pid、1 窗口；`App.exit.test.tsx` + 静态审计；真实退出弹窗待签 |
| M23 | P1 | 键盘 / 焦点 | 自动+待签 | `use-modal-focus.test.tsx`、install Escape/焦点用例；真实键盘待签 |
| M24 | P1 | 更新检查成本 | 自动+部分 | `update::tests::concurrency_is_capped_and_excess_sources_are_reported_as_skipped`、6h TTL、1 个真实来源只读检查；未对 20 个真实来源跑公网（限流），UI 不冻结仅由组件级 pending 状态测试覆盖，未在真实网络下验证 |

## 6. 命令记录

```bash
# Rust（含 303 个常规测试；2 个 ignored 为真实网络测试）
cd src-tauri && cargo test

# 真实 CLI sandbox + 真实只读更新检查
cargo test --test sandbox_real_cli -- --ignored --nocapture

# 100-skill fixture（debug；release 数字见 §3）
cargo test --test performance_fixture -- --nocapture

# release 数字：release profile 是 panic=abort，测试 harness 需临时覆盖为 unwind
cargo test --release --test performance_fixture \
  --config 'profile.release.panic="unwind"' -- --nocapture

# 前端
pnpm test
pnpm build

# 静态审计 + clippy
cargo clippy --all-targets

# release .app
env -u APPLE_ID -u APPLE_PASSWORD -u APPLE_TEAM_ID -u APPLE_API_KEY \
    -u APPLE_API_ISSUER -u APPLE_API_KEY_PATH -u APPLE_CERTIFICATE \
    -u APPLE_CERTIFICATE_PASSWORD -u APPLE_SIGNING_IDENTITY \
    pnpm tauri build --bundles app
```

## 7. 已知问题与待签

以下不是 P0 数据丢失/权限越界/CLI 回归，v1 按规格不阻塞：

| 项 | 类型 | 说明 | Owner | 目标版本 |
|---|---|---|---|---|
| `XDG_STATE_HOME` 分叉 | P1 已知限制 | 终端若设置该变量，CLI 把 lock 写到 `$XDG_STATE_HOME/skills/.skill-lock.json`；app 固定以 `~/.agents/.skill-lock.json` 为真相（研究 04 已记录）。当前不检测/不提示。 | @sunven | v1.1 |
| 未公证 / 无 DMG | 发布范围 | ad-hoc 签名在下载/隔离场景会被 Gatekeeper 拒绝；v1 明确不要求公证。 | @sunven | 后续版本 |
| 真实 20 来源公网检查 | 测试缺口 | 仅 fake 20 来源 + 1 个真实来源；GitHub 未认证限流 60/h，未消耗 20 个真实来源。 | @sunven | v1.1 验收复测 |
| Finder 双击 / 主线程卡顿 / 真实键盘 | 手工待签 | 无头会话无法完成；见 M01/M17/M22/M23。 | 用户 | v1 签收 |

P2（不阻塞）：`prefers-reduced-motion`、长文本截断、空态插画、app 图标品牌等按规格 §6 留待后续。

## 8. 签收状态

- P0：17/17 有自动化或脚本化证据；M01 的 Finder 双击与 M17 的主线程卡顿属于用户签收项，未代签。
- P1：7/7 有自动化或已记录限制（M24 的真实 20 来源部分列为测试缺口）。
- 未发现 P0 数据丢失、权限越界或 CLI 生态回归；sandbox 未触碰真实 HOME。
- 待用户签收：Finder 双击 release `.app` 主流程、视觉状态（M02/M04/M07）、主线程卡顿、退出保护与真实键盘操作。
