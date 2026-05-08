// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

const MAX_LOGGED_CHARS: usize = 256;

/// Sanitize a user-controlled string for logging.
///
/// Replaces every control character (Unicode `Cc`) with `?` and truncates the
/// output at [`MAX_LOGGED_CHARS`]. This blocks CR/LF/ESC/NUL log injection
/// without losing readable Unicode.
pub fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .take(MAX_LOGGED_CHARS)
        .collect()
}
