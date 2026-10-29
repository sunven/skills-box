//! Git tree object semantics shared by local baselines and remote checks.
//!
//! The lock records a Git tree SHA. Local skill folders and GitHub recursive
//! tree responses must both be reduced to the same object bytes: entry names
//! sorted by Git's tree key (files append NUL, directories append `/`), Git's
//! canonical mode strings, and SHA-1 object framing.

use sha1::{Digest, Sha1};

pub const EMPTY_TREE_SHA: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitMode {
    Tree,
    Blob,
    Executable,
    Symlink,
    Submodule,
}

impl GitMode {
    pub fn as_bytes(self) -> &'static [u8] {
        match self {
            Self::Tree => b"40000",
            Self::Blob => b"100644",
            Self::Executable => b"100755",
            Self::Symlink => b"120000",
            Self::Submodule => b"160000",
        }
    }

    pub fn is_tree(self) -> bool {
        self == Self::Tree
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitTreeEntry {
    pub name: Vec<u8>,
    pub mode: GitMode,
    pub sha: [u8; 20],
}

impl GitTreeEntry {
    pub fn new(name: impl Into<Vec<u8>>, mode: GitMode, sha: [u8; 20]) -> Self {
        Self {
            name: name.into(),
            mode,
            sha,
        }
    }

    fn sort_key(&self) -> Vec<u8> {
        let mut key = self.name.clone();
        key.push(if self.mode.is_tree() { b'/' } else { 0 });
        key
    }
}

pub fn object_sha(kind: &[u8], body: &[u8]) -> [u8; 20] {
    let mut hasher = Sha1::new();
    hasher.update(kind);
    hasher.update(b" ");
    hasher.update(body.len().to_string().as_bytes());
    hasher.update([0]);
    hasher.update(body);
    hasher.finalize().into()
}

pub fn hex(sha: &[u8; 20]) -> String {
    let mut output = String::with_capacity(40);
    for byte in sha {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

pub fn build_tree_sha(mut entries: Vec<GitTreeEntry>) -> [u8; 20] {
    entries.sort_by_key(GitTreeEntry::sort_key);
    let mut body = Vec::new();
    for entry in entries {
        body.extend_from_slice(entry.mode.as_bytes());
        body.push(b' ');
        body.extend_from_slice(&entry.name);
        body.push(0);
        body.extend_from_slice(&entry.sha);
    }
    object_sha(b"tree", &body)
}

pub fn decode_sha(value: &str) -> Option<[u8; 20]> {
    if value.len() != 40 {
        return None;
    }
    let mut output = [0u8; 20];
    for (index, byte) in output.iter_mut().enumerate() {
        let start = index * 2;
        *byte = u8::from_str_radix(value.get(start..start + 2)?, 16).ok()?;
    }
    Some(output)
}

pub fn is_git_tree_hash(hash: &str) -> bool {
    hash.len() == 40 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::update::github::{RemoteTree, RemoteTreeEntry};

    const ROOT: &str = "794d9f8160440397d6bb1132a68fa7e75b0ac4ed";
    const SKILL_MD: &str = "882a458e7b47d67722f6aeccb51f16b1842f1a19";
    const LINK: &str = "d69512cfc83b99237b23cf465aecb450d8df1a73";
    const RUN_SH: &str = "1a2485251c33a70432394c93fb89330ef214bfc9";
    const SUB_TREE: &str = "640ba63b141ba35b527fee65e78140148c8d0a5b";
    const NOTE: &str = "ce013625030ba8dba906f756967f9e9ca394464a";

    /// Generated with `git add -A -f . && git write-tree` for:
    /// SKILL.md, link -> SKILL.md, run.sh (100755), sub/note.txt.
    #[test]
    fn local_and_remote_adapters_agree_with_git() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("skill");
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::create_dir_all(dir.join("empty-dir")).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            "---\nname: alpha\ndescription: test\n---\n# Alpha\n",
        )
        .unwrap();
        fs::write(dir.join("sub/note.txt"), "hello\n").unwrap();
        fs::write(dir.join("run.sh"), "#!/bin/sh\n").unwrap();
        fs::set_permissions(dir.join("run.sh"), fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink("SKILL.md", dir.join("link")).unwrap();
        fs::write(dir.join(".DS_Store"), "noise").unwrap();
        fs::write(dir.join("metadata.json"), "{}").unwrap();

        assert_eq!(crate::baseline::git_tree_hash(&dir).unwrap(), ROOT);

        let mut entries = BTreeMap::new();
        entries.insert(
            "SKILL.md".to_string(),
            RemoteTreeEntry::new("blob", SKILL_MD, Some("100644")),
        );
        entries.insert(
            "link".to_string(),
            RemoteTreeEntry::new("blob", LINK, Some("120000")),
        );
        entries.insert(
            "run.sh".to_string(),
            RemoteTreeEntry::new("blob", RUN_SH, Some("100755")),
        );
        entries.insert(
            "sub".to_string(),
            RemoteTreeEntry::new("tree", SUB_TREE, Some("040000")),
        );
        entries.insert(
            "sub/note.txt".to_string(),
            RemoteTreeEntry::new("blob", NOTE, Some("100644")),
        );
        let tree = RemoteTree::new(ROOT, false, entries);

        assert_eq!(tree.folder_tree_sha("").as_deref(), Some(ROOT));
        assert_eq!(tree.folder_tree_sha("sub").as_deref(), Some(SUB_TREE));
    }

    #[test]
    fn both_adapters_use_git_tree_entry_order() {
        // `a` sorts before `a.b` in Git because files append NUL to the key.
        let mut entries = BTreeMap::new();
        entries.insert(
            "a".to_string(),
            RemoteTreeEntry::new(
                "blob",
                "c1b0730e0133447badcfd47fd144e254807b06e1",
                Some("100644"),
            ),
        );
        entries.insert(
            "a.b".to_string(),
            RemoteTreeEntry::new(
                "blob",
                "e25f1814e51579d5f55c0f1fe0135ddb28a47f4a",
                Some("100644"),
            ),
        );
        let tree = RemoteTree::new("846cef4236dfea2453c3513485f972921fedb43d", false, entries);
        assert_eq!(
            tree.folder_tree_sha("").as_deref(),
            Some("846cef4236dfea2453c3513485f972921fedb43d")
        );

        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a"), "x").unwrap();
        fs::write(tmp.path().join("a.b"), "y").unwrap();
        assert_eq!(
            crate::baseline::git_tree_hash(tmp.path()).unwrap(),
            "846cef4236dfea2453c3513485f972921fedb43d"
        );
    }

    #[test]
    fn empty_tree_and_sha_shape_are_shared() {
        assert_eq!(hex(&object_sha(b"tree", &[])), EMPTY_TREE_SHA);
        assert!(is_git_tree_hash(EMPTY_TREE_SHA));
        assert!(!is_git_tree_hash(&"a".repeat(64)));
        assert!(!is_git_tree_hash("not-a-sha"));
    }
}
