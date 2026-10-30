# 04: 项目安装、挂载与断链

**Parent:** [spec.md](../spec.md)

**What to build:** 安装向导在激活 scope 内运行，展示目标 scope 与 canonical 路径；项目内目标 agent 默认为空，可选集合为全局已检测到的 catalog agent + 已配置项目目录的 extra（目录不存在也可选）；始终追加 `universal`。预览与安装以项目根为 cwd、不带 `-g`；全局路径沿用隔离 cwd 与 `-g`。安装后校验 canonical、项目锁 key、catalog 挂载与 extra 原生链接。同名冲突、Copy-only/Stale 拦截只在当前 scope 生效。实现 scope 感知的挂载/断链对话框：挂载要求 canonical 存在，extra 由应用创建父目录与符号链接；断链只 unlink 当前 scope 的链接，不改 canonical 与 lock。为项目 skill 记录基线指纹（应用安装/更新时），外部安装按未知处理。

**Blocked by:** 02

**Notes:** 挂载/断链/安装链接复用 `attachment_target` 门控（ADR 0005），项目根与 extra 项目目录作为目标路径上下文传入；刷新走统一 mutation surface（ADR 0008）并按 scope 失效。

**Status:** ready-for-agent

- [ ] 项目安装预览与安装使用项目根 cwd、无 `-g`、始终含 `universal`
- [ ] 默认不勾选 agent；目标集合符合 spec §3.5；extra 目录不存在时可挂载并创建
- [ ] 安装后校验 canonical、lock key、catalog 链接与 extra 链接
- [ ] 冲突判断只看当前 scope；全局与项目同名互不影响
- [ ] 项目断链只影响当前 scope；canonical 与 lock 不变
- [ ] 项目基线指纹写入应用数据，按 scope 隔离
- [ ] 前端 fake backend 与 Rust 测试覆盖项目安装、挂载、断链
