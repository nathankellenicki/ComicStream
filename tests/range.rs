// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use comicstream::routes::{parse_range, RangeSpec};

const LEN: u64 = 1000;

fn partial(start: u64, end_inclusive: u64) -> RangeSpec {
    RangeSpec::Partial {
        start,
        end_inclusive,
    }
}

#[test]
fn absent_header_sends_whole_file() {
    assert_eq!(parse_range(None, LEN), RangeSpec::Full);
}

#[test]
fn closed_range_is_honoured() {
    assert_eq!(parse_range(Some("bytes=0-499"), LEN), partial(0, 499));
    assert_eq!(parse_range(Some("bytes=500-999"), LEN), partial(500, 999));
}

#[test]
fn open_ended_range_runs_to_last_byte() {
    assert_eq!(parse_range(Some("bytes=200-"), LEN), partial(200, 999));
}

#[test]
fn suffix_range_returns_trailing_bytes() {
    assert_eq!(parse_range(Some("bytes=-100"), LEN), partial(900, 999));
}

#[test]
fn suffix_longer_than_file_clamps_to_whole_file() {
    assert_eq!(parse_range(Some("bytes=-5000"), LEN), partial(0, 999));
}

#[test]
fn end_past_eof_is_clamped() {
    assert_eq!(parse_range(Some("bytes=900-99999"), LEN), partial(900, 999));
}

#[test]
fn start_past_eof_is_unsatisfiable() {
    assert_eq!(parse_range(Some("bytes=1000-"), LEN), RangeSpec::Unsatisfiable);
    assert_eq!(
        parse_range(Some("bytes=5000-6000"), LEN),
        RangeSpec::Unsatisfiable
    );
}

#[test]
fn reversed_range_is_unsatisfiable() {
    assert_eq!(
        parse_range(Some("bytes=500-100"), LEN),
        RangeSpec::Unsatisfiable
    );
}

#[test]
fn zero_length_suffix_is_unsatisfiable() {
    assert_eq!(parse_range(Some("bytes=-0"), LEN), RangeSpec::Unsatisfiable);
}

#[test]
fn any_range_against_empty_file_is_unsatisfiable() {
    assert_eq!(parse_range(Some("bytes=0-10"), 0), RangeSpec::Unsatisfiable);
    assert_eq!(parse_range(Some("bytes=-10"), 0), RangeSpec::Unsatisfiable);
}

#[test]
fn whole_file_range_is_partial_not_full() {
    // 0-999 of a 1000-byte file is a legitimate 206 with the entire body.
    assert_eq!(parse_range(Some("bytes=0-999"), LEN), partial(0, 999));
}

#[test]
fn unit_is_case_insensitive_and_whitespace_tolerant() {
    assert_eq!(parse_range(Some("BYTES=0-9"), LEN), partial(0, 9));
    assert_eq!(parse_range(Some("  bytes= 10 - 19 "), LEN), partial(10, 19));
}

#[test]
fn multi_range_falls_back_to_whole_file() {
    // Serving the full representation is a valid response to a range request,
    // and avoids having to build a multipart/byteranges body.
    assert_eq!(parse_range(Some("bytes=0-99,200-299"), LEN), RangeSpec::Full);
}

#[test]
fn unknown_units_and_malformed_values_are_ignored() {
    for raw in [
        "items=0-99",
        "bytes",
        "bytes=",
        "bytes=abc-def",
        "bytes=10-xyz",
        "nonsense",
    ] {
        assert_eq!(
            parse_range(Some(raw), LEN),
            RangeSpec::Full,
            "expected {:?} to be ignored",
            raw
        );
    }
}
