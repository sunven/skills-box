//! Name-collision detection shared by install preview and install execution.
//!
//! The CLI would overwrite a same-named skill (lock key or canonical folder)
//! without asking, so the app must reject conflicts before spawning. Both the
//! installer (ticket 05) and the preview reuse [`find_conflicts`].

use std::collections::HashMap;

use serde::Serialize;

use crate::error::AppError;
use crate::inventory::ManagementState;
use crate::paths::AppPaths;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictInfo {
    pub management: ManagementState,
    pub source: Option<String>,
}

/// Look up existing skills by name. Returns only names that already exist as
/// managed, copy-only, or unmanaged entries.
pub fn find_conflicts(
    paths: &AppPaths,
    names: &[String],
) -> Result<HashMap<String, ConflictInfo>, AppError> {
    if names.is_empty() {
        return Ok(HashMap::new());
    }
    let snapshot = crate::inventory::read(paths)?;
    let wanted: std::collections::HashSet<&str> = names.iter().map(String::as_str).collect();
    let mut conflicts = HashMap::new();
    for skill in snapshot.skills {
        if wanted.contains(skill.name.as_str()) {
            conflicts.insert(
                skill.name,
                ConflictInfo {
                    management: skill.management,
                    source: skill.source,
                },
            );
        }
    }
    Ok(conflicts)
}

pub trait ConflictProvider: Send + Sync {
    fn lookup(&self, names: &[String]) -> HashMap<String, ConflictInfo>;
}

/// Production adapter over the inventory read model. A read failure degrades to
/// "no conflicts known"; the installer re-checks server-side before writing.
#[derive(Debug, Clone)]
pub struct InventoryConflictProvider {
    paths: AppPaths,
}

impl InventoryConflictProvider {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }
}

impl ConflictProvider for InventoryConflictProvider {
    fn lookup(&self, names: &[String]) -> HashMap<String, ConflictInfo> {
        find_conflicts(&self.paths, names).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_skill(home: &std::path::Path, name: &str) {
        let dir = home.join(".agents/skills").join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {name} description\n---\n"),
        )
        .unwrap();
    }

    #[test]
    fn detects_managed_and_unmanaged_names_with_existing_source() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        write_skill(&home, "alpha");
        write_skill(&home, "beta");
        fs::write(
            paths.lock_path(),
            r#"{"version":3,"skills":{"alpha":{"source":"acme/skills","sourceType":"github","sourceUrl":"https://github.com/acme/skills.git","skillPath":"skills/alpha/SKILL.md","skillFolderHash":"0123456789abcdef0123456789abcdef01234567","installedAt":"2026-01-01T00:00:00.000Z","updatedAt":"2026-02-03T04:05:06.000Z"}}}"#,
        )
        .unwrap();

        let names = vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()];
        let conflicts = find_conflicts(&paths, &names).unwrap();

        assert_eq!(conflicts.len(), 2);
        assert_eq!(conflicts["alpha"].management, ManagementState::Managed);
        assert_eq!(conflicts["alpha"].source.as_deref(), Some("acme/skills"));
        assert_eq!(conflicts["beta"].management, ManagementState::Unmanaged);
        assert_eq!(conflicts["beta"].source, None);
        assert!(!conflicts.contains_key("gamma"));
    }

    #[test]
    fn empty_name_list_skips_inventory_read() {
        let paths = AppPaths::new("/nonexistent/home", "/nonexistent/app-data");
        assert!(find_conflicts(&paths, &[]).unwrap().is_empty());
    }
}
