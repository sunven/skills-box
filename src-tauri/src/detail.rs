//! Read-only skill detail assembly and server-resolved open targets.
//!
//! Detail combines the inventory scan with lock metadata and a capped read of
//! `SKILL.md` (1 MiB). The frontend never supplies a path: open commands resolve
//! the skill folder on the server and pass it through path guards.

use serde::Serialize;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::baseline::{BaselineStore, LocalModification};
use crate::error::AppError;
use crate::inventory::{
    self, AgentAttachment, AgentDir, ManagementState, UpdateState, frontmatter,
};
use crate::paths::AppPaths;
use crate::security::{self, AllowedRoot};

pub const SKILL_MD_LIMIT_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillMdStatus {
    Ok,
    Missing,
    TooLarge,
    Unreadable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMd {
    pub status: SkillMdStatus,
    pub path: Option<String>,
    pub content: Option<String>,
    pub size_bytes: Option<u64>,
    pub limit_bytes: u64,
    pub error: Option<String>,
}

impl SkillMd {
    fn missing(path: Option<String>) -> Self {
        Self {
            status: SkillMdStatus::Missing,
            path,
            content: None,
            size_bytes: None,
            limit_bytes: SKILL_MD_LIMIT_BYTES,
            error: None,
        }
    }

    fn unreadable(path: Option<String>, error: impl Into<String>, size_bytes: Option<u64>) -> Self {
        Self {
            status: SkillMdStatus::Unreadable,
            path,
            content: None,
            size_bytes,
            limit_bytes: SKILL_MD_LIMIT_BYTES,
            error: Some(error.into()),
        }
    }

    fn too_large(path: Option<String>, size_bytes: u64) -> Self {
        Self {
            status: SkillMdStatus::TooLarge,
            path,
            content: None,
            size_bytes: Some(size_bytes),
            limit_bytes: SKILL_MD_LIMIT_BYTES,
            error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDetail {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub source: Option<String>,
    pub source_type: Option<String>,
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    pub skill_path: Option<String>,
    pub installed_at: Option<String>,
    pub updated_at: Option<String>,
    pub canonical_path: Option<String>,
    /// Canonical path, or the agent copy path for copy-only skills.
    pub directory_path: Option<String>,
    pub directory_mtime_epoch_ms: Option<u64>,
    pub hash: Option<String>,
    pub management: ManagementState,
    pub update: UpdateState,
    pub local_modification: LocalModification,
    pub local_modification_detail: Option<String>,
    pub attachments: Vec<AgentAttachment>,
    pub warnings: Vec<String>,
    pub skill_md: SkillMd,
}

pub fn read(paths: &AppPaths, skill_id: &str) -> Result<SkillDetail, AppError> {
    let agents = inventory::detected_agent_dirs(paths);
    read_with_agents(paths, &agents, skill_id)
}

pub fn read_with_agents(
    paths: &AppPaths,
    agents: &[AgentDir],
    skill_id: &str,
) -> Result<SkillDetail, AppError> {
    security::validate_skill_id(skill_id)?;
    let located = inventory::locate_with_agents(paths, agents, skill_id)?
        .ok_or_else(|| AppError::skill_not_found(skill_id))?;
    let primary = located
        .primary_path
        .clone()
        .ok_or_else(|| AppError::skill_not_found(skill_id))?;
    let display_directory = Some(paths.display(&primary));
    let skill_md = read_skill_md(&primary, display_directory.clone());

    let lock_entry = located.lock_entry.as_ref();
    let mut warnings = Vec::new();
    let description = match skill_md.content.as_deref() {
        Some(raw) => match frontmatter::parse(raw) {
            Ok(Some(parsed)) => parsed.description,
            Ok(None) => None,
            Err(error) => {
                push_unique(
                    &mut warnings,
                    format!("SKILL.md frontmatter 解析失败：{error}"),
                );
                None
            }
        },
        None => None,
    };

    match skill_md.status {
        SkillMdStatus::Ok => {}
        SkillMdStatus::Missing => push_unique(&mut warnings, "未找到 SKILL.md".to_string()),
        SkillMdStatus::TooLarge => push_unique(
            &mut warnings,
            "SKILL.md 超过 1 MiB，未加载内容。".to_string(),
        ),
        SkillMdStatus::Unreadable => push_unique(
            &mut warnings,
            format!(
                "无法读取 SKILL.md：{}",
                skill_md.error.as_deref().unwrap_or("未知错误")
            ),
        ),
    }

    for attachment in &located.attachments {
        if attachment.mode == inventory::AttachmentMode::Broken {
            push_unique(&mut warnings, format!("断链：{}", attachment.agent_name));
        }
    }

    let baseline_store = BaselineStore::new(paths);
    let assessment = baseline_store.assess(
        skill_id,
        Some(&primary),
        lock_entry.map(|entry| entry.skill_folder_hash.as_str()),
    );
    let (local_modification, local_modification_detail) =
        if located.management == ManagementState::Unmanaged {
            (
                LocalModification::Unknown,
                Some("未受管：没有 lock 条目，无法判断本地改动。".to_string()),
            )
        } else {
            (assessment.state, assessment.detail)
        };

    let attachments = located
        .attachments
        .iter()
        .map(|attachment| AgentAttachment {
            agent_id: attachment.agent_id.clone(),
            agent_name: attachment.agent_name.clone(),
            path: paths.display(&attachment.path),
            mode: attachment.mode,
            detachable: inventory::attachment_is_detachable(
                located.management,
                located.canonical_path.is_some(),
                agents
                    .iter()
                    .find(|agent| agent.id == attachment.agent_id)
                    .map(|agent| agent.universal)
                    .unwrap_or(false),
                attachment.mode,
            ),
        })
        .collect();

    Ok(SkillDetail {
        id: skill_id.to_string(),
        name: skill_id.to_string(),
        description,
        source: lock_entry.map(|entry| entry.source.clone()),
        source_type: lock_entry.map(|entry| entry.source_type.clone()),
        reference: lock_entry.and_then(|entry| entry.reference.clone()),
        skill_path: lock_entry.and_then(|entry| entry.skill_path.clone()),
        installed_at: lock_entry.map(|entry| entry.installed_at.clone()),
        updated_at: lock_entry.map(|entry| entry.updated_at.clone()),
        canonical_path: located
            .canonical_path
            .as_ref()
            .map(|path| paths.display(path)),
        directory_path: display_directory,
        directory_mtime_epoch_ms: directory_mtime_epoch_ms(&primary),
        hash: lock_entry
            .map(|entry| entry.skill_folder_hash.clone())
            .filter(|hash| !hash.is_empty()),
        management: located.management,
        update: UpdateState::Unknown,
        local_modification,
        local_modification_detail,
        attachments,
        warnings,
        skill_md,
    })
}

fn read_skill_md(directory: &Path, display_directory: Option<String>) -> SkillMd {
    let path = directory.join("SKILL.md");
    let display_path = display_directory.map(|directory| format!("{directory}/SKILL.md"));

    let link_metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return SkillMd::missing(display_path);
        }
        Err(error) => {
            return SkillMd::unreadable(display_path, error.to_string(), None);
        }
    };

    if link_metadata.file_type().is_symlink() {
        let resolved = match fs::canonicalize(&path) {
            Ok(resolved) => resolved,
            Err(error) => {
                return SkillMd::unreadable(
                    display_path,
                    format!("SKILL.md 符号链接不可解析：{error}"),
                    None,
                );
            }
        };
        let directory_resolved =
            fs::canonicalize(directory).unwrap_or_else(|_| directory.to_path_buf());
        if !resolved.starts_with(&directory_resolved) {
            return SkillMd::unreadable(
                display_path,
                "SKILL.md 符号链接指向 skill 目录之外，已拒绝读取。",
                None,
            );
        }
    }

    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) => {
            return SkillMd::unreadable(display_path, error.to_string(), None);
        }
    };
    if !metadata.is_file() {
        return SkillMd::unreadable(
            display_path,
            "SKILL.md 不是普通文件。",
            Some(metadata.len()),
        );
    }
    if metadata.len() > SKILL_MD_LIMIT_BYTES {
        return SkillMd::too_large(display_path, metadata.len());
    }

    let file = match File::open(&path) {
        Ok(file) => file,
        Err(error) => {
            return SkillMd::unreadable(display_path, error.to_string(), Some(metadata.len()));
        }
    };
    let mut bytes = Vec::new();
    if let Err(error) = file.take(SKILL_MD_LIMIT_BYTES + 1).read_to_end(&mut bytes) {
        return SkillMd::unreadable(display_path, error.to_string(), Some(metadata.len()));
    }
    if bytes.len() as u64 > SKILL_MD_LIMIT_BYTES {
        return SkillMd::too_large(display_path, bytes.len() as u64);
    }
    let content = match String::from_utf8(bytes) {
        Ok(content) => content,
        Err(error) => {
            return SkillMd::unreadable(
                display_path,
                format!("SKILL.md 不是有效的 UTF-8：{error}"),
                Some(metadata.len()),
            );
        }
    };
    SkillMd {
        status: SkillMdStatus::Ok,
        path: display_path,
        content: Some(content),
        size_bytes: Some(metadata.len()),
        limit_bytes: SKILL_MD_LIMIT_BYTES,
        error: None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenTarget {
    Folder,
    SkillMd,
}

/// Resolve and guard a path for `open_skill_folder` / `open_skill_md`.
pub fn resolve_open_target(
    paths: &AppPaths,
    agents: &[AgentDir],
    skill_id: &str,
    target: OpenTarget,
) -> Result<PathBuf, AppError> {
    security::validate_skill_id(skill_id)?;
    let located = inventory::locate_with_agents(paths, agents, skill_id)?
        .ok_or_else(|| AppError::skill_not_found(skill_id))?;
    let folder = located
        .canonical_path
        .or(located.primary_path)
        .ok_or_else(|| AppError::skill_not_found(skill_id))?;
    let roots = allowed_roots(paths, agents);
    security::ensure_allowed(&folder, &roots)?;

    let resolved = match target {
        OpenTarget::Folder => folder,
        OpenTarget::SkillMd => folder.join("SKILL.md"),
    };
    security::ensure_allowed(&resolved, &roots)?;

    let metadata = fs::metadata(&resolved).map_err(|error| {
        AppError::new(
            crate::error::codes::SKILL_NOT_FOUND,
            "要打开的文件或目录不存在。",
        )
        .detail(format!("{}（{error}）", resolved.display()))
    })?;
    let ok = match target {
        OpenTarget::Folder => metadata.is_dir(),
        OpenTarget::SkillMd => metadata.is_file(),
    };
    if !ok {
        return Err(AppError::path_invalid(format!(
            "目标类型不匹配：{}",
            resolved.display()
        )));
    }
    Ok(resolved)
}

fn allowed_roots(paths: &AppPaths, agents: &[AgentDir]) -> Vec<AllowedRoot> {
    let canonical = paths.canonical_skills_dir();
    let mut roots = vec![AllowedRoot::canonical(canonical.clone())];
    for agent in agents {
        if same_dir(&agent.skills_dir, &canonical) {
            continue;
        }
        if roots
            .iter()
            .any(|root| same_dir(&root.path, &agent.skills_dir))
        {
            continue;
        }
        roots.push(AllowedRoot::agent(agent.skills_dir.clone()));
    }
    roots
}

fn same_dir(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn directory_mtime_epoch_ms(path: &Path) -> Option<u64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    modified
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis() as u64)
}

fn push_unique(list: &mut Vec<String>, item: String) {
    if !list.contains(&item) {
        list.push(item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::baseline::git_tree_hash;
    use crate::error::codes;
    use std::os::unix::fs::{PermissionsExt, symlink};

    struct Fixture {
        _tmp: tempfile::TempDir,
        paths: AppPaths,
        agents: Vec<AgentDir>,
        claude: PathBuf,
        canonical: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path().join("home");
            fs::create_dir_all(home.join(".agents/skills")).unwrap();
            fs::create_dir_all(home.join(".claude/skills")).unwrap();
            let canonical = home.join(".agents/skills");
            let claude = home.join(".claude/skills");
            let paths = AppPaths::new(&home, tmp.path().join("app-data"));
            let agents = vec![AgentDir {
                id: "claude-code".to_string(),
                display_name: "Claude Code".to_string(),
                universal: false,
                skills_dir: claude.clone(),
            }];
            Self {
                _tmp: tmp,
                paths,
                agents,
                claude,
                canonical,
            }
        }

        fn lock_entry(name: &str, hash: &str) -> serde_json::Value {
            serde_json::json!({
                "source": format!("acme/{name}"),
                "sourceType": "github",
                "sourceUrl": format!("https://github.com/acme/{name}.git"),
                "ref": "main",
                "skillPath": format!("skills/{name}/SKILL.md"),
                "skillFolderHash": hash,
                "installedAt": "2026-01-01T00:00:00.000Z",
                "updatedAt": "2026-02-03T04:05:06.000Z"
            })
        }

        fn write_lock(&self, entries: &[(&str, String)]) {
            let mut skills = serde_json::Map::new();
            for (name, hash) in entries {
                skills.insert((*name).to_string(), Self::lock_entry(name, hash));
            }
            let lock = serde_json::json!({"version": 3, "skills": skills});
            fs::create_dir_all(self.paths.agents_root()).unwrap();
            fs::write(self.paths.lock_path(), lock.to_string()).unwrap();
        }

        fn canonical_skill(&self, name: &str, body: &str) -> PathBuf {
            let dir = self.canonical.join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: {name} 的描述\n---\n{body}"),
            )
            .unwrap();
            dir
        }

        fn link(&self, root: &Path, name: &str, target: &Path) {
            symlink(target, root.join(name)).unwrap();
        }

        fn detail(&self, name: &str) -> SkillDetail {
            read_with_agents(&self.paths, &self.agents, name).unwrap()
        }
    }

    #[test]
    fn assembles_managed_detail_with_lock_metadata_and_rendered_source() {
        let fixture = Fixture::new();
        let dir = fixture.canonical_skill("alpha", "# Alpha\n\nBody\n");
        let hash = git_tree_hash(&dir).unwrap();
        fixture.write_lock(&[("alpha", hash.clone())]);
        fixture.link(&fixture.claude, "alpha", &dir);

        let detail = fixture.detail("alpha");
        assert_eq!(detail.id, "alpha");
        assert_eq!(detail.name, "alpha");
        assert_eq!(detail.description.as_deref(), Some("alpha 的描述"));
        assert_eq!(detail.source.as_deref(), Some("acme/alpha"));
        assert_eq!(detail.source_type.as_deref(), Some("github"));
        assert_eq!(detail.reference.as_deref(), Some("main"));
        assert_eq!(detail.skill_path.as_deref(), Some("skills/alpha/SKILL.md"));
        assert_eq!(
            detail.installed_at.as_deref(),
            Some("2026-01-01T00:00:00.000Z")
        );
        assert_eq!(
            detail.updated_at.as_deref(),
            Some("2026-02-03T04:05:06.000Z")
        );
        assert_eq!(
            detail.canonical_path.as_deref(),
            Some("~/.agents/skills/alpha")
        );
        assert_eq!(
            detail.directory_path.as_deref(),
            Some("~/.agents/skills/alpha")
        );
        assert_eq!(detail.hash.as_deref(), Some(hash.as_str()));
        assert_eq!(detail.management, ManagementState::Managed);
        assert_eq!(detail.update, UpdateState::Unknown);
        assert_eq!(detail.local_modification, LocalModification::Clean);
        assert!(detail.directory_mtime_epoch_ms.is_some());
        assert_eq!(detail.attachments.len(), 1);
        assert_eq!(detail.attachments[0].mode, inventory::AttachmentMode::Link);
        assert_eq!(detail.skill_md.status, SkillMdStatus::Ok);
        assert_eq!(
            detail.skill_md.path.as_deref(),
            Some("~/.agents/skills/alpha/SKILL.md")
        );
        assert!(
            detail
                .skill_md
                .content
                .as_deref()
                .unwrap()
                .contains("---\nname: alpha")
        );
        assert_eq!(detail.skill_md.limit_bytes, 1024 * 1024);
        assert!(detail.warnings.is_empty(), "{:?}", detail.warnings);

        let json = serde_json::to_value(&detail).unwrap();
        assert_eq!(json["localModification"], "clean");
        assert_eq!(json["management"], "managed");
        assert_eq!(json["update"], "unknown");
        assert_eq!(json["ref"], "main");
        assert_eq!(json["skillMd"]["status"], "ok");
        assert_eq!(json["attachments"][0]["agentId"], "claude-code");
        assert!(json["directoryMtimeEpochMs"].as_u64().unwrap() > 0);
    }

    #[test]
    fn copy_only_detail_uses_the_agent_path_and_has_no_lock_times() {
        let fixture = Fixture::new();
        let copy = fixture.claude.join("beta");
        fs::create_dir_all(&copy).unwrap();
        fs::write(
            copy.join("SKILL.md"),
            "---\nname: beta\ndescription: copy\n---\n# Beta\n",
        )
        .unwrap();
        fixture.write_lock(&[("beta", String::new())]);

        let detail = fixture.detail("beta");
        assert_eq!(detail.management, ManagementState::CopyOnly);
        assert!(detail.canonical_path.is_none());
        assert_eq!(
            detail.directory_path.as_deref(),
            Some("~/.claude/skills/beta")
        );
        assert!(detail.hash.is_none());
        assert_eq!(detail.local_modification, LocalModification::Unknown);
        assert!(detail.local_modification_detail.is_some());
        assert_eq!(detail.attachments.len(), 1);
        assert_eq!(detail.attachments[0].mode, inventory::AttachmentMode::Copy);
    }

    #[test]
    fn unmanaged_detail_is_read_only_with_directory_mtime() {
        let fixture = Fixture::new();
        let dir = fixture.canonical_skill("gamma", "# Gamma\n");

        let detail = fixture.detail("gamma");
        assert_eq!(detail.management, ManagementState::Unmanaged);
        assert!(detail.source.is_none());
        assert!(detail.installed_at.is_none());
        assert!(detail.updated_at.is_none());
        assert!(detail.hash.is_none());
        assert!(detail.directory_mtime_epoch_ms.is_some());
        assert_eq!(detail.local_modification, LocalModification::Unknown);
        assert_eq!(
            detail.canonical_path.as_deref(),
            Some("~/.agents/skills/gamma")
        );
        assert!(dir.exists());
    }

    #[test]
    fn missing_skill_md_is_reported_and_does_not_fail_detail() {
        let fixture = Fixture::new();
        let dir = fixture.canonical.join("zeta");
        fs::create_dir_all(&dir).unwrap();
        fixture.write_lock(&[("zeta", "0".repeat(40))]);

        let detail = fixture.detail("zeta");
        assert_eq!(detail.skill_md.status, SkillMdStatus::Missing);
        assert!(detail.skill_md.content.is_none());
        assert!(detail.description.is_none());
        assert!(
            detail
                .warnings
                .iter()
                .any(|warning| warning.contains("未找到 SKILL.md")),
            "{:?}",
            detail.warnings
        );
    }

    #[test]
    fn skill_md_over_one_mib_is_not_read() {
        let fixture = Fixture::new();
        let dir = fixture.canonical.join("huge");
        fs::create_dir_all(&dir).unwrap();
        let file = File::create(dir.join("SKILL.md")).unwrap();
        file.set_len(SKILL_MD_LIMIT_BYTES + 1).unwrap();
        fixture.write_lock(&[("huge", "0".repeat(40))]);

        let detail = fixture.detail("huge");
        assert_eq!(detail.skill_md.status, SkillMdStatus::TooLarge);
        assert!(detail.skill_md.content.is_none());
        assert_eq!(detail.skill_md.limit_bytes, SKILL_MD_LIMIT_BYTES);
        assert!(
            detail
                .warnings
                .iter()
                .any(|warning| warning.contains("1 MiB")),
            "{:?}",
            detail.warnings
        );
    }

    #[test]
    fn skill_md_at_exactly_one_mib_is_read() {
        let fixture = Fixture::new();
        let dir = fixture.canonical.join("boundary");
        fs::create_dir_all(&dir).unwrap();
        let file = File::create(dir.join("SKILL.md")).unwrap();
        file.set_len(SKILL_MD_LIMIT_BYTES).unwrap();
        fixture.write_lock(&[("boundary", "0".repeat(40))]);

        let detail = fixture.detail("boundary");
        assert_eq!(detail.skill_md.status, SkillMdStatus::Ok);
        assert_eq!(
            detail.skill_md.content.as_ref().unwrap().len() as u64,
            SKILL_MD_LIMIT_BYTES
        );
    }

    #[test]
    fn invalid_utf8_and_malformed_frontmatter_have_distinct_warnings() {
        let fixture = Fixture::new();
        let invalid = fixture.canonical.join("invalid");
        fs::create_dir_all(&invalid).unwrap();
        fs::write(invalid.join("SKILL.md"), [0xff, 0xfe, 0xfd]).unwrap();
        fixture.write_lock(&[("invalid", "0".repeat(40))]);
        let detail = fixture.detail("invalid");
        assert_eq!(detail.skill_md.status, SkillMdStatus::Unreadable);
        assert!(
            detail
                .warnings
                .iter()
                .any(|warning| warning.contains("UTF-8"))
        );

        let malformed = fixture.canonical.join("malformed");
        fs::create_dir_all(&malformed).unwrap();
        fs::write(
            malformed.join("SKILL.md"),
            "---\ndescription: \"unterminated\n---\n# Bad\n",
        )
        .unwrap();
        fixture.write_lock(&[("malformed", "0".repeat(40))]);
        let detail = fixture.detail("malformed");
        assert_eq!(detail.skill_md.status, SkillMdStatus::Ok);
        assert!(
            detail
                .warnings
                .iter()
                .any(|warning| warning.contains("frontmatter"))
        );
    }

    #[test]
    fn skill_md_symlink_outside_the_skill_folder_is_rejected() {
        let fixture = Fixture::new();
        let dir = fixture.canonical.join("sneaky");
        fs::create_dir_all(&dir).unwrap();
        let outside = fixture._tmp.path().join("secret.txt");
        fs::write(&outside, "secret").unwrap();
        symlink(&outside, dir.join("SKILL.md")).unwrap();
        fixture.write_lock(&[("sneaky", "0".repeat(40))]);

        let detail = fixture.detail("sneaky");
        assert_eq!(detail.skill_md.status, SkillMdStatus::Unreadable);
        assert!(
            detail
                .warnings
                .iter()
                .any(|warning| warning.contains("目录之外"))
        );
    }

    #[test]
    fn open_target_resolution_uses_server_paths_and_rejects_traversal() {
        let fixture = Fixture::new();
        let dir = fixture.canonical_skill("alpha", "# Alpha\n");
        fixture.write_lock(&[("alpha", git_tree_hash(&dir).unwrap())]);
        fixture.link(&fixture.claude, "alpha", &dir);

        let folder =
            resolve_open_target(&fixture.paths, &fixture.agents, "alpha", OpenTarget::Folder)
                .unwrap();
        assert_eq!(folder, dir);
        let md = resolve_open_target(
            &fixture.paths,
            &fixture.agents,
            "alpha",
            OpenTarget::SkillMd,
        )
        .unwrap();
        assert_eq!(md, dir.join("SKILL.md"));

        let traversal = resolve_open_target(
            &fixture.paths,
            &fixture.agents,
            "../alpha",
            OpenTarget::Folder,
        )
        .unwrap_err();
        assert_eq!(traversal.code, codes::PATH_INVALID);

        let unknown = resolve_open_target(
            &fixture.paths,
            &fixture.agents,
            "missing",
            OpenTarget::Folder,
        )
        .unwrap_err();
        assert_eq!(unknown.code, codes::SKILL_NOT_FOUND);
    }

    #[test]
    fn open_target_for_copy_only_uses_the_agent_copy() {
        let fixture = Fixture::new();
        let copy = fixture.claude.join("beta");
        fs::create_dir_all(&copy).unwrap();
        fs::write(copy.join("SKILL.md"), "---\nname: beta\n---\n").unwrap();
        fixture.write_lock(&[("beta", String::new())]);

        let folder =
            resolve_open_target(&fixture.paths, &fixture.agents, "beta", OpenTarget::Folder)
                .unwrap();
        assert_eq!(folder, copy);
    }

    #[test]
    fn open_target_rejects_an_agent_root_that_is_a_symlink() {
        let fixture = Fixture::new();
        let real = fixture._tmp.path().join("real-claude");
        fs::create_dir_all(real.join("beta")).unwrap();
        fs::write(real.join("beta/SKILL.md"), "---\nname: beta\n---\n").unwrap();
        fs::remove_dir_all(&fixture.claude).unwrap();
        symlink(&real, &fixture.claude).unwrap();
        fixture.write_lock(&[("beta", String::new())]);

        let error =
            resolve_open_target(&fixture.paths, &fixture.agents, "beta", OpenTarget::Folder)
                .unwrap_err();
        assert_eq!(error.code, codes::PATH_INVALID);
    }

    #[test]
    fn skill_id_validation_rejects_paths() {
        let fixture = Fixture::new();
        for id in ["", "..", "a/b", "a\\b", ".hidden"] {
            let error = read_with_agents(&fixture.paths, &fixture.agents, id).unwrap_err();
            assert_eq!(error.code, codes::PATH_INVALID, "{id:?}");
        }
    }

    #[test]
    fn skill_md_file_permissions_do_not_affect_readability() {
        let fixture = Fixture::new();
        let dir = fixture.canonical_skill("alpha", "# Alpha\n");
        let md = dir.join("SKILL.md");
        fs::set_permissions(&md, fs::Permissions::from_mode(0o644)).unwrap();
        fixture.write_lock(&[("alpha", "0".repeat(40))]);
        let detail = fixture.detail("alpha");
        assert_eq!(detail.skill_md.status, SkillMdStatus::Ok);
    }
}
