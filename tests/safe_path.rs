// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::fs;

use comicstream::safe_path::under;
use tempfile::tempdir;

#[test]
fn file_inside_root_is_accepted() {
    let root = tempdir().unwrap();
    let root_canon = root.path().canonicalize().unwrap();
    let file = root_canon.join("book.cbz");
    fs::write(&file, b"PK\x05\x06").unwrap();
    let resolved = under(&root_canon, &file).unwrap();
    assert!(resolved.starts_with(&root_canon));
}

#[test]
fn file_outside_root_is_rejected() {
    let root = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let evil = outside.path().join("evil.cbz");
    fs::write(&evil, b"x").unwrap();
    let root_canon = root.path().canonicalize().unwrap();
    assert!(under(&root_canon, &evil).is_none());
}

#[test]
fn nonexistent_path_is_rejected() {
    let root = tempdir().unwrap();
    let root_canon = root.path().canonicalize().unwrap();
    let ghost = root_canon.join("does_not_exist.cbz");
    assert!(under(&root_canon, &ghost).is_none());
}

#[cfg(unix)]
#[test]
fn symlink_pointing_outside_root_is_rejected() {
    use std::os::unix::fs::symlink;

    let root = tempdir().unwrap();
    let root_canon = root.path().canonicalize().unwrap();
    let outside = tempdir().unwrap();
    let target = outside.path().join("secret.txt");
    fs::write(&target, b"top secret").unwrap();

    // Place a symlink inside `root` that points to a file outside `root`.
    let link = root_canon.join("cover.jpg");
    symlink(&target, &link).unwrap();

    // canonicalize() resolves the symlink to its real (outside-root) path,
    // so the containment check correctly rejects it.
    assert!(under(&root_canon, &link).is_none());
}

#[cfg(unix)]
#[test]
fn symlink_pointing_inside_root_is_accepted() {
    use std::os::unix::fs::symlink;

    let root = tempdir().unwrap();
    let root_canon = root.path().canonicalize().unwrap();
    let real = root_canon.join("real.cbz");
    fs::write(&real, b"PK\x05\x06").unwrap();

    let link = root_canon.join("alias.cbz");
    symlink(&real, &link).unwrap();

    let resolved = under(&root_canon, &link).unwrap();
    assert!(resolved.starts_with(&root_canon));
}
