//! Bounded anonymous HTTP access to the ClawHub catalog.
//!
//! Caching is process memory only and obeys `Cache-Control` (the live API
//! sends `max-age=0`, so responses are effectively not reused). A 429 sets a
//! shared cooldown so repeated UI retries cannot hammer the upstream endpoint.

use crate::error::AppError;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime},
};

const API: &str = "https://clawhub.ai";
const PATH: &str = "/api/v1/skills";
const MAX_RESPONSE: usize = 8 * 1024 * 1024;
const MAX_CACHE_ENTRIES: usize = 12;
const MAX_CACHE_TTL: Duration = Duration::from_secs(300);
const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(60);
const MAX_RETRY_AFTER: Duration = Duration::from_secs(300);

struct CacheEntry {
    value: Value,
    expires: Instant,
}

#[derive(Default)]
struct Cache {
    entries: BTreeMap<String, CacheEntry>,
}

impl Cache {
    fn get(&self, key: &str, now: Instant) -> Option<&Value> {
        self.entries
            .get(key)
            .filter(|entry| now < entry.expires)
            .map(|entry| &entry.value)
    }

    fn put(&mut self, key: String, value: Value, ttl: Duration, now: Instant) {
        if ttl.is_zero() {
            return;
        }
        if self.entries.len() >= MAX_CACHE_ENTRIES && !self.entries.contains_key(&key) {
            if let Some(oldest) = self.entries.keys().next().cloned() {
                self.entries.remove(&oldest);
            }
        }
        self.entries.insert(
            key,
            CacheEntry {
                value,
                expires: now + ttl.min(MAX_CACHE_TTL),
            },
        );
    }
}

static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
static COOLDOWN: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

pub(super) async fn get(key: &str, query: &[(&str, String)]) -> Result<Value, AppError> {
    get_at(PATH, key, query).await
}

pub(super) async fn get_at(
    path: &str,
    key: &str,
    query: &[(&str, String)],
) -> Result<Value, AppError> {
    get_at_optional(path, key, query)
        .await?
        .ok_or_else(|| AppError::clawhub_unavailable("HTTP 404"))
}

/// Like [`get_at`], but an upstream `404` is a normal "no such public record"
/// answer instead of an error. Detail, version and file reads use this to
/// distinguish missing content from transport/rate-limit failures.
pub(super) async fn get_at_optional(
    path: &str,
    key: &str,
    query: &[(&str, String)],
) -> Result<Option<Value>, AppError> {
    if let Some(value) = cache_get(key) {
        return Ok(Some(value));
    }
    let mut response = send(path, query).await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let ttl = cache_ttl(header(response.headers(), reqwest::header::CACHE_CONTROL));
    let value: Value = serde_json::from_slice(&bounded_body(&mut response, MAX_RESPONSE).await?)
        .map_err(|error| AppError::clawhub_unavailable(format!("响应不是有效 JSON：{error}")))?;
    cache_put(key, value.clone(), ttl);
    Ok(Some(value))
}

/// Raw file bytes for the read-only preview. The caller always supplies the
/// app's own preview limit; a larger body is reported as oversized instead of
/// being downloaded into memory.
pub(super) enum Payload {
    NotFound,
    Body(Vec<u8>),
    TooLarge,
}

pub(super) async fn get_bytes_at(
    path: &str,
    query: &[(&str, String)],
    limit: u64,
) -> Result<Payload, AppError> {
    let mut response = send(path, query).await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(Payload::NotFound);
    }
    if response.content_length().is_some_and(|size| size > limit) {
        return Ok(Payload::TooLarge);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| AppError::clawhub_unavailable(error.to_string()))?
    {
        if bytes.len() as u64 + chunk.len() as u64 > limit {
            return Ok(Payload::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(Payload::Body(bytes))
}

/// One anonymous GET with the shared cooldown. `404` is returned to optional
/// callers; every other non-success status becomes a source-specific error.
async fn send(path: &str, query: &[(&str, String)]) -> Result<reqwest::Response, AppError> {
    if let Some(remaining) = cooldown_remaining() {
        return Err(AppError::clawhub_rate_limited(remaining.as_secs().max(1)));
    }
    let response = client()?
        .get(format!("{API}{path}"))
        .query(query)
        .send()
        .await
        .map_err(|error| AppError::clawhub_unavailable(error.to_string()))?;
    let status = response.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let wait = retry_after(
            header(response.headers(), reqwest::header::RETRY_AFTER),
            SystemTime::now(),
        );
        set_cooldown(wait);
        return Err(AppError::clawhub_rate_limited(wait.as_secs().max(1)));
    }
    if !status.is_success() && status != reqwest::StatusCode::NOT_FOUND {
        return Err(AppError::clawhub_unavailable(format!("HTTP {status}")));
    }
    Ok(response)
}

async fn bounded_body(response: &mut reqwest::Response, limit: usize) -> Result<Vec<u8>, AppError> {
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err(AppError::clawhub_unavailable("响应超过 8 MB 限制"));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| AppError::clawhub_unavailable(error.to_string()))?
    {
        if bytes.len() + chunk.len() > limit {
            return Err(AppError::clawhub_unavailable("响应超过 8 MB 限制"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn client() -> Result<&'static reqwest::Client, AppError> {
    if let Some(client) = CLIENT.get() {
        return Ok(client);
    }
    let built = reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .user_agent("SkillsBox/0.1 ClawHub-marketplace")
        .build()
        .map_err(AppError::clawhub_unavailable)?;
    Ok(CLIENT.get_or_init(|| built))
}

fn header<'a>(
    headers: &'a reqwest::header::HeaderMap,
    name: reqwest::header::HeaderName,
) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn cache() -> &'static Mutex<Cache> {
    CACHE.get_or_init(|| Mutex::new(Cache::default()))
}

fn cache_get(key: &str) -> Option<Value> {
    cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(key, Instant::now()).cloned())
}

fn cache_put(key: &str, value: Value, ttl: Duration) {
    if let Ok(mut cache) = cache().lock() {
        cache.put(key.to_string(), value, ttl, Instant::now());
    }
}

fn cooldown_remaining() -> Option<Duration> {
    let guard = COOLDOWN.get_or_init(|| Mutex::new(None)).lock().ok()?;
    let until = (*guard)?;
    until
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
}

fn set_cooldown(wait: Duration) {
    if let Ok(mut cooldown) = COOLDOWN.get_or_init(|| Mutex::new(None)).lock() {
        *cooldown = Some(Instant::now() + wait.min(MAX_RETRY_AFTER));
    }
}

/// `Retry-After` is either delta-seconds or an HTTP date. Missing or invalid
/// values fall back to a one-second pause; the wait is clamped to five minutes
/// so a broken header cannot disable the source for the whole session.
fn retry_after(raw: Option<&str>, now: SystemTime) -> Duration {
    let Some(raw) = raw.map(str::trim).filter(|raw| !raw.is_empty()) else {
        return Duration::from_secs(1);
    };
    if let Ok(seconds) = raw.parse::<u64>() {
        return Duration::from_secs(seconds).clamp(Duration::from_secs(1), MAX_RETRY_AFTER);
    }
    httpdate::parse_http_date(raw)
        .ok()
        .and_then(|at| at.duration_since(now).ok())
        .map(|wait| wait.clamp(Duration::from_secs(1), MAX_RETRY_AFTER))
        .unwrap_or(Duration::from_secs(1))
}

/// Session-cache lifetime derived from `Cache-Control`. `no-store`,
/// `no-cache` and `max-age=0` disable reuse; the live catalog currently sends
/// `max-age=0`.
fn cache_ttl(raw: Option<&str>) -> Duration {
    let Some(raw) = raw else {
        return DEFAULT_CACHE_TTL;
    };
    let raw = raw.to_ascii_lowercase();
    if raw.contains("no-store") || raw.contains("no-cache") {
        return Duration::ZERO;
    }
    raw.split(',')
        .find_map(|part| {
            part.trim()
                .strip_prefix("max-age=")
                .and_then(|value| value.trim_matches('"').parse::<u64>().ok())
                .map(Duration::from_secs)
        })
        .map_or(DEFAULT_CACHE_TTL, |ttl| ttl.min(MAX_CACHE_TTL))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_after_accepts_seconds_and_http_dates_with_bounds() {
        assert_eq!(retry_after(None, SystemTime::now()), Duration::from_secs(1));
        assert_eq!(
            retry_after(Some("120"), SystemTime::now()),
            Duration::from_secs(120)
        );
        assert_eq!(
            retry_after(Some("0"), SystemTime::now()),
            Duration::from_secs(1)
        );
        assert_eq!(
            retry_after(Some("99999"), SystemTime::now()),
            MAX_RETRY_AFTER
        );
        assert_eq!(
            retry_after(Some("soon"), SystemTime::now()),
            Duration::from_secs(1)
        );

        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let later = httpdate::fmt_http_date(now + Duration::from_secs(45));
        assert_eq!(retry_after(Some(&later), now), Duration::from_secs(45));
        let earlier = httpdate::fmt_http_date(now - Duration::from_secs(10));
        assert_eq!(retry_after(Some(&earlier), now), Duration::from_secs(1));
    }

    #[test]
    fn cache_lifetime_obeys_cache_control_and_is_bounded() {
        assert_eq!(cache_ttl(None), DEFAULT_CACHE_TTL);
        assert_eq!(
            cache_ttl(Some("public, max-age=60")),
            Duration::from_secs(60)
        );
        assert_eq!(cache_ttl(Some("public, max-age=0")), Duration::ZERO);
        assert_eq!(cache_ttl(Some("no-store")), Duration::ZERO);
        assert_eq!(cache_ttl(Some("private, no-cache")), Duration::ZERO);
        assert_eq!(cache_ttl(Some("max-age=999999")), MAX_CACHE_TTL);
    }

    #[test]
    fn cache_is_bounded_and_never_serves_expired_entries() {
        let now = Instant::now();
        let mut cache = Cache::default();
        cache.put(
            "old".into(),
            serde_json::json!({"items": []}),
            Duration::from_secs(30),
            now,
        );
        assert!(cache.get("old", now + Duration::from_secs(29)).is_some());
        assert!(cache.get("old", now + Duration::from_secs(30)).is_none());
        cache.put("zero".into(), serde_json::json!(1), Duration::ZERO, now);
        assert!(cache.get("zero", now).is_none());
        for index in 0..MAX_CACHE_ENTRIES + 3 {
            cache.put(
                format!("key-{index:02}"),
                serde_json::json!(index),
                Duration::from_secs(60),
                now,
            );
        }
        assert_eq!(cache.entries.len(), MAX_CACHE_ENTRIES);
        assert!(cache.get("key-00", now).is_none());
    }
}
