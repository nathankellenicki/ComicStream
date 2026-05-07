// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::sync::Arc;

use axum::body::Body;
use axum::extract::Request;
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;

#[derive(Debug, Clone)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

/// Tower middleware that requires HTTP Basic auth matching `creds` on every
/// request. Apply only to routes that should be protected; `/health` should
/// stay outside the layer so Docker healthchecks don't have to authenticate.
pub async fn require_basic(creds: Arc<Credentials>, req: Request, next: Next) -> Response {
    let header_value = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    if verify(&creds, header_value) {
        next.run(req).await
    } else {
        unauthorized()
    }
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
