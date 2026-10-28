# 01: 应用骨架与环境探测

**Parent:** [spec.md](../../global-skill-manager/spec.md)

**What to build:** 用户从 Finder/Dock 或终端启动 app，进入中文三栏工作台骨架，顶栏显示环境状态：PATH 来源、Node（>= 22.20）、npx、git、pinned CLI 可用性、检测到的 agent 与 lock 健康。PATH 按「用户覆盖 → Volta → fnm → nvm 默认别名/最高版本 → Homebrew → 系统 → 有界登录 shell 回退」探测，探测超时 5s，结果缓存为 EnvReport。Node/npx/git/CLI 缺失或版本不符时显示明确环境横幅，只读界面不崩溃、变更入口禁用。同时建立后续所有票据依赖的骨架：可注入 AppPaths（debug `SKILLS_BOX_ROOT` 生效、release 忽略）、结构化 AppError、ProcessExecutor 抽象（Tokio 实现 + scripted fake）、vendored 79-agent catalog 与检测规则、限制性 CSP 初版、测试地基（cargo test、Vitest + RTL + mockIPC、temp AppPaths 与 fixture 约定）；移除模板 greet 演示与示例资源。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] `get_environment` 返回 PATH 来源、Node/npx/git 版本、CLI 可用性、检测到的 agent 与 lock 健康；EnvReport 缓存
- [ ] PATH 探测顺序与回退有单测（Volta / fnm / nvm / Homebrew / 系统 / 登录 shell）
- [ ] Finder 启动语义成立：最小 PATH 下仍能解析 Node，并把 `<node_dir>` 注入子进程 PATH（npx shebang 与 npm exec 的二次查找）
- [ ] Node 缺失、版本 < 22.20、npx 缺失、git 缺失分别显示明确环境横幅；界面不崩溃
- [ ] 79-agent catalog 与 pinned 1.7.4 一致，含 ID、显示名、`globalSkillsDir`、universal 标志与检测规则
- [ ] AppPaths 可注入；`SKILLS_BOX_ROOT` 仅 debug 生效，release 构建忽略
- [ ] `AppError { code, message, detail?, hint?, retryable }` 与核心错误码映射有单测
- [ ] 子进程环境：`env_clear` + 最小 allowlist、`DISABLE_TELEMETRY=1`、`DO_NOT_TRACK=1`、清理 agent 检测变量、stdin null、cwd 为 app data 临时目录
- [ ] 前端为中文界面；三栏骨架与顶栏环境状态可见；无模板残留
- [ ] `cargo test` 与 `pnpm test` 可运行；fake executor 可编排成功/失败/超时输出
- [ ] CSP 非空且禁止远程资源；未启用 shell/fs 插件权限
