// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use comicstream::slug;

#[test]
fn slug_is_16_lowercase_hex_chars() {
    let s = slug::for_path("/Volumes/ssdmedia/Comics/Star Wars");
    assert_eq!(s.len(), 16);
    assert!(s
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()));
}

#[test]
fn slug_is_deterministic_for_same_path() {
    let a = slug::for_path("/library/Star Wars");
    let b = slug::for_path("/library/Star Wars");
    assert_eq!(a, b);
}

#[test]
fn different_paths_produce_different_slugs() {
    let a = slug::for_path("/library/Star Wars");
    let b = slug::for_path("/library/Star Trek");
    assert_ne!(a, b);
}

#[test]
fn whitespace_and_case_are_significant() {
    assert_ne!(slug::for_path("Star Wars"), slug::for_path("StarWars"));
    assert_ne!(slug::for_path("Star Wars"), slug::for_path("star wars"));
}

#[test]
fn empty_path_produces_a_slug() {
    let s = slug::for_path("");
    assert_eq!(s.len(), 16);
}
