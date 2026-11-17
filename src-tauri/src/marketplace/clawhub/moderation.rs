//! Install-time ClawHub moderation gate.
//!
//! Browse and detail never read or display scan fields (spec R6-Q3). This
//! module reads them only when resolving an installation, and fails closed on
//! explicit suspicious or malware results; there is no force path.

use super::client;
use crate::{
    error::AppError,
    marketplace::{Coordinate, MarketplaceSource, validate_version},
};
use serde_json::Value;

pub async fn ensure_installable(coordinate: &Coordinate, version: &str) -> Result<(), AppError> {
    ensure_installable_at(coordinate, Some(version)).await
}

/// Archive installs also check the version-level security status. GitHub-backed
/// installs have no ClawHub-hosted version, so only skill-level moderation is
/// available.
pub async fn ensure_installable_at(
    coordinate: &Coordinate,
    version: Option<&str>,
) -> Result<(), AppError> {
    if coordinate.source != MarketplaceSource::Clawhub {
        return Err(AppError::clawhub_install_unsupported(
            "安装安全检查需要 ClawHub 来源坐标",
        ));
    }
    coordinate.validate()?;

    let detail_key = format!("detail|{}", coordinate.canonical().trim_start_matches('@'));
    let detail = client::get_at_optional(
        &format!("/api/v1/skills/{}", coordinate.slug),
        &detail_key,
        &[("ownerHandle", coordinate.namespace.clone())],
    )
    .await?
    .ok_or_else(|| AppError::clawhub_detail_missing("上游返回 HTTP 404"))?;
    let detail = detail
        .as_object()
        .ok_or_else(|| AppError::clawhub_unavailable("安装安全状态不是对象"))?;
    if detail
        .get("skill")
        .and_then(|skill| skill.get("slug"))
        .and_then(Value::as_str)
        != Some(coordinate.slug.as_str())
    {
        return Err(AppError::clawhub_unavailable(
            "安装安全状态与请求的 ClawHub 标识不一致",
        ));
    }
    let moderation = detail.get("moderation");
    let flagged = |key: &str| {
        moderation
            .and_then(|value| value.get(key))
            .and_then(Value::as_bool)
            == Some(true)
    };
    if flagged("isSuspicious") || flagged("isMalwareBlocked") {
        return Err(AppError::clawhub_install_blocked(
            "上游将技能标记为可疑或恶意",
        ));
    }

    let Some(version) = version else {
        return Ok(());
    };
    validate_version(version)?;
    let version_key = format!(
        "version|{}/{}|{version}",
        coordinate.namespace, coordinate.slug
    );
    let version_value = client::get_at_optional(
        &format!("/api/v1/skills/{}/versions/{version}", coordinate.slug),
        &version_key,
        &[("ownerHandle", coordinate.namespace.clone())],
    )
    .await?
    .ok_or_else(|| AppError::clawhub_version_missing(version, "上游返回 HTTP 404"))?;
    let version_object = version_value
        .get("version")
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::clawhub_unavailable("版本安全状态缺少 version"))?;
    if version_object.get("version").and_then(Value::as_str) != Some(version) {
        return Err(AppError::clawhub_unavailable(
            "版本安全状态与请求版本不一致",
        ));
    }
    match version_object
        .get("security")
        .and_then(|security| security.get("status"))
    {
        None | Some(Value::Null) => Ok(()),
        Some(Value::String(status)) => {
            let status = status.to_ascii_lowercase();
            if status == "suspicious" || status == "malware" {
                Err(AppError::clawhub_install_blocked(format!(
                    "版本安全状态为 {status}"
                )))
            } else {
                Ok(())
            }
        }
        Some(_) => Err(AppError::clawhub_unavailable("版本安全状态无效")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::marketplace::MarketplaceSource;
    use serde_json::{Value, json};
    use std::{
        collections::HashMap,
        io::{BufRead, BufReader, Write},
        net::TcpListener,
    };

    fn serve(routes: Vec<(&str, Value)>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let routes: HashMap<String, String> = routes
            .into_iter()
            .map(|(path, body)| (path.to_string(), body.to_string()))
            .collect();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let Ok(clone) = stream.try_clone() else {
                    continue;
                };
                let mut reader = BufReader::new(clone);
                let mut request = String::new();
                if reader.read_line(&mut request).is_err() {
                    continue;
                }
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).is_err()
                        || header == "\r\n"
                        || header.is_empty()
                    {
                        break;
                    }
                }
                let target = request.split_whitespace().nth(1).unwrap_or("/");
                let path = target.split('?').next().unwrap_or(target);
                let body = routes.get(path).cloned().unwrap_or_else(|| "{}".into());
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nCache-Control: public, max-age=0\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        address
    }

    fn coordinate(slug: &str) -> Coordinate {
        Coordinate::from_canonical_for(MarketplaceSource::Clawhub, &format!("@owner/{slug}"))
            .unwrap()
    }

    fn detail_path(slug: &str) -> String {
        format!("/api/v1/skills/{slug}")
    }
    fn version_path(slug: &str) -> String {
        format!("/api/v1/skills/{slug}/versions/1.0.0")
    }
    fn detail(slug: &str, moderation: Value) -> Value {
        json!({"skill": {"slug": slug}, "moderation": moderation})
    }
    fn version(slug: &str, security: Value) -> Value {
        json!({"skill": {"slug": slug}, "version": {"version": "1.0.0", "security": security}})
    }

    async fn check(slug: &str, detail: Value, version: Value) -> Result<(), AppError> {
        let address = serve(vec![
            (&detail_path(slug), detail),
            (&version_path(slug), version),
        ]);
        let _guard = client::use_test_base(&address);
        ensure_installable(&coordinate(slug), "1.0.0").await
    }

    #[tokio::test]
    async fn allows_clean_and_missing_scan_fields() {
        assert!(
            check(
                "clean",
                detail(
                    "clean",
                    json!({"isSuspicious": false, "isMalwareBlocked": false})
                ),
                version("clean", json!({"status": "clean"})),
            )
            .await
            .is_ok()
        );
        assert!(
            check(
                "missing",
                detail("missing", json!({})),
                version("missing", Value::Null)
            )
            .await
            .is_ok()
        );
        assert!(
            check(
                "missing-security",
                detail("missing-security", json!({})),
                json!({"skill": {"slug": "missing-security"}, "version": {"version": "1.0.0"}}),
            )
            .await
            .is_ok()
        );
    }

    #[tokio::test]
    async fn blocks_flagged_skills_and_versions_without_force() {
        for (slug, scan) in [
            ("suspicious", json!({"isSuspicious": true})),
            ("malware", json!({"isMalwareBlocked": true})),
            (
                "verdict",
                json!({"isSuspicious": true, "verdict": "suspicious"}),
            ),
        ] {
            let error = check(
                slug,
                detail(slug, scan),
                version(slug, json!({"status": "clean"})),
            )
            .await
            .unwrap_err();
            assert_eq!(error.code, crate::error::ErrorCode::MarketplaceUnavailable);
            assert!(!error.retryable, "{slug}");
        }
        for slug in ["version-suspicious", "version-malware"] {
            let status = if slug == "version-suspicious" {
                "suspicious"
            } else {
                "malware"
            };
            let error = check(
                slug,
                detail(slug, json!({"isSuspicious": false})),
                version(slug, json!({"status": status})),
            )
            .await
            .unwrap_err();
            assert!(!error.retryable, "{slug}");
        }
    }

    #[tokio::test]
    async fn rejects_identity_mismatches_and_unreadable_states() {
        for (slug, detail, version) in [
            (
                "other-owner",
                json!({"skill": {"slug": "other"}, "moderation": {}}),
                version("other-owner", json!({"status": "clean"})),
            ),
            (
                "bad-version",
                detail("bad-version", json!({})),
                json!({"skill": {"slug": "bad-version"}, "version": {"version": "2.0.0"}}),
            ),
            (
                "bad-security",
                detail("bad-security", json!({})),
                version("bad-security", json!({"status": 7})),
            ),
        ] {
            let error = check(slug, detail, version).await.unwrap_err();
            assert_eq!(error.code, crate::error::ErrorCode::MarketplaceUnavailable);
        }
    }
}
