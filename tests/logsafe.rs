// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use comicstream::logsafe::sanitize;

#[test]
fn ascii_printable_passes_through() {
    let s = "alice@example.com";
    assert_eq!(sanitize(s), s);
}

#[test]
fn control_chars_become_question_marks() {
    let s = "alice\nWARN: forged\rok";
    assert_eq!(sanitize(s), "alice?WARN: forged?ok");
}

#[test]
fn ansi_escape_is_neutralized() {
    // ESC = 0x1b, the lead byte of ANSI escape sequences. Stripping it stops
    // a logged user-controlled value from hijacking a terminal.
    let s = "user\x1b[2J\x1b[H";
    assert_eq!(sanitize(s), "user?[2J?[H");
}

#[test]
fn nul_byte_becomes_question_mark() {
    let s = "a\0b";
    assert_eq!(sanitize(s), "a?b");
}

#[test]
fn non_ascii_unicode_passes_through() {
    let s = "café — 日本語";
    assert_eq!(sanitize(s), s);
}

#[test]
fn output_is_truncated_at_256_chars() {
    let s = "x".repeat(1000);
    let out = sanitize(&s);
    assert_eq!(out.chars().count(), 256);
}

#[test]
fn empty_input_returns_empty() {
    assert_eq!(sanitize(""), "");
}
