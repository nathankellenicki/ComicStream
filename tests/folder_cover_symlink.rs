// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::fs;

use comicstream::scan::find_cover;
use tempfile::tempdir;

#[test]
fn real_cover_jpg_is_picked_up() {
    let dir = tempdir().unwrap();
    let cover = dir.path().join("cover.jpg");
    fs::write(&cover, b"\xff\xd8\xff").unwrap();
    let found = find_cover(dir.path()).unwrap();
    assert_eq!(found, cover.to_string_lossy());
}

#[cfg(unix)]
#[test]
fn symlinked_cover_jpg_is_ignored() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let target = outside.path().join("secret.txt");
    fs::write(&target, b"top secret").unwrap();

    // A `cover.jpg` symlink inside the folder pointing outside the library.
    // Pre-fix, find_cover would have happily recorded this and folder_cover
    // would have served the secret file.
    let link = dir.path().join("cover.jpg");
    symlink(&target, &link).unwrap();

    assert!(find_cover(dir.path()).is_none());
}

#[cfg(unix)]
#[test]
fn symlink_skipped_does_not_block_real_fallback_name() {
    use std::os::unix::fs::symlink;

    // cover.jpg is a (rejected) symlink, but folder.jpg is a real file —
    // find_cover should fall through to it instead of giving up at the first
    // candidate.
    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let target = outside.path().join("evil");
    fs::write(&target, b"x").unwrap();
    symlink(&target, dir.path().join("cover.jpg")).unwrap();

    let real = dir.path().join("folder.jpg");
    fs::write(&real, b"\xff\xd8\xff").unwrap();

    let found = find_cover(dir.path()).unwrap();
    assert_eq!(found, real.to_string_lossy());
}
