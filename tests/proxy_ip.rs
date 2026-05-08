// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::Request;

use comicstream::peer_ip::{client_ip, ProxyConfig};

fn req(peer: IpAddr, xff: Option<&str>) -> Request<Body> {
    let mut b = Request::builder().uri("/");
    if let Some(v) = xff {
        b = b.header("x-forwarded-for", v);
    }
    let mut req = b.body(Body::empty()).unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::new(peer, 0)));
    req
}

fn ip(b: u8) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(127, 0, 0, b))
}

#[test]
fn no_trusted_proxies_returns_peer_ip() {
    let cfg = ProxyConfig::default();
    let r = req(ip(1), Some("9.9.9.9"));
    assert_eq!(client_ip(&r, &cfg), Some(ip(1)));
}

#[test]
fn untrusted_peer_returns_peer_ip_even_with_xff() {
    let cfg = ProxyConfig::new(vec!["10.0.0.0/8".parse().unwrap()]);
    let r = req(ip(1), Some("9.9.9.9"));
    assert_eq!(client_ip(&r, &cfg), Some(ip(1)));
}

#[test]
fn trusted_peer_with_single_xff_hop_uses_xff() {
    let cfg = ProxyConfig::new(vec!["127.0.0.0/8".parse().unwrap()]);
    let r = req(ip(1), Some("9.9.9.9"));
    assert_eq!(
        client_ip(&r, &cfg),
        Some(IpAddr::V4(Ipv4Addr::new(9, 9, 9, 9)))
    );
}

#[test]
fn trusted_peer_walks_chain_skipping_trusted_hops() {
    // The request walked through two trusted proxies before reaching us; the
    // real client is the leftmost untrusted entry, which here is 9.9.9.9.
    let cfg = ProxyConfig::new(vec!["127.0.0.0/8".parse().unwrap()]);
    let r = req(ip(1), Some("9.9.9.9, 127.0.0.50, 127.0.0.42"));
    assert_eq!(
        client_ip(&r, &cfg),
        Some(IpAddr::V4(Ipv4Addr::new(9, 9, 9, 9)))
    );
}

#[test]
fn trusted_peer_missing_xff_falls_back_to_peer() {
    let cfg = ProxyConfig::new(vec!["127.0.0.0/8".parse().unwrap()]);
    let r = req(ip(1), None);
    assert_eq!(client_ip(&r, &cfg), Some(ip(1)));
}

#[test]
fn malformed_xff_entries_are_skipped() {
    // "garbage" doesn't parse as an IP; the real client is the next entry.
    let cfg = ProxyConfig::new(vec!["127.0.0.0/8".parse().unwrap()]);
    let r = req(ip(1), Some("9.9.9.9, garbage"));
    assert_eq!(
        client_ip(&r, &cfg),
        Some(IpAddr::V4(Ipv4Addr::new(9, 9, 9, 9)))
    );
}

#[test]
fn all_trusted_chain_falls_back_to_peer() {
    let cfg = ProxyConfig::new(vec!["127.0.0.0/8".parse().unwrap()]);
    let r = req(ip(1), Some("127.0.0.50, 127.0.0.51"));
    assert_eq!(client_ip(&r, &cfg), Some(ip(1)));
}

#[test]
fn missing_connectinfo_returns_none() {
    let cfg = ProxyConfig::default();
    let r = Request::builder().uri("/").body(Body::empty()).unwrap();
    assert_eq!(client_ip(&r, &cfg), None);
}
