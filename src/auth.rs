// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{ConnectInfo, Request};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use tracing::warn;

use crate::rate_limit::{Limiter, Verdict};

#[derive(Debug, Clone)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

/// Tower middleware that requires HTTP Basic auth matching `creds` on every
/// request. Apply only to routes that should be protected; `/health` should
/// stay outside the layer so Docker healthchecks don't have to authenticate.
///
/// `limiter` is consulted before the credential check: peers currently blocked
/// for excessive failures get a 429 without their attempt being verified at all.
pub async fn require_basic(
    creds: Arc<Credentials>,
    limiter: Arc<Limiter>,
    req: Request,
    next: Next,
) -> Response {
    let peer_ip = peer_ip(&req);

    if let Some(ip) = peer_ip {
        if let Verdict::Blocked { retry_after } = limiter.check(ip) {
            return rate_limited(retry_after);
        }
    }

    let header_value = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    if verify(&creds, header_value) {
        if let Some(ip) = peer_ip {
            limiter.record_success(ip);
        }
        return next.run(req).await;
    }

    if let Some(ip) = peer_ip {
        limiter.record_failure(ip);
    }

    let peer_label = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.to_string())
        .unwrap_or_else(|| "<unknown>".to_string());
    let path = req.uri().path().to_string();
    let attempted = attempted_username(header_value);
    let reason = if header_value.is_none() {
        "missing credentials"
    } else {
        "invalid credentials"
    };
    warn!(
        peer = %peer_label,
        path = %path,
        attempted_user = attempted.as_deref().unwrap_or("<none>"),
        "auth failure: {}",
        reason
    );

    unauthorized()
}

fn peer_ip(req: &Request) -> Option<IpAddr> {
    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip())
}

fn rate_limited(retry_after: std::time::Duration) -> Response {
    let secs = retry_after.as_secs().max(1);
    let mut resp = Response::new(Body::from(
        "too many failed authentication attempts; try again later\n",
    ));
    *resp.status_mut() = StatusCode::TOO_MANY_REQUESTS;
    if let Ok(v) = HeaderValue::from_str(&secs.to_string()) {
        resp.headers_mut().insert(header::RETRY_AFTER, v);
    }
    resp.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static("Basic realm=\"ComicStream\""),
    );
    resp.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    resp
}

/// Extract just the username portion of a `Basic` Authorization header, for
/// logging. Returns `None` for a missing or malformed header. Never returns
/// the password.
fn attempted_username(header_value: Option<&str>) -> Option<String> {
    let raw = header_value?;
    let payload = match raw.get(..6) {
        Some(prefix) if prefix.eq_ignore_ascii_case("Basic ") => raw[6..].trim(),
        _ => return None,
    };
    let decoded = B64.decode(payload).ok()?;
    let s = std::str::from_utf8(&decoded).ok()?;
    let (user, _) = s.split_once(':')?;
    Some(user.to_string())
}

/// Returns true iff the supplied `Authorization` header value is well-formed
/// HTTP Basic and matches `creds`.
pub fn verify(creds: &Credentials, header_value: Option<&str>) -> bool {
    let raw = match header_value {
        Some(v) => v,
        None => return false,
    };
    // RFC 7617 says the scheme is case-insensitive.
    let payload = match raw.get(..6) {
        Some(prefix) if prefix.eq_ignore_ascii_case("Basic ") => raw[6..].trim(),
        _ => return false,
    };
    let decoded = match B64.decode(payload) {
        Ok(d) => d,
        Err(_) => return false,
    };
    let s = match std::str::from_utf8(&decoded) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let (user, pass) = match s.split_once(':') {
        Some(p) => p,
        None => return false,
    };
    // Compute both comparisons before short-circuiting so a wrong-username
    // attempt takes the same time as a wrong-password attempt.
    let user_ok = ct_eq(user.as_bytes(), creds.username.as_bytes());
    let pass_ok = ct_eq(pass.as_bytes(), creds.password.as_bytes());
    user_ok & pass_ok
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff: usize = a.len() ^ b.len();
    let max_len = a.len().max(b.len());
    for i in 0..max_len {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= usize::from(x ^ y);
    }
    diff == 0
}

fn unauthorized() -> Response {
    let mut resp = Response::new(Body::from("authentication required\n"));
    *resp.status_mut() = StatusCode::UNAUTHORIZED;
    resp.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static("Basic realm=\"ComicStream\""),
    );
    resp.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp.headers_mut()
        .insert(header::VARY, HeaderValue::from_static("Authorization"));
    resp.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    resp
}
