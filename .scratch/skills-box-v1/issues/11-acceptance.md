# 11: 验收、回归与打包

**Parent:** [spec.md](../../global-skill-manager/spec.md)

**What to build:** 交付可签收的 v1：真实 CLI sandbox 集成（临时 HOME/XDG、pinned `npx -y skills@1.7.4`、小型公开测试仓库）跑通 install / update / detach / uninstall 闭环；每次 mutation 后执行回归配方对比 `list -g --json` 名称/路径、检查 canonical、agent 目录、lock v3，并断言 fake 日志不含 `update`、`check`、`remove --all`、`--all`、`--copy`、`-a '*'`；生成 100-skill 性能 fixture 并按阈值表测量；构建 release `.app`（ad-hoc 签名、`minimumSystemVersion 12.0`、不公证），从 Finder 启动验证完整闭环、环境探测、单实例与无明显卡顿；执行 M01–M24 手工矩阵并记录结果；把验收记录与命令写回规格 §10，P0 全过、P1 全过或记录已知问题（owner + 目标版本）。

**Blocked by:** 10（硬化：并发、安全与错误态）

**Status:** ready-for-agent

- [ ] sandbox 真实 CLI 完成 install / update / detach / uninstall 闭环（对齐 M05–M14）
- [ ] 回归断言通过：名称/路径集合符合预期，未受管目录未被删除，lock v3 完好，agent 目录未被误删，禁用 flag 未出现（对齐 M16）
- [ ] 100-skill fixture 达到性能阈值：启动 < 2s、warm inventory < 500ms、cold < 1.5s、搜索/筛选 < 50ms、详情 < 150ms、刷新 < 1s、首个任务事件 < 500ms、主线程卡顿 < 100ms（对齐 M17）
- [ ] Finder 启动 release `.app` 完成主流程；环境探测正确；单实例与退出保护可用（对齐 M01 / M22）
- [ ] M01–M24 手工矩阵有执行记录；P0 全勾选；P1 已知问题有 owner 与目标版本
- [ ] 验收结果与命令写回规格 §10；无 P0 数据丢失、权限越界或 CLI 生态回归
- [ ] `pnpm tauri build --bundles app` 产物可从 Finder 运行；pinned CLI 不可用时报错且不回落 latest（对齐 M21）
