# Tauri 执行外部 CLI 与打包

Type: research
Status: resolved
Blocked by: —

## Question

查清 Tauri 2 桌面应用在 macOS 上可靠调用本机 `npx skills` 的工程方案，为架构票据提供事实基础。需要覆盖：

1. 进程执行：Rust `std::process::Command` 与 `tauri-plugin-shell` 的取舍；参数传递、环境变量、工作目录、stdin/stdout/stderr、退出码、超时、取消 / 杀死进程。
2. macOS GUI 的 PATH 问题：从 Finder/Dock 启动的 .app 不继承 shell PATH 时，如何定位 `node` / `npx` / `skills`（nvm、fnm、Homebrew、Volta、系统路径）；推荐探测顺序与用户可配置覆盖。
3. 长任务：安装 / 更新可能持续数十秒，如何把进度与日志流式传到 React（Tauri events / channel），如何处理并发任务与避免多个 CLI 同时写 lock。
4. 安全与权限：Tauri 2 capabilities、shell scope、避免命令注入、路径校验、只以当前用户身份运行；是否需要 fs / dialog 插件。
5. 打包：个人使用场景下 `pnpm tauri build` 的产物形态；不签名 / 不公证时本机运行的限制；随包 Node sidecar 与依赖系统 Node 的取舍（v1 已倾向依赖系统 Node）。
6. 可测试性：如何在 Rust 侧抽象 CLI 执行器以便单测 / 替换 fake；如何在开发环境 mock 慢任务与失败。

方法：Tauri 2 官方文档与示例、CLI 源码；可在本机做最小验证（例如从 Finder 启动的 .app 是否能找到 node），但不要改动用户 skill。把结论写成 `.scratch/global-skill-manager/research/04-tauri-cli.md`，给出推荐执行器设计与风险清单。在本票据下追加 `## Answer`，把 `Status` 改为 `resolved`。不要编辑 `map.md` 或其他票据。

## Answer

完整证据、对比矩阵与实现骨架：[research/04-tauri-cli.md](../research/04-tauri-cli.md)。

**决策要点**

1. **执行器**：用 `tokio::process::Command`，包在自定义 `ProcessExecutor` trait 后；不用 `std::process::Command` 跑长任务，不把 `tauri-plugin-shell` 暴露给前端。shell 插件 Rust API 可用，但无 timeout、取消只有 SIGKILL；若用，只作为实现细节并启用 `set_process_group` / `kill_on_drop`。
2. **PATH**：Finder/Dock 启动的 .app 实测只有 `PATH=/usr/bin:/bin:/usr/sbin:/sbin`（无 nvm/Homebrew Node）。按「用户覆盖 → Volta → fnm → nvm 默认别名/最高版本 → Homebrew → 系统 → 有界登录 shell 回退」探测，校验 Node >= 22.20、npx、git，缓存 `EnvReport`。**`npx` 的 shebang 和 npm exec 会二次按 PATH 找 node；即使把 node 绝对路径传给 npx，也必须把 `<node_dir>` 放进子进程 PATH。**
3. **调用**：固定 `<node_dir>/npx -y skills@1.7.4 ...`（spec 常量，09 可改为跟随 latest）；`env_clear` + 最小 allowlist；stdin=null；cwd 用 app 数据目录下的空目录；不设 `XDG_STATE_HOME`。
4. **长任务**：每个任务一个 `Channel<CliEvent>`，按行/50–100ms 批量推送；`TaskManager` 用 `Semaphore(1)` 串行化，single-instance 插件防双开；取消先对进程组 SIGTERM、3s 后 SIGKILL；mutating 前快照 `.skill-lock.json` 并在结束后校验（CLI 的 `writeSkillLock` 是无锁、非原子的 read-modify-write，取消/并发可能损坏）。超时：探测 5s、读 30s、写 15min 硬上限。
5. **安全**：前端只传类型化枚举，不传 argv/可执行路径/env；不添加 `shell:allow-execute/spawn`；不需要 fs/dialog 插件。自定义 Tauri command 默认对所有窗口开放，Rust 命令层就是校验边界。
6. **打包**：`pnpm tauri build --bundles app`，产物 `src-tauri/target/release/bundle/macos/skills-box.app`；建议 `signingIdentity: "-"` ad-hoc、`minimumSystemVersion: "12.0"`、不公证；本机自用可直接运行，网络分发会触发 Gatekeeper。**依赖系统 Node + npx + git，不捆绑 Node sidecar**（Node 二进制 77–114MB，且 skills 还要调 git）。
7. **可测试性**：`EnvResolver`（文件系统/版本探测 seam）、`CliInvocation`（纯 argv/env 构造）、`ProcessExecutor`（Tokio 实现 + Scripted fake）、`TaskManager`（假时钟/事件）、前端 `CliTransport`（假事件流 + `mockIPC`）。开发期用 `SKILLS_BOX_FAKE_CLI=1` 模拟慢/失败/取消，仅 debug 编译。
