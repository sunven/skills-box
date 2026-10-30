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
use crate::scope_layout::ScopeLayout;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictInfo {
    pub management: ManagementState,
    pub source: Option<String>,
}

/// Look up existing skills by name. Returns only names that already exist as
/// managed, copy-only, or unmanaged entries.
///
/// Stale lock entries have no folder and are therefore dropped from the
/// inventory snapshot, but they still occupy the lock key: installing the same
/// name would make the CLI overwrite that entry, so they are reported as
/// managed conflicts too (spec §7.1).
pub fn find_conflicts(
    paths: &AppPaths,
    layout: &ScopeLayout,
    names: &[String],
) -> Result<HashMap<String, ConflictInfo>, AppError> {
    if names.is_empty() {
        return Ok(HashMap::new());
    }
    let catalog = layout.catalog(paths)?;
    let mut conflicts = HashMap::new();
    for name in names {
        if let Some(located) = crate::inventory::locate_in_catalog(&catalog, name) {
            let source = located.lock_entry.as_ref().map(|entry| {
                crate::update_target::resolve_source(entry)
                    .ok()
                    .unwrap_or_else(|| entry.source.clone())
            });
            conflicts.insert(
                name.clone(),
                ConflictInfo {
                    management: located.management,
                    source,
                },
            );
            continue;
        }
        // Present in the strict scope lock but absent from the snapshot: a
        // stale lock entry. Never install over it silently.
        if let Some(entry) = catalog.lock.get(name) {
            let source = crate::update_target::resolve_source(entry)
                .ok()
                .or_else(|| Some(entry.source.clone()));
            conflicts.insert(
                name.clone(),
                ConflictInfo {
                    management: ManagementState::Managed,
                    source,
                },
            );
        }
    }
    Ok(conflicts)
}

pub trait ConflictProvider: Send + Sync {
    fn lookup(&self, layout: &ScopeLayout, names: &[String]) -> HashMap<String, ConflictInfo>;
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
    fn lookup(&self, layout: &ScopeLayout, names: &[String]) -> HashMap<String, ConflictInfo> {
        find_conflicts(&self.paths, layout, names).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn find_conflicts(
        paths: &AppPaths,
        names: &[String],
    ) -> Result<HashMap<String, ConflictInfo>, AppError> {
        let layout = ScopeLayout::global(paths, Vec::new());
        super::find_conflicts(paths, &layout, names)
    }

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

    #[test]
    fn stale_lock_entries_still_reserve_their_name() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let paths = AppPaths::new(&home, tmp.path().join("app-data"));
        fs::create_dir_all(paths.agents_root()).unwrap();
        // Lock entry with no canonical folder and no agent copy anywhere.
        fs::write(
            paths.lock_path(),
            r#"{"version":3,"skills":{"ghost":{"source":"acme/skills","sourceType":"github","sourceUrl":"https://github.com/acme/skills.git","skillPath":"skills/ghost/SKILL.md","skillFolderHash":"0123456789abcdef0123456789abcdef01234567","installedAt":"2026-01-01T00:00:00.000Z","updatedAt":"2026-02-03T04:05:06.000Z"}}}"#,
        )
        .unwrap();

        let conflicts = find_conflicts(&paths, &["ghost".to_string()]).unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts["ghost"].management, ManagementState::Managed);
        assert_eq!(conflicts["ghost"].source.as_deref(), Some("acme/skills"));
    }
}
