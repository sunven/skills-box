//! Read-only inventory projection.
//!
//! Truth comes from the physical filesystem: the canonical store, each
//! detected agent's global skills directory, and `lstat` for every entry.
//! `skills list -g --json.agents` is never consulted; the CLI over-reports
//! universal agents and is not a mounting source of truth.
//!
//! A missing lock is normal and yields an unmanaged inventory. A corrupt or
//! non-v3 lock aborts the whole read with a `lock.*` error so the UI can show
//! a full-screen failure instead of an empty (and misleading) list.

pub mod frontmatter;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::agents::{DetectedAgent, DetectionContext, detect_agents};
use crate::env::capture_process_env;
use crate::env::child_env::{ChildEnvParams, build_child_env};
use crate::error::AppError;
use crate::lock::LockFile;
use crate::paths::AppPaths;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagementState {
    Managed,
    CopyOnly,
    Unmanaged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateState {
    Current,
    Update,
    Unknown,
    Error,
    PathMissing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentMode {
    Link,
    Copy,
    Native,
    Broken,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAttachment {
    pub agent_id: String,
    pub agent_name: String,
    pub path: String,
    pub mode: AttachmentMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventorySkill {
    pub name: String,
    pub description: Option<String>,
    pub source: Option<String>,
    pub management: ManagementState,
    pub update: UpdateState,
    pub updated_at: Option<String>,
    pub canonical_path: Option<String>,
    pub attachments: Vec<AgentAttachment>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryAgent {
    pub id: String,
    pub display_name: String,
    pub universal: bool,
    /// True only when the agent's global directory resolves to the canonical
    /// store. Catalog `universal` alone is not enough (CLI over-reporting).
    pub shared: bool,
    pub global_skills_dir: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnomalyGroup {
    pub count: usize,
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnomalySummary {
    pub total: usize,
    pub stale: AnomalyGroup,
    pub unmanaged: AnomalyGroup,
    pub broken: AnomalyGroup,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventorySnapshot {
    pub skills: Vec<InventorySkill>,
    pub agents: Vec<InventoryAgent>,
    pub anomalies: AnomalySummary,
    pub warnings: Vec<String>,
    pub generated_at_epoch_ms: u64,
}

/// A detected agent resolved to an absolute global skills directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDir {
    pub id: String,
    pub display_name: String,
    pub universal: bool,
    pub skills_dir: PathBuf,
}

impl AgentDir {
    pub fn from_detected(agent: &DetectedAgent, home: &Path) -> Option<Self> {
        let raw = agent.global_skills_dir.as_deref()?;
        Some(Self {
            id: agent.id.clone(),
            display_name: agent.display_name.clone(),
            universal: agent.universal,
            skills_dir: expand_tilde(raw, home)?,
        })
    }
}

/// Detect agents exactly like the environment report (same allowlisted env),
/// then resolve their global directories for scanning.
pub fn detected_agent_dirs(paths: &AppPaths) -> Vec<AgentDir> {
    let parent = capture_process_env();
    let env = build_child_env(ChildEnvParams {
        home: paths.home(),
        node_dir: None,
        git_dir: None,
        parent: &parent,
    });
    let context = DetectionContext::new(paths.home(), paths.cli_cwd(), env);
    detect_agents(&context)
        .iter()
        .filter_map(|agent| AgentDir::from_detected(agent, paths.home()))
        .collect()
}

pub fn read(paths: &AppPaths) -> Result<InventorySnapshot, AppError> {
    let agents = detected_agent_dirs(paths);
    read_with_agents(paths, &agents)
}

/// Injectable entry point for fixtures and tests.
pub fn read_with_agents(
    paths: &AppPaths,
    agents: &[AgentDir],
) -> Result<InventorySnapshot, AppError> {
    // Strict lock read happens first: corruption must never degrade into an
    // empty list or partial scan.
    let lock = LockFile::read(&paths.lock_path())?;
    let canonical_dir = paths.canonical_skills_dir();
    let mut warnings = Vec::new();
    let canonical = scan_canonical(&canonical_dir, &mut warnings);
    let scanned_agents = scan_agents(agents, &canonical_dir, &canonical, &lock, &mut warnings);

    let mut aggregates: BTreeMap<String, Aggregate> = BTreeMap::new();
    for scanned in &scanned_agents {
        if scanned.shared {
            for (name, path) in &canonical {
                let aggregate = aggregates.entry(name.clone()).or_default();
                aggregate.has_real_content = true;
                aggregate.attachments.push(AgentAttachment {
                    agent_id: scanned.agent.id.clone(),
                    agent_name: scanned.agent.display_name.clone(),
                    path: paths.display(path),
                    mode: AttachmentMode::Native,
                });
            }
        } else {
            for entry in &scanned.entries {
                let aggregate = aggregates.entry(entry.name.clone()).or_default();
                if entry.mode != AttachmentMode::Broken {
                    aggregate.has_real_content = true;
                    if aggregate.primary.is_none() {
                        aggregate.primary = Some(entry.path.clone());
                    }
                }
                aggregate.attachments.push(AgentAttachment {
                    agent_id: scanned.agent.id.clone(),
                    agent_name: scanned.agent.display_name.clone(),
                    path: paths.display(&entry.path),
                    mode: entry.mode,
                });
            }
        }
    }

    let mut names: BTreeSet<String> = BTreeSet::new();
    names.extend(canonical.keys().cloned());
    names.extend(aggregates.keys().cloned());
    names.extend(lock.entries().keys().cloned());

    let mut skills = Vec::new();
    let mut stale: Vec<String> = Vec::new();
    for name in names {
        let canonical_path = canonical.get(&name);
        let aggregate = aggregates.get(&name);
        let lock_entry = lock.get(&name);
        let has_real_content = canonical_path.is_some()
            || aggregate
                .map(|aggregate| aggregate.has_real_content)
                .unwrap_or(false);

        let management = match lock_entry {
            Some(_) if canonical_path.is_some() => ManagementState::Managed,
            Some(_) if has_real_content => ManagementState::CopyOnly,
            Some(_) => {
                stale.push(name);
                continue;
            }
            None => ManagementState::Unmanaged,
        };

        let primary = canonical_path
            .cloned()
            .or_else(|| aggregate.and_then(|aggregate| aggregate.primary.clone()));
        let (description, warnings) = read_description(primary.as_deref());

        skills.push(InventorySkill {
            name: name.clone(),
            description,
            source: lock_entry.map(|entry| entry.source.clone()),
            management,
            update: UpdateState::Unknown,
            updated_at: lock_entry.map(|entry| entry.updated_at.clone()),
            canonical_path: canonical_path.map(|path| paths.display(path)),
            attachments: aggregate
                .map(|aggregate| aggregate.attachments.clone())
                .unwrap_or_default(),
            warnings,
        });
    }

    let unmanaged_names: Vec<String> = skills
        .iter()
        .filter(|skill| skill.management == ManagementState::Unmanaged)
        .map(|skill| skill.name.clone())
        .collect();
    let broken_examples: Vec<String> = skills
        .iter()
        .flat_map(|skill| {
            skill
                .attachments
                .iter()
                .filter(|attachment| attachment.mode == AttachmentMode::Broken)
                .map(|attachment| format!("{} · {}", skill.name, attachment.agent_name))
        })
        .collect();
    let broken_count = broken_examples.len();
    let stale_group = group(stale.iter().cloned(), stale.len());
    let unmanaged_group = group(unmanaged_names.iter().cloned(), unmanaged_names.len());
    let broken_group = group(broken_examples.into_iter(), broken_count);
    let anomalies = AnomalySummary {
        total: stale_group.count + unmanaged_group.count + broken_group.count,
        stale: stale_group,
        unmanaged: unmanaged_group,
        broken: broken_group,
    };

    Ok(InventorySnapshot {
        skills,
        agents: agents
            .iter()
            .map(|agent| InventoryAgent {
                id: agent.id.clone(),
                display_name: agent.display_name.clone(),
                universal: agent.universal,
                shared: same_dir(&agent.skills_dir, &canonical_dir),
                global_skills_dir: Some(paths.display(&agent.skills_dir)),
            })
            .collect(),
        anomalies,
        warnings,
        generated_at_epoch_ms: epoch_ms(),
    })
}

#[derive(Debug, Default)]
struct Aggregate {
    attachments: Vec<AgentAttachment>,
    has_real_content: bool,
    primary: Option<PathBuf>,
}

#[derive(Debug)]
struct ScannedAgent<'a> {
    agent: &'a AgentDir,
    shared: bool,
    entries: Vec<ScannedEntry>,
}

#[derive(Debug)]
struct ScannedEntry {
    name: String,
    path: PathBuf,
    mode: AttachmentMode,
}

fn scan_canonical(canonical_dir: &Path, warnings: &mut Vec<String>) -> BTreeMap<String, PathBuf> {
    let mut canonical = BTreeMap::new();
    let entries = match read_entries(canonical_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return canonical,
        Err(error) => {
            warnings.push(format!("无法读取 canonical store：{error}"));
            return canonical;
        }
    };
    for (name, path) in entries {
        // A canonical entry is a skill when the path (possibly a symlink in
        // link mode) resolves to a directory, even if SKILL.md is missing.
        if fs::metadata(&path)
            .map(|metadata| metadata.is_dir())
            .unwrap_or(false)
        {
            canonical.insert(name, path);
        }
    }
    canonical
}

fn scan_agents<'a>(
    agents: &'a [AgentDir],
    canonical_dir: &Path,
    canonical: &BTreeMap<String, PathBuf>,
    lock: &LockFile,
    warnings: &mut Vec<String>,
) -> Vec<ScannedAgent<'a>> {
    agents
        .iter()
        .map(|agent| {
            let shared = same_dir(&agent.skills_dir, canonical_dir);
            if shared {
                return ScannedAgent {
                    agent,
                    shared,
                    entries: Vec::new(),
                };
            }
            let mut entries = Vec::new();
            match read_entries(&agent.skills_dir) {
                Ok(found) => {
                    for (name, path) in found {
                        let Ok(metadata) = fs::symlink_metadata(&path) else {
                            continue;
                        };
                        let mode = if metadata.file_type().is_symlink() {
                            match fs::metadata(&path) {
                                Ok(target) if target.is_dir() => AttachmentMode::Link,
                                _ => AttachmentMode::Broken,
                            }
                        } else if metadata.is_dir() {
                            let relevant = lock.contains(&name)
                                || canonical.contains_key(&name)
                                || path.join("SKILL.md").is_file();
                            if !relevant {
                                continue;
                            }
                            AttachmentMode::Copy
                        } else {
                            continue;
                        };
                        entries.push(ScannedEntry { name, path, mode });
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => warnings.push(format!(
                    "无法读取 {} 的 skill 目录：{error}",
                    agent.display_name
                )),
            }
            ScannedAgent {
                agent,
                shared,
                entries,
            }
        })
        .collect()
}

fn read_entries(dir: &Path) -> std::io::Result<Vec<(String, PathBuf)>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        entries.push((name, entry.path()));
    }
    Ok(entries)
}

fn read_description(primary: Option<&Path>) -> (Option<String>, Vec<String>) {
    let Some(primary) = primary else {
        return (None, vec!["未找到 SKILL.md".to_string()]);
    };
    let path = primary.join("SKILL.md");
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (None, vec!["未找到 SKILL.md".to_string()]);
        }
        Err(error) => {
            return (None, vec![format!("无法读取 SKILL.md：{error}")]);
        }
    };
    match frontmatter::parse(&raw) {
        Ok(Some(frontmatter)) => (frontmatter.description, Vec::new()),
        Ok(None) => (None, Vec::new()),
        Err(error) => (
            None,
            vec![format!("SKILL.md frontmatter 解析失败：{error}")],
        ),
    }
}

fn group(items: impl Iterator<Item = String>, count: usize) -> AnomalyGroup {
    AnomalyGroup {
        count,
        examples: items.take(3).collect(),
    }
}

fn same_dir(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn expand_tilde(raw: &str, home: &Path) -> Option<PathBuf> {
    if raw == "~" {
        Some(home.to_path_buf())
    } else if let Some(rest) = raw.strip_prefix("~/") {
        Some(home.join(rest))
    } else if raw.starts_with('/') {
        Some(PathBuf::from(raw))
    } else {
        None
    }
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::codes;
    use std::os::unix::fs::symlink;

    struct Fixture {
        _tmp: tempfile::TempDir,
        paths: AppPaths,
        agents: Vec<AgentDir>,
        claude: PathBuf,
        gemini: PathBuf,
        codex: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path().join("home");
            fs::create_dir_all(home.join(".agents/skills")).unwrap();
            fs::create_dir_all(home.join(".claude/skills")).unwrap();
            fs::create_dir_all(home.join(".gemini/skills")).unwrap();
            let claude = home.join(".claude/skills");
            let gemini = home.join(".gemini/skills");
            let codex = home.join(".agents/skills");
            let paths = AppPaths::new(&home, tmp.path().join("app-data"));
            let agents = vec![
                AgentDir {
                    id: "claude-code".to_string(),
                    display_name: "Claude Code".to_string(),
                    universal: false,
                    skills_dir: claude.clone(),
                },
                AgentDir {
                    id: "gemini-cli".to_string(),
                    display_name: "Gemini CLI".to_string(),
                    universal: true,
                    skills_dir: gemini.clone(),
                },
                AgentDir {
                    id: "codex".to_string(),
                    display_name: "Codex".to_string(),
                    universal: true,
                    skills_dir: codex.clone(),
                },
            ];
            Self {
                _tmp: tmp,
                paths,
                agents,
                claude,
                gemini,
                codex,
            }
        }

        fn write_lock(&self, names: &[&str]) {
            let mut skills = serde_json::Map::new();
            for name in names {
                skills.insert((*name).to_string(), lock_entry(name));
            }
            let lock = serde_json::json!({
                "version": 3,
                "skills": skills,
                "dismissed": { "findSkillsPrompt": true }
            });
            fs::create_dir_all(self.paths.agents_root()).unwrap();
            fs::write(self.paths.lock_path(), lock.to_string()).unwrap();
        }

        fn canonical_skill(&self, name: &str, description: &str) -> PathBuf {
            let dir = self.paths.canonical_skills_dir().join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n"),
            )
            .unwrap();
            dir
        }

        fn agent_copy(&self, root: &Path, name: &str, description: &str) -> PathBuf {
            let dir = root.join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: {description}\n---\n"),
            )
            .unwrap();
            dir
        }

        fn link(&self, root: &Path, name: &str, target: &Path) {
            symlink(target, root.join(name)).unwrap();
        }

        fn read(&self) -> InventorySnapshot {
            read_with_agents(&self.paths, &self.agents).unwrap()
        }
    }

    fn lock_entry(name: &str) -> serde_json::Value {
        serde_json::json!({
            "source": format!("acme/{name}"),
            "sourceType": "github",
            "sourceUrl": format!("https://github.com/acme/{name}.git"),
            "skillPath": format!("skills/{name}/SKILL.md"),
            "skillFolderHash": "0123456789abcdef0123456789abcdef01234567",
            "installedAt": "2026-01-01T00:00:00.000Z",
            "updatedAt": "2026-02-03T04:05:06.000Z"
        })
    }

    fn skill<'a>(snapshot: &'a InventorySnapshot, name: &str) -> &'a InventorySkill {
        snapshot
            .skills
            .iter()
            .find(|skill| skill.name == name)
            .unwrap_or_else(|| panic!("missing skill {name}"))
    }

    fn modes(skill: &InventorySkill) -> Vec<(String, AttachmentMode)> {
        skill
            .attachments
            .iter()
            .map(|attachment| (attachment.agent_id.clone(), attachment.mode))
            .collect()
    }

    #[test]
    fn classifies_managed_copy_only_unmanaged_agent_only_and_stale() {
        let fixture = Fixture::new();
        fixture.write_lock(&["alpha", "beta", "epsilon", "ghost", "zeta"]);

        let alpha = fixture.canonical_skill("alpha", "managed alpha");
        fixture.link(&fixture.claude, "alpha", &alpha);
        fixture.link(&fixture.gemini, "alpha", &alpha);
        fixture.canonical_skill("gamma", "unmanaged canonical");
        fixture.agent_copy(&fixture.claude, "delta", "agent only");
        // Copy-only: no canonical, a real copy inside Claude Code.
        fixture.agent_copy(&fixture.claude, "beta", "copy only beta");
        // Managed skill with one dangling link.
        let epsilon = fixture.canonical_skill("epsilon", "broken link");
        fixture.link(&fixture.claude, "epsilon", &epsilon);
        fs::remove_file(fixture.claude.join("epsilon")).unwrap();
        symlink(
            fixture.claude.join("missing-target"),
            fixture.claude.join("epsilon"),
        )
        .unwrap();
        // Managed skill without SKILL.md.
        fs::create_dir_all(fixture.paths.canonical_skills_dir().join("zeta")).unwrap();

        let snapshot = fixture.read();

        let alpha = skill(&snapshot, "alpha");
        assert_eq!(alpha.management, ManagementState::Managed);
        assert_eq!(alpha.update, UpdateState::Unknown);
        assert_eq!(alpha.source.as_deref(), Some("acme/alpha"));
        assert_eq!(alpha.description.as_deref(), Some("managed alpha"));
        assert_eq!(
            alpha.updated_at.as_deref(),
            Some("2026-02-03T04:05:06.000Z")
        );
        assert_eq!(
            alpha.canonical_path.as_deref(),
            Some("~/.agents/skills/alpha")
        );
        assert_eq!(
            modes(alpha),
            vec![
                ("claude-code".to_string(), AttachmentMode::Link),
                ("gemini-cli".to_string(), AttachmentMode::Link),
                ("codex".to_string(), AttachmentMode::Native),
            ]
        );

        let beta = skill(&snapshot, "beta");
        assert_eq!(beta.management, ManagementState::CopyOnly);
        assert_eq!(beta.description.as_deref(), Some("copy only beta"));
        assert!(beta.canonical_path.is_none());
        assert_eq!(
            modes(beta),
            vec![("claude-code".to_string(), AttachmentMode::Copy)]
        );

        let gamma = skill(&snapshot, "gamma");
        assert_eq!(gamma.management, ManagementState::Unmanaged);
        assert!(gamma.source.is_none());
        assert!(gamma.updated_at.is_none());
        assert_eq!(gamma.description.as_deref(), Some("unmanaged canonical"));
        assert_eq!(
            modes(gamma),
            vec![("codex".to_string(), AttachmentMode::Native)]
        );

        let delta = skill(&snapshot, "delta");
        assert_eq!(delta.management, ManagementState::Unmanaged);
        assert!(delta.canonical_path.is_none());
        assert_eq!(
            modes(delta),
            vec![("claude-code".to_string(), AttachmentMode::Copy)]
        );

        let epsilon = skill(&snapshot, "epsilon");
        assert_eq!(epsilon.management, ManagementState::Managed);
        assert_eq!(
            modes(epsilon),
            vec![
                ("claude-code".to_string(), AttachmentMode::Broken),
                ("codex".to_string(), AttachmentMode::Native),
            ]
        );

        let zeta = skill(&snapshot, "zeta");
        assert!(zeta.description.is_none());
        assert!(
            zeta.warnings
                .iter()
                .any(|warning| warning.contains("SKILL.md")),
            "{:?}",
            zeta.warnings
        );

        assert!(snapshot.skills.iter().all(|skill| skill.name != "ghost"));
        assert_eq!(snapshot.anomalies.stale.count, 1);
        assert_eq!(snapshot.anomalies.stale.examples, vec!["ghost"]);
        assert_eq!(snapshot.anomalies.unmanaged.count, 2);
        assert_eq!(snapshot.anomalies.broken.count, 1);
        assert_eq!(
            snapshot.anomalies.broken.examples,
            vec!["epsilon · Claude Code"]
        );
        assert_eq!(snapshot.anomalies.total, 4);
        assert_eq!(snapshot.agents.len(), 3);
        let codex = snapshot
            .agents
            .iter()
            .find(|agent| agent.id == "codex")
            .unwrap();
        assert!(codex.shared);
        let gemini = snapshot
            .agents
            .iter()
            .find(|agent| agent.id == "gemini-cli")
            .unwrap();
        assert!(gemini.universal);
        assert!(!gemini.shared, "universal flag alone must not imply shared");
    }

    #[test]
    fn snapshot_serialization_matches_the_frontend_contract() {
        let fixture = Fixture::new();
        fixture.write_lock(&["alpha"]);
        let alpha = fixture.canonical_skill("alpha", "alpha");
        fixture.link(&fixture.claude, "alpha", &alpha);

        let snapshot = fixture.read();
        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(json["skills"][0]["name"], "alpha");
        assert_eq!(json["skills"][0]["management"], "managed");
        assert_eq!(json["skills"][0]["update"], "unknown");
        assert_eq!(json["skills"][0]["canonicalPath"], "~/.agents/skills/alpha");
        assert_eq!(json["skills"][0]["updatedAt"], "2026-02-03T04:05:06.000Z");
        assert_eq!(
            json["skills"][0]["attachments"][0]["agentId"],
            "claude-code"
        );
        assert_eq!(json["skills"][0]["attachments"][0]["mode"], "link");
        assert_eq!(json["agents"][2]["shared"], true);
        assert!(json["anomalies"]["stale"]["count"].is_number());
        assert!(json["generatedAtEpochMs"].is_number());
    }

    #[test]
    fn universal_agent_only_receives_physical_mounts_not_every_canonical_skill() {
        let fixture = Fixture::new();
        fixture.write_lock(&["alpha", "beta"]);
        let alpha = fixture.canonical_skill("alpha", "alpha");
        fixture.canonical_skill("beta", "beta");
        fixture.link(&fixture.gemini, "alpha", &alpha);

        let snapshot = fixture.read();
        assert_eq!(
            modes(skill(&snapshot, "alpha")),
            vec![
                ("gemini-cli".to_string(), AttachmentMode::Link),
                ("codex".to_string(), AttachmentMode::Native),
            ]
        );
        // Gemini is flagged universal by the catalog but does not share the
        // canonical directory, so it must not inherit beta.
        assert_eq!(
            modes(skill(&snapshot, "beta")),
            vec![("codex".to_string(), AttachmentMode::Native)]
        );
    }

    #[test]
    fn copy_only_can_be_reached_through_a_symlink_without_canonical() {
        let fixture = Fixture::new();
        fixture.write_lock(&["beta"]);
        let external = fixture._tmp.path().join("external/beta");
        fs::create_dir_all(&external).unwrap();
        fs::write(
            external.join("SKILL.md"),
            "---\nname: beta\ndescription: linked copy\n---\n",
        )
        .unwrap();
        fixture.link(&fixture.claude, "beta", &external);

        let snapshot = fixture.read();
        let beta = skill(&snapshot, "beta");
        assert_eq!(beta.management, ManagementState::CopyOnly);
        assert_eq!(beta.description.as_deref(), Some("linked copy"));
        assert_eq!(
            modes(beta),
            vec![("claude-code".to_string(), AttachmentMode::Link)]
        );
    }

    #[test]
    fn missing_lock_reads_unmanaged_skills_without_errors() {
        let fixture = Fixture::new();
        fixture.canonical_skill("alpha", "alpha");
        fixture.agent_copy(&fixture.claude, "delta", "delta");

        let snapshot = fixture.read();
        assert_eq!(snapshot.skills.len(), 2);
        assert_eq!(
            skill(&snapshot, "alpha").management,
            ManagementState::Unmanaged
        );
        assert_eq!(
            skill(&snapshot, "delta").management,
            ManagementState::Unmanaged
        );
        assert_eq!(snapshot.anomalies.total, 2);
        assert_eq!(snapshot.anomalies.stale.count, 0);
    }

    #[test]
    fn corrupt_and_v2_locks_fail_the_whole_read() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.paths.agents_root()).unwrap();
        fs::write(fixture.paths.lock_path(), "{not json").unwrap();
        let error = read_with_agents(&fixture.paths, &fixture.agents).unwrap_err();
        assert_eq!(error.code, codes::LOCK_CORRUPT);

        fs::write(fixture.paths.lock_path(), r#"{"version":2,"skills":{}}"#).unwrap();
        let error = read_with_agents(&fixture.paths, &fixture.agents).unwrap_err();
        assert_eq!(error.code, codes::LOCK_UNSUPPORTED_VERSION);
    }

    #[test]
    fn one_bad_skill_md_only_affects_its_row() {
        let fixture = Fixture::new();
        fixture.write_lock(&["alpha", "beta"]);
        fixture.canonical_skill("alpha", "good");
        let beta = fixture.paths.canonical_skills_dir().join("beta");
        fs::create_dir_all(&beta).unwrap();
        fs::write(
            beta.join("SKILL.md"),
            "---\ndescription: \"unterminated\n---\n",
        )
        .unwrap();

        let snapshot = fixture.read();
        assert_eq!(
            skill(&snapshot, "alpha").description.as_deref(),
            Some("good")
        );
        let beta = skill(&snapshot, "beta");
        assert!(beta.description.is_none());
        assert!(
            beta.warnings
                .iter()
                .any(|warning| warning.contains("frontmatter")),
            "{:?}",
            beta.warnings
        );
    }

    #[test]
    fn missing_frontmatter_yields_no_description_without_a_warning() {
        let fixture = Fixture::new();
        fixture.write_lock(&["alpha"]);
        let alpha = fixture.paths.canonical_skills_dir().join("alpha");
        fs::create_dir_all(&alpha).unwrap();
        fs::write(alpha.join("SKILL.md"), "# Alpha\n\nno frontmatter\n").unwrap();

        let snapshot = fixture.read();
        let alpha = skill(&snapshot, "alpha");
        assert!(alpha.description.is_none());
        assert!(alpha.warnings.is_empty());
    }

    #[test]
    fn renders_one_hundred_rows_directly() {
        let fixture = Fixture::new();
        for index in 0..100 {
            fixture.canonical_skill(&format!("skill-{index:03}"), "bulk");
        }
        let snapshot = fixture.read();
        assert_eq!(snapshot.skills.len(), 100);
        assert_eq!(snapshot.anomalies.unmanaged.count, 100);
    }

    #[test]
    fn dot_noise_and_skill_less_agent_directories_are_ignored() {
        let fixture = Fixture::new();
        fs::write(fixture.claude.join(".DS_Store"), "noise").unwrap();
        fs::create_dir_all(fixture.claude.join(".system")).unwrap();
        fs::create_dir_all(fixture.claude.join("random-dir")).unwrap();

        let snapshot = fixture.read();
        assert!(snapshot.skills.is_empty());
        assert_eq!(snapshot.anomalies.total, 0);
    }

    #[test]
    fn agent_dirs_are_optional_and_shared_detection_uses_resolved_paths() {
        let fixture = Fixture::new();
        fixture.write_lock(&["alpha"]);
        fixture.canonical_skill("alpha", "alpha");

        let agents = vec![AgentDir {
            id: "codex".to_string(),
            display_name: "Codex".to_string(),
            universal: true,
            // Same directory spelled through a symlinked alias.
            skills_dir: fixture._tmp.path().join("codex-alias"),
        }];
        symlink(&fixture.codex, fixture._tmp.path().join("codex-alias")).unwrap();

        let snapshot = read_with_agents(&fixture.paths, &agents).unwrap();
        assert_eq!(
            modes(skill(&snapshot, "alpha")),
            vec![("codex".to_string(), AttachmentMode::Native)]
        );
    }
}
