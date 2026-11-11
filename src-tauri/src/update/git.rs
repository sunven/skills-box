//! Standard Git acquisition for public HTTPS sources. All work stays in a
//! temporary checkout; dropping a check cancels the worker and its processes.

use std::io::{self, Write};
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use super::archive::{ARCHIVE_DOWNLOAD_TIMEOUT, ArchiveClient, ArchiveLimits};
use super::github::GitHubFetchError;
use crate::env::{EnvResolver, GitRuntime};
use crate::paths::AppPaths;
use crate::process::{CancelToken, CommandSpec, ProcessExecutor, tokio_executor::TokioExecutor};
use crate::tasks::source::validate_source;

pub struct GitArchiveClient {
    resolver: EnvResolver,
    executor: Arc<dyn ProcessExecutor>,
    runtime: Mutex<Option<GitRuntime>>,
}

impl GitArchiveClient {
    pub fn new(paths: AppPaths) -> Self {
        Self {
            resolver: EnvResolver::from_process(paths),
            executor: Arc::new(TokioExecutor),
            runtime: Mutex::new(None),
        }
    }

    async fn runtime(&self) -> Result<GitRuntime, GitHubFetchError> {
        let mut cached = self.runtime.lock().await;
        if let Some(runtime) = cached.as_ref() {
            return Ok(runtime.clone());
        }
        let runtime = self
            .resolver
            .git_runtime()
            .await
            .map_err(|error| GitHubFetchError::git(error.message))?;
        *cached = Some(runtime.clone());
        Ok(runtime)
    }
}

fn clone_url(source: &str) -> Result<String, GitHubFetchError> {
    let spec = validate_source(source).map_err(|error| GitHubFetchError::git(error.message))?;
    if !spec.normalized.starts_with("https://") {
        return Err(GitHubFetchError::git("Git 来源必须是完整的 HTTPS 仓库地址"));
    }
    Ok(spec.normalized)
}

struct CancelOnDrop(CancelToken);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

async fn worker<T: Send + 'static>(
    work: impl FnOnce(CancelToken) -> futures::future::BoxFuture<'static, Result<T, GitHubFetchError>>,
) -> Result<T, GitHubFetchError> {
    let cancel = CancelToken::new();
    let _guard = CancelOnDrop(cancel.clone());
    tokio::spawn(work(cancel))
        .await
        .map_err(|error| GitHubFetchError::git(format!("Git 来源读取失败：{error}")))?
}

async fn run(
    executor: &Arc<dyn ProcessExecutor>,
    runtime: &GitRuntime,
    cwd: &Path,
    args: Vec<String>,
    cancel: &CancelToken,
) -> Result<String, GitHubFetchError> {
    let mut env = runtime.env.clone();
    env.insert("GIT_TERMINAL_PROMPT".into(), "0".into());
    env.insert("GIT_LFS_SKIP_SMUDGE".into(), "1".into());
    env.insert("GIT_ALLOW_PROTOCOL".into(), "https".into());
    let output = executor
        .run(
            CommandSpec::new(&runtime.program, args, cwd, env, ARCHIVE_DOWNLOAD_TIMEOUT),
            cancel.clone(),
        )
        .await
        .map_err(|error| GitHubFetchError::git(error.to_string()))?;
    if !output.success() {
        return Err(GitHubFetchError::git(format!(
            "无法读取 Git 仓库：{}",
            output.stderr_trimmed()
        )));
    }
    Ok(output.stdout)
}

fn resolve_commit(output: &str, reference: Option<&str>) -> Result<String, GitHubFetchError> {
    let refs: std::collections::BTreeMap<_, _> = output
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .filter(|(sha, _)| crate::git_tree::is_git_tree_hash(sha))
        .map(|(sha, name)| (name, sha))
        .collect();
    let candidates = match reference {
        None => vec!["HEAD".to_string()],
        Some(reference) => vec![
            format!("refs/heads/{reference}"),
            format!("refs/tags/{reference}^{{}}"),
            format!("refs/tags/{reference}"),
            format!("{reference}^{{}}"),
            reference.to_string(),
        ],
    };
    candidates
        .iter()
        .find_map(|name| refs.get(name.as_str()).map(|sha| sha.to_string()))
        .ok_or_else(|| GitHubFetchError::git("远端没有可用的默认分支或指定 ref"))
}

#[async_trait]
impl ArchiveClient for GitArchiveClient {
    fn cache_namespace(&self, _source: &str) -> &'static str {
        "git"
    }
    async fn latest_commit(
        &self,
        source: &str,
        reference: Option<&str>,
    ) -> Result<String, GitHubFetchError> {
        let url = clone_url(source)?;
        // Existing locks may pin an immutable commit directly.
        if let Some(reference) = reference {
            if crate::git_tree::is_git_tree_hash(reference) {
                return Ok(reference.to_ascii_lowercase());
            }
            if reference.starts_with('-') || reference.chars().any(char::is_control) {
                return Err(GitHubFetchError::git("Git ref 格式无效"));
            }
        }
        let reference = reference.map(str::to_string);
        let runtime = self.runtime().await?;
        let executor = Arc::clone(&self.executor);
        worker(move |cancel| {
            Box::pin(async move {
                let temp = tempfile::tempdir()
                    .map_err(|error| GitHubFetchError::git(error.to_string()))?;
                let mut args = vec!["ls-remote".into(), "--symref".into(), "--".into(), url];
                if let Some(reference) = reference.as_deref() {
                    args.extend([
                        reference.to_string(),
                        format!("refs/heads/{reference}"),
                        format!("refs/tags/{reference}"),
                        format!("refs/tags/{reference}^{{}}"),
                        format!("{reference}^{{}}"),
                    ]);
                } else {
                    args.push("HEAD".into());
                }
                let output = run(&executor, &runtime, temp.path(), args, &cancel).await?;
                resolve_commit(&output, reference.as_deref())
            })
        })
        .await
    }

    async fn download_archive(
        &self,
        source: &str,
        commit: &str,
    ) -> Result<Vec<u8>, GitHubFetchError> {
        let url = clone_url(source)?;
        if !crate::git_tree::is_git_tree_hash(commit) {
            return Err(GitHubFetchError::git("Git commit 不是 40 位 SHA"));
        }
        let commit = commit.to_string();
        let runtime = self.runtime().await?;
        let executor = Arc::clone(&self.executor);
        worker(move |cancel| {
            Box::pin(async move {
                let temp = tempfile::tempdir()
                    .map_err(|error| GitHubFetchError::git(error.to_string()))?;
                run(
                    &executor,
                    &runtime,
                    temp.path(),
                    vec!["init".into(), "--quiet".into()],
                    &cancel,
                )
                .await?;
                run(
                    &executor,
                    &runtime,
                    temp.path(),
                    vec![
                        "fetch".into(),
                        "--depth=1".into(),
                        "--no-tags".into(),
                        "--".into(),
                        url,
                        commit,
                    ],
                    &cancel,
                )
                .await?;
                run(
                    &executor,
                    &runtime,
                    temp.path(),
                    vec![
                        "-c".into(),
                        "core.hooksPath=/dev/null".into(),
                        "checkout".into(),
                        "--quiet".into(),
                        "--detach".into(),
                        "FETCH_HEAD".into(),
                    ],
                    &cancel,
                )
                .await?;
                // Keep the temp checkout owned by the blocking worker until it ends.
                tokio::task::spawn_blocking(move || {
                    pack_checkout(temp.path(), ArchiveLimits::default(), &cancel)
                })
                .await
                .map_err(|error| GitHubFetchError::git(error.to_string()))?
            })
        })
        .await
    }
}

struct CappedWriter {
    bytes: Vec<u8>,
    limit: u64,
}
impl Write for CappedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.bytes.len() as u64 + bytes.len() as u64 > self.limit {
            return Err(io::Error::other("来源归档超过压缩大小上限"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn pack_checkout(
    root: &Path,
    limits: ArchiveLimits,
    cancel: &CancelToken,
) -> Result<Vec<u8>, GitHubFetchError> {
    let pack = || -> io::Result<Vec<u8>> {
        let writer = CappedWriter {
            bytes: Vec::new(),
            limit: limits.max_compressed_bytes,
        };
        let gzip = flate2::write::GzEncoder::new(writer, flate2::Compression::default());
        let mut tar = tar::Builder::new(gzip);
        tar.follow_symlinks(false);
        let mut total = 0u64;
        let mut count = 0usize;
        for entry in walkdir::WalkDir::new(root)
            .into_iter()
            .filter_entry(|entry| entry.depth() == 0 || entry.file_name() != ".git")
        {
            if cancel.is_cancelled() {
                return Err(io::Error::other("来源读取已取消"));
            }
            let entry = entry?;
            if !entry.file_type().is_file() && !entry.file_type().is_symlink() {
                continue;
            }
            let size = entry.metadata()?.len();
            total = total.saturating_add(size);
            count += 1;
            if size > limits.max_file_bytes
                || total > limits.max_uncompressed_bytes
                || count > limits.max_files
            {
                return Err(io::Error::other("来源内容超过大小或文件数量上限"));
            }
            let relative = entry.path().strip_prefix(root).map_err(io::Error::other)?;
            tar.append_path_with_name(entry.path(), Path::new("source").join(relative))?;
        }
        let gzip = tar.into_inner()?;
        Ok(gzip.finish()?.bytes)
    };
    pack().map_err(|error| GitHubFetchError::git(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::fake::{ScriptedExecutor, ScriptedResponse};
    use crate::process::{ExecError, ExecOutput};
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicBool, Ordering};

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
    const URL: &str = "https://cnb.cool/cnb/skills/cnb-skill.git";

    fn client(executor: Arc<dyn ProcessExecutor>) -> GitArchiveClient {
        GitArchiveClient {
            resolver: EnvResolver::from_process(AppPaths::new("/unused/home", "/unused/app")),
            executor,
            runtime: Mutex::new(Some(GitRuntime {
                program: "/test/git".into(),
                env: BTreeMap::new(),
            })),
        }
    }

    #[test]
    fn resolves_default_branch_branch_and_peeled_tags() {
        let output = format!(
            "ref: refs/heads/main\tHEAD\n{SHA}\tHEAD\n{SHA}\trefs/heads/dev/next\n{}\trefs/tags/v1\n{SHA}\trefs/tags/v1^{{}}\n",
            "a".repeat(40)
        );
        for reference in [None, Some("dev/next"), Some("v1")] {
            assert_eq!(resolve_commit(&output, reference).unwrap(), SHA);
        }
        assert!(resolve_commit(&output, Some("missing")).is_err());
        assert!(resolve_commit("broken response", None).is_err());
    }

    #[tokio::test]
    async fn reads_https_source_without_node_and_cleans_temp_directory() {
        let executor = Arc::new(ScriptedExecutor::new().expect(
            "/test/git",
            &["ls-remote", URL],
            ScriptedResponse::ok(format!("{SHA}\tHEAD\n")),
        ));
        assert_eq!(
            client(executor.clone())
                .latest_commit(URL, None)
                .await
                .unwrap(),
            SHA
        );
        let calls = executor.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].env["GIT_TERMINAL_PROMPT"], "0");
        assert_eq!(calls[0].env["GIT_ALLOW_PROTOCOL"], "https");
        assert!(!calls[0].cwd.exists());
    }

    #[tokio::test]
    async fn invalid_source_and_ref_never_spawn() {
        let executor = Arc::new(ScriptedExecutor::new());
        let client = client(executor.clone());
        assert!(
            client
                .latest_commit("file:///tmp/repo", None)
                .await
                .is_err()
        );
        assert!(
            client
                .latest_commit(URL, Some("--upload-pack=bad"))
                .await
                .is_err()
        );
        assert!(client.download_archive(URL, "../bad").await.is_err());
        assert_eq!(executor.call_count(), 0);
    }

    #[test]
    fn packs_checkout_with_cli_exclusions_and_content_hash_parity() {
        let temp = tempfile::tempdir().unwrap();
        for (path, content) in [
            ("SKILL.md", "skill"),
            ("references/guide.md", "guide"),
            (".git/data", "excluded"),
            ("node_modules/pkg/data", "excluded"),
            (".gitattributes", "references/guide.md export-ignore\n"),
        ] {
            let path = temp.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        std::os::unix::fs::symlink("SKILL.md", temp.path().join("linked.md")).unwrap();
        let raw =
            pack_checkout(temp.path(), ArchiveLimits::default(), &CancelToken::new()).unwrap();
        let archive =
            super::super::archive::SourceArchive::from_tar_gz(&raw, ArchiveLimits::default())
                .unwrap();
        assert!(archive.contains_file("references/guide.md"));
        assert!(!archive.contains_file("linked.md"));
        assert_eq!(archive.files.len(), 4);
        assert_eq!(
            super::super::content_hash::hash_entries(archive.files_under("")).unwrap(),
            super::super::content_hash::folder_hash(temp.path()).unwrap()
        );
        let portable = super::super::content_hash::HashVersion::Portable;
        assert_eq!(
            portable
                .hash_entries(archive.hash_files_under("", portable).unwrap())
                .unwrap(),
            portable.folder_hash(temp.path()).unwrap()
        );
        assert!(
            pack_checkout(
                temp.path(),
                ArchiveLimits {
                    max_files: 1,
                    ..ArchiveLimits::default()
                },
                &CancelToken::new()
            )
            .is_err()
        );
        assert!(
            pack_checkout(
                temp.path(),
                ArchiveLimits {
                    max_compressed_bytes: 1,
                    ..ArchiveLimits::default()
                },
                &CancelToken::new()
            )
            .is_err()
        );
    }

    #[test]
    fn portable_archive_hash_matches_disk_with_nested_links_and_skill_boundaries() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("skills/alpha");
        for (path, content) in [
            ("skills/alpha/SKILL.md", "skill\r\n"),
            ("skills/alpha/references/nested/guide.md", "guide\n"),
            ("skills/alpha/references/other.md", "other\n"),
            ("skills/alpha/metadata.json", "excluded"),
            ("skills/alpha/node_modules/dep/index.js", "included"),
            ("skills/alpha/__pycache__/cache.pyc", "excluded"),
            ("skills/beta/secret.md", "outside skill"),
        ] {
            let full = temp.path().join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, content).unwrap();
        }
        for (name, target) in [
            ("linked.md", "SKILL.md"),
            ("alias", "references/nested"),
            ("indirect.md", "alias/../other.md"),
            ("cycle", "."),
            ("references/back", ".."),
            ("outside.md", "../beta/secret.md"),
            ("broken.md", "missing"),
            ("loop-a", "loop-b"),
            ("loop-b", "loop-a"),
        ] {
            symlink(target, root.join(name)).unwrap();
        }
        let raw =
            pack_checkout(temp.path(), ArchiveLimits::default(), &CancelToken::new()).unwrap();
        let archive =
            super::super::archive::SourceArchive::from_tar_gz(&raw, ArchiveLimits::default())
                .unwrap();
        let portable = super::super::content_hash::HashVersion::Portable;
        let files = archive.hash_files_under("skills/alpha", portable).unwrap();
        assert!(files.iter().any(|(path, _)| path == "indirect.md"));
        assert!(
            files
                .iter()
                .all(|(path, _)| path != "outside.md" && path != "broken.md")
        );
        assert_eq!(
            portable.hash_entries(files).unwrap(),
            portable.folder_hash(&root).unwrap()
        );
    }

    #[derive(Default)]
    struct HangingExecutor {
        stopped: tokio::sync::Notify,
        cancelled: AtomicBool,
        cwd: std::sync::Mutex<Option<std::path::PathBuf>>,
    }

    #[async_trait]
    impl ProcessExecutor for HangingExecutor {
        async fn run(
            &self,
            spec: CommandSpec,
            cancel: CancelToken,
        ) -> Result<ExecOutput, ExecError> {
            *self.cwd.lock().unwrap() = Some(spec.cwd);
            cancel.cancelled().await;
            self.cancelled.store(true, Ordering::SeqCst);
            self.stopped.notify_one();
            Err(ExecError::Cancelled)
        }
    }

    #[tokio::test]
    async fn dropping_check_cancels_worker_and_cleans_checkout() {
        let executor = Arc::new(HangingExecutor::default());
        let client = client(executor.clone());
        assert!(
            tokio::time::timeout(
                std::time::Duration::from_millis(20),
                client.latest_commit(URL, None)
            )
            .await
            .is_err()
        );
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            executor.stopped.notified(),
        )
        .await
        .unwrap();
        assert!(executor.cancelled.load(Ordering::SeqCst));
        assert!(!executor.cwd.lock().unwrap().as_ref().unwrap().exists());
    }
}
