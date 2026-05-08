// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;

use comicstream::auth::{verify, Credentials};

fn creds(user: &str, pass: &str) -> Credentials {
    Credentials::new(user, pass)
}

fn header(user: &str, pass: &str) -> String {
    format!("Basic {}", B64.encode(format!("{}:{}", user, pass)))
}

#[test]
fn missing_header_rejected() {
    assert!(!verify(&creds("alice", "secret"), None));
}

#[test]
fn empty_header_rejected() {
    assert!(!verify(&creds("alice", "secret"), Some("")));
}

#[test]
fn correct_credentials_accepted() {
    let h = header("alice", "secret");
    assert!(verify(&creds("alice", "secret"), Some(&h)));
}

#[test]
fn wrong_username_rejected() {
    let h = header("bob", "secret");
    assert!(!verify(&creds("alice", "secret"), Some(&h)));
}

#[test]
fn wrong_password_rejected() {
    let h = header("alice", "wrong");
    assert!(!verify(&creds("alice", "secret"), Some(&h)));
}

#[test]
fn scheme_is_case_insensitive() {
    let h = header("alice", "secret").replace("Basic", "basic");
    assert!(verify(&creds("alice", "secret"), Some(&h)));
}

#[test]
fn other_scheme_rejected() {
    let token = B64.encode("alice:secret");
    let h = format!("Bearer {}", token);
    assert!(!verify(&creds("alice", "secret"), Some(&h)));
}

#[test]
fn malformed_base64_rejected() {
    assert!(!verify(
        &creds("alice", "secret"),
        Some("Basic !!!not-base64!!!")
    ));
}

#[test]
fn missing_colon_in_payload_rejected() {
    let h = format!("Basic {}", B64.encode("aliceonly"));
    assert!(!verify(&creds("alice", "secret"), Some(&h)));
}

#[test]
fn password_with_colon_is_preserved() {
    // Per RFC 7617, only the FIRST colon separates user from password; further
    // colons are part of the password.
    let h = format!("Basic {}", B64.encode("alice:s:e:c:r:e:t"));
    assert!(verify(&creds("alice", "s:e:c:r:e:t"), Some(&h)));
}

#[test]
fn empty_password_handled_consistently() {
    let h = format!("Basic {}", B64.encode("alice:"));
    assert!(verify(&creds("alice", ""), Some(&h)));
    assert!(!verify(&creds("alice", "secret"), Some(&h)));
}

#[test]
fn whitespace_around_payload_tolerated() {
    let h = format!("Basic   {}   ", B64.encode("alice:secret"));
    assert!(verify(&creds("alice", "secret"), Some(&h)));
}

#[test]
fn non_utf8_payload_rejected() {
    // Bytes that aren't valid UTF-8 in the decoded payload.
    let h = format!("Basic {}", B64.encode([0xff, 0xfe, 0xfd]));
    assert!(!verify(&creds("alice", "secret"), Some(&h)));
}
