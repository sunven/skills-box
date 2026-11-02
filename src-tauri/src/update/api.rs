//! Shared GitHub REST requests. Credentials are resolved for each request and
//! never attached to the separate codeload client or followed to redirects.

use std::sync::Arc;

use reqwest::header::{AUTHORIZATION, HeaderValue};
use serde::{Deserialize, Serialize};

use super::github::{
    GITHUB_API_BASE, GITHUB_REQUEST_TIMEOUT, GitHubFetchError, classify_http_error,
};
use crate::settings::{Credentials, SettingsService, TokenSource};

#[derive(Debug, Clone)]
pub struct GitHubApiClient {
    client: reqwest::Client,
    settings: Option<Arc<SettingsService>>,
    base: reqwest::Url,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubAuthTestResult {
    pub source: TokenSource,
    pub limit: u64,
    pub remaining: u64,
    pub reset_at_epoch_ms: u64,
    #[serde(skip)]
    pub(crate) context: String,
    #[serde(skip)]
    pub(crate) revision: u64,
}

impl GitHubApiClient {
    pub fn new(settings: Arc<SettingsService>) -> Self {
        Self {
            settings: Some(settings),
            ..Self::anonymous()
        }
    }

    pub fn anonymous() -> Self {
        Self::with_client(
            reqwest::Client::builder()
                .timeout(GITHUB_REQUEST_TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .user_agent(concat!("skills-box/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("valid GitHub HTTP client configuration"),
        )
    }

    /// Injected anonymous transport; production authentication uses `new`.
    pub fn with_client(client: reqwest::Client) -> Self {
        Self {
            client,
            settings: None,
            base: reqwest::Url::parse(GITHUB_API_BASE).unwrap(),
        }
    }

    pub fn revision(&self) -> u64 {
        self.settings
            .as_ref()
            .and_then(|s| s.credentials().ok())
            .map(|c| c.revision)
            .unwrap_or(0)
    }
    pub fn context(&self) -> Result<String, GitHubFetchError> {
        self.credential_context().map(|(context, _)| context)
    }
    pub fn credential_context(&self) -> Result<(String, u64), GitHubFetchError> {
        let credentials = match &self.settings {
            Some(s) => s
                .credentials()
                .map_err(|error| GitHubFetchError::Settings { error })?,
            None => Credentials {
                source: TokenSource::Anonymous,
                token: None,
                revision: 0,
            },
        };
        Ok((credential_context(&credentials), credentials.revision))
    }

    pub async fn get_text(&self, url: reqwest::Url) -> Result<String, GitHubFetchError> {
        self.get(url).await.map(|(body, _, _, _)| body)
    }

    async fn get(
        &self,
        mut url: reqwest::Url,
    ) -> Result<(String, TokenSource, String, u64), GitHubFetchError> {
        // The production origin is fixed. Test-only base replacement allows
        // wire-level tests without real credentials or requests to GitHub.
        if url.origin() != reqwest::Url::parse(GITHUB_API_BASE).unwrap().origin() {
            return Err(GitHubFetchError::http(400, "认证请求仅允许 GitHub API"));
        }
        url.set_scheme(self.base.scheme())
            .map_err(|_| GitHubFetchError::parse("invalid API scheme"))?;
        url.set_host(self.base.host_str())
            .map_err(|_| GitHubFetchError::parse("invalid API host"))?;
        url.set_port(self.base.port())
            .map_err(|_| GitHubFetchError::parse("invalid API port"))?;
        let credentials = match &self.settings {
            Some(settings) => settings
                .credentials()
                .map_err(|error| GitHubFetchError::Settings { error })?,
            None => Credentials {
                source: TokenSource::Anonymous,
                token: None,
                revision: 0,
            },
        };
        let context = credential_context(&credentials);
        let mut request = self
            .client
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(token) = &credentials.token {
            let mut header = HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| {
                GitHubFetchError::Settings {
                    error: crate::error::AppError::settings_invalid_token(),
                }
            })?;
            header.set_sensitive(true);
            request = request.header(AUTHORIZATION, header);
        }
        let response = request.send().await.map_err(|error| {
            let detail = redact(&error.to_string(), credentials.token.as_deref());
            if error.is_connect() || error.is_timeout() {
                GitHubFetchError::offline(detail)
            } else {
                GitHubFetchError::http(0, detail)
            }
        })?;
        let status = response.status().as_u16();
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string)
        };
        let remaining = header("x-ratelimit-remaining");
        let retry_after = header("retry-after");
        let reset = header("x-ratelimit-reset");
        let body = response.text().await.map_err(|error| {
            GitHubFetchError::offline(redact(&error.to_string(), credentials.token.as_deref()))
        })?;
        // Parsers and their diagnostics also receive a redacted body.
        let body = redact(&body, credentials.token.as_deref());
        if !(200..300).contains(&status) {
            let mut error =
                classify_http_error(status, remaining.as_deref(), retry_after.as_deref(), &body);
            if let GitHubFetchError::RateLimited {
                detail,
                reset_at,
                context: error_context,
                revision,
            } = &mut error
            {
                *reset_at = recovery_time(
                    reset.as_deref(),
                    retry_after.as_deref(),
                    crate::time::now_ms(),
                );
                *error_context = Some(context);
                *revision = Some(credentials.revision);
                if let Some(reset) = reset {
                    detail.push_str(&format!(
                        "；x-ratelimit-reset={}",
                        redact(&reset, credentials.token.as_deref())
                    ));
                }
            }
            return Err(error);
        }
        Ok((body, credentials.source, context, credentials.revision))
    }

    pub async fn test_auth(&self) -> Result<GitHubAuthTestResult, GitHubFetchError> {
        #[derive(Deserialize)]
        struct Quota {
            limit: u64,
            remaining: u64,
            reset: u64,
        }
        #[derive(Deserialize)]
        struct Resources {
            core: Quota,
        }
        #[derive(Deserialize)]
        struct Response {
            resources: Resources,
        }
        let url = reqwest::Url::parse("https://api.github.com/rate_limit").unwrap();
        let (body, source, context, revision) = self.get(url).await?;
        let response: Response = serde_json::from_str(&body)
            .map_err(|_| GitHubFetchError::parse("无法解析 GitHub 额度响应"))?;
        let core = response.resources.core;
        let reset_at_epoch_ms = core
            .reset
            .checked_mul(1_000)
            .ok_or_else(|| GitHubFetchError::parse("GitHub 额度恢复时间无效"))?;
        Ok(GitHubAuthTestResult {
            source,
            context,
            revision,
            limit: core.limit,
            remaining: core.remaining,
            reset_at_epoch_ms,
        })
    }

    #[cfg(test)]
    fn with_test_base(mut self, base: &str) -> Self {
        self.base = reqwest::Url::parse(base).unwrap();
        self
    }
}

fn credential_context(credentials: &Credentials) -> String {
    use sha2::{Digest, Sha256};
    // Only a fingerprint is persisted for associating rate limits with the
    // credentials that received them. No secret crosses the IPC boundary.
    let input = credentials.token.as_deref().unwrap_or("anonymous");
    format!("{:x}", Sha256::digest(input.as_bytes()))
}

fn recovery_time(reset: Option<&str>, retry: Option<&str>, now: u64) -> Option<u64> {
    let reset = reset
        .and_then(|s| s.parse::<u64>().ok())
        .and_then(|s| s.checked_mul(1000));
    let retry = retry.and_then(|s| {
        s.parse::<u64>()
            .ok()
            .and_then(|s| s.checked_mul(1000))
            .map(|ms| now.saturating_add(ms))
            .or_else(|| {
                httpdate::parse_http_date(s)
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis().min(u64::MAX as u128) as u64)
            })
    });
    reset.into_iter().chain(retry).max()
}

fn redact(text: &str, token: Option<&str>) -> String {
    match token {
        Some(token) => text.replace(token, "[redacted]"),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use crate::paths::AppPaths;
    use crate::update::archive::{ArchiveClient, ReqwestArchiveClient};
    use crate::update::github::{GitHubTreeClient, ReqwestGitHubClient};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    struct Server {
        base: String,
        requests: mpsc::Receiver<String>,
        worker: std::thread::JoinHandle<()>,
    }

    impl Server {
        fn start(responses: Vec<(u16, String, String)>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let (tx, requests) = mpsc::channel();
            let worker = std::thread::spawn(move || {
                for (status, headers, body) in responses {
                    let deadline = Instant::now() + Duration::from_secs(10);
                    let mut socket = loop {
                        match listener.accept() {
                            Ok((socket, _)) => break socket,
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                assert!(
                                    Instant::now() < deadline,
                                    "HTTP test request never arrived"
                                );
                                std::thread::sleep(Duration::from_millis(5));
                            }
                            Err(error) => panic!("{error}"),
                        }
                    };
                    // Accepted sockets can inherit nonblocking mode on macOS.
                    socket.set_nonblocking(false).unwrap();
                    socket
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    let mut raw = Vec::new();
                    let mut buffer = [0; 1_024];
                    while !raw.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                        let length = socket.read(&mut buffer).unwrap();
                        assert!(length > 0);
                        raw.extend_from_slice(&buffer[..length]);
                    }
                    tx.send(String::from_utf8(raw).unwrap()).unwrap();
                    write!(socket, "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}", body.len()).unwrap();
                }
            });
            Self {
                base,
                requests,
                worker,
            }
        }

        fn request(&self) -> String {
            self.requests.recv_timeout(Duration::from_secs(5)).unwrap()
        }

        fn finish(self) {
            self.worker.join().unwrap();
        }
    }

    fn settings(tmp: &tempfile::TempDir, token: Option<&str>) -> Arc<SettingsService> {
        Arc::new(SettingsService::new(
            AppPaths::new(tmp.path().join("home"), tmp.path().join("app")),
            token.map(str::to_string),
        ))
    }

    fn response(status: u16, body: &str) -> (u16, String, String) {
        (status, String::new(), body.to_string())
    }

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
    const TREE: &str = r#"{"sha":"0123456789abcdef0123456789abcdef01234567","tree":[]}"#;
    const QUOTA: &str =
        r#"{"resources":{"core":{"limit":5000,"remaining":4999,"reset":1793586399}}}"#;

    #[tokio::test]
    async fn tree_commit_and_quota_requests_use_current_credentials_but_download_does_not() {
        let tmp = tempfile::tempdir().unwrap();
        let settings = settings(&tmp, Some("dummy-environment-token"));
        let server = Server::start(vec![
            response(200, TREE),
            response(200, &format!(r#"{{"sha":"{SHA}"}}"#)),
            response(200, QUOTA),
            response(200, TREE),
            response(200, "archive-bytes"),
        ]);
        let api = GitHubApiClient::new(Arc::clone(&settings)).with_test_base(&server.base);
        let trees = ReqwestGitHubClient::with_api(api.clone());
        let archives = ReqwestArchiveClient::with_api(api.clone());
        trees.fetch_tree("acme/skills", None).await.unwrap();
        let request = server.request().to_ascii_lowercase();
        assert!(request.starts_with("get /repos/acme/skills/git/trees/head?recursive=1 "));
        assert!(request.contains("authorization: bearer dummy-environment-token"));

        settings.save("dummy-saved-token").unwrap();
        assert_eq!(
            archives.latest_commit("acme/skills", None).await.unwrap(),
            SHA
        );
        let request = server.request().to_ascii_lowercase();
        assert!(request.starts_with("get /repos/acme/skills/commits/head "));
        assert!(request.contains("authorization: bearer dummy-saved-token"));
        let quota = api.test_auth().await.unwrap();
        assert_eq!(quota.source, TokenSource::Settings);
        assert_eq!(quota.remaining, 4_999);
        assert_eq!(quota.reset_at_epoch_ms, 1_793_586_399_000);
        assert!(server.request().contains("Bearer dummy-saved-token"));

        settings.clear().unwrap();
        trees.fetch_tree("acme/skills", None).await.unwrap();
        assert!(server.request().contains("Bearer dummy-environment-token"));

        settings.save("dummy-saved-token").unwrap();
        assert_eq!(
            archives
                .download(reqwest::Url::parse(&format!("{}/archive", server.base)).unwrap())
                .await
                .unwrap(),
            b"archive-bytes"
        );
        assert!(
            !server
                .request()
                .to_ascii_lowercase()
                .contains("authorization:")
        );
        assert!(!format!("{api:?} {archives:?}").contains("dummy-saved-token"));
        server.finish();
    }

    #[tokio::test]
    async fn absent_token_makes_anonymous_requests_and_reports_anonymous_quota() {
        let tmp = tempfile::tempdir().unwrap();
        let server = Server::start(vec![response(
            200,
            r#"{"resources":{"core":{"limit":60,"remaining":59,"reset":100}}}"#,
        )]);
        let api = GitHubApiClient::new(settings(&tmp, None)).with_test_base(&server.base);
        let quota = api.test_auth().await.unwrap();
        assert_eq!(quota.source, TokenSource::Anonymous);
        assert_eq!(quota.limit, 60);
        assert!(
            !server
                .request()
                .to_ascii_lowercase()
                .contains("authorization:")
        );
        server.finish();
    }

    #[tokio::test]
    async fn rejected_token_is_not_retried_with_environment_and_never_echoes_in_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let settings = settings(&tmp, Some("dummy-environment-token"));
        settings.save("dummy-rejected-token").unwrap();
        let server = Server::start(vec![response(401, "Bad credentials dummy-rejected-token")]);
        let api = GitHubApiClient::new(settings).with_test_base(&server.base);
        let error = api.test_auth().await.unwrap_err();
        assert_eq!(error.to_app_error().code, ErrorCode::NetworkAuthFailed);
        assert!(!format!("{error:?}").contains("dummy-rejected-token"));
        assert!(
            !serde_json::to_string(&error.to_app_error())
                .unwrap()
                .contains("dummy-rejected-token")
        );
        assert!(server.request().contains("Bearer dummy-rejected-token"));
        server.finish();
    }

    #[tokio::test]
    async fn rate_limit_error_retains_structured_recovery_and_request_credential_context() {
        let tmp = tempfile::tempdir().unwrap();
        let settings = settings(&tmp, Some("dummy-token"));
        let headers = "x-ratelimit-remaining: 0\r\nx-ratelimit-reset: 1793586399\r\n".to_string();
        let server = Server::start(vec![(403, headers, "API rate limit exceeded".into())]);
        let api = GitHubApiClient::new(settings).with_test_base(&server.base);
        let error = api.test_auth().await.unwrap_err();
        let GitHubFetchError::RateLimited {
            reset_at,
            context,
            revision,
            ..
        } = error
        else {
            panic!("expected rate limit")
        };
        assert_eq!(reset_at, Some(1_793_586_399_000));
        assert_eq!(context.as_deref(), Some(api.context().unwrap().as_str()));
        assert_eq!(revision, Some(api.revision()));
        server.request();
        server.finish();
    }

    #[tokio::test]
    async fn both_check_clients_preserve_rate_limit_reset_metadata() {
        let tmp = tempfile::tempdir().unwrap();
        let limited = || {
            (
                403,
                "x-ratelimit-remaining: 0\r\nx-ratelimit-reset: 1793586399\r\n".into(),
                "API rate limit exceeded".into(),
            )
        };
        let server = Server::start(vec![limited(), limited()]);
        let api = GitHubApiClient::new(settings(&tmp, Some("dummy-environment-token")))
            .with_test_base(&server.base);
        let trees = ReqwestGitHubClient::with_api(api.clone());
        let archives = ReqwestArchiveClient::with_api(api);
        for error in [
            trees.fetch_tree("acme/skills", None).await.unwrap_err(),
            archives
                .latest_commit("acme/skills", None)
                .await
                .unwrap_err(),
        ] {
            assert_eq!(error.to_app_error().code, ErrorCode::NetworkRateLimited);
            assert!(
                error
                    .to_app_error()
                    .detail
                    .unwrap()
                    .contains("x-ratelimit-reset=1793586399")
            );
            server.request();
        }
        server.finish();
    }

    #[tokio::test]
    async fn authenticated_requests_never_follow_redirects() {
        let tmp = tempfile::tempdir().unwrap();
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        target.set_nonblocking(true).unwrap();
        let headers = format!(
            "Location: http://{}/secret-target\r\n",
            target.local_addr().unwrap()
        );
        let server = Server::start(vec![(302, headers, String::new())]);
        let api = GitHubApiClient::new(settings(&tmp, Some("dummy-environment-token")))
            .with_test_base(&server.base);
        assert!(matches!(
            api.test_auth().await,
            Err(GitHubFetchError::Http { status: 302, .. })
        ));
        server.request();
        assert_eq!(
            target.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        server.finish();
    }

    #[tokio::test]
    async fn unrelated_origins_and_corrupt_settings_are_rejected_without_network_requests() {
        let tmp = tempfile::tempdir().unwrap();
        let settings = settings(&tmp, Some("dummy-environment-token"));
        let api = GitHubApiClient::new(settings);
        assert!(matches!(
            api.get_text(reqwest::Url::parse("https://codeload.github.com/acme/skills").unwrap())
                .await,
            Err(GitHubFetchError::Http { status: 400, .. })
        ));
        std::fs::create_dir_all(tmp.path().join("app")).unwrap();
        std::fs::write(tmp.path().join("app/settings.json"), "broken").unwrap();
        let api = GitHubApiClient::new(self::settings(&tmp, Some("dummy-environment-token")));
        assert_eq!(
            api.test_auth().await.unwrap_err().to_app_error().code,
            ErrorCode::SettingsReadFailed
        );
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::recovery_time;
    #[test]
    fn structured_recovery_uses_reset_or_retry_after_including_http_dates() {
        assert_eq!(
            recovery_time(Some("100"), Some("30"), 90_000),
            Some(120_000)
        );
        assert_eq!(
            recovery_time(Some("invalid"), Some("30"), 90_000),
            Some(120_000)
        );
        assert_eq!(
            recovery_time(None, Some("Thu, 01 Jan 1970 00:02:00 GMT"), 90_000),
            Some(120_000)
        );
        assert_eq!(recovery_time(None, None, 90_000), None);
    }
}
