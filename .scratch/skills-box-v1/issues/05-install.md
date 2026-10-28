# 05: 安装执行与校验

**Parent:** [spec.md](../../global-skill-manager/spec.md)

**What to build:** 用户完成安装向导后三步：选择 agent（只展示本机检测到的；默认勾选已含受管 skill 的；只有未受管目录的 agent 默认不勾选并标注；universal 标为「共享 canonical」；后台始终追加 `-a universal`）、确认页（归一化来源、所选 skill、所选 agent、目标路径、「写入阶段不可取消」）、执行与结果（阶段进度：preparing → running_cli → verifying → completed/failed；逐 skill 校验 canonical 是否存在、每个所选 agent 的 attachment 是否存在及 `lstat` 模式、CLI 静默 copy 回退提示 warning；partial failure 按 skill 展示并逐项重试；完成后刷新 inventory/detail）。执行契约：每来源一次 `skills add <source> -g -s <names> -a <ids> -a universal -y --json`；spawn 前快照 lock；spawn 后不可取消；后端校验 InstallRequest（source/skills/agents），拒绝 `--copy`、`--all`、`-a '*'` 与同名冲突。

**Blocked by:** 04（Mutation task 骨架与安装预览）

**Status:** resolved

- [x] sandbox 真实 CLI 安装单 skill 与多 skill 成功；canonical 与所选 agent 挂载事后校验通过（对齐 M05 / M06）
- [x] 命令固定显式 `-a <ids> -a universal -y --json -g -s`；fake 日志不含 `--all`、`-a '*'`、`--copy`、`update`、`check`
- [x] 同名已安装/未受管由后端拒绝（`skill.conflict`），不发生覆盖
- [x] 写入阶段取消返回 `task.cannot_cancel`；spawn 前取消可用且 lock 无损
- [x] CLI 静默 copy 回退被识别为 `verify.copy_fallback` warning 并展示
- [x] partial failure 不抛任务级错误，返回 `overall=partial` + 每 skill 结果；失败项可单独重试
- [x] CLI exit 0 不作为成功依据；以 JSON per-skill 状态 + 事后校验为准
- [x] 安装后 inventory/detail 失效刷新，`npx skills list -g --json` 可识别预期 skill
- [x] 单 skill 仓库与 preview fallback 场景可完成安装；来源非法时向导在第一步阻止

## Comments

- 2026-10-28: Resolved on integration/skills-box-v1. All tickets merged; code-review fixes merged in 3e9d2ad.
- Evidence: 真实 CLI sandbox 的 `add-single` 与 `add-multiple` 步骤，canonical/agent 挂载/lock v3 逐项校验通过（acceptance.md §2、§5 M05/M06）；fake executor 的 install/partial/copy-fallback/取消用例见 acceptance.md §5 M07–M10。
