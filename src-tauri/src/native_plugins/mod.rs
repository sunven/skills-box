//! Native Plugin boundary. Installation truth belongs to the host. Stored
//! records describe only operations and never replace the native inventory.
mod contracts;
mod inventory;
mod model;
mod operations;
#[cfg(test)]
mod tests;
mod transport;
use crate::{paths::AppPaths, projects::ProjectRegistry, tasks::TaskManager};
use inventory::Observation;
pub use model::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    sync::{Arc, Mutex},
};
use transport::Runtime;

#[derive(Clone)]
pub struct Service(Arc<Inner>);
struct Inner {
    paths: AppPaths,
    tasks: TaskManager,
    projects: Arc<ProjectRegistry>,
    previews: Mutex<HashMap<String, (Preview, u64)>>,
    contracts: Mutex<HashMap<String, contracts::Contract>>,
    #[cfg(test)]
    runtime: Mutex<Option<Runtime>>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationRecord {
    pub phase: String,
    pub preview: Preview,
    pub outcome: Option<Outcome>,
    pub message: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageState {
    pub snapshot: Option<Snapshot>,
    pub error: Option<String>,
    pub operation: Option<OperationRecord>,
}
impl Service {
    pub fn new(paths: AppPaths, tasks: TaskManager, projects: Arc<ProjectRegistry>) -> Self {
        Self(Arc::new(Inner {
            paths,
            tasks,
            projects,
            previews: Mutex::new(HashMap::new()),
            contracts: Mutex::new(HashMap::new()),
            #[cfg(test)]
            runtime: Mutex::new(None),
        }))
    }
    fn runtime(&self, host: Host) -> Result<Runtime, String> {
        #[cfg(test)]
        if let Some(rt) = self.0.runtime.lock().unwrap().clone() {
            return Ok(rt);
        }
        Runtime::discover(host, &self.0.paths)
    }
    fn observation(&self, rt: &Runtime) -> Result<Observation, String> {
        let version = rt.version()?;
        let key = contracts::key(rt, &version);
        let contract = self
            .0
            .contracts
            .lock()
            .map_err(|_| "兼容性状态不可用")?
            .get(&key)
            .cloned()
            .unwrap_or_else(|| contracts::defaults(rt.host));
        let projects = self.0.projects.list();
        let roots = projects
            .projects
            .iter()
            .map(|p| p.project.root.clone())
            .collect::<Vec<_>>();
        let mut observation = inventory::load(rt, contract.capabilities, &roots)?;
        if observation.snapshot.version != version {
            return Err("CLI 在读取期间变化，请重新检测。".into());
        }
        observation.snapshot.source_kinds = contract.sources;
        if !projects.warnings.is_empty() {
            for m in &mut observation.snapshot.markets {
                m.remove_blocked_reason = Some("项目注册信息读取不完整，无法确认移除影响。".into());
            }
        }
        Ok(observation)
    }
    fn record_path(&self, host: Host) -> std::path::PathBuf {
        self.0
            .paths
            .runtime_dir()
            .join(format!("native-plugin-{}.json", host.command()))
    }
    fn write_record(&self, record: &OperationRecord) -> Result<(), String> {
        crate::fsutil::write_atomic(
            &self.record_path(record.preview.request.host),
            &serde_json::to_vec_pretty(record).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("无法保存插件操作记录：{e}"))
    }
    fn record(&self, host: Host) -> Result<Option<OperationRecord>, String> {
        match fs::read(self.record_path(host)) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|_| "插件操作记录不可读；上次操作可能中断，请在宿主核对。".into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
    pub async fn read(&self, host: Host) -> Result<PageState, String> {
        let service = self.clone();
        tokio::task::spawn_blocking(move||{
            let mut operation=service.record(host)?;
            let active=service.0.tasks.active_mutation().is_some_and(|m|m.kind==crate::interrupted::MutationKind::NativePlugin);
            if let Some(r)=&mut operation {if matches!(r.phase.as_str(),"writing"|"verifying")&&!active{r.phase="interrupted".into();r.message=Some("上次操作异常中断。请核对当前可读取的原生状态；命令是否完整结束仍未知，必要时在宿主处理。".into());service.write_record(r)?;}}
            if active{return Ok(PageState{snapshot:None,error:Some("原生操作正在执行，等待写入和核验结束。".into()),operation});}
            match service.runtime(host).and_then(|rt|service.observation(&rt)) {
                Ok(obs)=>Ok(PageState{snapshot:Some(obs.snapshot),error:None,operation}),
                Err(e)=>Ok(PageState{snapshot:None,error:Some(e),operation}),
            }
        }).await.map_err(|e|e.to_string())?
    }
    pub async fn prepare(&self, request: Request) -> Result<Preview, String> {
        let service = self.clone();
        tokio::task::spawn_blocking(move || {
            let rt = service.runtime(request.host)?;
            let obs = service.observation(&rt)?;
            let args = operations::arguments(&request, &obs)?;
            let fingerprint = operations::confirmation_fingerprint(&request, &obs)?;
            let (consequences, affected_plugins) = operations::consequences(&request, &obs);
            let token = uuid::Uuid::new_v4().to_string();
            let command = std::iter::once(rt.executable.to_string_lossy().into_owned())
                .chain(args)
                .map(|a| shell_display(&a))
                .collect::<Vec<_>>()
                .join(" ");
            let preview = Preview {
                token: token.clone(),
                request,
                config_root: obs.snapshot.config_root,
                version: obs.snapshot.version,
                command,
                consequences,
                affected_plugins,
                fingerprint,
            };
            let mut previews = service.0.previews.lock().map_err(|_| "确认状态不可用")?;
            previews.retain(|_, (_, time)| crate::time::now_ms().saturating_sub(*time) < 600_000);
            if previews.len() >= 16 {
                previews.clear();
            }
            previews.insert(token, (preview.clone(), crate::time::now_ms()));
            Ok(preview)
        })
        .await
        .map_err(|e| e.to_string())?
    }
    pub async fn execute(&self, token: String) -> Result<Outcome, String> {
        let (preview, created) = self
            .0
            .previews
            .lock()
            .map_err(|_| "确认状态不可用")?
            .remove(&token)
            .ok_or("确认已失效，请重新预览。")?;
        if crate::time::now_ms().saturating_sub(created) > 600_000 {
            return Err("确认已过期，请重新预览。".into());
        }
        // Acquire and register before spawning, move ownership into the worker.
        // Dropping the IPC future cannot release the write slot or kill a child.
        let guard = self
            .0
            .tasks
            .begin_native_plugin(format!(
                "{} · {} · {}",
                preview.request.host.command(),
                preview.request.action.label(),
                if preview.request.action == Action::AddMarket {
                    &preview.request.source
                } else {
                    &preview.request.target
                }
            ))
            .map_err(|e| e.message)?;
        let service = self.clone();
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            let rt = service.runtime(preview.request.host)?;
            let before = service.observation(&rt)?;
            operations::arguments(&preview.request, &before)?;
            if operations::confirmation_fingerprint(&preview.request, &before)?
                != preview.fingerprint
            {
                return Ok(Outcome {
                    status: "conflict".into(),
                    message:
                        "原生配置、目录、CLI 或安装记录已变化。请重新读取并再次确认；本次没有写入。"
                            .into(),
                    executed: false,
                    command_succeeded: None,
                    verification: "changed".into(),
                    snapshot: Some(before.snapshot),
                    preview,
                });
            }
            let mut record = OperationRecord {
                phase: "writing".into(),
                preview: preview.clone(),
                outcome: None,
                message: None,
            };
            service.write_record(&record)?;
            let output = operations::execute(&rt, &preview.request, &before);
            record.phase = "verifying".into();
            let record_error = service.write_record(&record).err();
            let mut after = service.observation(&rt);
            let contract_changed = output.as_ref().is_ok_and(|out| {
                out.success
                    && (!operations::response_matches(&preview.request, out)
                        || after.as_ref().is_ok_and(|next| {
                            next.snapshot.capabilities.iter().any(|c| {
                                c.action == preview.request.action && c.blocked_reason.is_none()
                            }) && !operations::postcondition(&preview.request, &before, next, out)
                        }))
            });
            if contract_changed {
                let reason = if matches!(preview.request.action, Action::SyncMarket | Action::RemoveMarket) {
                    "原生命令返回或写后状态不符合预期；此操作已停用，请在宿主中核对。基本兼容性检查不覆盖此操作。"
                } else {
                    "原生命令返回或写后状态不符合预期；此操作已停用，请运行检查兼容性后重试。"
                }.to_string();
                if let Ok(mut cache) = service.0.contracts.lock() {
                    let key = contracts::key(&rt, &before.snapshot.version);
                    let contract = cache
                        .entry(key)
                        .or_insert_with(|| contracts::defaults(rt.host));
                    if let Some(c) = contract
                        .capabilities
                        .iter_mut()
                        .find(|c| c.action == preview.request.action)
                    {
                        c.blocked_reason = Some(reason.clone());
                    }
                }
                if let Ok(next) = &mut after {
                    if let Some(c) = next
                        .snapshot
                        .capabilities
                        .iter_mut()
                        .find(|c| c.action == preview.request.action)
                    {
                        c.blocked_reason = Some(reason);
                    }
                }
            }
            let mut outcome = operations::outcome(preview, &before, after, output);
            if let Some(e) = record_error {
                outcome.message.push_str(&format!(" 操作记录更新失败：{e}"));
            }
            record.phase = "finished".into();
            record.outcome = Some(outcome.clone());
            if let Err(e) = service.write_record(&record) {
                outcome.message.push_str(&format!(" {e}"));
            }
            Ok(outcome)
        })
        .await
        .map_err(|e| format!("插件 worker 异常结束，操作记录保留，结果待核对：{e}"))?
    }
    pub async fn details(&self, host: Host, id: String) -> Result<Details, String> {
        let service = self.clone();
        tokio::task::spawn_blocking(move || {
            let rt = service.runtime(host)?;
            let obs = service.observation(&rt)?;
            let p = obs
                .snapshot
                .plugins
                .iter()
                .find(|p| p.id == id)
                .ok_or("插件已不存在。")?;
            Ok(inventory::details(p))
        })
        .await
        .map_err(|e| e.to_string())?
    }
    pub async fn verify(&self, host: Host) -> Result<PageState, String> {
        let service = self.clone();
        let guard = self
            .0
            .tasks
            .begin_native_plugin(format!("{} · 隔离兼容性验证", host.command()))
            .map_err(|e| e.message)?;
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            let rt = service.runtime(host)?;
            let version = rt.version()?;
            let key = contracts::key(&rt, &version);
            let tested = contracts::verify(&rt)?;
            let mut cache = service.0.contracts.lock().map_err(|_| "兼容性状态不可用")?;
            cache
                .entry(key)
                .or_insert_with(|| contracts::defaults(host))
                .apply_probe(&tested);
            Ok::<(), String>(())
        })
        .await
        .map_err(|e| e.to_string())??;
        self.read(host).await
    }
}
fn shell_display(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || "/-_@.:".contains(c))
    {
        s.into()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

#[tauri::command]
pub async fn read_native_plugins(
    state: tauri::State<'_, crate::AppState>,
    host: Host,
) -> Result<PageState, String> {
    state.native_plugins.read(host).await
}
#[tauri::command]
pub async fn prepare_native_plugin(
    state: tauri::State<'_, crate::AppState>,
    request: Request,
) -> Result<Preview, String> {
    state.native_plugins.prepare(request).await
}
#[tauri::command]
pub async fn execute_native_plugin(
    state: tauri::State<'_, crate::AppState>,
    token: String,
) -> Result<Outcome, String> {
    state.native_plugins.execute(token).await
}
#[tauri::command]
pub async fn native_plugin_details(
    state: tauri::State<'_, crate::AppState>,
    host: Host,
    id: String,
) -> Result<Details, String> {
    state.native_plugins.details(host, id).await
}
#[tauri::command]
pub async fn verify_native_plugin_host(
    state: tauri::State<'_, crate::AppState>,
    host: Host,
) -> Result<PageState, String> {
    state.native_plugins.verify(host).await
}
