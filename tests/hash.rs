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
    assert_eq!(h.len(), 64);
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
fn size_change_changes_hash() {
    let dir = tempdir().unwrap();
    let a = dir.path().join("a.bin");
    let b = dir.path().join("b.bin");
    fs::write(&a, b"X".repeat(100)).unwrap();
    fs::write(&b, b"X".repeat(200)).unwrap();
    assert_ne!(file_hash(&a).unwrap(), file_hash(&b).unwrap());
}

#[test]
fn middle_byte_change_changes_hash() {
    // BLAKE3 hashes the whole file, so a change anywhere — including bytes
    // far from the start and end — produces a different hash. (The previous
    // xxh3 implementation only sampled head + tail and would miss this.)
    let dir = tempdir().unwrap();
    let a = dir.path().join("a.bin");
    let b = dir.path().join("b.bin");

    let size = 200 * 1024;
    let mut file_a = vec![0u8; size];
    let mut file_b = vec![0u8; size];
    file_a[size / 2] = 0xAA;
    file_b[size / 2] = 0xBB;
    fs::write(&a, &file_a).unwrap();
    fs::write(&b, &file_b).unwrap();
    assert_ne!(file_hash(&a).unwrap(), file_hash(&b).unwrap());
}
