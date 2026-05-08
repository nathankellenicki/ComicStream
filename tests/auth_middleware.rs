// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use axum::Router;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use tower::ServiceExt;

use comicstream::auth::{require_basic, Credentials};
use comicstream::rate_limit::Limiter;

fn app(creds: Credentials) -> Router {
    let creds = Arc::new(creds);
    // Generous limits so the auth-middleware tests aren't accidentally
    // throttled by the rate limiter; rate-limit-specific tests live in
    // tests/rate_limit.rs.
    let limiter = Arc::new(Limiter::new(
        1_000_000,
        Duration::from_secs(60),
        Duration::from_secs(60),
    ));
    Router::new()
        .route("/", get(|| async { "ok" }))
        .layer(axum::middleware::from_fn(move |req, next| {
            let creds = creds.clone();
            let limiter = limiter.clone();
            require_basic(creds, limiter, req, next)
        }))
}

fn auth_header(user: &str, pass: &str) -> String {
    format!("Basic {}", B64.encode(format!("{}:{}", user, pass)))
}

#[tokio::test]
async fn missing_credentials_return_401_with_full_challenge_headers() {
    let resp = app(Credentials {
        username: "alice".into(),
        password: "secret".into(),
    })
    .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
    .await
    .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        resp.headers().get("www-authenticate").unwrap(),
        "Basic realm=\"ComicStream\""
    );
    assert_eq!(resp.headers().get("cache-control").unwrap(), "no-store");
    assert_eq!(resp.headers().get("vary").unwrap(), "Authorization");
    assert_eq!(
        resp.headers().get("x-content-type-options").unwrap(),
        "nosniff"
    );
}

#[tokio::test]
async fn wrong_credentials_return_401() {
    let resp = app(Credentials {
        username: "alice".into(),
        password: "secret".into(),
    })
    .oneshot(
        Request::builder()
            .uri("/")
            .header("authorization", auth_header("alice", "wrong"))
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn valid_credentials_pass_through_to_handler() {
    let resp = app(Credentials {
        username: "alice".into(),
        password: "secret".into(),
    })
    .oneshot(
        Request::builder()
            .uri("/")
            .header("authorization", auth_header("alice", "secret"))
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn different_length_credentials_still_rejected() {
    // Covers the wrong-length branch of the constant-time compare without
    // poking at the private `ct_eq` helper.
    let resp = app(Credentials {
        username: "alice".into(),
        password: "secret".into(),
    })
    .oneshot(
        Request::builder()
            .uri("/")
            .header("authorization", auth_header("aliceeeee", "secret"))
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
