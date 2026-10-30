import type { TaskPhase, TaskSkillStatus } from "./types";

export const TASK_PHASE_LABELS: Record<TaskPhase, string> = {
  preparing: "准备中",
  cloning: "正在克隆仓库",
  backup: "正在备份",
  running_cli: "正在执行 CLI",
  verifying: "正在校验结果",
  completed: "已完成",
  failed: "失败",
  cancelled: "已取消",
};

export const TASK_SKILL_STATUS_LABELS: Record<TaskSkillStatus, string> = {
  installed: "已安装",
  updated: "已更新",
  removed: "已卸载",
  skipped: "跳过",
  failed: "失败",
};
