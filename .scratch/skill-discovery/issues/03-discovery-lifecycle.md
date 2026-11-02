# 03 — Source discovery 生命周期与 IPC

Status: resolved
Type: task
Parent: [spec.md](../spec.md)

## 工作

建立应用级 Discovery service：从 Global 与所有注册 Project 的安装记录收集规范来源，原生查询默认分支版本，必要时预览，分批发布 catalog，并保护用户状态与来源生命周期。

- 复用现有 GitHubApiClient/SettingsService 与 Git 版本获取；同源去重，版本未变且 catalog 有效时不重复预览。
- 为限流保留结构化恢复时间，保留原错误展示含义；调度不从错误消息解析 header。401 不匿名回退，冷却适用当前认证上下文。
- 每批最多 20、并发最多 4；未扫描/最久未扫描优先，余量自动续扫。一轮不重复尝试失败来源。
- source 级六小时间隔、手动绕过间隔、限流暂停和恢复；GitHub 暂停不阻断其他 Git 来源。
- 单一 run、短协调锁、增量保存；本轮结果合并最新用户状态，不持 Mutation permit 执行网络或 CLI。
- Scope 来源快照按 Project revision 校验。暂不可读保留同 revision 的快照；Relocate/Forget、停用再恢复和迟到扫描不恢复旧关联。
- 发布前检验当前 Source 活跃生命周期与 run。预览期间证实默认分支变化时不发布不确定版本的成功目录。
- 普通状态读取遵守读取期限；长扫描使用异步 start/事件或状态进度，避免整批被 30 秒读取期限裁断。
- 提供 get state、start scan、acknowledge items/all、set ignored 的语义 IPC；后端从记录解析身份，不接受任意 argv、运行环境或读写路径。
- AppState 注入共享实例，应用退出取消队列和只读进程组；Project 生命周期成功后撤销旧资格，即使记录清理写入失败。

## 验证

注入 Clock、版本客户端、只读 preview 和 registry/lock fixtures，验证多 Scope 去重、45 来源续扫、并发上限、版本缓存、失败保留、限流/凭据变化、Single flight、A→B→A/Forget/重新激活与用户状态并发合并。IPC 测试检验不读取已撤销 Project root、无 token 输出和无安装写入。

## Comments

- 2026-11-02：扫描以实际 CLI 预览为 catalog 依据；初次/来源变化需 Node/npx/Git。

- 2026-11-02：用户确认完整规格，已可实施。

## Answer

已实现跨 Scope 来源去重、默认分支版本查询、六小时尝试间隔、20 个来源分批/并发 4、Single flight、当前凭据限流与异步进度。Project revision、Forget/Relocate、停用恢复、迟到结果及退出取消测试通过；凭据指纹与 revision 从同一快照读取。

验收：[verification.md](../verification.md)。
