# 04: Mutation task 骨架与安装预览

**Parent:** [spec.md](../../global-skill-manager/spec.md)

**What to build:** 用户点击「安装」打开向导，第一、二步可用：输入公开 GitHub 根仓库（`owner/repo` 或 `https://github.com/owner/repo`，只装默认分支；拒绝 subdir、`#ref`、GitLab、SSH、local、Notion 并给出明确错误，确认页展示归一化 `owner/repo`），运行 `skills add <source> -l -g` 预览仓库内 skill 并解析 name + description（解析失败展示 raw text 并允许手动输入 skill 名；单 skill 仓库自动跳过选择；无 SKILL.md 显示空态且禁止安装；同名已安装/未受管禁用并显示现有来源）。底层建立 mutation task 骨架：TaskId、`Channel<TaskEvent>` 阶段事件、单写者队列（Semaphore(1)）、进程组 SIGTERM → 3s → SIGKILL 取消、`cancel_task` 语义；preview 为只读任务，任意阶段可取消，60s 超时，不占写锁。CLI 调用固定 `<node_dir>/npx -y skills@1.7.4`，只解析 stdout，不依赖颜色。

**Blocked by:** 01（应用骨架与环境探测）、02（库存读取模型与平铺列表）

**Status:** resolved

- [x] preview 任务端到端可用：成功解析、raw fallback、超时、取消后无残留进程
- [x] `add -l` 解析器针对 pinned 1.7.4 输出 fixture 有单测；不依赖 path 字段
- [x] 来源校验与归一化：非法、subdir、`#ref`、非 GitHub 来源被拒绝；错误码 `source.*` 稳定
- [x] 同名冲突（已安装 / 未受管）在预览中禁用并展示现有来源；服务端校验逻辑就绪，供 05 复用
- [x] 单 skill 仓库自动跳过选择但仍进入确认流；无 SKILL.md 空态
- [x] 两个并发 mutation 请求不会同时执行：第二个返回 `task.busy`；preview 不占写锁
- [x] 任务事件阶段与 `completed/total` 推送正确；UI 显示阶段进度且不阻塞整窗
- [x] preview 失败显示 `source.preview_failed` / 网络错误；可重试
- [x] 向导 Esc 关闭、取消按钮语义正确；写入阶段提示在确认页可见（执行在 05）

## Comments

- 2026-10-28: Resolved on integration/skills-box-v1. All tickets merged; code-review fixes merged in 3e9d2ad.
- Evidence: `tasks::manager` preview 套件（成功、raw fallback、超时、取消、`task.busy`、单写者）与 `tasks::parser`/`tasks::source` 用例，见 acceptance.md §1（303 passed）与 §5 M20；向导用例见 `App.install.test.tsx`。
