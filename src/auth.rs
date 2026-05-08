// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{ConnectInfo, Request};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use tracing::warn;

use crate::logsafe;
use crate::peer_ip::{client_ip, ProxyConfig};
use crate::rate_limit::{Limiter, Verdict};

/// 32-byte BLAKE3 keyed-hash digest of `username\0password`.
///
/// Storing only the digest avoids holding the plaintext password in process
/// memory and lets `verify` do a fixed-length constant-time compare regardless
/// of attacker input length — closing the credential-length timing channel
/// that the previous variable-length `ct_eq` had.
#[derive(Debug, Clone)]
pub struct Credentials {
    digest: [u8; 32],
}

/// Domain-separation key for the credential digest. A 32-byte fixed string
/// (BLAKE3 keyed-hash requires exactly 32 bytes); ties this digest to its
/// purpose so it can never be confused with another keyed-hash output.
const CRED_KEY: &[u8; 32] = b"comicstream-auth-v1\0\0\0\0\0\0\0\0\0\0\0\0\0";

fn cred_digest(user: &str, pass: &str) -> [u8; 32] {
    let mut input = Vec::with_capacity(user.len() + 1 + pass.len());
    input.extend_from_slice(user.as_bytes());
    input.push(0);
    input.extend_from_slice(pass.as_bytes());
    let h = blake3::keyed_hash(CRED_KEY, &input);
    *h.as_bytes()
}

impl Credentials {
    pub fn new(username: &str, password: &str) -> Self {
        Self {
            digest: cred_digest(username, password),
        }
    }
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
    proxy: Arc<ProxyConfig>,
    req: Request,
    next: Next,
) -> Response {
    let client_ip = client_ip(&req, &proxy);

    if let Some(ip) = client_ip {
        if let Verdict::Blocked { retry_after } = limiter.check(ip) {
            return rate_limited(retry_after);
        }
    }

    let header_value = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    if verify(&creds, header_value) {
        if let Some(ip) = client_ip {
            limiter.record_success(ip);
        }
        return next.run(req).await;
    }

    if let Some(ip) = client_ip {
        limiter.record_failure(ip);
    }

    let peer_label = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.to_string())
        .unwrap_or_else(|| "<unknown>".to_string());
    let path = req.uri().path().to_string();
    let attempted = attempted_username(header_value).map(|u| logsafe::sanitize(&u));
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
/// HTTP Basic and its keyed digest matches `creds`.
pub fn verify(creds: &Credentials, header_value: Option<&str>) -> bool {
    use subtle::ConstantTimeEq;

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
    let attempt = cred_digest(user, pass);
    attempt.ct_eq(&creds.digest).into()
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
