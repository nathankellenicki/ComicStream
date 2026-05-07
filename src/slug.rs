// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use xxhash_rust::xxh3::xxh3_64;

/// Compute a stable, opaque URL slug for a folder path.
///
/// The slug is derived purely from the path string, so a given folder path
/// always maps to the same slug regardless of when or where the database was
/// built. That property is what lets cached folder URLs in clients (Panels'
/// thumbnail cache, intermediate HTTP caches) stay coherent across data dir
/// resets and server reinstalls.
pub fn for_path(path: &str) -> String {
    format!("{:016x}", xxh3_64(path.as_bytes()))
}
