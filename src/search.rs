// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use anyhow::Result;
use sqlx::SqlitePool;

use crate::models::{Book, Folder};

/// Cap on returned hits per kind. Search is not a full-library dump — these
/// caps prevent a `*`-only query from streaming the entire catalog back.
const FOLDER_LIMIT: i64 = 50;
const BOOK_LIMIT: i64 = 100;

/// Compile a user-supplied search query into a SQL LIKE pattern.
///
/// Two-step transform:
/// 1. Escape the SQL LIKE specials (`\`, `%`, `_`) so user-typed characters
///    of those types match literally.
/// 2. Translate the user's wildcard `*` into SQL's `%`.
///
/// Then wrap with `%...%` so even literal queries do substring matching.
///
/// Examples:
/// - `men`        → `%men%`              (substring)
/// - `star*wars`  → `%star%wars%`        (wildcard between literals)
/// - `100%`       → `%100\%%`            (literal % is not a wildcard)
/// - `*hood`      → `%%hood%`            (extra `%` is harmless; same as `%hood%`)
///
/// Pair with `ESCAPE '\'` in the SQL statement.
pub fn to_like_pattern(q: &str) -> String {
    let mut escaped = String::with_capacity(q.len() + 2);
    escaped.push('%');
    for c in q.chars() {
        match c {
            '\\' => escaped.push_str("\\\\"),
            '%' => escaped.push_str("\\%"),
            '_' => escaped.push_str("\\_"),
            '*' => escaped.push('%'),
            other => escaped.push(other),
        }
    }
    escaped.push('%');
    escaped
}

/// Run a search across folder names and book filenames. Returns
/// `(folders, books)`, each sorted by `sort_key` and capped.
///
/// An empty `q` returns `(vec![], vec![])` — a feed with zero results.
pub async fn run(pool: &SqlitePool, q: &str) -> Result<(Vec<Folder>, Vec<Book>)> {
    if q.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    let pattern = to_like_pattern(q);

    let folders: Vec<Folder> = sqlx::query_as(
        "SELECT * FROM folder WHERE name LIKE ? ESCAPE '\\' ORDER BY sort_key LIMIT ?",
    )
    .bind(&pattern)
    .bind(FOLDER_LIMIT)
    .fetch_all(pool)
    .await?;

    let books: Vec<Book> = sqlx::query_as(
        "SELECT * FROM book WHERE name LIKE ? ESCAPE '\\' ORDER BY sort_key LIMIT ?",
    )
    .bind(&pattern)
    .bind(BOOK_LIMIT)
    .fetch_all(pool)
    .await?;

    Ok((folders, books))
}
