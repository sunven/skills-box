# 08: Detach（断开单个 agent）

**Parent:** [spec.md](../../global-skill-manager/spec.md)

**What to build:** 用户在详情面板的 agent 行发起 Detach：确认弹窗显示 agent、attachment 模式、canonical 与 lock 将保留、其他受影响的 agent。仅 managed 且有 canonical 的 skill 可断开；symlink 直接 unlink，不动 canonical/lock；copy 仅在没有任何其他 agent symlink（经 realpath）指向该目录时才允许删除，否则阻止并解释（`agent.dependency_conflict`），不自动改指；copy 断开前走备份管线（copy 目录 + lock 快照）。universal/native 与无 canonical 的 agent-only skill 不提供断开入口。路径守卫：目标必须在 agent 目录内、不得是 canonical、拒绝 agent skills 目录自身为 symlink。断链操作走单写者队列；完成后刷新详情与列表，无挂载时显示「未挂载」，后续更新按当前物理挂载集重建。断链异常保持只读，不提供修复/清理动作。

**Blocked by:** 03（Skill 详情与 SKILL.md）、04（Mutation task 骨架与安装预览）、07（备份与更新执行）

**Status:** ready-for-agent

- [ ] sandbox 中对 symlink attachment 断开：canonical 与 lock 不变，其他 agent 不受影响（对齐 M11）
- [ ] copy attachment 存在跨 agent symlink 依赖时被阻止并给出解释，状态不变（对齐 M12）
- [ ] copy 断开前生成备份；删除前 realpath 校验
- [ ] universal/native、agent-only unmanaged、copy-only 均不提供断开入口
- [ ] 路径守卫拒绝越界目标、canonical 目标、agent skills 目录自身为 symlink
- [ ] 操作经单写者队列；失败不触碰 canonical/lock；错误码稳定
- [ ] 断开后详情显示「未挂载」；后续更新（07）不重挂已断开的 agent
- [ ] 断链状态在 agent 行只读展示，无修复动作
