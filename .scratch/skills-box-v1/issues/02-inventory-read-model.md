# 02: 库存读取模型与平铺列表

**Parent:** [spec.md](../../global-skill-manager/spec.md)

**What to build:** 用户打开 app 即看到完整库存：`get_inventory` 严格解析 lock v3，扫描 canonical store 与每个检测到的 agent 目录，以 `lstat` 判定 agent attachment（link / copy / native / broken / missing），把 skill 分类为 managed / copy-only / unmanaged / agent-only，stale lock entry 不进入主列表。UI 为三栏工作台：左侧来源、agent、状态筛选；中间平铺列表，含搜索（name / description / source）、排序（默认 lock.updatedAt 倒序，可切名称、来源）、结果计数、agent 头像堆叠（最多 5 + n）、管理徽章与更新时间；未受管时间显示「—」；选中项被过滤掉时自动选首条；空库存与无搜索结果各有空态；lock 损坏或 `version != 3` 时全屏错误并禁用变更，不显示空列表；顶部「异常 N」只读横幅与异常弹窗 v1（stale、unmanaged、broken，计数含不在列表中的 stale）。挂载真相一律来自物理目录与 `lstat`，不使用 `list -g --json.agents`。

**Blocked by:** 01（应用骨架与环境探测）

**Status:** resolved

- [x] lock v3 严格读取；损坏或非 v3 返回 `lock.*` 错误、全屏错误态、变更禁用、不显示空列表
- [x] fixture 同时包含 managed / unmanaged / copy-only / broken / agent-only / stale；分类与附件模式判定正确
- [x] stale lock entry 只在异常弹窗出现；异常总包含 stale；未挂载只提示、不计异常
- [x] 挂载判定全部来自物理目录与 `lstat`；模拟 `list --json.agents` 高报不影响显示
- [x] 平铺列表的搜索、来源/agent/状态筛选、排序、结果计数、选中重置均正确（对齐 M03）
- [x] 描述来自 `SKILL.md` frontmatter；缺失显示「—」；单个 skill 读取失败只影响该行/详情，不拖垮列表
- [x] 未受管与仅副本进入主列表但动作受限；管理徽章正确
- [x] 空库存、无搜索结果、无选中分别为独立空态
- [x] 100 条 fixture 直接渲染滚动正常，无虚拟列表/分页

## Comments

- 2026-10-28: Resolved on integration/skills-box-v1. All tickets merged; code-review fixes merged in 3e9d2ad.
- Evidence: `cargo test` 的 inventory/lock/baseline 套件与 `pnpm test` 的 inventory-view 用例（搜索/来源/agent/状态/排序/计数/选中、异常分组）（acceptance.md §1、§5 M02/M03）；100-skill fixture 直接渲染与 <50ms 预算见 acceptance.md §3。
