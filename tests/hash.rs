// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::fs;

use comicstream::hash::file_hash;
use tempfile::tempdir;

#[test]
fn empty_file_hashes_deterministically() {
    let dir = tempdir().unwrap();
    let p = dir.path().join("empty.bin");
    fs::write(&p, b"").unwrap();
    let h = file_hash(&p).unwrap();
    assert_eq!(h.len(), 16);
    // Recompute and verify stability across calls.
    assert_eq!(file_hash(&p).unwrap(), h);
}

#[test]
fn different_content_produces_different_hash() {
    let dir = tempdir().unwrap();
    let a = dir.path().join("a.bin");
    let b = dir.path().join("b.bin");
    fs::write(&a, b"hello world").unwrap();
    fs::write(&b, b"goodbye world").unwrap();
    assert_ne!(file_hash(&a).unwrap(), file_hash(&b).unwrap());
}

#[test]
fn identical_content_in_different_paths_produces_same_hash() {
    let dir = tempdir().unwrap();
    let a = dir.path().join("a.cbz");
    let b = dir.path().join("nested").join("b.cbz");
    fs::create_dir_all(b.parent().unwrap()).unwrap();
    fs::write(&a, b"identical bytes here").unwrap();
    fs::write(&b, b"identical bytes here").unwrap();
    assert_eq!(file_hash(&a).unwrap(), file_hash(&b).unwrap());
}

#[test]
fn size_change_changes_hash_even_with_same_head_and_tail() {
    let dir = tempdir().unwrap();
    let a = dir.path().join("a.bin");
    let b = dir.path().join("b.bin");
    let head_tail = b"PADDING".repeat(10);
    let middle_short = b"X".repeat(100);
    let middle_long = b"X".repeat(200);
    let mut file_a = head_tail.clone();
    file_a.extend_from_slice(&middle_short);
    file_a.extend_from_slice(&head_tail);
    let mut file_b = head_tail.clone();
    file_b.extend_from_slice(&middle_long);
    file_b.extend_from_slice(&head_tail);
    fs::write(&a, &file_a).unwrap();
    fs::write(&b, &file_b).unwrap();
    assert_ne!(file_hash(&a).unwrap(), file_hash(&b).unwrap());
}

#[test]
fn large_file_with_distinct_head_or_tail_hashes_differently() {
    let dir = tempdir().unwrap();
    let a = dir.path().join("a.bin");
    let b = dir.path().join("b.bin");

    // Both 200KB, identical except in the first 64KB sample window.
    let mut file_a = vec![0u8; 200 * 1024];
    let mut file_b = vec![0u8; 200 * 1024];
    file_a[0] = 0xAA;
    file_b[0] = 0xBB;
    fs::write(&a, &file_a).unwrap();
    fs::write(&b, &file_b).unwrap();
    assert_ne!(file_hash(&a).unwrap(), file_hash(&b).unwrap());

    // Differ only in the last 64KB instead.
    file_a.copy_from_slice(&vec![0u8; 200 * 1024]);
    file_b.copy_from_slice(&vec![0u8; 200 * 1024]);
    file_a[200 * 1024 - 1] = 0xAA;
    file_b[200 * 1024 - 1] = 0xBB;
    fs::write(&a, &file_a).unwrap();
    fs::write(&b, &file_b).unwrap();
    assert_ne!(file_hash(&a).unwrap(), file_hash(&b).unwrap());
}
