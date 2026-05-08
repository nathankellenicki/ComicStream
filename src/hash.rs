// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

const READ_CHUNK: usize = 64 * 1024;

/// Compute a content hash over the entire file using BLAKE3.
///
/// Output is 64 lowercase hex characters (256 bits). BLAKE3 is collision-
/// resistant, so the previous "drop a same-hash file to overwrite a DB row"
/// attack — feasible against the prior 64-bit head/tail xxh3 — is no longer
/// practical.
pub fn file_hash(path: &Path) -> std::io::Result<String> {
    let f = File::open(path)?;
    let mut r = BufReader::new(f);
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; READ_CHUNK];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}
