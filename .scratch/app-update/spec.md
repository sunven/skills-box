# Skills Box — 应用更新

Status: Implemented and locally verified; live delivery awaits GitHub prerequisites
Date: 2026-11-03

## 目标

参考 `/Users/sunven/github/fanyifanyi`，为 Skills Box 应用自身提供更新功能。

## 已确认决策

- Q1：更新对象是 Skills Box 桌面应用自身。术语使用 **App update / 应用更新**；现有 **Update / Skill 更新** 继续遵循原定义。
- Q2：自动检查并提醒。用户点击更新后才下载、安装并重启；安装与重启必须与所有 Skill 写入互斥。
- Q3：首期支持 macOS Apple Silicon 和 Intel，分别生成更新包；Windows/Linux 延后。
- Q4：采用 `sunven/skills-box` GitHub Releases 的稳定渠道，补齐 `v*` 标签触发的构建、签名与更新清单流程；预发布不作为候选，开发模式不自动检查。目标仓库的可访问性尚待落实。
- Q5：生产启动延迟 1 秒检查，运行期间每 6 小时检查；设置页提供手动检查。失败不连续重试；自动失败只留在设置页，手动失败明确展示。
- Q6：应用内提醒进入设置页，展示当前版本、新版、更新说明与更新按钮；关闭只屏蔽当前会话的该版本，首期不持久化跳过版本。
- Q7：生成本项目独立更新签名密钥，私钥存放在仓库外、发布时通过 Actions Secrets 使用。macOS 继续 ad-hoc 签名且不公证，沿用当前自用范围；首次网络安装可能需用户放行。
- Q8：公开分发更新包，不依赖用户的 GitHub token；应用更新不复用 Skill 源认证。
- Q9：下载与 Skill 操作并行；安装前原子获取全局变更许可，忙时保留本次已验签的包，由用户再次点击安装。安装和重启阶段禁止新写操作、关窗和 Cmd+Q，不中断已有任务或自动排队重启。
- Q10：分阶段显示检查/下载/验签/安装错误并提供恢复入口；不误报最新版，不重复安装已成功的包。只读目录提示先安装到可写目录；首期不提供断点续传、跨重启下载缓存或额外自动回滚。
- Q11：两种 macOS 架构构建先上传草稿 Release；完整清单、版本、产物与签名验证通过后才发布稳定版。

## 设计树

- 更新对象（Q1，已确认：应用自身）
  - 自动化边界（Q2，已确认：检查与提醒自动；更新点击后执行）
    - 检查触发与提示（Q5、Q6，已确认）
    - Skill 变更任务让路与失败恢复（Q9、Q10，已确认）
  - 发布平台（Q3，已确认：macOS 双架构）
    - 首期 Apple 签名/公证约定（Q7，已确认：ad-hoc，不公证）
  - 发布渠道与分发（Q4，已确认：GitHub 稳定版）
    - 签名与公开分发（Q7、Q8，已确认）
    - 双架构发布完整性（Q11，已确认）
    - 仓库访问（事实缺口；远程发布前落实）
  - 产品选择与完整设计共同理解已确认；用户已授权并要求继续实现

## 实现前调查事实

- 现有自动更新检查针对 Managed skill，应用自身更新在 [现有规格](../auto-update-check/spec.md) 中明确不在范围。
- 当前项目使用 Tauri 2，尚未配置应用 updater，也没有 GitHub Actions 发布流程。三个版本文件均为 `0.1.0`。
- 设置页目前只有 GitHub 认证和额外 Agent。应用更新状态需跨设置页挂载/卸载保持；现有 Skill 页头的检查按钮继续使用 Skill 语义。
- 当前 macOS 最低系统版本为 12.0，使用 ad-hoc 签名。`src-tauri/tests/security_audit.rs` 明确测试不公证的现有发布约定。Updater 更新包签名与 Apple 应用签名/公证是两套不同机制。
- 安装、更新、卸载之外，原生 Attach/Detach、Relocate/Forget 及阻塞规划阶段也持有 Rust 变更许可。`get_active_mutation()` 不能覆盖这些操作；应用安装/重启需与同一许可互斥。
- `quit_app` 是用户确认后的强制退出路径，会取消正在进行的写入，不能直接作为安全更新重启入口。
- 官方 updater 原生 API 支持检查、下载、安装分离，下载成功返回已验签的更新包字节。安装可以单独获取变更许可，避免整个网络下载占用 Skill 写入许可。
- 当前 Tauri 的重启退出码不受 `ExitRequestApi::prevent_exit()` 拦截，不能依赖现有关闭确认来保护 updater 重启。安装入口必须先取得 Rust 许可，贯穿业务停机和重启交接。
- fanyifanyi 使用 Tauri 2 updater；生产启动延迟 1 秒检查一次，无周期检查。首页提示用户前往设置，用户点击后下载安装并立即重启。
- fanyifanyi 的关闭提醒仅影响当前会话；跳过版本会持久化，并连手动检查一起屏蔽该版本。本项目经 Q6 确认不采用永久跳过。
- fanyifanyi 实际 CI 只发布 macOS Apple Silicon；`update-pubkey.cjs` 写旧版 `bundle.updater`，不能照抄。真实配置为 `plugins.updater`。
- fanyifanyi 使用 GitHub Releases 的 `latest.json`，由标签触发的 Tauri Action 构建并签名。Skills Box 需要自己的发布地址和密钥。
- 本机 `gh` 可用且当前账号为 `sunven`，但 `sunven/skills-box` 的已认证仓库/Release/Secrets 查询无法解析或返回 404，匿名仓库查询也为 404。现阶段无法确认仓库存在、可见性、Release 或 Secrets 状态；不能把发布地址当作已可用。
- 本机 Tauri signer 支持将新密钥写入仓库外目录；`gh secret set` 支持从标准输入加密提交，但需先落实可访问的仓库及权限。实现阶段已生成独立密钥并存放于仓库外；未修改远程配置。

### 主要证据

- 当前项目：`src/components/settings-page.tsx`、`src-tauri/tauri.conf.json`、`src-tauri/src/tasks/manager.rs`、`src-tauri/src/commands.rs`、`src-tauri/src/lib.rs`、[ADR 0013](../../docs/adr/0013-project-root-change-serialization.md)、[ADR 0016](../../docs/adr/0016-scope-mutation-worker-ownership.md)。
- 参考项目：`src/lib/updater.ts`、`src/contexts/UpdateContext.tsx`、`src/components/update-toast.tsx`、`src/pages/Settings.tsx`、`.github/workflows/release.yml`、`src-tauri/tauri.conf.json`。

## 设计确认状态

Q1–Q11 及完整设计共同理解均已由用户确认，用户已授权实现并要求继续。下述为已确认的产品与验收规格。真实远程发布仍有仓库可访问性和 Secrets 配置的事实前提。

## 产品行为

### 检查与提示（已确认）

- 应用更新独立于 Scope 和 Skill 更新检查；不要求 Node、npx、Git 或用户 GitHub token。
- 生产启动后延迟 1 秒检查，运行期间每 6 小时检查，设置页提供立即检查。开发/浏览器预览不请求真实更新服务。
- 同时最多一个检查；StrictMode、自动/手动交叠以及页面切换不导致重复请求。
- 手动检查重置下次周期计时；检查失败不连续重试。自动错误只记入设置页，不弹打扰性错误提醒。
- 设置新增「应用更新」，显示当前版本、检查状态、当前候选、新版更新说明与操作按钮。未检查、检查失败、已是最新必须是不同状态。
- 新版应用内提醒可从任意页面进入对应设置分区。查看更新只导航，不下载。
- 关闭提醒只影响当前会话的同一版本，下一版本或下一次启动可再次提醒；手动检查不会受关闭提醒影响。
- 首期没有持久化跳过版本、自动检查开关、频率设置、系统通知或退出后后台检查。

### 下载、安装与重启（已确认）

- 用户点击「更新到 X」后下载；无已登记写任务时也必须由 Rust 在安装前原子取得全局变更许可，不能只依赖 UI 禁用。
- 下载可与 Skill 写操作并行，不占全局写入许可。必须等待官方 `download()` 返回成功且验签通过后才标记可安装；进度 Finished 回调不等于验签成功。
- 进度区分下载、验签/准备、等待可安装、安装、准备重启。总大小未知时显示不定进度；显示百分比时不超过 100%。
- 下载完成后若许可空闲，继续安装并受控重启；若忙，保留本次运行内已验签的包并提示用户任务结束后再次点击。不会强制取消 Skill 任务或自动排队重启。
- 安装工作和许可由后台 worker 持有；请求取消/页面卸载不能释放仍在运行的安装许可。
- 安装和准备重启阶段阻止新写操作，覆盖 Skill、额外 Agent、项目注册/迁移/忘记以及配置保存等写入口；只读浏览保留。
- 原生 CloseRequested、Cmd+Q 与确认退出入口均识别应用安装阶段，不能通过现有 `quit_app` 强制退出路径绕过。
- 安装成功后在 Rust 中完成 discovery 和子进程清理，随后受控重启；重启事件不会再进入普通退出分支。
- 更新状态跨设置页挂载/卸载保持，不伪装成 Skill MutationKind::Update，不写 Skill lock、baseline、操作备份或任务中断标记。

### 错误与恢复（已确认）

- 清单 404、无对应架构、解析错误和网络失败均为检查失败；只有成功完成检查且无更高稳定版时显示已是最新。
- 下载或验签失败不进入安装；区分错误原因，保留候选以供用户重试。
- 安装失败显示错误与 GitHub Release 手动下载入口。沿用插件自身的替换行为，不新增应用级自动回滚承诺。
- 应用处于只读 DMG/不可替换目录时提示先安装到可写目录，不尝试自动迁移应用。
- 成功安装的状态与失败区分；后续重启操作不能重复下载或再次安装同一包。
- 首期不做断点续传、跨重启保存已下载更新包或额外自动回滚。

## 发布与签名

### 已确认约定

- `sunven/skills-box` 的公开 GitHub Releases 为分发目标；生产端点为 `https://github.com/sunven/skills-box/releases/latest/download/latest.json`。
- 只发布稳定版自动更新；预发布不能替换正式版清单。版本标签为稳定 SemVer 的 `v<version>`，并与 package、Cargo、Tauri 版本保持一致。
- 更新清单同时包含 `darwin-aarch64` 和 `darwin-x86_64`，每项带 HTTPS 下载地址与签名。
- 使用本项目独立 updater 公钥/私钥。私钥及密码存放在仓库外，工作流通过 `TAURI_SIGNING_PRIVATE_KEY` 和 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 使用；私钥不会写入仓库、日志或产品设置。
- 配置使用 Tauri 2 的 `plugins.updater` 和 `bundle.createUpdaterArtifacts`，不复制参考项目旧版 `bundle.updater` 脚本。
- 首期保留 macOS 12.0+、ad-hoc 签名且不公证。Updater 签名独立于 Apple Developer 签名。
- 在尚无稳定版/清单时如实显示未能检查，不生成虚假的「已是最新」。

### 发布门槛（已确认）

- `v*` 标签触发两种 macOS 架构构建；两种构建均生成 `.app` 更新包、签名及便于首次安装的分发包。
- 构建产物先上传草稿 Release；汇总作业生成/校验完整 `latest.json`。
- 两种架构、版本一致性、签名、公钥匹配和产物引用校验通过后才发布稳定版；失败不公开不完整候选。
- 工作流实现与本机验收在本仓库完成。真正上线前需落实可访问 GitHub 仓库和 Actions Secrets；本次设计确认不表示已经发布任何代码或版本。

## 验收条件

- 应用更新与 Skill 更新术语和 UI 入口互不混用。
- 自动/手动检查去重，定时与开发短路正确；失败、无候选、有候选分别展示。
- 提醒只导航；关闭后同会话同版本不再显示，下一版本重新显示；切换 Scope 或设置页不丢失状态。
- 下载进度涵盖已知/未知大小；验签失败不调用安装。
- 并发测试覆盖已登记 Skill 任务及未登记的原生写入/规划许可；忙时保留已下载包，用户重试后安装。
- 安装后台 worker 在调用 future 被丢弃后仍持有许可；安装中关闭、Cmd+Q、确认退出和新写入口均受保护。
- 已安装状态不重复安装；正常完成业务停机和重启交接，错误可恢复并有手动分发入口。
- 发布校验覆盖双架构、版本不一致、缺失包/签名、公钥不匹配与稳定版门槛。
- `pnpm test`、`pnpm build`、`cargo test` 通过；本机 release `.app`/签名产物构建与设置界面实际检查完成。
- 线上两版本升级验收需要真实可访问的分发目标；若尚未发布，验证记录必须明确本机与线上的界限。

## 共同理解确认

所有产品分支和完整设计已确认，按下列票据实现并验收：

1. [原生应用更新生命周期](issues/01-native-app-update.md)
2. [设置与应用内更新提醒](issues/02-app-update-ui.md)
3. [签名与双架构发布流程](issues/03-signed-macos-release.md)
4. [完整验证与验收记录](issues/04-verification.md)

架构边界记录于 [ADR 0020](../../docs/adr/0020-native-app-update-lifecycle.md)。

## 实现与验收结果

用户确认完整设计后已完成原生生命周期、设置与提醒、签名脚本和双架构发布工作流。前端与 Rust 回归通过，Apple Silicon/Intel 实际产物通过签名、签名版本、架构和应用版本校验，完整清单已在本机生成。真实原生应用已完成启动、设置与自动/手动失败反馈检查。GitHub 目标仍不可访问，尚未配置远程 Secrets、发布 Release 或完成线上两版本升级。完整证据与边界见 [verification.md](verification.md)。
