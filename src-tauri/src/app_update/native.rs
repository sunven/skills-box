use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use async_trait::async_trait;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use super::{Candidate, Driver, Progress, Release};
use crate::AppState;
use crate::error::AppError;

pub struct NativeDriver(pub AppHandle);

#[async_trait]
impl Driver for NativeDriver {
    async fn check(&self) -> Result<Option<Arc<dyn Release>>, AppError> {
        let updater = self
            .0
            .updater_builder()
            .timeout(Duration::from_secs(300))
            .version_comparator(|current, remote| {
                remote.version.pre.is_empty() && remote.version > current
            })
            .build()
            .map_err(AppError::app_update_check_failed)?;
        let result = tokio::time::timeout(Duration::from_secs(30), updater.check())
            .await
            .map_err(AppError::app_update_check_failed)?
            .map_err(AppError::app_update_check_failed)?;
        match result {
            Some(update) if update.download_url.scheme() == "https" => {
                Ok(Some(Arc::new(NativeRelease(update))))
            }
            Some(_) => Err(AppError::app_update_check_failed(
                "更新包下载地址必须使用 HTTPS。",
            )),
            None => Ok(None),
        }
    }

    async fn prepare_restart(&self) -> Result<(), AppError> {
        let state = self.0.state::<AppState>();
        state.discovery.shutdown();
        state.tasks.shutdown_for_restart().await?;
        state.discovery.shutdown_and_wait().await;
        Ok(())
    }

    fn restart(&self) {
        self.0
            .state::<AppState>()
            .exiting
            .store(true, Ordering::SeqCst);
        self.0.request_restart();
    }
}

struct NativeRelease(Update);

#[async_trait]
impl Release for NativeRelease {
    fn candidate(&self) -> Candidate {
        Candidate {
            version: self.0.version.clone(),
            notes: self.0.body.clone(),
        }
    }

    async fn download(&self, progress: Progress) -> Result<Vec<u8>, AppError> {
        let finished = progress.clone();
        self.0
            .download(
                move |chunk, total| progress(chunk as u64, total, false),
                move || finished(0, None, true),
            )
            .await
            .map_err(|error| match error {
                tauri_plugin_updater::Error::Minisign(_)
                | tauri_plugin_updater::Error::Base64(_)
                | tauri_plugin_updater::Error::SignatureUtf8(_)
                | tauri_plugin_updater::Error::SignedVersionMismatch { .. }
                | tauri_plugin_updater::Error::MissingSignedVersion => {
                    AppError::app_update_signature_failed()
                }
                other => AppError::app_update_download_failed(other),
            })
    }

    fn install(&self, bytes: &[u8]) -> Result<(), AppError> {
        ensure_writable_volume()?;
        self.0
            .install(bytes)
            .map_err(AppError::app_update_install_failed)
    }
}

fn ensure_writable_volume() -> Result<(), AppError> {
    let exe = std::env::current_exe().map_err(AppError::app_update_install_failed)?;
    let app = exe
        .ancestors()
        .find(|path| path.extension().is_some_and(|ext| ext == "app"))
        .ok_or_else(|| AppError::app_update_install_failed("未找到当前运行的 .app 安装目录。"))?;
    let path =
        CString::new(app.as_os_str().as_bytes()).map_err(AppError::app_update_install_failed)?;
    let mut filesystem = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: path is NUL-terminated and statfs initializes filesystem on success.
    if unsafe { libc::statfs(path.as_ptr(), filesystem.as_mut_ptr()) } != 0 {
        return Err(AppError::app_update_install_failed(
            std::io::Error::last_os_error(),
        ));
    }
    // SAFETY: the call above succeeded and initialized every field.
    let filesystem = unsafe { filesystem.assume_init() };
    if filesystem.f_flags & libc::MNT_RDONLY as u32 != 0 {
        return Err(AppError::app_update_read_only());
    }
    Ok(())
}
