// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::cmp::Ordering;

use comicstream::natsort;

#[test]
fn numeric_segments_sort_numerically_not_lexically() {
    assert_eq!(natsort::cmp("Issue 2", "Issue 10"), Ordering::Less);
    assert_eq!(natsort::cmp("Issue 10", "Issue 2"), Ordering::Greater);
    assert_eq!(natsort::cmp("Issue 2", "Issue 2"), Ordering::Equal);
}

#[test]
fn comparison_is_case_insensitive() {
    assert_eq!(natsort::cmp("batman", "BATMAN"), Ordering::Equal);
    assert_eq!(natsort::cmp("Batman 1", "batman 1"), Ordering::Equal);
}

#[test]
fn handles_mixed_alpha_and_numeric_runs() {
    let mut titles = vec![
        "Vol. 10 - Last",
        "Vol. 1 - First",
        "Vol. 2 - Second",
        "Vol. 11 - Eleventh",
    ];
    titles.sort_by(|a, b| natsort::cmp(a, b));
    assert_eq!(
        titles,
        vec![
            "Vol. 1 - First",
            "Vol. 2 - Second",
            "Vol. 10 - Last",
            "Vol. 11 - Eleventh",
        ]
    );
}

#[test]
fn empty_strings_compare_to_nonempty_correctly() {
    assert_eq!(natsort::cmp("", ""), Ordering::Equal);
    assert_eq!(natsort::cmp("", "anything"), Ordering::Less);
    assert_eq!(natsort::cmp("anything", ""), Ordering::Greater);
}

#[test]
fn key_pads_numeric_runs_for_lexical_correctness() {
    let mut titles = vec!["Issue 10", "Issue 2", "Issue 1"];
    titles.sort_by_key(|a| natsort::key(a));
    assert_eq!(titles, vec!["Issue 1", "Issue 2", "Issue 10"]);
}

#[test]
fn key_is_lowercase_and_stable_under_case_changes() {
    assert_eq!(natsort::key("Batman"), natsort::key("BATMAN"));
    assert_eq!(natsort::key("Batman 1"), natsort::key("BATMAN 1"));
}

#[test]
fn cmp_does_not_overflow_on_huge_numeric_runs() {
    // Numbers wider than u128 should not panic; saturating_mul prevents overflow.
    let huge = "9".repeat(100);
    let other = "9".repeat(99);
    let _ = natsort::cmp(&huge, &other);
}
