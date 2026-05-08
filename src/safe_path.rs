// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::path::{Path, PathBuf};

/// Resolve `candidate` and confirm it lives under `root`.
///
/// `root` is expected to already be canonical. `candidate` is canonicalized at
/// call time so symlinks have to point at something inside `root` to be
/// accepted.
///
/// Returns the canonicalized path on success, or `None` if `candidate` cannot
/// be resolved or escapes the root.
pub fn under(root: &Path, candidate: &Path) -> Option<PathBuf> {
    let resolved = std::fs::canonicalize(candidate).ok()?;
    if resolved.starts_with(root) {
        Some(resolved)
    } else {
        None
    }
}
