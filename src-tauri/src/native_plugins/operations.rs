use super::{
    hosts::{CommandResult, Observation},
    model::*,
};

pub fn outcome(
    preview: Preview,
    before: &Observation,
    after: Result<Observation, String>,
    output: Result<CommandResult, String>,
) -> Outcome {
    let executed = true;
    let command_succeeded = output.as_ref().ok().map(|o| o.success);
    let error = output.as_ref().err().cloned().or_else(|| {
        output
            .as_ref()
            .ok()
            .filter(|o| !o.response_accepted)
            .map(|o| o.diagnostic.clone())
    });
    match after {
        Err(e) => Outcome {
            status: "unconfirmed".into(),
            message: format!("原生命令已结束，但状态核验失败：{e}。请重新读取，不要直接重复写入。"),
            executed,
            command_succeeded,
            verification: "unknown".into(),
            snapshot: None,
            preview,
        },
        Ok(after) => {
            if let Some(reason) = after
                .snapshot
                .capabilities
                .iter()
                .find(|c| c.action == preview.request.action)
                .and_then(|c| c.blocked_reason.as_ref())
            {
                return Outcome {
                    status: "unconfirmed".into(),
                    message: format!("原生命令已结束，结果待核对：{reason} 没有自动重试。"),
                    executed,
                    command_succeeded,
                    verification: "unknown".into(),
                    snapshot: Some(after.snapshot),
                    preview,
                };
            }
            let verified = after
                .snapshot
                .capabilities
                .iter()
                .any(|c| c.action == preview.request.action && c.blocked_reason.is_none())
                && output.as_ref().is_ok_and(|o| {
                    o.response_accepted && before.verify(&preview.request, &after, o)
                });
            let changed = before.snapshot.fingerprint != after.snapshot.fingerprint;
            let needs_host = error.as_ref().is_some_and(|e| {
                let e = e.to_lowercase();
                [
                    "approval",
                    "authentication",
                    "credential",
                    "trust",
                    "permission",
                    "command_source_refused",
                    "headers_helper_refused",
                    "not an interactive terminal",
                ]
                .iter()
                .any(|w| e.contains(w))
            });
            let status = if verified {
                "succeeded"
            } else if changed {
                "partial"
            } else if needs_host {
                "needs_host"
            } else if command_succeeded == Some(false) {
                "failed"
            } else {
                "unconfirmed"
            };
            let message = if verified {
                "原生命令完成，所选配置层设置和原生记录已核对。请在宿主新会话中确认实际效果。"
                    .into()
            } else {
                format!(
                    "{}{} 当前可读取的原生状态已保留；没有自动回滚或重试。",
                    if needs_host {
                        "需要在宿主中完成批准、认证或信任。 "
                    } else {
                        "操作未完整通过核验。 "
                    },
                    if needs_host {
                        String::new()
                    } else {
                        error.unwrap_or_default()
                    }
                )
            };
            Outcome {
                status: status.into(),
                message,
                executed,
                command_succeeded,
                verification: if verified { "verified" } else { "observed" }.into(),
                snapshot: Some(after.snapshot),
                preview,
            }
        }
    }
}
