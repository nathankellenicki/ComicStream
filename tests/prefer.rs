// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use axum::http::{HeaderMap, HeaderValue};

use comicstream::routes::parse_thumbnail_pref;

const DEFAULT: u32 = 300;

fn headers(prefer: Option<&str>) -> HeaderMap {
    let mut h = HeaderMap::new();
    if let Some(p) = prefer {
        h.insert("prefer", HeaderValue::from_str(p).unwrap());
    }
    h
}

#[test]
fn no_prefer_header_returns_none() {
    assert_eq!(parse_thumbnail_pref(&headers(None), DEFAULT), None);
}

#[test]
fn variant_thumbnail_without_width_uses_default() {
    assert_eq!(
        parse_thumbnail_pref(&headers(Some("variant=thumbnail")), DEFAULT),
        Some(DEFAULT)
    );
}

#[test]
fn variant_thumbnail_with_width_overrides_default() {
    assert_eq!(
        parse_thumbnail_pref(&headers(Some("variant=thumbnail; width=600")), DEFAULT),
        Some(600)
    );
}

#[test]
fn whitespace_around_tokens_is_tolerated() {
    assert_eq!(
        parse_thumbnail_pref(
            &headers(Some("  variant = thumbnail ;   width = 250  ")),
            DEFAULT
        ),
        Some(250)
    );
}

#[test]
fn case_insensitive_token_and_value() {
    assert_eq!(
        parse_thumbnail_pref(&headers(Some("VARIANT=THUMBNAIL; WIDTH=400")), DEFAULT),
        Some(400)
    );
}

#[test]
fn quoted_values_are_unwrapped() {
    assert_eq!(
        parse_thumbnail_pref(
            &headers(Some("variant=\"thumbnail\"; width=\"180\"")),
            DEFAULT
        ),
        Some(180)
    );
}

#[test]
fn unrecognized_variants_fall_through_to_full_size() {
    assert_eq!(
        parse_thumbnail_pref(&headers(Some("variant=preview; width=300")), DEFAULT),
        None
    );
}

#[test]
fn unrelated_preferences_are_ignored() {
    assert_eq!(
        parse_thumbnail_pref(&headers(Some("respond-async, wait=10")), DEFAULT),
        None
    );
}

#[test]
fn comma_separated_with_thumbnail_still_parses() {
    assert_eq!(
        parse_thumbnail_pref(
            &headers(Some("respond-async, variant=thumbnail; width=320")),
            DEFAULT
        ),
        Some(320)
    );
}

#[test]
fn malformed_width_falls_back_to_default() {
    assert_eq!(
        parse_thumbnail_pref(&headers(Some("variant=thumbnail; width=huge")), DEFAULT),
        Some(DEFAULT)
    );
}

#[test]
fn negative_width_is_ignored() {
    // u32 parse fails on a leading minus; default is used.
    assert_eq!(
        parse_thumbnail_pref(&headers(Some("variant=thumbnail; width=-50")), DEFAULT),
        Some(DEFAULT)
    );
}
