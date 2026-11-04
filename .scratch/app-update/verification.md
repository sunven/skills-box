# 应用更新验收

Status: Passed locally; live delivery pending
Date: 2026-11-03

用户已确认 Q1–Q11 和完整设计并授权实现。本机实现与验收完成；真实 GitHub 分发和两版本升级尚未完成。

## 测试与构建

| 检查 | 结果 |
| --- | --- |
| `pnpm test` | 30 个文件、275 项通过；含 8 项应用更新流程测试 |
| `pnpm test:release` | 4 项发布门槛测试通过 |
| `cargo test --quiet` | 552 项单元测试、35 项集成测试通过；4 项现有网络/真实 CLI opt-in 测试未运行 |
| `cargo fmt --check`、`git diff --check` | 通过 |
| `pnpm build:signed --bundles app,dmg` | Apple Silicon `.app`、DMG、更新归档及签名生成成功；包含 `pnpm build` |
| `pnpm build:signed --target x86_64-apple-darwin --bundles app,dmg` | Intel 交叉构建成功，生成同类产物 |
| `codesign --verify --deep --strict` | 两架构 `.app` 均通过 |
| `prepare-release.mjs prepare` | 对最终两架构真实产物验签，核对签名版本、Mach-O 架构与归档应用版本，生成完整 `latest.json` |

前端构建保留现有大于 500 KB 的 bundle 提示。严格 Clippy 检查被现有 `discovery`、`scope`、`update/project_checks`、`update_target` 的 `collapsible_if`/`derivable_impls` 警告阻止；未将无关清理纳入此功能。

## 生命周期与交互证据

| 验收项 | 证据与结果 |
| --- | --- |
| 独立应用更新，保留 Skill Update 语义 | 独立服务、三个 IPC 命令和设置分区；不进入 Skill 任务、baseline、lock 或中断 marker |
| 检查去重、失败/当前/候选状态 | Rust 单操作许可测试；失败不显示最新版；原生启动和手动检查已实际显示清单不可用提示 |
| 1 秒启动、6 小时周期、手动重置、开发短路 | App StrictMode/计时器测试和 Rust disabled driver 测试 |
| 提醒只导航、会话关闭、下一版本重现 | App 测试和浏览器设置流程；查看没有调用安装 |
| 页面切换保留状态，未知/已知大小进度 | App 测试；百分比上限 100%，未知大小为不定进度 |
| Finished 不等于验签成功 | 暂停下载验签测试；签名失败未进入安装 |
| 下载允许其他写入，安装原子取许可 | 未登记 writer 持有真实 TaskManager 许可的暂停测试；安装忙时保留验签包，重试下载计数仍为 1 |
| 后台 worker 持有安装许可 | 请求完成/丢弃后安装仍进行，新的写许可和退出保护仍拒绝；安装失败后才释放 |
| 安装、配置和退出不竞态 | 配置/退出 guard 与安装 write gate 测试；原生关窗、Cmd+Q 和 quit_app 均接入同一保护 |
| 安装期禁用写入口，保留浏览 | App 测试覆盖 Skill、作用域、GitHub、额外 Agent、发现已读/忽略；发现详情可读且不自动确认已读 |
| 安装成功后只重试重启 | 原生测试证明重启准备失败后下载/安装次数均为 1，许可保持至重启交接 |
| 保留验签包的错误恢复 | 快照显式记录包是否已验签；App 测试证明安装失败后跨两个周期不再次检查，也不禁用更新重试 |
| 同步 IPC 可启动后台任务 | 真实原生首次启动发现无 Tokio reactor 崩溃，已改为 Tauri 运行时；普通线程回归测试及修复后的原生启动通过 |
| 只读应用目录 | 原生安装前检查只读卷并提供迁移提示；未使用真实 DMG 进行安装替换验收 |
| 受控业务停机 | 原生 restart driver 停止 discovery、取消并等待 preview 子进程，再请求重启；真实线上替换/重启尚未执行 |

浏览器预览完成提醒导航、设置和模拟安装保护检查，无 JavaScript 错误。原生 Apple Silicon 应用已实际启动，并通过 macOS 辅助功能操作进入应用更新设置、触发手动检查，确认网络/清单失败展示和恢复按钮。验收未执行任何 Skill 修改或发布动作。

截图：[可用更新设置](screenshots/settings.png)、[原生失败反馈](screenshots/native-settings.png)、[原生手动检查](screenshots/native-manual-check.png)。

## 真实签名与发布拒绝检查

使用最终真实归档以及临时副本完成以下检查；临时密钥和修改后的测试包已随临时目录清理，正式密钥未替换。

- 归档字节篡改：签名验证拒绝。
- 无关私钥签名：提交公钥验证拒绝。
- 使用正确私钥但签名版本为 `9.9.9`，或不携带版本：验证拒绝。
- 缺少 Intel 产物：不生成清单。
- 将 Apple Silicon 的有效签名归档放入 Intel 目录：实际架构检查拒绝，不生成清单。
- 将归档应用版本改为 `9.9.9`，再用正确私钥签为 `0.1.0`：归档应用版本检查拒绝，不生成清单。

最终可发布的本机验证目录为 `src-tauri/target/app-update-validation/release-ready/`，含两架构的归档、签名、DMG 和 `latest.json` 共 7 个文件。正式构建位于 `target/release/bundle/` 与 `target/x86_64-apple-darwin/release/bundle/`。这些目录被 Git 忽略，未上传。

私钥和密码位于仓库外 `~/.local/share/skills-box/signing/`，权限为 `0600`，仓库只提交公钥。发布工作流须等两架构构建、真实签名/版本/架构检查和草稿资产完整性校验通过，才公开稳定 Release；已公开 Release 不会被重跑覆盖。

## 尚未验收的远程前提

2026-11-03 再次查询 `sunven/skills-box`，认证仓库查询无法解析，最新 Release API 返回 HTTP 404。尚未确认目标公开可访问、设置 Actions Secrets、推送代码/标签、运行远程工作流或发布 Release。

因此未验收真正的旧版 → 新版下载、替换及重启，也未在 Intel 真机或 macOS 12 上运行。Intel 本机结果是交叉构建和产物检查。浏览器 fake、Rust 测试与本机产物验证均不等同于线上升级。

远程配置与真实升级步骤见 [docs/releasing.md](../../docs/releasing.md)。包含 updater 的第一个版本仍需手动安装，之后才能接受更高稳定版本的应用更新。

## 2026-11-04：严格按照 fanyifanyi 调整自动发布

以上记录对应 2026-11-03 的原双架构草稿发布实现。用户现要求严格按照
`/Users/sunven/github/fanyifanyi`，当前发布改用参考项目的单 Apple Silicon
任务和 `tauri-apps/tauri-action@v1` 直接发布流程。官方 Action 负责更新清单，
原 `prepare-release.mjs` 汇总/生成流程已移除；发布后按参考文件检查清单。
本地脚本接入参考项目的 `bump-version.sh`、`cliff.toml` 和 CHANGELOG 流程。

本次验证：

- 逐行比较工作流：除仓库名称、main 分支、Node 22 和删除未使用的认证变量外，与参考文件一致。
- `cliff.toml` 与参考项目一致；真实 git-cliff 成功生成当前版本的 CHANGELOG。
- `node --test scripts/*.node-test.mjs`：14/14 通过，覆盖真实 Cargo/Git 的发布准备、CHANGELOG 成功/失败/缺少工具及符号链接密钥目录。
- actionlint 1.7.12、Bash/Node 语法检查、版本一致性及 `git diff --check` 均通过。
- `pnpm install --frozen-lockfile --ignore-scripts` 通过，依赖锁文件未改变。
- `pnpm build:signed --target aarch64-apple-darwin --bundles dmg,app` 通过，包含前端生产构建；生成 `.app`、DMG、更新归档和签名，实际二进制为 arm64。
- 构建报告签名私钥与仓库公钥不匹配；仅比较外部 `.pub` 文件与配置，也确认不一致。未修改既有公钥或轮换密钥，当前更新归档不能记为已通过签名验收。
- 对实际生成的归档执行 `cargo run --locked --example verify-update`，返回 `UnexpectedKeyId`，确认当前私钥不能生成仓库公钥所信任的更新签名。
- `gh api repos/sunven/skills-box` 仍返回 HTTP 404，未运行线上工作流或发布 Release。

当前代码与参考发布流程已对齐；有效签名和真实分发需要匹配既有公钥的私钥、对应 Actions Secrets，以及可访问的 GitHub 仓库。
