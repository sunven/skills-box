//! Per-source recursive tree cache (6h TTL), stored under app data `cache/`.
//!
//! Cache entries are keyed by `(source, ref)` and validated on read: a cache
//! file that names a different source/ref or a future schema version is a miss.
//! Writes are atomic; a read/write failure never fails an update check.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::github::RemoteTree;
use crate::paths::AppPaths;

pub const UPDATE_CACHE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedSourceTree {
    pub version: u32,
    pub source: String,
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    pub fetched_at_epoch_ms: u64,
    pub tree: RemoteTree,
}

#[derive(Debug, Clone)]
pub struct UpdateCache {
    dir: PathBuf,
}

impl UpdateCache {
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            dir: paths.cache_dir(),
        }
    }

    pub fn from_dir(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Stable filename for a source/ref pair. The source/ref never appears in
    /// the filesystem path, so untrusted lock values cannot escape `cache/`.
    pub fn path_for(&self, source: &str, reference: Option<&str>) -> PathBuf {
        self.dir
            .join(format!("{}.json", cache_key(source, reference)))
    }

    pub fn load(&self, source: &str, reference: Option<&str>) -> Option<CachedSourceTree> {
        let raw = fs::read_to_string(self.path_for(source, reference)).ok()?;
        let cached: CachedSourceTree = serde_json::from_str(&raw).ok()?;
        if cached.version != UPDATE_CACHE_VERSION
            || !cached.source.eq_ignore_ascii_case(source)
            || cached.reference.as_deref() != reference
        {
            return None;
        }
        Some(cached)
    }

    pub fn store(
        &self,
        source: &str,
        reference: Option<&str>,
        tree: &RemoteTree,
        fetched_at_epoch_ms: u64,
    ) -> std::io::Result<CachedSourceTree> {
        fs::create_dir_all(&self.dir)?;
        let cached = CachedSourceTree {
            version: UPDATE_CACHE_VERSION,
            source: source.to_string(),
            reference: reference.map(str::to_string),
            fetched_at_epoch_ms,
            tree: tree.clone(),
        };
        let serialized = serde_json::to_string(&cached)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        let destination = self.path_for(source, reference);
        crate::fsutil::write_atomic(&destination, serialized.as_bytes())?;
        Ok(cached)
    }
}

/// `now` is fresh while `now < fetched_at + ttl`. A cache timestamp in the
/// future (clock skew) is treated as fresh.
pub fn is_fresh(fetched_at_epoch_ms: u64, now_epoch_ms: u64, ttl_ms: u64) -> bool {
    now_epoch_ms < fetched_at_epoch_ms.saturating_add(ttl_ms)
}

pub fn cache_key(source: &str, reference: Option<&str>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source.trim().to_ascii_lowercase().as_bytes());
    hasher.update([0]);
    hasher.update(reference.unwrap_or("HEAD").as_bytes());
    let digest = hasher.finalize();
    let mut output = String::with_capacity(68);
    output.push_str("update-tree-");
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::github::RemoteTreeEntry;
    use std::collections::BTreeMap;

    fn tree() -> RemoteTree {
        let mut entries = BTreeMap::new();
        entries.insert(
            "skills/alpha/SKILL.md".to_string(),
            RemoteTreeEntry::new(
                "blob",
                "882a458e7b47d67722f6aeccb51f16b1842f1a19",
                Some("100644"),
            ),
        );
        RemoteTree::new("1aee802d9139d4df84d017de44fb1b37a7b614e9", false, entries)
    }

    #[test]
    fn round_trips_a_source_tree_and_rejects_corrupt_or_foreign_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = UpdateCache::from_dir(tmp.path().join("cache"));

        let stored = cache
            .store("acme/skills", Some("main"), &tree(), 1_000)
            .unwrap();
        assert_eq!(stored.version, UPDATE_CACHE_VERSION);
        let loaded = cache.load("acme/skills", Some("main")).unwrap();
        assert_eq!(loaded.tree, tree());
        assert_eq!(loaded.fetched_at_epoch_ms, 1_000);

        assert!(cache.load("other/skills", Some("main")).is_none());
        assert!(cache.load("acme/skills", None).is_none());

        fs::write(cache.path_for("acme/skills", Some("main")), "{not json").unwrap();
        assert!(cache.load("acme/skills", Some("main")).is_none());
    }

    #[test]
    fn keys_are_stable_and_never_embed_source_paths() {
        assert_eq!(
            cache_key("Acme/Skills", Some("main")),
            cache_key("acme/skills", Some("main"))
        );
        assert_ne!(
            cache_key("acme/skills", Some("main")),
            cache_key("acme/skills", Some("dev"))
        );
        assert_ne!(
            cache_key("acme/skills", Some("main")),
            cache_key("acme/skills", None)
        );

        let cache = UpdateCache::from_dir("/tmp/cache");
        let path = cache.path_for("../../evil/repo", Some("main"));
        assert_eq!(path.parent(), Some(Path::new("/tmp/cache")));
        assert!(!path.to_string_lossy().contains("evil"));
    }

    #[test]
    fn freshness_uses_the_ttl_boundary_and_tolerates_clock_skew() {
        const TTL: u64 = 6 * 60 * 60 * 1000;
        assert!(is_fresh(1_000, 1_000, TTL));
        assert!(is_fresh(1_000, 1_000 + TTL - 1, TTL));
        assert!(!is_fresh(1_000, 1_000 + TTL, TTL));
        assert!(!is_fresh(1_000, 1_000 + TTL + 1, TTL));
        assert!(is_fresh(1_000, 0, TTL));
    }

    #[test]
    fn concurrent_stores_for_different_sources_keep_distinct_temp_files() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = UpdateCache::from_dir(tmp.path().join("cache"));
        std::thread::scope(|scope| {
            for index in 0..12 {
                let cache = cache.clone();
                scope.spawn(move || {
                    cache
                        .store(&format!("acme/repo-{index}"), Some("main"), &tree(), 1_000)
                        .unwrap();
                });
            }
        });
        for index in 0..12 {
            assert!(
                cache
                    .load(&format!("acme/repo-{index}"), Some("main"))
                    .is_some(),
                "cache entry {index} was lost"
            );
        }
    }
}
