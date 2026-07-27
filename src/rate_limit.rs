// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// Collapse an address to the key it is rate-limited under.
///
/// IPv4 is keyed exactly. IPv6 is keyed by its /64 prefix: end sites are
/// routinely delegated a /64 or larger, so keying on the full /128 would let a
/// single attacker draw from 2^64 source addresses — never tripping the
/// threshold, and growing the state map with every address they burn.
pub fn bucket(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => ip,
        IpAddr::V6(v6) => {
            let mut prefix = [0u8; 16];
            prefix[..8].copy_from_slice(&v6.octets()[..8]);
            IpAddr::V6(Ipv6Addr::from(prefix))
        }
    }
}

/// Outcome of a `check` call: whether the IP may proceed or how long it must
/// wait before retrying.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Blocked { retry_after: Duration },
}

/// Per-IP failure window + active block.
#[derive(Debug, Clone)]
struct IpState {
    /// Start of the currently-tracked window. When `now - window_start` exceeds
    /// `window`, the counter resets on the next failure.
    window_start: Instant,
    /// Failures observed inside the current window.
    failures_in_window: u32,
    /// If `Some(t)` and `t > now`, the IP is currently blocked until `t`.
    blocked_until: Option<Instant>,
    /// Last touch — used for opportunistic cleanup.
    last_seen: Instant,
}

/// In-memory rate limiter for failed authentication attempts. Threshold and
/// timings are configured at construction; state lives in a single Mutex-guarded
/// HashMap keyed by peer IP.
pub struct Limiter {
    inner: Mutex<HashMap<IpAddr, IpState>>,
    max_attempts: u32,
    window: Duration,
    block_duration: Duration,
}

impl Limiter {
    pub fn new(max_attempts: u32, window: Duration, block_duration: Duration) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            max_attempts: max_attempts.max(1),
            window,
            block_duration,
        }
    }

    /// Check whether `ip` may proceed.
    pub fn check(&self, ip: IpAddr) -> Verdict {
        self.check_at(ip, Instant::now())
    }

    /// Note a failed authentication from `ip`. If this trips the threshold,
    /// the IP becomes blocked for `block_duration`.
    pub fn record_failure(&self, ip: IpAddr) {
        self.record_failure_at(ip, Instant::now());
    }

    /// Note a successful authentication from `ip`. Clears any past failures
    /// and unblocks the IP.
    pub fn record_success(&self, ip: IpAddr) {
        let mut map = self.inner.lock();
        map.remove(&bucket(ip));
    }

    // ----- Test seams below: explicit-time variants. -----

    pub fn check_at(&self, ip: IpAddr, now: Instant) -> Verdict {
        let mut map = self.inner.lock();
        self.gc(&mut map, now);
        match map.get(&bucket(ip)) {
            Some(state) => match state.blocked_until {
                Some(t) if t > now => Verdict::Blocked {
                    retry_after: t.saturating_duration_since(now),
                },
                _ => Verdict::Allow,
            },
            None => Verdict::Allow,
        }
    }

    pub fn record_failure_at(&self, ip: IpAddr, now: Instant) {
        let mut map = self.inner.lock();
        let state = map.entry(bucket(ip)).or_insert_with(|| IpState {
            window_start: now,
            failures_in_window: 0,
            blocked_until: None,
            last_seen: now,
        });

        // If still inside an active block, just refresh last_seen and bail.
        if let Some(t) = state.blocked_until {
            if t > now {
                state.last_seen = now;
                return;
            }
            // Block expired — start fresh.
            state.blocked_until = None;
            state.window_start = now;
            state.failures_in_window = 0;
        }

        // Roll the window if the previous one is stale.
        if now.duration_since(state.window_start) > self.window {
            state.window_start = now;
            state.failures_in_window = 0;
        }

        state.failures_in_window = state.failures_in_window.saturating_add(1);
        state.last_seen = now;

        if state.failures_in_window >= self.max_attempts {
            state.blocked_until = Some(now + self.block_duration);
        }
    }

    /// Drop entries that haven't been touched in a while and aren't currently
    /// blocking. Combined with the /64 bucketing in [`bucket`], this bounds
    /// memory under sustained attack: one attacker owns one entry per /64 they
    /// control, not one per source address they can spoof into.
    fn gc(&self, map: &mut HashMap<IpAddr, IpState>, now: Instant) {
        let stale_after = self.window + self.block_duration;
        map.retain(|_, st| {
            if let Some(t) = st.blocked_until {
                if t > now {
                    return true;
                }
            }
            now.duration_since(st.last_seen) < stale_after
        });
    }
}
