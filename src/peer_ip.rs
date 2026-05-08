// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::net::{IpAddr, SocketAddr};

use axum::extract::ConnectInfo;
use axum::http::Request;
use ipnet::IpNet;

/// Trusted-reverse-proxy configuration for `client_ip`.
#[derive(Debug, Clone, Default)]
pub struct ProxyConfig {
    pub trusted: Vec<IpNet>,
}

impl ProxyConfig {
    pub fn new(trusted: Vec<IpNet>) -> Self {
        Self { trusted }
    }

    fn is_trusted(&self, ip: &IpAddr) -> bool {
        self.trusted.iter().any(|n| n.contains(ip))
    }
}

/// Determine the real client IP for `req`.
///
/// If the TCP peer is in `cfg.trusted`, walk `X-Forwarded-For` from the right,
/// skipping additional trusted hops, and return the first untrusted address.
/// Otherwise return the peer IP unchanged. Returns `None` only when the
/// `ConnectInfo` extension is missing — which shouldn't happen with axum's
/// `into_make_service_with_connect_info::<SocketAddr>`.
pub fn client_ip<B>(req: &Request<B>, cfg: &ProxyConfig) -> Option<IpAddr> {
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip())?;

    if cfg.trusted.is_empty() || !cfg.is_trusted(&peer) {
        return Some(peer);
    }

    if let Some(xff) = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
    {
        for hop in xff.rsplit(',').map(str::trim) {
            if let Ok(ip) = hop.parse::<IpAddr>() {
                if !cfg.is_trusted(&ip) {
                    return Some(ip);
                }
            }
        }
    }

    Some(peer)
}
