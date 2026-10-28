# 主界面与关键流程原型

Type: prototype
Status: resolved
Blocked by: —

## Question

用最低成本做出可点击的界面原型，让“三栏主界面 + 安装向导 + 危险操作确认”具体到可以反驳，并记录由此确定的信息架构与交互决策。

范围：

- 三栏主界面：左侧筛选（来源、agent、更新状态），中间 skill 列表（含状态徽章、搜索），右侧详情（元数据、挂载的 agent、来源、时间、只读 SKILL.md 渲染）。
- 安装向导：输入公开 GitHub source → 预览仓库内 skill（多选）→ 选择 agent（本机检测到的默认勾选）→ 确认并展示进度 / 结果。
- 危险操作：更新单个 / 全部、断开某 agent、彻底卸载；确认文案、影响范围、本地改动警告。
- 异常与空状态：Node / CLI / git 缺失、无全局 skill、锁与目录不一致的只读提示、网络失败、部分失败。
- 规模：用 50–60 条示例数据，验证滚动、筛选与详情切换。

约束：

- 原型是抛弃型，放在 `.scratch/global-skill-manager/prototype/`，用静态 HTML/CSS/JS 或最少的 React，不接真实后端，不写入 `src/`。
- 视觉上贴近 macOS 桌面工具，中文界面；不需要像素级打磨。
- 先在会话中与用户确认一版布局，再补关键弹窗；把用户反馈与最终决策写进本票据的 `## Answer`，附原型入口文件链接。

## Prototype

- 入口：[prototype/index.html](../prototype/index.html)
- 运行：`python3 -m http.server 4173 --directory .scratch/global-skill-manager/prototype`
- 变体：`?variant=three|drawer|cards`；场景：`?scenario=normal|empty|cli-missing`
- 已验证：三栏 / 表格+抽屉 / 来源画廊；搜索与来源、agent、状态筛选；安装向导（来源 → skill 多选 → agent 多选 → 确认 → 进度 → 成功 / 部分失败）；更新、断链、彻底卸载确认；异常只读弹窗；CLI 缺失与空列表。
- 截图：[three](../prototype/screenshots/three.png) · [drawer](../prototype/screenshots/drawer.png) · [cards](../prototype/screenshots/cards.png) · [install](../prototype/screenshots/install.png) · [danger](../prototype/screenshots/danger.png) · [anomaly](../prototype/screenshots/anomaly.png)

## Answer

用户确认（2026-10-28）：

- 主界面采用 **A 三栏工作台**：左侧来源 / agent / 状态筛选，中间列表 + 搜索 / 排序，右侧持久详情。B「表格 + 抽屉」与 C「来源画廊」不进入 v1。
- 未受管 skill 保留在主列表，用「未受管」徽章与筛选区分；详情只读，禁用更新 / 卸载。不做独立分区或标签页。
- stale lock / ghost 不混入主列表；通过列表顶部的「异常 N」只读横幅打开异常弹窗，列出 stale lock、未受管、断链、本地改动、源路径缺失。v1 不提供一键修复。
- 安装使用 modal 四步向导：来源 → skill 多选 → agent 多选 → 确认 → 进度 → 成功 / 部分失败；已安装项在预览中禁用；失败态保留重试入口。
- 危险操作使用 modal 确认：更新显示来源、影响 agent、本地改动与「不支持回滚」；断链显示共享副本 / lock 保留与 cross-link 警告；彻底卸载显示删除范围。破坏性操作需勾选 checkbox 解锁确认按钮。
- 详情面板包含状态徽章、概览元数据、agent 挂载（链接 / 副本 / 共享 / 未受管 + 断开入口）、`SKILL.md` 渲染 / 原文切换、Finder 入口；异常状态显示只读警告。
- 安装 / 更新的进度、取消、部分失败与 lock 快照已在原型中表达；具体执行契约留给「安装向导的来源预览与选择契约」「更新、断链与卸载的安全语义」「技术架构与命令边界」。

原型与截图入口见上方 `## Prototype`。本地运行：`python3 -m http.server 4173 --directory .scratch/global-skill-manager/prototype`。
