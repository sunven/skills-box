//! App-level credentials. Only presence and source cross the read IPC seam.
//! Secrets are never forwarded to CLI/Git processes or included in Debug.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::paths::AppPaths;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TokenSource {
    Settings,
    Environment,
    Anonymous,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubSettingsStatus {
    pub has_saved_token: bool,
    pub has_environment_token: bool,
    pub effective_source: TokenSource,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsFile {
    version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    github_token: Option<String>,
    #[serde(default)]
    data_management: DataPreferences,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DataPreferences {
    pub retain_backups: bool,
    pub operation_logging: bool,
}
impl Default for DataPreferences {
    fn default() -> Self {
        Self {
            retain_backups: true,
            operation_logging: true,
        }
    }
}
impl Default for SettingsFile {
    fn default() -> Self {
        Self {
            version: 1,
            github_token: None,
            data_management: DataPreferences::default(),
            extra: Default::default(),
        }
    }
}

pub(crate) fn data_preferences(paths: &AppPaths) -> DataPreferences {
    load(&paths.settings_path())
        .map(|config| config.data_management)
        .unwrap_or_default()
}

pub(crate) struct Credentials {
    pub source: TokenSource,
    pub token: Option<String>,
    pub revision: u64,
}

pub struct SettingsService {
    paths: AppPaths,
    saved: Mutex<Result<SettingsFile, AppError>>,
    environment: Option<String>,
    revision: AtomicU64,
}

impl std::fmt::Debug for SettingsService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsService").finish_non_exhaustive()
    }
}

impl SettingsService {
    pub fn from_process(paths: AppPaths) -> Self {
        Self::new(paths, std::env::var("GITHUB_TOKEN").ok())
    }

    /// Injectable startup environment; tests never inspect real credentials.
    pub fn new(paths: AppPaths, environment: Option<String>) -> Self {
        let saved = load(&paths.settings_path());
        Self {
            paths,
            saved: Mutex::new(saved),
            revision: AtomicU64::new(0),
            environment: environment
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
        }
    }

    pub fn status(&self) -> Result<GitHubSettingsStatus, AppError> {
        let saved = self.saved.lock().unwrap_or_else(|p| p.into_inner());
        self.status_for(
            saved
                .as_ref()
                .map_err(Clone::clone)?
                .github_token
                .as_deref(),
        )
    }

    fn status_for(&self, saved: Option<&str>) -> Result<GitHubSettingsStatus, AppError> {
        Ok(GitHubSettingsStatus {
            has_saved_token: saved.is_some(),
            has_environment_token: self.environment.is_some(),
            effective_source: if saved.is_some() {
                TokenSource::Settings
            } else if self.environment.is_some() {
                TokenSource::Environment
            } else {
                TokenSource::Anonymous
            },
        })
    }

    pub(crate) fn credentials(&self) -> Result<Credentials, AppError> {
        let saved = self.saved.lock().unwrap_or_else(|p| p.into_inner());
        let token = &saved.as_ref().map_err(Clone::clone)?.github_token;
        let source = self.status_for(token.as_deref())?.effective_source;
        let token = token.as_ref().or(self.environment.as_ref());
        Ok(Credentials {
            source,
            revision: self.revision.load(Ordering::SeqCst),
            token: token.map(|value| normalize_token(value)).transpose()?,
        })
    }

    pub fn data_preferences(&self) -> Result<DataPreferences, AppError> {
        let saved = self.saved.lock().unwrap_or_else(|p| p.into_inner());
        Ok(saved.as_ref().map_err(Clone::clone)?.data_management)
    }

    pub fn save_data_preferences(
        &self,
        preferences: DataPreferences,
    ) -> Result<DataPreferences, AppError> {
        let mut saved = self.saved.lock().unwrap_or_else(|p| p.into_inner());
        let mut config = saved.as_ref().map_err(Clone::clone)?.clone();
        config.data_management = preferences;
        persist(&self.paths.settings_path(), &config)?;
        *saved = Ok(config);
        Ok(preferences)
    }

    pub fn save(&self, token: &str) -> Result<GitHubSettingsStatus, AppError> {
        self.replace(Some(normalize_token(token)?))
    }

    pub fn clear(&self) -> Result<GitHubSettingsStatus, AppError> {
        self.replace(None)
    }

    fn replace(&self, token: Option<String>) -> Result<GitHubSettingsStatus, AppError> {
        let mut saved = self.saved.lock().unwrap_or_else(|p| p.into_inner());
        let mut config = saved.as_ref().cloned().unwrap_or_default();
        config.github_token = token;
        persist(&self.paths.settings_path(), &config)?;
        *saved = Ok(config);
        self.revision.fetch_add(1, Ordering::SeqCst);
        self.status_for(
            saved
                .as_ref()
                .map_err(Clone::clone)?
                .github_token
                .as_deref(),
        )
    }
}

fn normalize_token(raw: &str) -> Result<String, AppError> {
    let token = raw.trim();
    if token.is_empty() || token.len() > 4_096 || !token.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(AppError::settings_invalid_token());
    }
    Ok(token.to_string())
}

fn load(path: &Path) -> Result<SettingsFile, AppError> {
    let mut file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SettingsFile::default());
        }
        Err(_) => return Err(AppError::settings_read_failed()),
    };
    let metadata = file
        .metadata()
        .map_err(|_| AppError::settings_read_failed())?;
    if !metadata.is_file() || metadata.len() > 65_536 {
        return Err(AppError::settings_read_failed());
    }
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|_| AppError::settings_read_failed())?;
    let mut raw = String::new();
    (&mut file)
        .take(65_537)
        .read_to_string(&mut raw)
        .map_err(|_| AppError::settings_read_failed())?;
    if raw.len() > 65_536 {
        return Err(AppError::settings_read_failed());
    }
    let mut config: SettingsFile =
        serde_json::from_str(&raw).map_err(|_| AppError::settings_read_failed())?;
    if config.version != 1 {
        return Err(AppError::settings_read_failed());
    }
    config.github_token = config
        .github_token
        .map(|token| normalize_token(&token).map_err(|_| AppError::settings_read_failed()))
        .transpose()?;
    Ok(config)
}

fn persist(path: &Path, config: &SettingsFile) -> Result<(), AppError> {
    let write = || -> std::io::Result<()> {
        if let Ok(metadata) = fs::symlink_metadata(path)
            && !metadata.is_file()
        {
            return Err(std::io::Error::other("settings must be a regular file"));
        }
        let parent = path
            .parent()
            .ok_or_else(|| std::io::Error::other("missing parent"))?;
        fs::create_dir_all(parent)?;
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
        let raw = serde_json::to_vec(config)?;
        temp.write_all(&raw)?;
        temp.write_all(b"\n")?;
        temp.as_file().sync_all()?;
        temp.persist(path).map_err(|error| error.error)?;
        Ok(())
    };
    write().map_err(|_| AppError::settings_write_failed())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use std::os::unix::fs::symlink;

    fn paths(tmp: &tempfile::TempDir) -> AppPaths {
        AppPaths::new(tmp.path().join("home"), tmp.path().join("app"))
    }

    #[test]
    fn save_overrides_environment_clear_restores_it_and_restart_preserves_configuration() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths(&tmp);
        let service = SettingsService::new(paths.clone(), Some("  dummy-env-token  ".into()));
        assert_eq!(
            service.credentials().unwrap().token.as_deref(),
            Some("dummy-env-token")
        );
        let status = service.save("\n dummy-saved-token \n").unwrap();
        assert_eq!(status.effective_source, TokenSource::Settings);
        assert!(status.has_saved_token && status.has_environment_token);
        assert_eq!(
            service.credentials().unwrap().token.as_deref(),
            Some("dummy-saved-token")
        );
        let restarted = SettingsService::new(paths.clone(), None);
        assert_eq!(
            restarted.credentials().unwrap().token.as_deref(),
            Some("dummy-saved-token")
        );
        assert_eq!(
            service.clear().unwrap().effective_source,
            TokenSource::Environment
        );
        assert_eq!(
            service.credentials().unwrap().token.as_deref(),
            Some("dummy-env-token")
        );
        assert_eq!(
            SettingsService::new(paths, None)
                .status()
                .unwrap()
                .effective_source,
            TokenSource::Anonymous
        );
    }

    #[test]
    fn missing_and_blank_environment_use_anonymous_requests() {
        let tmp = tempfile::tempdir().unwrap();
        for env in [None, Some(" \n\t ".into())] {
            let service = SettingsService::new(paths(&tmp), env);
            assert_eq!(
                service.status().unwrap().effective_source,
                TokenSource::Anonymous
            );
            assert!(service.credentials().unwrap().token.is_none());
        }
    }

    #[test]
    fn invalid_input_does_not_replace_current_token_or_echo_input() {
        let tmp = tempfile::tempdir().unwrap();
        let service = SettingsService::new(paths(&tmp), None);
        service.save("original-dummy-secret").unwrap();
        for value in ["", "  ", "bad secret", "bad\nsecret", "bad\0secret", "令牌"] {
            let error = service.save(value).unwrap_err();
            assert_eq!(error.code, ErrorCode::SettingsInvalidToken);
            assert!(!serde_json::to_string(&error).unwrap().contains("bad"));
        }
        assert!(service.save(&"x".repeat(4_097)).is_err());
        assert_eq!(
            service.credentials().unwrap().token.as_deref(),
            Some("original-dummy-secret")
        );
    }

    #[test]
    fn settings_and_replacement_files_are_private_without_temporary_leftovers() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths(&tmp);
        let service = SettingsService::new(paths.clone(), None);
        for token in ["first-dummy-secret", "second-dummy-secret"] {
            service.save(token).unwrap();
            assert_eq!(
                fs::metadata(paths.settings_path())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
            assert_eq!(fs::read_dir(paths.app_data()).unwrap().count(), 1);
        }
        fs::set_permissions(paths.settings_path(), fs::Permissions::from_mode(0o644)).unwrap();
        let restarted = SettingsService::new(paths.clone(), None);
        assert!(restarted.status().unwrap().has_saved_token);
        assert_eq!(
            fs::metadata(paths.settings_path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        service.clear().unwrap();
        assert!(
            !fs::read_to_string(paths.settings_path())
                .unwrap()
                .contains("secret")
        );
    }

    #[test]
    fn corrupt_configuration_fails_closed_and_can_be_repaired_or_cleared() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths(&tmp);
        fs::create_dir_all(paths.app_data()).unwrap();
        for raw in [
            "{\"githubToken\":\"do-not-echo-this-secret\" broken",
            "{\"version\":2,\"githubToken\":\"do-not-echo-this-secret\"}",
            "{\"version\":1,\"githubToken\":\"bad secret\"}",
        ] {
            fs::write(paths.settings_path(), raw).unwrap();
            let service = SettingsService::new(paths.clone(), Some("dummy-env-token".into()));
            let error = service.status().unwrap_err();
            assert_eq!(error.code, ErrorCode::SettingsReadFailed);
            assert!(!format!("{error:?}").contains("secret"));
            assert!(service.credentials().is_err());
            service.save("repaired-dummy-secret").unwrap();
            assert_eq!(
                service.status().unwrap().effective_source,
                TokenSource::Settings
            );
        }
        fs::write(paths.settings_path(), "not json").unwrap();
        let service = SettingsService::new(paths, Some("dummy-env-token".into()));
        assert_eq!(
            service.clear().unwrap().effective_source,
            TokenSource::Environment
        );
    }

    #[test]
    fn symlinks_are_rejected_and_failed_save_preserves_the_original_file_and_memory() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths(&tmp);
        let service = SettingsService::new(paths.clone(), None);
        service.save("original-dummy-secret").unwrap();
        let original = paths.app_data().join("original.json");
        fs::rename(paths.settings_path(), &original).unwrap();
        symlink(&original, paths.settings_path()).unwrap();
        let before = fs::read(&original).unwrap();
        assert_eq!(
            service.save("replacement-dummy-secret").unwrap_err().code,
            ErrorCode::SettingsWriteFailed
        );
        assert_eq!(
            service.clear().unwrap_err().code,
            ErrorCode::SettingsWriteFailed
        );
        assert_eq!(fs::read(original).unwrap(), before);
        assert_eq!(
            service.credentials().unwrap().token.as_deref(),
            Some("original-dummy-secret")
        );
        let restarted = SettingsService::new(paths, Some("dummy-env-token".into()));
        assert_eq!(
            restarted.status().unwrap_err().code,
            ErrorCode::SettingsReadFailed
        );
    }

    #[test]
    fn status_and_debug_never_return_stored_or_environment_secrets() {
        let tmp = tempfile::tempdir().unwrap();
        let service = SettingsService::new(paths(&tmp), Some("dummy-env-secret".into()));
        let status = service.save("dummy-saved-secret").unwrap();
        let text = format!("{service:?} {}", serde_json::to_string(&status).unwrap());
        assert!(!text.contains("dummy-env-secret"));
        assert!(!text.contains("dummy-saved-secret"));
        assert_eq!(
            serde_json::to_value(status).unwrap()["effectiveSource"],
            "settings"
        );
    }

    #[test]
    fn invalid_environment_is_not_used_when_a_saved_token_is_available() {
        let tmp = tempfile::tempdir().unwrap();
        let service = SettingsService::new(paths(&tmp), Some("invalid secret".into()));
        assert!(service.credentials().is_err());
        service.save("valid-dummy-secret").unwrap();
        assert_eq!(
            service.credentials().unwrap().token.as_deref(),
            Some("valid-dummy-secret")
        );
    }
}
