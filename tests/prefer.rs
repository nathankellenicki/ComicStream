// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use axum::http::{HeaderMap, HeaderValue};

use comicstream::routes::parse_thumbnail_pref;
use comicstream::thumb::{snap_width, MAX_PAGE_THUMB_WIDTH, PAGE_THUMB_WIDTHS};

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

// -----------------------------------------------------------------------------
// Width snapping — parsing stays faithful to the header, policy is applied after
// -----------------------------------------------------------------------------

#[test]
fn snapping_rounds_up_to_the_next_rung() {
    assert_eq!(snap_width(1), 150);
    assert_eq!(snap_width(151), 300);
    assert_eq!(snap_width(301), 600);
    assert_eq!(snap_width(1201), MAX_PAGE_THUMB_WIDTH);
}

#[test]
fn exact_rung_values_are_unchanged() {
    for w in PAGE_THUMB_WIDTHS {
        assert_eq!(snap_width(*w), *w);
    }
}

#[test]
fn oversized_requests_cap_at_the_maximum() {
    assert_eq!(snap_width(MAX_PAGE_THUMB_WIDTH + 1), MAX_PAGE_THUMB_WIDTH);
    assert_eq!(snap_width(u32::MAX), MAX_PAGE_THUMB_WIDTH);
}

#[test]
fn zero_width_snaps_to_smallest_rung() {
    // Also keeps a degenerate 0x0 resize away from the JPEG encoder.
    assert_eq!(snap_width(0), 150);
}

#[test]
fn arbitrary_widths_collapse_onto_a_bounded_set() {
    // The disk-fill vector: many distinct requested widths must not become many
    // distinct cache entries.
    let distinct: std::collections::BTreeSet<u32> = (0..=MAX_PAGE_THUMB_WIDTH)
        .map(snap_width)
        .collect();
    assert_eq!(distinct.len(), PAGE_THUMB_WIDTHS.len());
}

#[test]
fn snapping_never_returns_a_smaller_image_than_requested() {
    for requested in (0..=MAX_PAGE_THUMB_WIDTH).step_by(7) {
        assert!(snap_width(requested) >= requested);
    }
}
