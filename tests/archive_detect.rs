// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::fs;

use comicstream::archive::{detect, Kind};
use tempfile::tempdir;

#[test]
fn detects_zip_by_local_file_header_magic() {
    let dir = tempdir().unwrap();
    let p = dir.path().join("looks.cbr");
    fs::write(&p, b"PK\x03\x04rest of an apparent zip").unwrap();
    assert_eq!(detect(&p).unwrap(), Kind::Zip);
}

#[test]
fn detects_zip_by_empty_archive_magic() {
    let dir = tempdir().unwrap();
    let p = dir.path().join("empty.zip");
    fs::write(&p, b"PK\x05\x06").unwrap();
    assert_eq!(detect(&p).unwrap(), Kind::Zip);
}

#[test]
fn detects_rar4_magic() {
    let dir = tempdir().unwrap();
    let p = dir.path().join("classic.cbr");
    fs::write(&p, b"Rar!\x1a\x07\x00rest").unwrap();
    assert_eq!(detect(&p).unwrap(), Kind::Rar);
}

#[test]
fn detects_rar5_magic() {
    let dir = tempdir().unwrap();
    let p = dir.path().join("modern.cbr");
    fs::write(&p, b"Rar!\x1a\x07\x01\x00rest").unwrap();
    assert_eq!(detect(&p).unwrap(), Kind::Rar);
}

#[test]
fn unknown_magic_returns_error() {
    let dir = tempdir().unwrap();
    let p = dir.path().join("garbage.cbz");
    fs::write(&p, b"this is not an archive").unwrap();
    assert!(detect(&p).is_err());
}

#[test]
fn extension_is_ignored_when_choosing_kind() {
    // A file named .cbr but containing zip magic is detected as Zip.
    let dir = tempdir().unwrap();
    let p = dir.path().join("misnamed.cbr");
    fs::write(&p, b"PK\x03\x04...").unwrap();
    assert_eq!(detect(&p).unwrap(), Kind::Zip);
}
