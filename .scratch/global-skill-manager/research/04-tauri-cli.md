# Tauri 2 在 macOS 上执行 `npx skills` 的工程方案

Ticket: `04-tauri-cli` · 日期：2026-10-28 · 环境：macOS 27.2 (arm64)、rustc 1.98.1、Node v24.15.0 (nvm) / v26.12.1 (Homebrew)、`skills` 1.7.4（npx 缓存）

> 标注约定：**[已验证]** 为官方文档、源码或本机实验直接支持；**[推荐]** 为工程决策；**[未验证]** 为需要实现阶段确认的推断。

## 结论（给 09 的决策输入）

1. **执行器用 `tokio::process::Command`，包在自定义 `ProcessExecutor` trait 后面；不要用 `std::process::Command` 直接跑长任务，也不要把 `tauri-plugin-shell` 暴露给前端。** 需要流式输出、超时、优雅取消和可测试性；shell 插件的 Rust API 可用，但没有超时和优雅终止，且它解决不了 PATH 问题。[推荐]
2. **PATH 不能依赖继承。** Finder/Dock 启动的 .app 只拿到 `/usr/bin:/bin:/usr/sbin:/sbin`，看不到 nvm/Homebrew 的 Node。用「用户覆盖 → 已知安装位置探测 → 有界登录 shell 回退」三层策略，并校验 Node >= 22.20、`npx`、`git`。[已验证 + 推荐]
3. **调用形式固定为 `<node_dir>/npx -y skills@<pinned> ...`，同时把 `<node_dir>` 放进子进程 PATH。** 只把 `node` 绝对路径传给 `npx` 不够：`npx` 的 shebang 和 npm exec 还会按 PATH 找 `node`。[已验证]
4. **长任务用 Tauri Channel 流式传输**，按行/批量发送；全局单写者队列 + 单实例插件；取消先 SIGTERM 进程组、再 SIGKILL；mutating 任务前备份 `.skill-lock.json`。[推荐]
5. **安全边界放在 Rust 命令层**：前端只能传类型化枚举，不能传可执行文件、任意 argv 或 env；不添加 `shell:allow-execute/spawn` 权限。自定义 Tauri command 默认对所有窗口开放，必须自行校验。[已验证 + 推荐]
6. **打包：`pnpm tauri build --bundles app`，ad-hoc 签名（`signingIdentity: "-"`），不公证，依赖系统 Node + npx + git；不捆绑 Node sidecar。** 本机自用可直接运行；一旦经网络分发会触发 Gatekeeper。[已验证 + 推荐]

---

## 1. 本机事实核查

### 1.1 Finder 启动的 GUI 环境

用最小 Swift `.app` 通过 Finder / `env -i open` 启动，实际写出的环境：

```text
PATH=/usr/bin:/bin:/usr/sbin:/sbin
HOME=/Users/sunven
TMPDIR=/var/folders/.../T/
SHELL=/bin/zsh
USER=sunven
LOGNAME=sunven
LANG=<nil>
```

- `launchctl print gui/$(id -u)` 的 `environment` 只有 `SSH_AUTH_SOCK`，没有 PATH。[已验证]
- 这与 Tauri 官方文档的说明一致：*"GUI apps on macOS and Linux do not inherit the `$PATH` from your shell dotfiles (`.bashrc`, `.bash_profile`, `.zshrc`, etc). Check out Tauri's fix-path-env-rs crate to fix this issue."* —— [Tauri macOS Application Bundle](https://v2.tauri.app/distribute/macos-application-bundle/)
- 因此本机从 Finder 启动时，`node`/`npx` 均不可见；`nvm`/Homebrew 的路径完全不在 PATH 中。[已验证]

**测试陷阱：** 在终端里执行 `open App.app` 会把调用者的环境传给新进程（本机实验：通过 Orca 终端 `open` 的探针拿到了完整 shell PATH）。只有 Finder/双击，或 `env -i HOME="$HOME" PATH=/usr/bin:/bin:/usr/sbin:/sbin open ...` 才复现 GUI 的 PATH。开发时 `pnpm tauri dev` 也继承终端 PATH，**会掩盖这个 bug**。验收必须用 Finder 启动打包后的 `.app`。[已验证]

### 1.2 本机 Node / skills 布局

| 项 | 结果 |
|---|---|
| `which node`（当前 shell） | `~/.nvm/versions/node/v24.15.0/bin/node` |
| `which -a node` | nvm v24.15.0、`/opt/homebrew/bin/node`（v26.12.1） |
| nvm 默认别名 | `~/.nvm/alias/default` 内容为 `24` |
| `skills` 全局命令 | **不存在**；只有 npx 缓存 `~/.npm/_npx/*/node_modules/skills` |
| `npx --no-install skills --version` | `1.7.4` |
| `skills` engines | `node >= 22.20.0`（缓存包 `package.json`） |
| `skills` 依赖 | `tar`、`yaml`；通过 `simple-git` / `execFile("git", ...)` 调用系统 `git` |
| `git` | `/usr/bin/git`，Apple Git 2.54.0，Xcode CLT 已安装 |
| 登录 shell `zsh -ilc 'echo $PATH'` | 0.50s，解析到 nvm v24.15.0 的 node/npx；`env -i` 下同样成立 |

`skills` 缓存包：`/Users/sunven/.npm/_npx/0757c8f0298e564a/node_modules/skills`。CLI 入口 `bin/cli.mjs` 的 shebang 是 `#!/usr/bin/env node`。[已验证]

### 1.3 npx 的两段式解析

在模拟 GUI 的最小环境中：

```bash
# 直接跑绝对路径的 npx：失败
env -i HOME="$HOME" PATH=/usr/bin:/bin:/usr/sbin:/sbin \
  /opt/homebrew/bin/npx --no-install skills --version
# -> env: node: No such file or directory (127)

# 用 node 显式跑 npx-cli.js：仍然失败
env -i HOME="$HOME" PATH=/usr/bin:/bin:/usr/sbin:/sbin \
  /opt/homebrew/bin/node /opt/homebrew/lib/node_modules/npm/bin/npx-cli.js \
  --no-install skills --version
# -> env: node: No such file or directory (127)

# PATH 里加入 node 所在目录：成功
env -i HOME="$HOME" PATH="$HOME/.nvm/versions/node/v24.15.0/bin:/usr/bin:/bin" \
  "$HOME/.nvm/versions/node/v24.15.0/bin/npx" --no-install skills --version
# -> 1.7.4
```

原因：`npx` 是 `#!/usr/bin/env node` 脚本；`npx-cli.js` 最终走 `npm exec`，后者会以 PATH 查找并启动包的 bin（`skills` 的 bin 也是 `env node`）。**结论：无论用哪种方式启动，子进程 PATH 都必须包含所选 Node 的 `bin` 目录。** [已验证]

---

## 2. 进程执行：三种方案对比

| 维度 | `std::process::Command` | `tauri-plugin-shell` (Rust API) | `tokio::process::Command`（推荐） |
|---|---|---|---|
| 阻塞模型 | 同步阻塞；在 async command 中需 `spawn_blocking` + 线程读管道 | 异步，`spawn()` 返回 `Receiver<CommandEvent>` | 异步，`spawn()` 返回 `Child` |
| 流式输出 | 自己起线程读 stdout/stderr，处理粘包/分行 | 内建按行（`\n` / `\r`）切分；`set_raw_out(true)` 可拿原始字节 | 自己用 `BufReader::lines()` / `read_until` 并发读取 |
| 退出码 | `ExitStatus` | `CommandEvent::Terminated(TerminatedPayload { code, signal })` | `ExitStatus`（`code()` / `signal()`） |
| 环境变量 / cwd | `env_clear` / `envs` / `current_dir` | 同名 API | 同名 API |
| 取消 / 杀死 | `Child::kill()` 只杀直接子进程；需自行 `process_group(0)` + `killpg` | 2.6.0 支持 `set_process_group(true)`、`kill_on_drop(true)`；`kill()` 对进程组发 **SIGKILL** 并等待 | `process_group(0)` + `kill_on_drop(true)`；可自行 SIGTERM→SIGKILL |
| 超时 | 无，需 `wait_timeout` 或线程 | 无，需 `tokio::time::timeout` / `select!` | 无内建，需 `tokio::time::timeout` / `select!`（可控性最好） |
| 权限 / scope | 无 Tauri ACL，Rust 直接执行 | **Rust API 不经过 scope**；scope 只在 IPC 的 `execute`/`spawn` 命令里检查 | 无 |
| 前端暴露风险 | 无 | 若同时启用 JS API，需要 `shell:allow-execute` / `shell:allow-spawn` + 命令/参数 scope；等于把 `npx` 暴露给 webview | 无 |
| 可测试性 | 需包装；阻塞接口不适合 fake | `Command` 是具体类型，`ShellExt` 依赖 `AppHandle`，单测需额外抽象 | 命令构造与执行可分离，最容易注入 fake |
| 依赖 / 版本 | 标准库 | 增加 `tauri-plugin-shell`（2.6.0 文档要求 Rust >= 1.90、Tauri 2.12） | 增加 `tokio`（Tauri 本身已用 tokio） |

关键源码事实：

- shell 插件的 scope 检查在 IPC 命令的 `prepare_cmd()` 中；Rust 的 `Shell::command()` 直接 `Command::new`，不查 scope —— [commands.rs](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/shell/src/commands.rs)、[lib.rs](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/shell/src/lib.rs)。
- `CommandChild::kill()` 在启用 `process_group` 时对进程组发 `SIGKILL`（`libc::killpg(pgid, SIGKILL)`），没有 SIGTERM 阶段 —— [process/mod.rs](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/shell/src/process/mod.rs)、[CommandChild API](https://docs.rs/tauri-plugin-shell/latest/tauri_plugin_shell/process/struct.CommandChild.html)。
- `Command` 提供 `set_process_group`、`kill_on_drop`，但没有 timeout —— [Command API](https://docs.rs/tauri-plugin-shell/latest/tauri_plugin_shell/process/struct.Command.html)。
- tokio 的 `Command` 提供 `process_group(0)`、`kill_on_drop(true)`、`env_clear`、`pre_exec` —— [tokio::process::Command](https://docs.rs/tokio/latest/tokio/process/struct.Command.html)。

### 推荐

**用 `tokio::process::Command` 实现 `TokioExecutor`**：

- 不引入 shell 插件，不增加 `shell:` 权限面。
- 自己控制 `env_clear` + 最小环境、进程组、SIGTERM→SIGKILL、超时。
- 通过 trait 注入，Rust 单测不需要 Tauri runtime。

`tauri-plugin-shell` 不是坏选择，但它的价值主要是「已经写好的行切分 + 进程组 + JS API」；我们只用 Rust 侧，收益不足以抵消新增依赖和权限误解。若实现阶段想省行切分代码，可以只用它的 `process` 模块（Rust API），**绝不要**给前端 `shell:allow-execute`。

---

## 3. PATH：定位 node / npx / skills

### 3.1 探测顺序（推荐）

1. **用户设置覆盖**：完整 `node` 路径（以及可选额外 PATH 目录）。最高优先级；校验失败直接报错，不静默回退。
2. **开发/测试环境变量**：`SKILLS_BOX_NODE`（仅 debug 或显式允许）。
3. **Volta**：`$VOLTA_HOME/bin/node`，默认 `~/.volta/bin/node`。Volta 文档明确 `VOLTA_HOME=$HOME/.volta`、`$VOLTA_HOME/bin` 在 PATH 前部 —— [Volta Getting Started](https://docs.volta.sh/guide/getting-started)。
4. **fnm**：`$FNM_DIR/aliases/default/bin/node`；macOS 默认数据目录是 `~/Library/Application Support/fnm`，也兼容 `$XDG_DATA_HOME/fnm`、`~/.local/share/fnm`、legacy `~/.fnm`。fnm 源码确认 `aliases/default` 和 `~/.fnm` legacy 逻辑 —— [fnm README](https://github.com/Schniz/fnm)、[directories.rs](https://github.com/Schniz/fnm/blob/master/src/directories.rs)。
5. **nvm**：`$NVM_DIR/alias/default`（默认 `~/.nvm`，或 `$XDG_CONFIG_HOME/nvm`）；把 `24` 这类别名解析到 `$NVM_DIR/versions/node/v24.* /bin/node`；再按 semver 从高到低扫描 `$NVM_DIR/versions/node/*/bin/node`，选 >= 22.20 的最高版本。nvm 文档确认 `NVM_DIR` 与 `versions/node/vX.X.X` —— [nvm README](https://github.com/nvm-sh/nvm#installing-and-updating)。
6. **Homebrew**：`/opt/homebrew/bin/node`（Apple Silicon），`/usr/local/bin/node`（Intel）。Homebrew 官方文档确认两个前缀 —— [Homebrew Installation](https://docs.brew.sh/Installation)。
7. **系统 / 官方 pkg**：`/usr/local/bin/node`、`/usr/bin/node`。
8. **其他 shim（低优先级）**：`~/.asdf/shims/node`、`~/.local/share/mise/shims/node`、`~/.local/bin/node`。本机 shell PATH 里出现过 asdf/mise，但 shim 实际不存在，所以必须做存在性检查。
9. **有界登录 shell 回退**：以上都失败时，执行 `$SHELL -ilc 'printf "%s" "$PATH"'`（macOS 默认 `/bin/zsh`），3 秒超时，只取 PATH，再按上面的规则在其中找 node。不要用 shell 执行真正的 CLI 命令。

**不要用 `which node` / `command -v node` 作为主策略**：它查的就是错误继承来的 PATH。可以用它做「登录 shell 回退」内部的 PATH 搜索。

### 3.2 每个候选的校验

对候选 `node`：

1. `symlink_metadata`/`metadata` 确认是文件、可执行；`canonicalize` 后记录。
2. 用最小环境执行 `<candidate> --version`，5 秒超时，解析 `vX.Y.Z`，要求 `>= 22.20.0`（`skills` 的 engines 下限）。
3. 确认同目录有 `npx`（或能找到 `<prefix>/lib/node_modules/npm/bin/npx-cli.js`）；没有 npx 时返回 `NpxMissing`，不要盲目继续。
4. 确认 `git`：优先 `/usr/bin/git`、`/opt/homebrew/bin/git`、`/usr/local/bin/git`，或登录 PATH 中的 git；执行 `git --version`，3 秒超时。`skills` 通过 `simple-git` 调 git，安装/更新必须有 git。
5. 记录 `EnvReport`：`node_path`、`node_version`、`source`（volta/fnm/nvm/homebrew/system/login-shell/override）、`npx_path`、`git_path`、`path_used`、`warnings`、被拒绝候选的原因。

探测结果缓存到 session；设置变更、CLI 启动失败、用户点「重新检测」时重跑。UI 的「环境检测」页展示来源和版本，便于诊断。

### 3.3 子进程环境（推荐）

```
env_clear()
HOME, USER, LOGNAME, TMPDIR, LANG/LC_ALL（缺失时补 en_US.UTF-8）
PATH = <node_dir>:<git_dir>:<homebrew>/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin
SHELL（可选，某些工具用）
NO_COLOR=1
DISABLE_TELEMETRY=1
npm_config_yes=true
npm_config_update_notifier=false
npm_config_fund=false
npm_config_audit=false
VOLTA_HOME / NVM_DIR / FNM_DIR（仅在使用对应 shim 时需要）
```

- **不要继承** `NODE_OPTIONS`、`NODE_PATH`、`npm_config_prefix`、`GITHUB_TOKEN`/`GH_TOKEN`、`GIT_*`、代理变量。v1 只支持公开仓库；token 泄漏和无意的 npm 配置覆盖都弊大于利。
- **不要设置 `XDG_STATE_HOME`**：`skills` 在 `XDG_STATE_HOME` 存在时把 lock 写到 `$XDG_STATE_HOME/skills/.skill-lock.json`，否则写 `~/.agents/.skill-lock.json`（源码 `getSkillLockPath()`）。GUI 环境本来没有它，保持不设置，确保与 v1 的 `~/.agents` 真相一致。[已验证]
- `cwd` 用一个 app 数据目录下的空目录（如 `~/Library/Application Support/com.sunven.skills-box/cli-cwd`），避免 CLI 扫描到某个项目目录的本地 skill/agent。不要用 `/`。
- `stdin` 设为 `null`，防止 CLI 意外等待输入；同时始终带 `-y`。

### 3.4 npx 调用形态

```text
program = <resolved_node_dir>/npx
args    = ["-y", "skills@1.7.4", <subcommand>, ...]
env.PATH 包含 <resolved_node_dir>
cwd      = <app_data>/cli-cwd
stdin    = null
```

- 版本固定为已验证的 `1.7.4`（`skills` 当前本机版本）。是否固定由 09 决定；执行器把 package spec 做成常量，支持后续升级。
- 检测阶段用 `npx --no-install skills --version`（不触网，缓存命中才成功）；失败时区分「Node/npx 存在但 CLI 未缓存」，首次安装/更新时用 `-y` 允许下载。冷缓存会访问 npm registry，UI 要有「准备 CLI」阶段。
- 不要用 `sh -c` 拼接命令；不要把用户输入的 source/skill/agent 放进一个字符串。

### 3.5 关于官方 `fix-path-env-rs`

Tauri 文档推荐 [fix-path-env-rs](https://github.com/tauri-apps/fix-path-env-rs)。它的实现是：

- 用 `$SHELL -ilc 'echo ...; env; ...'` 读取**交互式登录 shell**的完整 env，然后 `std::env::set_var` 回当前进程；`fix()` 只设置 PATH。
- 优点：覆盖 nvm/fnm/asdf/mise 等所有 shell 配置，官方背书。
- 风险：没有超时；用户 `.zshrc` 慢/卡住/等待输入时 app 启动会被阻塞；会覆盖全局 env；`cargo search fix-path-env` 当前找不到该 crate（README 让用 git dependency），构建可复现性稍差。

**推荐**：保留同样的「登录 shell」思路，但自己实现**有界**版本（3s 超时，只解析 PATH，失败就继续报错）。如果实现阶段不想写这段，可以在 `main()` 里调用 `fix_path_env::fix()`，但必须接受启动延迟和潜在卡死，并加超时看门狗（例如放到线程 + `recv_timeout`，超时后不采用其结果）。[推荐 + 未完全验证 fix-path-env 在异常 rc 下的行为]

---

## 4. 长任务：流式输出、取消、超时、并发与 lock

### 4.1 事件系统 vs Channel

Tauri 官方文档明确：

- Event system *"was designed for situations where small amounts of data need to be streamed or you need to implement a multi consumer multi producer pattern"*，*"not designed for low latency or high throughput"*，payload 是 JSON 字符串，且没有 capabilities 细粒度控制。
- Channels *"designed to be fast and deliver ordered data. They are used internally for streaming operations such as download progress, child process output and WebSocket messages."*

来源：[Calling the Frontend from Rust](https://v2.tauri.app/develop/calling-frontend/)。shell 插件的 JS API 内部也正是用 `Channel<JSCommandEvent>` 传子进程事件 —— [commands.rs](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/shell/src/commands.rs)。

**推荐**：每个任务一个 `tauri::ipc::Channel<CliEvent>`，由前端在 `invoke` 时传入；不要把日志走全局 event。全局 event 只用于少量、多消费者的小状态（如果确实需要）。

前端：

```ts
import { invoke, Channel } from '@tauri-apps/api/core';

const onEvent = new Channel<CliEvent>();
onEvent.onmessage = (e) => { /* append log / update phase */ };
const taskId = await invoke<TaskId>('cli_run', { req, onEvent });
```

Rust 命令签名：

```rust
#[tauri::command]
async fn cli_run(
    state: tauri::State<'_, AppState>,
    req: CliRequest,                 // 类型化枚举，不接受任意 argv
    on_event: tauri::ipc::Channel<CliEvent>,
) -> Result<TaskId, CliError>;

#[tauri::command]
async fn cli_cancel(state: tauri::State<'_, AppState>, task_id: TaskId) -> Result<(), CliError>;

#[tauri::command]
async fn cli_detect(state: tauri::State<'_, AppState>) -> Result<EnvReport, CliError>;
```

### 4.2 事件模型（推荐）

```rust
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "event", content = "data")]
enum CliEvent {
    Started { task_id: TaskId, command: String, argv_display: String, node_version: String },
    Phase { task_id: TaskId, phase: Phase },          // Preparing / Running / Cancelling
    Log { task_id: TaskId, seq: u64, stream: Stream, lines: Vec<String> },
    Finished { task_id: TaskId, code: Option<i32>, signal: Option<i32>, duration_ms: u64, outcome: Outcome },
    Failed { task_id: TaskId, error: CliError },
}
```

- **按行**读取 stdout/stderr（同时处理 `\r` 进度行和 `\n`），保留最后不完整行到下一次。
- 每 50–100ms 或每 20 行批量发送一次 `Log`；单行上限 64KB，超出截断并标记。不要每个字节发一次 IPC。
- 序号 `seq` 单调递增，前端按序追加；Channel 本身保证顺序。
- 全量日志写入 `~/Library/Logs/com.sunven.skills-box/tasks/<taskId>.log`（或 app log dir），内存只保留环形缓冲（如 500 行），用于诊断和「复制诊断信息」。
- 任务完成/失败后发送 `Finished`/`Failed`，前端据此收敛状态；不要只依赖 invoke 的 Promise（长任务期间用户可能切换页面）。

**Channel 关闭的处理**：窗口刷新/关闭后 `send` 可能失败；**不要因此杀死 mutating 任务**。任务继续跑并写日志，前端重连后通过 `cli_task_snapshot(task_id)` 取回状态/末尾日志，再决定是否重新订阅。若 app 退出，则走取消流程。[推荐]

### 4.3 超时与取消

| 操作 | 超时 | 处理 |
|---|---|---|
| `node --version` / `git --version` | 5s / 3s | 超时 kill，候选判定失败 |
| `npx --no-install skills --version` | 10s | 区分未缓存 / 卡住 |
| `list` / `check` | 30s | 超时取消，报 `Timeout` |
| install / update / remove | 硬上限 15min；2min 无输出仅提示，不杀 | 用户可随时取消；npm 下载可能长时间静默 |

- 用 `tokio::select!` 同时等 `child.wait()`、取消 token、超时计时器；不要只用 `tokio::time::timeout` 包住整个 future 后直接丢弃 child（`kill_on_drop(true)` 只杀直接子进程，不会杀 npm 再拉起的 git）。
- **取消顺序**：`killpg(pgid, SIGTERM)` → 等 3s → `killpg(pgid, SIGKILL)` → `wait()` 回收。报告 `Cancelled` 而不是 `Failed`。
- 子进程必须 `process_group(0)`（新进程组，pgid = child pid），否则 `npx -> npm -> skills -> git` 的孙进程会残留。这是长任务取消的核心。
- app 退出（`RunEvent::ExitRequested` / `Exit`）时遍历运行中任务，先取消并等待约 2s，再强杀；`kill_on_drop(true)` 作为兜底。
- 强制退出（Force Quit）无法保证清理；下次启动可检查任务 pid 文件并提示/清理残留（可选，v1 可只记录风险）。[推荐]

### 4.4 并发与 lock 安全

`skills` 1.7.4 的 `writeSkillLock()` 是**普通 read-modify-write**：

```js
const lock = await readSkillLock();
lock.skills[skillName] = { ... };
await writeFile(lockPath, JSON.stringify(lock, null, 2), "utf-8");
```

没有文件锁、没有临时文件 + rename。两个 CLI 进程并发写会丢更新；进程在 `writeFile` 中途被 SIGKILL 可能留下截断/非法 JSON；`readSkillLock()` 解析失败会返回空 lock，下一次写入可能覆盖全部条目。源码位置：本机 `~/.npm/_npx/0757c8f0298e564a/node_modules/skills/dist/cli.mjs`（`getSkillLockPath` / `readSkillLock` / `writeSkillLock`）。[已验证]

因此：

1. **单写者**：`TaskManager` 用 `tokio::sync::Semaphore(1)` 串行化所有 CLI 调用（读也一起串行，最简单、最安全）；或写独占、读共享，但读也可能看到写一半的文件，v1 建议全串行。
2. **单实例**：加 `tauri-plugin-single-instance`，防止两个 app 实例各跑一个 CLI。官方文档要求它最先注册 —— [Single Instance](https://v2.tauri.app/plugin/single-instance/)。
3. **mutating 前快照**：`add` / `update` / `remove` 前把 `.skill-lock.json` 复制到 app 数据目录（含 task id、时间、hash）。任务结束后重新读取并校验 JSON 与 version；若非法/缺条目，标记 `NeedsReview`，提供「从快照恢复」的显式操作（不要在用户不知情时自动覆盖）。
4. **外部 CLI 检测**：任务前后比较 lock 的 mtime + hash；若被终端里的 `npx skills` 改过，警告「检测到外部修改」，不要盲目覆盖。
5. **任务队列**：FIFO；UI 显示 `Queued / Preparing / Running / Cancelling / Done / Failed / NeedsReview`；取消排队中的任务直接移除并标记 Cancelled。
6. 可选：对 `~/.agents/.skills-box.lock` 做 advisory `flock`，作为单实例之外的 belt-and-braces；但 `skills` CLI 本身不会遵守这个锁，不能替代串行化。

**注意**：如果用户在 shell 里设置了 `XDG_STATE_HOME`，终端里的 `skills` 会写 `$XDG_STATE_HOME/skills/.skill-lock.json`，而 GUI（env_clear）会写 `~/.agents/.skill-lock.json`，两边会分叉。v1 明确以 `~/.agents` 为真相；把「检测到非默认 lock 路径」作为已知限制/诊断项。[已验证 + 风险]

---

## 5. 安全与权限

### 5.1 Capabilities 管什么

官方 Capabilities 文档：capabilities *"granularly enable and constrain the core exposure to the application frontend running in the system WebView"*；它**不能**防护 *"Malicious or insecure Rust code"*、*"Intentional bypasses from Rust code"*。同时：*"By default, all commands that you registered in your app (using `Builder::invoke_handler`) are allowed to be used by all the windows and webviews of the app."* —— [Capabilities](https://v2.tauri.app/security/capabilities/)

含义：

- 我们的 `cli_run` 默认对所有窗口开放；capabilities 不会替我们校验参数。**Rust 命令层就是安全边界。**
- 不需要给 `shell` 插件加权限；因为根本不用 JS shell API。
- 如果未来要限制到 main window，可用 `tauri_build::AppManifest::new().commands(&[...])`（build.rs），但它只管窗口暴露，不替代参数校验。

### 5.2 具体规则

- **前端只传类型化枚举**：`List`、`Check`、`Install { source, skills: Vec<SkillName>, agents: Vec<AgentId>, copy: bool }`、`Update { skills }`、`Remove { skills, agents }`。可执行文件、argv、env、cwd 都由 Rust 决定。
- **固定 npx spec**：`skills@1.7.4` 是常量；用户输入只能作为 source/skill/agent 的单个 argv。
- **参数校验**：skill 名 `[A-Za-z0-9._-]+`；source 用 01 票据定义的格式白名单（owner/repo、GitHub URL 等）；agent 用已知枚举；拒绝 NUL、换行、控制字符和以 `-` 开头的用户值（或确认 CLI 支持 `--` 后安全分隔）。
- **路径校验**：用户覆盖的 node 路径 `canonicalize`、必须是可执行普通文件；不要从前端接收路径。校验失败时显示具体原因，不静默换一个 node。
- **无 shell**：`Command::new(node)` + `args([...])`，永远不 `sh -c`。Rust 的 `Command` 不做 shell 展开，主要风险是「选项注入」和「参数语义」，用上面的白名单处理。
- **最小环境**：`env_clear` + 3.3 的 allowlist；不继承 token、代理、`NODE_OPTIONS`。
- **当前用户**：不调用 `sudo`、不申请 admin、不启用 App Sandbox（v1 默认无沙箱）。CLI 以 app 用户身份运行；如果未来上架 App Store，沙箱会限制 spawn 外部 Node，需要重新设计。
- **破坏性操作的二次确认**：安装/更新/卸载前 UI 预览（01/09 的职责）；如果要防 XSS 直接调用 `cli_run` 卸载，可在 Rust 侧发一次性 `preview_token`，`cli_execute(token)` 才真正执行。v1 个人工具可先做「类型化枚举 + UI 确认」，但要把这个风险写进 09。
- **插件取舍**：不需要 `fs` 插件（Rust 直接读 lock/日志）；不需要 `dialog` 插件（React 确认框即可）；现有 `opener` 继续用于打开 http(s) 链接，且只允许 http(s)。[推荐]
- **当前配置**：`capabilities/default.json` 只有 `core:default` 与 `opener:default`，没有 shell 权限，这是正确的起点；但 `tauri.conf.json` 的 `security.csp` 目前是 `null`。如果 UI 会渲染远端 `SKILL.md`，09 应同时收紧 CSP/做 Markdown 清洗，避免 XSS 直接调用 `cli_run`。[已验证 + 推荐]

---

## 6. 打包（个人 macOS build）

### 6.1 产物

```bash
pnpm tauri build --bundles app
# 产物：src-tauri/target/release/bundle/macos/skills-box.app
```

- 当前 `tauri.conf.json`：`bundle.active: true`、`targets: "all"`、无 macOS 覆盖。`all` 在 macOS 会尝试 app + dmg；个人自用建议 `--bundles app`（或把 `bundle.targets` 改成 `["app"]`），减少构建时间与签名面。
- Tauri 官方 macOS bundle 文档给出了 `pnpm tauri build --bundles app` 的用法：[macOS Application Bundle](https://v2.tauri.app/distribute/macos-application-bundle/)。
- 默认 `bundle.macOS.minimumSystemVersion` 是 `10.13`，`hardenedRuntime` 默认 `true`，`signingIdentity` 默认 `null` —— [Config Reference](https://v2.tauri.app/reference/config/)。建议把 `minimumSystemVersion` 提到 `"12.0"`（或至少 11.0）；10.13 对现代 Node/WebKit 没有实际意义。[推荐]

### 6.2 不签名 / 不公证

- 本机写入的构建产物不会带 `com.apple.quarantine`，通常 Finder 可直接启动；本机最小 Swift `.app`（链接器 ad-hoc 签名）已实测可从 Finder 运行。[已验证] Tauri 构建产物的同等行为尚未实际构建验证。[未验证]
- Apple Silicon 上建议显式 ad-hoc 签名：`"bundle": { "macOS": { "signingIdentity": "-" } }`。Tauri 文档：ad-hoc *"useful on ARM (Apple Silicon) devices, where code-signing is required for all apps from the Internet"*，但它**不能**阻止 macOS 对下载来的 app 要求用户在 Privacy & Security 中放行 —— [macOS Code Signing](https://v2.tauri.app/distribute/sign/macos/)。
- 不公证（notarization）只影响「从网络下载后首次打开」的 Gatekeeper 体验；本机自用不需要 Apple Developer 账号。若把 `.app` zip/AirDrop 给别的机器，接收方会被 quarantine，需要右键 Open 或系统设置放行；这是可接受的个人使用限制。[推荐]
- 可选：`pnpm tauri build --bundles app --no-sign` 在签名环境缺失时跳过签名；但 ad-hoc 更稳。

### 6.3 系统 Node vs Node sidecar

| 方案 | 体积 / 复杂度 | 运行时依赖 | 结论 |
|---|---|---|---|
| 系统 Node + npx + git（v1） | app 保持小体积；无额外运行时 | Node >= 22.20、npx、git、网络（首次 npx 下载） | **推荐**：与 `skills` CLI 的生态一致；用户本来就要 Node |
| 捆绑 Node runtime（`externalBin` sidecar） | 本机 Node 二进制 77MB（Homebrew Cellar）/ 114MB（nvm v24）；再加 app 与资源 | 仍需 `git`（`skills` 调 git），仍需处理 PATH；Node 更新需重建 app | 不推荐 v1 |
| 用 `pkg` 把 skills 打成单文件 sidecar | `skills` 是 ESM + 多 chunk（640KB），`pkg` 对 ESM/动态 import 支持有限；仍需 git | git + 网络 | 风险高，不推荐 v1 |

- 官方 sidecar 文档说明 sidecar 是为「避免用户安装额外依赖（如 Node.js/Python）」；Node sidecar 指南用 `pkg` 打包。对本项目，`skills` 不是单个自包含 JS，且必须调 git，捆绑收益低、维护成本高。[Sidecar](https://v2.tauri.app/develop/sidecar/)、[Node.js as a sidecar](https://v2.tauri.app/learn/sidecar-nodejs/)
- npx 首次运行会把 `skills` 下载到 `~/.npm/_npx`，之后离线可用（同一版本）。固定 `skills@1.7.4` 可避免版本漂移；如果 09 决定跟随 latest，则每次启动/安装前要检测版本并提示。[推荐]
- 应用不打包 pnpm；`pnpm tauri build` 只在构建时使用。运行时只需要 node/npx/git。
- 缺失引导：启动检测失败时显示「未找到 Node 22.20+ / npx / git」的具体项、探测过的路径、复制诊断信息、打开 nodejs.org / git-scm.com 的按钮（用现有 opener，仅 http(s)）。不要自动安装系统级依赖。

---

## 7. 可测试性

### 7.1 分层与 seam

```text
Tauri command (thin)
  -> TaskManager (queue, cancel, lock snapshot, events)
     -> CliInvocation::to_spec()  (pure: argv/env/cwd)
        -> ProcessExecutor trait (tokio impl / fake)
           -> OS process group
  -> Channel adapter (CliEvent -> tauri::ipc::Channel)
```

```rust
#[async_trait::async_trait]
pub trait ProcessExecutor: Send + Sync {
    async fn run(
        &self,
        spec: CommandSpec,
        sink: EventSink,              // 不依赖 Tauri；测试用 Vec/mpsc
        cancel: CancellationToken,
    ) -> Result<ExitInfo, ExecError>;
}
```

- `CommandSpec`：`program: PathBuf`、`args: Vec<OsString>`、`cwd`、`env: BTreeMap<OsString, OsString>`、`kind: Read | Write`、`timeout`。
- `EventSink` 用 `Arc<dyn Fn(CliEvent) + Send + Sync>` 或 `mpsc::UnboundedSender<CliEvent>`；**trait 不出现 `AppHandle` / `Channel`**，这样单测不需要 Tauri runtime。命令层再把 sink 适配到 `Channel`。
- 生产 `TokioExecutor`；测试 `ScriptedExecutor` 按脚本产生 `Stdout/Stderr/Sleep/Exit/Signal/Hang`，支持取消检查。

### 7.2 测试矩阵

| 层 | 测什么 | 方式 |
|---|---|---|
| 解析器 | 候选顺序、nvm 默认别名、fnm 路径、Homebrew、版本过低、缺 npx/git、override 优先 | `tempfile` 构造假目录 + 假 `node --version` 输出；不跑真实 node |
| 命令构造 | argv 精确匹配、env allowlist、cwd、无 shell、注入样例（`--foo`、换行、`$()`、空格） | 纯函数断言 `CommandSpec` |
| 执行器 | 分行、CR 进度、输出上限、退出码、signal、超时、SIGTERM→SIGKILL、孙进程清理 | `ScriptedExecutor` + 真实 `/bin/sh` 小脚本（如 `sh -c 'sleep 60'`）集成测试 |
| TaskManager | 单写者队列、取消排队/运行中、lock 快照、NeedsReview、事件顺序 | 假 executor + `tokio::time::pause` |
| 前端 | 事件流渲染、慢任务、失败、取消、重连快照 | `CliTransport` 接口 + fake 事件流；`mockIPC`/`clearMocks` 做 invoke 级测试（[Tauri Mocking](https://v2.tauri.app/develop/tests/mocking/)） |
| 手工 | Finder 启动、环境检测、缺 Node、npx 冷缓存、取消后 lock 完整 | 打包 `.app`，双击/`env -i open`，不要用普通终端 `open` |

### 7.3 开发期 mock

- `SKILLS_BOX_FAKE_CLI=1`（仅 `#[cfg(debug_assertions)]`）让 Rust 选择 `ScriptedExecutor`，fixture 定义在 `src-tauri/tests/fixtures/cli/*.json`：`success`、`slow`、`fail-network`、`fail-auth`、`hang`、`partial-output`。
- 前端 `CliTransport` 的 fake 版本可用于 Storybook/组件测试；生产 adapter 才 import `@tauri-apps/api/core`。
- 不把 fake 分支编译进 release（`cfg` 或 feature），避免用户误触发。

---

## 8. 风险清单

| # | 风险 | 影响 | 缓解 |
|---|---|---|---|
| 1 | Finder 启动 PATH 缺 Node | 所有 CLI 调用 ENOENT | 解析器 + 覆盖 + 诊断；验收用 Finder |
| 2 | `npx` shebang / npm exec 二次找 node | 即使传了 node 绝对路径仍失败 | 子进程 PATH 必含 node bin；本机已验证 |
| 3 | Node < 22.20 | CLI 启动即失败 | 预检 semver，阻止并引导 |
| 4 | git 缺失 / CLT 未装 | add/update 失败，可能弹系统安装框 | 预检 `git --version`；引导 `xcode-select --install` |
| 5 | `npx skills` 跟随 latest 漂移 | 行为/JSON 不兼容 | 固定 `skills@1.7.4`，升级需测试 |
| 6 | npx 冷缓存 / 网络慢 | 首次操作长时间无输出 | Preparing 阶段、15min 硬上限、可取消 |
| 7 | lock 非原子写 + 取消/强杀 | lock 截断/丢条目 | 单写者、快照、JSON 校验、NeedsReview/恢复 |
| 8 | 两个 app 实例 / 终端 CLI 并发 | 丢更新 | single-instance + 串行化 + mtime/hash 检测 |
| 9 | `XDG_STATE_HOME` 分叉 | app 与终端看到不同 lock | 不设置该变量；文档化限制 |
| 10 | 参数/选项注入 | 执行非预期 CLI 动作 | 类型化枚举、白名单、固定 spec、无 shell |
| 11 | 前端 XSS 直接调 `cli_run` | 可触发卸载/安装 | 预览确认；必要时 Rust 一次性 token；收紧 CSP |
| 12 | 取消只杀直接子进程 | git/npm 残留、锁竞争 | 进程组 SIGTERM→SIGKILL |
| 13 | app 强制退出 | 子进程残留 | 退出钩子 + kill_on_drop；下次启动清理提示 |
| 14 | 输出洪水/非法 UTF-8 | IPC 卡顿/内存涨 | 按行批处理、64KB 上限、`from_utf8_lossy`、日志文件 |
| 15 | 登录 shell rc 卡住 | 启动挂起 | 3s 超时、只取 PATH、缓存；不用它跑命令 |
| 16 | Gatekeeper（分发后） | 对方无法直接打开 | 本地自用；ad-hoc + 右键 Open；不承诺分发 |
| 17 | 环境变量泄漏（token/代理） | 隐私/行为异常 | env_clear + allowlist；v1 公开仓库 |
| 18 | 用户覆盖指向错误/恶意 node | 执行非预期二进制 | canonicalize、可执行校验、显示版本与来源 |

---

## 9. 推荐实现骨架（供 09 落架构）

```toml
# src-tauri/Cargo.toml（建议新增）
tokio = { version = "1", features = ["process", "time", "sync", "io-util"] }
thiserror = "2"
async-trait = "0.1"
semver = "1"
libc = "0.2"                 # Unix killpg；或使用 nix
# 可选
tauri-plugin-single-instance = "2"
```

```rust
// 伪代码：命令边界
#[tauri::command]
async fn cli_run(
    state: State<'_, AppState>,
    req: CliRequest,
    on_event: Channel<CliEvent>,
) -> Result<TaskId, CliError> {
    let spec = CliInvocation::from_request(req, state.env.snapshot())?;
    let task_id = state.tasks.enqueue(spec, on_event).await?;
    Ok(task_id)
}
```

- `CliInvocation` 产生固定的 `program`/`args`/`env`/`cwd`；`TaskManager` 负责 semaphore、快照、事件、取消。
- `TokioExecutor` 负责 `env_clear`、`process_group(0)`、`kill_on_drop(true)`、并发读 stdout/stderr、SIGTERM/SIGKILL、`wait()`。
- `EnvResolver` 负责候选探测、版本校验、登录 shell 回退、`EnvReport`。
- 所有 `CliError` 在 Rust 归一化：`NodeNotFound`、`NpxNotFound`、`GitNotFound`、`NodeTooOld`、`CliNotCached`、`Timeout`、`Cancelled`、`Exit { code, signal, stderr_tail }`、`Spawn { io }`、`NeedsReview`。前端只做展示。

### 仍待 09 决定

1. `skills` 版本固定 `1.7.4` 还是跟随 latest。
2. 是否允许读操作与写操作并发（建议 v1 全串行）。
3. lock 损坏时是否提供「从快照恢复」按钮（建议提供，但不自动）。
4. 是否引入 `tauri-plugin-single-instance`（建议引入）。
5. 是否使用官方 `fix-path-env` crate 还是自写有界登录 shell 回退（建议后者，理由见 3.5）。
6. 破坏性操作是否需要 Rust 侧一次性 preview token（建议至少 09 评估）。
7. `minimumSystemVersion` 定为 11.0 还是 12.0（建议 12.0，个人自用）。

---

## Sources

**Tauri 官方**

- [Shell plugin](https://v2.tauri.app/plugin/shell/)
- [tauri-plugin-shell `Command` API](https://docs.rs/tauri-plugin-shell/latest/tauri_plugin_shell/process/struct.Command.html)
- [tauri-plugin-shell `CommandChild` API](https://docs.rs/tauri-plugin-shell/latest/tauri_plugin_shell/process/struct.CommandChild.html)
- [tauri-plugin-shell `TerminatedPayload`](https://docs.rs/tauri-plugin-shell/latest/tauri_plugin_shell/process/struct.TerminatedPayload.html)
- [plugins-workspace shell `commands.rs`](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/shell/src/commands.rs)
- [plugins-workspace shell `process/mod.rs`](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/shell/src/process/mod.rs)
- [Calling the Frontend from Rust（Events / Channels）](https://v2.tauri.app/develop/calling-frontend/)
- [Capabilities](https://v2.tauri.app/security/capabilities/)
- [macOS Application Bundle（含 PATH 说明）](https://v2.tauri.app/distribute/macos-application-bundle/)
- [macOS Code Signing / Ad-Hoc](https://v2.tauri.app/distribute/sign/macos/)
- [Embedding External Binaries（sidecar）](https://v2.tauri.app/develop/sidecar/)
- [Node.js as a sidecar](https://v2.tauri.app/learn/sidecar-nodejs/)
- [Single Instance plugin](https://v2.tauri.app/plugin/single-instance/)
- [Mock Tauri APIs](https://v2.tauri.app/develop/tests/mocking/)
- [Tauri CLI reference](https://v2.tauri.app/reference/cli/)
- [Tauri config reference](https://v2.tauri.app/reference/config/)
- [fix-path-env-rs](https://github.com/tauri-apps/fix-path-env-rs)
- [Tokio `process::Command`](https://docs.rs/tokio/latest/tokio/process/struct.Command.html)

**Node / 版本管理器**

- [nvm README](https://github.com/nvm-sh/nvm#installing-and-updating)
- [fnm README](https://github.com/Schniz/fnm)
- [fnm `directories.rs`](https://github.com/Schniz/fnm/blob/master/src/directories.rs)
- [Volta Getting Started](https://docs.volta.sh/guide/getting-started)
- [Homebrew Installation](https://docs.brew.sh/Installation)
- [No-color.org](https://no-color.org/)（`NO_COLOR` 约定）

**本机证据**

- `skills@1.7.4` npx 缓存：`~/.npm/_npx/0757c8f0298e564a/node_modules/skills`；`package.json` engines `>=22.20.0`；`dist/cli.mjs` 的 `writeSkillLock()` 无锁、直接 `writeFile`。
- `~/.agents/.skill-lock.json`：version 3，61 条；`skills` CLI 只在 mutating 时写。
- 最小 `.app` 探针（`/tmp/PathProbe.app`）通过 Finder 启动：`PATH=/usr/bin:/bin:/usr/sbin:/sbin`，`LANG` 缺失。
- `env -i` 下 npx/node 两段式解析实验；`zsh -ilc` 0.50s 解析到 nvm v24.15.0；`/usr/bin/git` 2.54.0。
- Node 二进制体积：Homebrew Cellar 77MB、nvm v24 114MB；`skills` 包 640KB。
