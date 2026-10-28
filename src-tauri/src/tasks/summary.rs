//! Shared per-skill outcome roll-up for install / update / uninstall.
//!
//! The three mutations differ only in which [`TaskSkillStatus`] counts as a
//! success and in their user-facing wording. Keeping one implementation here
//! guarantees that the overall state and summary sentence can never drift
//! apart between the actions.

use super::manager::{TaskAction, TaskOverall, TaskSkillResult, TaskSkillStatus};

/// Number of successful rows and of everything else.
pub fn counts(results: &[TaskSkillResult], success: TaskSkillStatus) -> (usize, usize) {
    let succeeded = results
        .iter()
        .filter(|result| result.status == success)
        .count();
    (succeeded, results.len().saturating_sub(succeeded))
}

/// Roll per-skill rows up into the task-level outcome. An empty result set is
/// a failure, never a vacuous success.
pub fn overall_for(results: &[TaskSkillResult], success: TaskSkillStatus) -> TaskOverall {
    let (succeeded, _) = counts(results, success);
    if results.is_empty() {
        TaskOverall::Failed
    } else if succeeded == results.len() {
        TaskOverall::Success
    } else if succeeded > 0 {
        TaskOverall::Partial
    } else {
        TaskOverall::Failed
    }
}

/// One summary sentence per action and overall state. Uninstall keeps its
/// single-skill wording; install/update use the per-skill counts.
pub fn message(action: TaskAction, overall: TaskOverall, results: &[TaskSkillResult]) -> String {
    let success = match action {
        TaskAction::Install => TaskSkillStatus::Installed,
        TaskAction::Update => TaskSkillStatus::Updated,
        TaskAction::Uninstall => TaskSkillStatus::Removed,
        TaskAction::Preview => TaskSkillStatus::Installed,
    };
    let (succeeded, unfinished) = counts(results, success);
    let name = results
        .first()
        .map(|result| result.name.as_str())
        .unwrap_or("");
    match action {
        TaskAction::Install => match overall {
            TaskOverall::Success => format!("安装完成，已验证 {succeeded} 个 skill。"),
            TaskOverall::Partial => {
                format!("部分安装完成：{succeeded} 个成功，{unfinished} 个未完成。")
            }
            TaskOverall::Failed => format!("安装失败：{unfinished} 个 skill 未完成。"),
            TaskOverall::Cancelled => "安装已取消。".to_string(),
        },
        TaskAction::Update => match overall {
            TaskOverall::Success => format!("更新完成：{succeeded} 个 skill 已更新。"),
            TaskOverall::Partial => {
                format!("部分更新完成：{succeeded} 个成功，{unfinished} 个未完成。")
            }
            TaskOverall::Failed => format!("更新失败：{unfinished} 个 skill 未完成。"),
            TaskOverall::Cancelled => format!("更新已取消：{succeeded} 个已更新。"),
        },
        TaskAction::Uninstall => match overall {
            TaskOverall::Success => format!("已彻底卸载「{name}」。"),
            TaskOverall::Partial => format!("卸载部分完成：「{name}」仍有残留。"),
            TaskOverall::Failed => {
                format!("卸载未完成：「{name}」仍保留部分文件或 lock 条目。")
            }
            TaskOverall::Cancelled => "卸载已取消。".to_string(),
        },
        TaskAction::Preview => match overall {
            TaskOverall::Success => "预览完成。".to_string(),
            TaskOverall::Partial => "预览部分完成。".to_string(),
            TaskOverall::Failed => "预览失败。".to_string(),
            TaskOverall::Cancelled => "预览已取消。".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, status: TaskSkillStatus) -> TaskSkillResult {
        TaskSkillResult {
            name: name.to_string(),
            status,
            error: None,
            backup_path: None,
        }
    }

    #[test]
    fn overall_rolls_up_per_action_and_never_treats_empty_as_success() {
        assert_eq!(
            overall_for(&[], TaskSkillStatus::Installed),
            TaskOverall::Failed
        );
        assert_eq!(
            overall_for(
                &[row("a", TaskSkillStatus::Installed)],
                TaskSkillStatus::Installed
            ),
            TaskOverall::Success
        );
        assert_eq!(
            overall_for(
                &[
                    row("a", TaskSkillStatus::Installed),
                    row("b", TaskSkillStatus::Failed)
                ],
                TaskSkillStatus::Installed
            ),
            TaskOverall::Partial
        );
        assert_eq!(
            overall_for(
                &[row("a", TaskSkillStatus::Failed)],
                TaskSkillStatus::Installed
            ),
            TaskOverall::Failed
        );
        assert_eq!(
            overall_for(
                &[row("a", TaskSkillStatus::Updated)],
                TaskSkillStatus::Updated
            ),
            TaskOverall::Success
        );
        assert_eq!(
            overall_for(
                &[row("a", TaskSkillStatus::Removed)],
                TaskSkillStatus::Removed
            ),
            TaskOverall::Success
        );
    }

    #[test]
    fn messages_keep_the_action_wording_and_counts() {
        let installed = vec![
            row("a", TaskSkillStatus::Installed),
            row("b", TaskSkillStatus::Failed),
        ];
        assert_eq!(
            message(TaskAction::Install, TaskOverall::Partial, &installed),
            "部分安装完成：1 个成功，1 个未完成。"
        );
        let updated = vec![row("a", TaskSkillStatus::Updated)];
        assert_eq!(
            message(TaskAction::Update, TaskOverall::Success, &updated),
            "更新完成：1 个 skill 已更新。"
        );
        let removed = vec![row("alpha", TaskSkillStatus::Removed)];
        assert_eq!(
            message(TaskAction::Uninstall, TaskOverall::Success, &removed),
            "已彻底卸载「alpha」。"
        );
    }
}
