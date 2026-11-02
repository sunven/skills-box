use super::history::History;
use crate::error::AppError;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

const MAX_BYTES: u64 = 32 * 1024 * 1024;
fn read_error() -> AppError {
    AppError::discovery_read_failed()
}
pub fn load(path: &Path) -> Result<History, AppError> {
    let mut file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(History::default()),
        Err(_) => return Err(read_error()),
    };
    let meta = file.metadata().map_err(|_| read_error())?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return Err(read_error());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| read_error())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(read_error());
    }
    let history: History = serde_json::from_slice(&bytes).map_err(|_| read_error())?;
    if !history.validate() {
        return Err(read_error());
    }
    Ok(history)
}
pub fn persist(path: &Path, history: &History) -> Result<(), AppError> {
    let write = || -> std::io::Result<()> {
        if fs::symlink_metadata(path).is_ok_and(|m| !m.is_file()) {
            return Err(std::io::Error::other("discovery must be a regular file"));
        }
        let raw = serde_json::to_vec(history)?;
        if raw.len() as u64 > MAX_BYTES {
            return Err(std::io::Error::other("history too large"));
        }
        let parent = path
            .parent()
            .ok_or_else(|| std::io::Error::other("no parent"))?;
        fs::create_dir_all(parent)?;
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
        temp.write_all(&raw)?;
        temp.as_file().sync_all()?;
        temp.persist(path).map_err(|e| e.error)?;
        Ok(())
    };
    write().map_err(|_| AppError::discovery_write_failed())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_round_trip_corrupt_unknown_version_and_symlink_preserve_history() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("discovery.json");
        let h = load(&path).unwrap();
        persist(&path, &h).unwrap();
        assert_eq!(load(&path).unwrap().version, 1);
        for raw in [
            "broken",
            r#"{"version":2,"sources":{},"scopes":{},"cooldown":null}"#,
        ] {
            fs::write(&path, raw).unwrap();
            assert!(load(&path).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), raw);
        }
        fs::remove_file(&path).unwrap();
        let target = tmp.path().join("target");
        fs::write(&target, "keep").unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(load(&path).is_err());
        assert!(persist(&path, &h).is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "keep");
    }
}
