use super::*;
use crate::update::github::GitHubFetchError;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use tokio::sync::{mpsc, oneshot};

const COMMIT: &str = "1aee802d9139d4df84d017de44fb1b37a7b614e9";
const SOURCE: &str = "acme/skills";

struct TestClock(AtomicU64);

impl Clock for TestClock {
    fn now_epoch_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

struct FakeClient {
    commit: Result<String, GitHubFetchError>,
    archive: Result<Vec<u8>, GitHubFetchError>,
    commit_calls: AtomicUsize,
    download_calls: AtomicUsize,
    pause: Option<mpsc::UnboundedSender<oneshot::Sender<()>>>,
}

impl FakeClient {
    fn new() -> Self {
        Self {
            commit: Ok(COMMIT.to_string()),
            archive: Ok(archive()),
            commit_calls: AtomicUsize::new(0),
            download_calls: AtomicUsize::new(0),
            pause: None,
        }
    }
}

#[async_trait::async_trait]
impl ArchiveClient for FakeClient {
    fn cache_namespace(&self, source: &str) -> &'static str {
        if source.starts_with("https://") {
            "git"
        } else {
            ""
        }
    }
    async fn latest_commit(
        &self,
        _source: &str,
        _reference: Option<&str>,
    ) -> Result<String, GitHubFetchError> {
        self.commit_calls.fetch_add(1, Ordering::SeqCst);
        self.commit.clone()
    }

    async fn download_archive(
        &self,
        _source: &str,
        _commit: &str,
    ) -> Result<Vec<u8>, GitHubFetchError> {
        self.download_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(paused) = &self.pause {
            let (resume, waiting) = oneshot::channel();
            paused.send(resume).unwrap();
            waiting.await.unwrap();
        }
        self.archive.clone()
    }
}

#[tokio::test]
async fn acquisition_methods_keep_separate_commit_caches() {
    let fixture = Fixture::new(FakeClient::new());
    let archives = fixture.archives();
    let first = archives.acquire(SOURCE, None).await.unwrap();
    let second = archives
        .acquire("https://cnb.cool/group/repo.git", None)
        .await
        .unwrap();
    assert!(!first.cached && !second.cached);
    assert_eq!(fixture.downloads(), 2);
    assert!(
        fixture
            .cache
            .load(COMMIT, ArchiveLimits::default())
            .is_some()
    );
    assert!(
        fixture
            .cache
            .namespaced("git")
            .load(COMMIT, ArchiveLimits::default())
            .is_some()
    );
    let archives = fixture.archives();
    assert!(
        archives
            .acquire("https://cnb.cool/group/repo.git", None)
            .await
            .unwrap()
            .cached
    );
    assert_eq!(fixture.downloads(), 2);
}

struct Fixture {
    _dir: tempfile::TempDir,
    client: Arc<FakeClient>,
    clock: Arc<TestClock>,
    cache: CommitArchiveCache,
}

impl Fixture {
    fn new(client: FakeClient) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let cache = CommitArchiveCache::from_dir(dir.path().join("archives"));
        Self {
            _dir: dir,
            client: Arc::new(client),
            clock: Arc::new(TestClock(AtomicU64::new(1_000))),
            cache,
        }
    }

    fn archives(&self) -> SourceArchives {
        SourceArchives::new(
            self.client.clone(),
            self.cache.clone(),
            self.clock.clone(),
            100,
            ArchiveLimits::default(),
        )
    }

    fn downloads(&self) -> usize {
        self.client.download_calls.load(Ordering::SeqCst)
    }
}

fn archive() -> Vec<u8> {
    use flate2::{Compression, write::GzEncoder};
    use std::io::Write;
    let content = b"hello";
    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(content.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(&mut header, "root/SKILL.md", &content[..])
        .unwrap();
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&builder.into_inner().unwrap()).unwrap();
    encoder.finish().unwrap()
}

#[tokio::test]
async fn same_commit_waiters_share_download_and_preserve_cached_flags() {
    let (paused, mut requests) = mpsc::unbounded_channel();
    let mut client = FakeClient::new();
    client.pause = Some(paused);
    let fixture = Fixture::new(client);
    let archives = fixture.archives();
    let mut first = Box::pin(archives.acquire(SOURCE, Some("main")));
    let mut second = Box::pin(archives.acquire(SOURCE, Some("dev")));
    assert!(futures::poll!(&mut first).is_pending());
    assert!(futures::poll!(&mut second).is_pending());
    assert_eq!(fixture.downloads(), 1);
    requests.try_recv().unwrap().send(()).unwrap();

    let (first, second) = futures::join!(first, second);
    let first = first.unwrap();
    let second = second.unwrap();
    assert_eq!(first.archive.files["SKILL.md"], b"hello");
    assert_eq!(second.archive.files["SKILL.md"], b"hello");
    assert!(!first.cached);
    assert!(second.cached);
    assert_eq!(first.fetched_at_epoch_ms, 1_000);
    assert_eq!(second.fetched_at_epoch_ms, 1_000);
    assert_eq!(fixture.client.commit_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn dropping_the_first_waiter_keeps_the_shared_download_alive() {
    let (paused, mut requests) = mpsc::unbounded_channel();
    let mut client = FakeClient::new();
    client.pause = Some(paused);
    let fixture = Fixture::new(client);
    let archives = fixture.archives();
    let mut first = Box::pin(archives.acquire(SOURCE, Some("main")));
    let mut second = Box::pin(archives.acquire(SOURCE, Some("dev")));
    assert!(futures::poll!(&mut first).is_pending());
    assert!(futures::poll!(&mut second).is_pending());
    let resume = requests.try_recv().unwrap();
    drop(first);
    assert!(!resume.is_closed());
    resume.send(()).unwrap();

    let acquired = second.await.unwrap();
    assert_eq!(acquired.archive.files["SKILL.md"], b"hello");
    assert!(acquired.cached);
    assert_eq!(fixture.downloads(), 1);
}

#[tokio::test]
async fn dropping_an_attempt_releases_download_and_allows_a_new_attempt() {
    let (paused, mut requests) = mpsc::unbounded_channel();
    let mut client = FakeClient::new();
    client.pause = Some(paused);
    let fixture = Fixture::new(client);
    let weak_client = Arc::downgrade(&fixture.client);
    let archives = fixture.archives();
    let mut waiting = Box::pin(archives.acquire(SOURCE, None));
    assert!(futures::poll!(&mut waiting).is_pending());
    let abandoned = requests.try_recv().unwrap();
    drop(waiting);
    drop(archives);
    assert!(abandoned.is_closed());
    assert!(
        fixture
            .cache
            .load(COMMIT, ArchiveLimits::default())
            .is_none()
    );

    let next = fixture.archives();
    let mut waiting = Box::pin(next.acquire(SOURCE, None));
    assert!(futures::poll!(&mut waiting).is_pending());
    requests.try_recv().unwrap().send(()).unwrap();
    assert!(!waiting.await.unwrap().cached);
    assert_eq!(fixture.downloads(), 2);
    drop(next);
    drop(fixture);
    assert!(weak_client.upgrade().is_none());
}

#[tokio::test]
async fn separate_attempts_have_independent_in_flight_downloads() {
    let (paused, mut requests) = mpsc::unbounded_channel();
    let mut client = FakeClient::new();
    client.pause = Some(paused);
    let fixture = Fixture::new(client);
    let first_attempt = fixture.archives();
    let second_attempt = fixture.archives();
    let mut first = Box::pin(first_attempt.acquire(SOURCE, None));
    let mut second = Box::pin(second_attempt.acquire(SOURCE, None));
    assert!(futures::poll!(&mut first).is_pending());
    assert!(futures::poll!(&mut second).is_pending());
    assert_eq!(fixture.downloads(), 2);
    requests.try_recv().unwrap().send(()).unwrap();
    requests.try_recv().unwrap().send(()).unwrap();
    let (first, second) = futures::join!(first, second);
    assert!(!first.unwrap().cached);
    assert!(!second.unwrap().cached);
}

#[tokio::test]
async fn fresh_disk_cache_preserves_timestamp_after_commit_resolution() {
    let fixture = Fixture::new(FakeClient::new());
    fixture.cache.store(COMMIT, &archive(), 990).unwrap();
    let acquired = fixture.archives().acquire(SOURCE, None).await.unwrap();
    assert_eq!(acquired.archive.files["SKILL.md"], b"hello");
    assert!(acquired.cached);
    assert_eq!(acquired.fetched_at_epoch_ms, 990);
    assert_eq!(fixture.downloads(), 0);
    assert_eq!(fixture.client.commit_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn expired_cache_records_the_time_before_the_new_download() {
    let (paused, mut requests) = mpsc::unbounded_channel();
    let mut client = FakeClient::new();
    client.pause = Some(paused);
    let fixture = Fixture::new(client);
    fixture.cache.store(COMMIT, &archive(), 900).unwrap();
    let archives = fixture.archives();
    let mut waiting = Box::pin(archives.acquire(SOURCE, None));
    assert!(futures::poll!(&mut waiting).is_pending());
    fixture.clock.0.store(2_000, Ordering::SeqCst);
    requests.try_recv().unwrap().send(()).unwrap();
    let acquired = waiting.await.unwrap();
    assert!(!acquired.cached);
    assert_eq!(acquired.fetched_at_epoch_ms, 1_000);
    assert_eq!(fixture.downloads(), 1);
    assert_eq!(
        fixture
            .cache
            .load(COMMIT, ArchiveLimits::default())
            .unwrap()
            .fetched_at_epoch_ms,
        1_000,
    );
}

#[tokio::test]
async fn acquisition_failure_is_shared_and_a_later_attempt_retries() {
    let (paused, mut requests) = mpsc::unbounded_channel();
    let mut client = FakeClient::new();
    client.archive = Err(GitHubFetchError::offline("offline"));
    client.pause = Some(paused);
    let fixture = Fixture::new(client);
    fixture.cache.store(COMMIT, &archive(), 900).unwrap();
    let archives = fixture.archives();
    let mut first = Box::pin(archives.acquire(SOURCE, Some("main")));
    let mut second = Box::pin(archives.acquire(SOURCE, Some("dev")));
    assert!(futures::poll!(&mut first).is_pending());
    assert!(futures::poll!(&mut second).is_pending());
    requests.try_recv().unwrap().send(()).unwrap();
    let (first, second) = futures::join!(first, second);
    let first = first.unwrap_err();
    let second = second.unwrap_err();
    assert_eq!(first.error.code, crate::error::ErrorCode::NetworkOffline);
    assert_eq!(first.previous_fetched_at, Some(900));
    assert_eq!(second.error, first.error);
    assert_eq!(second.previous_fetched_at, Some(900));
    assert_eq!(fixture.downloads(), 1);
    assert_eq!(
        fixture
            .cache
            .load(COMMIT, ArchiveLimits::default())
            .unwrap()
            .fetched_at_epoch_ms,
        900,
    );

    let next = fixture.archives();
    let mut waiting = Box::pin(next.acquire(SOURCE, None));
    assert!(futures::poll!(&mut waiting).is_pending());
    requests.try_recv().unwrap().send(()).unwrap();
    waiting.await.unwrap_err();
    assert_eq!(fixture.downloads(), 2);
}

#[tokio::test]
async fn corrupt_fresh_payload_is_redownloaded() {
    let fixture = Fixture::new(FakeClient::new());
    fixture.cache.store(COMMIT, &archive(), 990).unwrap();
    std::fs::write(
        fixture.cache.dir().join(format!("{COMMIT}.tar.gz")),
        b"broken",
    )
    .unwrap();
    let acquired = fixture.archives().acquire(SOURCE, None).await.unwrap();
    assert_eq!(acquired.archive.files["SKILL.md"], b"hello");
    assert!(!acquired.cached);
    assert_eq!(acquired.fetched_at_epoch_ms, 1_000);
    assert_eq!(fixture.downloads(), 1);
}

#[tokio::test]
async fn corrupt_payload_has_history_only_when_metadata_is_stale() {
    for (fetched_at, previous) in [(990, None), (900, Some(900))] {
        let mut client = FakeClient::new();
        client.archive = Err(GitHubFetchError::offline("offline"));
        let fixture = Fixture::new(client);
        fixture.cache.store(COMMIT, &archive(), fetched_at).unwrap();
        let payload = fixture.cache.dir().join(format!("{COMMIT}.tar.gz"));
        std::fs::write(&payload, b"broken").unwrap();
        let failure = fixture.archives().acquire(SOURCE, None).await.unwrap_err();
        assert_eq!(failure.previous_fetched_at, previous);
        assert_eq!(fixture.downloads(), 1);
        assert_eq!(std::fs::read(&payload).unwrap(), b"broken");
    }
}

#[tokio::test]
async fn commit_resolution_failure_has_no_archive_history() {
    let mut client = FakeClient::new();
    client.commit = Err(GitHubFetchError::offline("offline"));
    let fixture = Fixture::new(client);
    fixture.cache.store(COMMIT, &archive(), 990).unwrap();
    let failure = fixture.archives().acquire(SOURCE, None).await.unwrap_err();
    assert_eq!(failure.error.code, crate::error::ErrorCode::NetworkOffline);
    assert_eq!(failure.previous_fetched_at, None);
    assert_eq!(fixture.downloads(), 0);
}

#[tokio::test]
async fn cache_write_failure_preserves_success_and_in_attempt_reuse() {
    let fixture = Fixture::new(FakeClient::new());
    std::fs::write(fixture.cache.dir(), b"not a directory").unwrap();
    let archives = fixture.archives();
    let first = archives.acquire(SOURCE, Some("main")).await.unwrap();
    let second = archives.acquire(SOURCE, Some("dev")).await.unwrap();
    assert_eq!(first.archive.files["SKILL.md"], b"hello");
    assert_eq!(second.archive.files["SKILL.md"], b"hello");
    assert!(!first.cached);
    assert!(second.cached);
    assert_eq!(fixture.downloads(), 1);
    assert!(
        fixture
            .cache
            .load(COMMIT, ArchiveLimits::default())
            .is_none()
    );
}

#[tokio::test]
async fn archive_limits_fail_acquisition_without_persisting() {
    for limits in [
        ArchiveLimits {
            max_compressed_bytes: 1,
            ..ArchiveLimits::default()
        },
        ArchiveLimits {
            max_file_bytes: 4,
            ..ArchiveLimits::default()
        },
        ArchiveLimits {
            max_uncompressed_bytes: 4,
            ..ArchiveLimits::default()
        },
        ArchiveLimits {
            max_files: 0,
            ..ArchiveLimits::default()
        },
    ] {
        let fixture = Fixture::new(FakeClient::new());
        let archives = SourceArchives::new(
            fixture.client.clone(),
            fixture.cache.clone(),
            fixture.clock.clone(),
            100,
            limits,
        );
        let failure = archives.acquire(SOURCE, None).await.unwrap_err();
        assert_eq!(failure.error.code, crate::error::ErrorCode::NetworkError);
        assert_eq!(fixture.downloads(), 1);
        assert!(
            fixture
                .cache
                .load(COMMIT, ArchiveLimits::default())
                .is_none()
        );
    }
}
