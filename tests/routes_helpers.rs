// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use axum::http::header::HeaderName;
use axum::http::{header, HeaderMap, HeaderValue};

use comicstream::routes::{apply_authenticated_response_headers, display_header_value};

#[test]
fn authenticated_responses_get_private_caching_and_vary_on_authorization() {
    let mut headers = HeaderMap::new();
    apply_authenticated_response_headers(&mut headers);

    assert_eq!(
        headers.get(header::CACHE_CONTROL).unwrap(),
        "private, max-age=86400"
    );
    assert_eq!(headers.get(header::VARY).unwrap(), "Authorization");
    assert_eq!(
        headers.get(header::X_CONTENT_TYPE_OPTIONS).unwrap(),
        "nosniff"
    );
}

#[test]
fn helper_overwrites_any_pre_existing_cache_control() {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=3600"),
    );
    apply_authenticated_response_headers(&mut headers);
    assert_eq!(
        headers.get(header::CACHE_CONTROL).unwrap(),
        "private, max-age=86400"
    );
}

#[test]
fn authorization_header_redacted_in_logs() {
    let name = HeaderName::from_static("authorization");
    let value = HeaderValue::from_static("Basic dXNlcjpwYXNz");
    assert_eq!(display_header_value(&name, &value), "<redacted>");
}

#[test]
fn cookie_and_set_cookie_redacted_in_logs() {
    let cookie = HeaderName::from_static("cookie");
    let set_cookie = HeaderName::from_static("set-cookie");
    let val = HeaderValue::from_static("session=abc");

    assert_eq!(display_header_value(&cookie, &val), "<redacted>");
    assert_eq!(display_header_value(&set_cookie, &val), "<redacted>");
}

#[test]
fn proxy_authorization_redacted_in_logs() {
    let name = HeaderName::from_static("proxy-authorization");
    let value = HeaderValue::from_static("Bearer xyz");
    assert_eq!(display_header_value(&name, &value), "<redacted>");
}

#[test]
fn non_sensitive_headers_passed_through_in_logs() {
    let cases = [
        ("accept", "application/xml"),
        ("user-agent", "Panels/889"),
        ("accept-language", "en-GB"),
        ("content-type", "image/jpeg"),
    ];
    for (name, expected) in cases {
        let n = HeaderName::from_static(name);
        let v = HeaderValue::from_static(expected);
        assert_eq!(display_header_value(&n, &v), expected);
    }
}
