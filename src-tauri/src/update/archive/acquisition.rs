//! Source archive acquisition shared only within one Check attempt.

use std::collections::BTreeMap;
use std::sync::Arc;

use futures::FutureExt;
use futures::future::{BoxFuture, Shared};
use tokio::sync::Mutex;

use super::{ArchiveClient, ArchiveError, ArchiveLimits, CommitArchiveCache, SourceArchive};
use crate::error::AppError;
use crate::update::Clock;
use crate::update::cache::is_fresh;

type Acquisition = Shared<BoxFuture<'static, Result<AcquiredArchive, AcquisitionFailure>>>;

pub(crate) struct SourceArchives {
    client: Arc<dyn ArchiveClient>,
    cache: CommitArchiveCache,
    clock: Arc<dyn Clock>,
    cache_ttl_ms: u64,
    limits: ArchiveLimits,
    acquisitions: Mutex<BTreeMap<String, Acquisition>>,
}

#[derive(Debug, Clone)]
pub(crate) struct AcquiredArchive {
    pub archive: Arc<SourceArchive>,
    pub fetched_at_epoch_ms: u64,
    pub cached: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct AcquisitionFailure {
    pub error: AppError,
    pub previous_fetched_at: Option<u64>,
}

impl SourceArchives {
    pub(crate) fn new(
        client: Arc<dyn ArchiveClient>,
        cache: CommitArchiveCache,
        clock: Arc<dyn Clock>,
        cache_ttl_ms: u64,
        limits: ArchiveLimits,
    ) -> Self {
        Self {
            client,
            cache,
            clock,
            cache_ttl_ms,
            limits,
            acquisitions: Mutex::new(BTreeMap::new()),
        }
    }

    pub(crate) async fn acquire(
        &self,
        source: &str,
        reference: Option<&str>,
    ) -> Result<AcquiredArchive, AcquisitionFailure> {
        let commit = self
            .client
            .latest_commit(source, reference)
            .await
            .map_err(|error| AcquisitionFailure {
                error: error.to_app_error(),
                previous_fetched_at: None,
            })?;
        let (acquisition, reused) = {
            let mut acquisitions = self.acquisitions.lock().await;
            if let Some(acquisition) = acquisitions.get(&commit) {
                (acquisition.clone(), true)
            } else {
                let acquisition = self.acquisition(source.to_string(), commit.clone());
                acquisitions.insert(commit, acquisition.clone());
                (acquisition, false)
            }
        };
        acquisition.await.map(|mut acquired| {
            acquired.cached |= reused;
            acquired
        })
    }

    fn acquisition(&self, source: String, commit: String) -> Acquisition {
        // Capture dependencies, not this module: the map owns the future and
        // must release it when the Check attempt ends, without a holding cycle.
        let client = Arc::clone(&self.client);
        let cache = self.cache.clone();
        let clock = Arc::clone(&self.clock);
        let cache_ttl_ms = self.cache_ttl_ms;
        let limits = self.limits;
        async move {
            let mut previous_fetched_at = cache.fetched_at(&commit);
            if let Some(fetched_at) = previous_fetched_at
                && is_fresh(fetched_at, clock.now_epoch_ms(), cache_ttl_ms)
            {
                if let Some(cached) = cache.load(&commit, limits) {
                    return Ok(AcquiredArchive {
                        archive: Arc::new(cached.archive),
                        fetched_at_epoch_ms: cached.fetched_at_epoch_ms,
                        cached: true,
                    });
                }
                // Fresh metadata with an unreadable payload is not a hit.
                previous_fetched_at = None;
            }
            let now = clock.now_epoch_ms();
            let result = async {
                let raw = client
                    .download_archive(&source, &commit)
                    .await
                    .map_err(ArchiveError::from)?;
                if raw.len() as u64 > limits.max_compressed_bytes {
                    return Err(ArchiveError::TooLarge {
                        detail: format!("压缩包为 {} 字节", raw.len()),
                    });
                }
                let archive = SourceArchive::from_tar_gz(&raw, limits)?;
                Ok::<_, ArchiveError>((raw, archive))
            }
            .await;
            match result {
                Ok((raw, archive)) => {
                    // Persistence failure cannot invalidate a parsed archive.
                    let _ = cache.store(&commit, &raw, now);
                    Ok(AcquiredArchive {
                        archive: Arc::new(archive),
                        fetched_at_epoch_ms: now,
                        cached: false,
                    })
                }
                Err(error) => Err(AcquisitionFailure {
                    error: error.to_app_error(),
                    previous_fetched_at,
                }),
            }
        }
        .boxed()
        .shared()
    }
}

#[cfg(test)]
mod tests;
