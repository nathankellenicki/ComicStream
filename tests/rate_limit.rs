// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use axum::Router;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use tower::ServiceExt;

use comicstream::auth::{require_basic, Credentials};
use comicstream::peer_ip::ProxyConfig;
use comicstream::rate_limit::{Limiter, Verdict};

fn ip(b: u8) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(127, 0, 0, b))
}

// -----------------------------------------------------------------------------
// Pure limiter logic (controlled clock)
// -----------------------------------------------------------------------------

#[test]
fn fresh_ip_is_allowed() {
    let lim = Limiter::new(3, Duration::from_secs(60), Duration::from_secs(300));
    assert_eq!(lim.check_at(ip(1), Instant::now()), Verdict::Allow);
}

#[test]
fn failures_below_threshold_do_not_block() {
    let lim = Limiter::new(3, Duration::from_secs(60), Duration::from_secs(300));
    let t0 = Instant::now();
    lim.record_failure_at(ip(1), t0);
    lim.record_failure_at(ip(1), t0 + Duration::from_secs(1));
    assert_eq!(
        lim.check_at(ip(1), t0 + Duration::from_secs(2)),
        Verdict::Allow
    );
}

#[test]
fn threshold_failures_block_with_retry_after() {
    let lim = Limiter::new(3, Duration::from_secs(60), Duration::from_secs(300));
    let t0 = Instant::now();
    for i in 0..3 {
        lim.record_failure_at(ip(1), t0 + Duration::from_secs(i));
    }
    match lim.check_at(ip(1), t0 + Duration::from_secs(4)) {
        Verdict::Blocked { retry_after } => {
            assert!(retry_after > Duration::from_secs(0));
            assert!(retry_after <= Duration::from_secs(300));
        }
        Verdict::Allow => panic!("expected blocked"),
    }
}

#[test]
fn block_lifts_after_block_duration() {
    let lim = Limiter::new(3, Duration::from_secs(60), Duration::from_secs(300));
    let t0 = Instant::now();
    for i in 0..3 {
        lim.record_failure_at(ip(1), t0 + Duration::from_secs(i));
    }
    let after_block = t0 + Duration::from_secs(3) + Duration::from_secs(301);
    assert_eq!(lim.check_at(ip(1), after_block), Verdict::Allow);
}

#[test]
fn failures_outside_window_do_not_accumulate() {
    let lim = Limiter::new(3, Duration::from_secs(60), Duration::from_secs(300));
    let t0 = Instant::now();
    lim.record_failure_at(ip(1), t0);
    lim.record_failure_at(ip(1), t0 + Duration::from_secs(120));
    lim.record_failure_at(ip(1), t0 + Duration::from_secs(180));
    // Only the second and third failures fall inside any single 60s window;
    // count stays below threshold.
    assert_eq!(
        lim.check_at(ip(1), t0 + Duration::from_secs(181)),
        Verdict::Allow
    );
}

#[test]
fn record_success_clears_state() {
    let lim = Limiter::new(3, Duration::from_secs(60), Duration::from_secs(300));
    let t0 = Instant::now();
    for i in 0..3 {
        lim.record_failure_at(ip(1), t0 + Duration::from_secs(i));
    }
    lim.record_success(ip(1));
    assert_eq!(
        lim.check_at(ip(1), t0 + Duration::from_secs(4)),
        Verdict::Allow
    );
}

#[test]
fn ips_are_tracked_independently() {
    let lim = Limiter::new(3, Duration::from_secs(60), Duration::from_secs(300));
    let t0 = Instant::now();
    for i in 0..3 {
        lim.record_failure_at(ip(1), t0 + Duration::from_secs(i));
    }
    assert!(matches!(
        lim.check_at(ip(1), t0 + Duration::from_secs(4)),
        Verdict::Blocked { .. }
    ));
    assert_eq!(
        lim.check_at(ip(2), t0 + Duration::from_secs(4)),
        Verdict::Allow
    );
}

// -----------------------------------------------------------------------------
// Integration: 429 surfaces through the auth middleware
// -----------------------------------------------------------------------------

fn app(limiter: Arc<Limiter>) -> Router {
    let creds = Arc::new(Credentials::new("alice", "secret"));
    let proxy = Arc::new(ProxyConfig::default());
    Router::new()
        .route("/", get(|| async { "ok" }))
        .layer(axum::middleware::from_fn(move |req, next| {
            let creds = creds.clone();
            let limiter = limiter.clone();
            let proxy = proxy.clone();
            require_basic(creds, limiter, proxy, req, next)
        }))
}

fn req_with_peer(uri: &str, auth: Option<&str>, peer: IpAddr) -> Request<Body> {
    let mut b = Request::builder().uri(uri);
    if let Some(a) = auth {
        b = b.header("authorization", a);
    }
    let mut req = b.body(Body::empty()).unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(std::net::SocketAddr::new(peer, 0)));
    req
}

fn bad_auth() -> String {
    format!("Basic {}", B64.encode("alice:wrong"))
}

#[tokio::test]
async fn middleware_returns_429_after_threshold() {
    let limiter = Arc::new(Limiter::new(
        3,
        Duration::from_secs(60),
        Duration::from_secs(300),
    ));
    let app_router = app(limiter);

    // First 3 wrong-cred attempts return 401.
    for _ in 0..3 {
        let resp = app_router
            .clone()
            .oneshot(req_with_peer("/", Some(&bad_auth()), ip(7)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // 4th attempt — even with wrong creds — gets 429.
    let resp = app_router
        .clone()
        .oneshot(req_with_peer("/", Some(&bad_auth()), ip(7)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(resp.headers().get("retry-after").is_some());
    assert_eq!(
        resp.headers().get("www-authenticate").unwrap(),
        "Basic realm=\"ComicStream\""
    );
}

#[tokio::test]
async fn correct_creds_after_block_still_429() {
    // Even a request with valid credentials gets 429 while the IP is blocked.
    // (Block applies before verification — that's the whole point.)
    let limiter = Arc::new(Limiter::new(
        2,
        Duration::from_secs(60),
        Duration::from_secs(300),
    ));
    let app_router = app(limiter);

    for _ in 0..2 {
        let _ = app_router
            .clone()
            .oneshot(req_with_peer("/", Some(&bad_auth()), ip(8)))
            .await
            .unwrap();
    }

    let good = format!("Basic {}", B64.encode("alice:secret"));
    let resp = app_router
        .clone()
        .oneshot(req_with_peer("/", Some(&good), ip(8)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn other_ips_unaffected_by_one_blocked_ip() {
    let limiter = Arc::new(Limiter::new(
        2,
        Duration::from_secs(60),
        Duration::from_secs(300),
    ));
    let app_router = app(limiter);

    // Block ip(9).
    for _ in 0..2 {
        let _ = app_router
            .clone()
            .oneshot(req_with_peer("/", Some(&bad_auth()), ip(9)))
            .await
            .unwrap();
    }

    // ip(10) with valid creds passes.
    let good = format!("Basic {}", B64.encode("alice:secret"));
    let resp = app_router
        .oneshot(req_with_peer("/", Some(&good), ip(10)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
