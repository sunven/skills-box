//! Shared Project root path facts, without attachment or scanning policy.
//!
//! Callers choose the physical root (including any historical resolution
//! fallback), whether missing paths are allowed, and how to present failures.
//! Lexical normalization and physical resolution are separate: attachment
//! targets resolve the normalized path, while layout/catalog paths retain
//! their original filesystem semantics, including symlink parents and `..`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::security::normalize_absolute;

#[derive(Debug)]
pub(crate) enum PathError {
    InvalidRoot,
    InvalidPath,
    LexicalEscape(PathBuf),
    Missing(io::Error),
    Inspect { path: PathBuf, error: io::Error },
    NoAncestor,
    Resolve { path: PathBuf, error: io::Error },
    PhysicalEscape(PathBuf),
}

#[derive(Clone, Copy)]
pub(crate) enum Existence {
    Existing,
    AllowMissing,
}

pub(crate) struct PathFacts {
    resolved: PathBuf,
    metadata: fs::Metadata,
}

impl PathFacts {
    pub fn is_dir(&self) -> bool {
        if self.metadata.file_type().is_symlink() {
            self.resolved.is_dir()
        } else {
            self.metadata.is_dir()
        }
    }
}

/// Check lexical containment and return the normalized candidate. Physical
/// containment must still be checked before following or creating the path.
pub(crate) fn lexical(root: &Path, path: &Path) -> Result<PathBuf, PathError> {
    let root = normalize_absolute(root).ok_or(PathError::InvalidRoot)?;
    let path = normalize_absolute(path).ok_or(PathError::InvalidPath)?;
    if !path.starts_with(root) {
        return Err(PathError::LexicalEscape(path));
    }
    Ok(path)
}

/// Resolve an existing path or its nearest existing ancestor. Only NotFound
/// permits climbing: an unreadable ancestor or a broken symlink is a failure.
/// Canonicalizing the whole path also checks symlinks in parent directories.
/// With AllowMissing, facts describe that ancestor; callers still inspect the
/// target's directory state before creating or changing it.
pub(crate) fn resolve(
    path: &Path,
    physical_root: &Path,
    existence: Existence,
) -> Result<PathFacts, PathError> {
    let mut ancestor = path;
    let metadata = loop {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => break metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if matches!(existence, Existence::Existing) {
                    return Err(PathError::Missing(error));
                }
                ancestor = ancestor.parent().ok_or(PathError::NoAncestor)?;
            }
            Err(error) => {
                return Err(PathError::Inspect {
                    path: ancestor.to_path_buf(),
                    error,
                });
            }
        }
    };
    let resolved = fs::canonicalize(ancestor).map_err(|error| PathError::Resolve {
        path: ancestor.to_path_buf(),
        error,
    })?;
    if !resolved.starts_with(physical_root) {
        return Err(PathError::PhysicalEscape(resolved));
    }
    Ok(PathFacts { resolved, metadata })
}

/// Existing read paths require both lexical and physical containment. The
/// supplied root is already resolved by ScopeLayout/ProjectRegistry.
pub(crate) fn existing(root: &Path, path: &Path) -> Result<PathFacts, PathError> {
    lexical(root, path)?;
    resolve(path, root, Existence::Existing)
}
