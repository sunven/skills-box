# 更新检测机制与成本

Type: research
Status: resolved
Blocked by: —

## Question

查清“是否有更新”这一状态如何可靠、低成本地获得，为 GUI 的更新入口、状态徽章和刷新策略提供依据。需要覆盖：

1. `skills check` 的实现：它如何比较本地与远端（GitHub API、`git ls-remote`、commit SHA、`skillFolderHash`），每次检查发多少请求，是否有缓存 / ETag，是否支持 `--json` 与退出码。
2. `skills update` 的实现：拉取什么、写入什么、如何重新挂载 agent、是否保留 copy 模式、失败时是否回滚。
3. 在不调用 `check` 的情况下，GUI 能否自行计算更新状态：需要哪些远端信息、GitHub API 速率限制与未认证配额、50–100 个 skill 的成本、离线时的降级。
4. 触发策略建议：手动刷新、启动时后台检查、按来源批量检查、单 skill 检查的成本与体验权衡。
5. 边界：来源仓库改名 / 删除、默认分支变化、私有仓库、tag/commit 锁定、本地改动与远端更新冲突。

方法：源码 + 本机只读实验（可对单个无害 skill 做只读检查，**不要执行 update 改动用户文件**）；必要时用 GitHub API 文档验证速率限制。把结论写成 `.scratch/global-skill-manager/research/03-update-detection.md`，给出推荐方案与成本估算。在本票据下追加 `## Answer`，把 `Status` 改为 `resolved`。不要编辑 `map.md` 或其他票据。

## Answer

结论（可直接作为 GUI 规格输入）：

1. **不要调用 `skills check`。** v1.7.4 中 `check` 与 `update`/`upgrade` 路由到同一个 `runUpdate`，发现更新会实际重装；`--json`/`--dry-run` 被静默忽略，退出码只反映更新执行失败，网络/限流失败可能返回 0 并打印 “All global skills are up to date”。文档的 “without reinstalling” 与实现不符。
2. **自行实现只读检查器。** 读 lock v3，按 `(source, ref)` 分组，每个来源一次 `GET /repos/{owner}/{repo}/git/trees/{ref|HEAD}?recursive=1`，比较 40 位 `skillFolderHash` 与目标文件夹 tree SHA。`skillFolderHash` 不是 commit SHA；40 位是 Git tree SHA，64 位是文件夹 SHA-256，空值/64 位/路径缺失只能标 unknown，不能混比。
3. **成本可控。** 50–100 个 skill 通常来自 5–20 个来源，即 5–20 请求/次；未认证 60 请求/小时/IP，6 小时 TTL 平均 0.8–3.3 请求/小时；20 个来源的一次启动检查会占 1/3 小时配额。建议按来源批量、并发 3–4、单次最多 20–25 个来源，并给安装留配额；100 个来源需 token 或分批。
4. **刷新策略。** v1 默认“手动检查 + 缓存即时展示”；可选“启动时一次性后台检查”（仅缓存超过 6h，不做轮询）；单 skill 检查等价于刷新其来源，必须合并/去重。
5. **边界。** 无 `ref` 时用 `HEAD`，默认分支变化可跟上；有 `ref` 只检查该 ref；仓库删除/私有/限流/离线判 `error` 并保留上次结果；路径缺失判 `source-path-missing`，不要误报“有更新”；上游删除时 `skills update` 可能提示删除本地副本。
6. **本地改动。** lock 的 hash 是远端 tree SHA，`skills update` 不检测本地改动且直接覆盖；GUI 需自建本地指纹/基线。用 Git tree SHA 对照只能作启发式（本机 40/41 命中；`metadata.json` 被安装器排除会造成误报）。
7. **更新执行风险。** `skills update` 不保留 copy 模式、不按原 agent 列表更新、无回滚；建议 07 票据评估逐 skill 调 `skills add` 并显式指定 agent/copy。

本机采样（2026-10-28）：62 条 lock、41 个本地目录，得到 28 已最新 / 5 可更新 / 8 路径缺失 / 21 ghost，证明“按来源树比较”可行，也暴露了路径缺失与 ghost 的处理需求。

完整证据、源码行号、实测成本与推荐 Tauri 设计见 [research/03-update-detection.md](../research/03-update-detection.md)。
