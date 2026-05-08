// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use sqlx::SqlitePool;
use tempfile::TempDir;

use comicstream::db;
use comicstream::search;

// -----------------------------------------------------------------------------
// Pattern compilation (no DB)
// -----------------------------------------------------------------------------

#[test]
fn plain_word_compiles_to_substring_pattern() {
    assert_eq!(search::to_like_pattern("men"), "%men%");
}

#[test]
fn star_becomes_sql_percent() {
    assert_eq!(search::to_like_pattern("star*wars"), "%star%wars%");
}

#[test]
fn leading_or_trailing_star_works() {
    assert_eq!(search::to_like_pattern("*hood"), "%%hood%");
    assert_eq!(search::to_like_pattern("hood*"), "%hood%%");
}

#[test]
fn user_typed_percent_is_escaped() {
    assert_eq!(search::to_like_pattern("100%"), "%100\\%%");
}

#[test]
fn user_typed_underscore_is_escaped() {
    assert_eq!(search::to_like_pattern("a_b"), "%a\\_b%");
}

#[test]
fn user_typed_backslash_is_escaped() {
    assert_eq!(search::to_like_pattern("path\\to"), "%path\\\\to%");
}

#[test]
fn empty_query_compiles_to_match_anything() {
    // run() short-circuits on empty before reaching here, but the helper itself
    // is total — wrapping `%...%` around nothing yields `%%`.
    assert_eq!(search::to_like_pattern(""), "%%");
}

// -----------------------------------------------------------------------------
// DB-backed search
// -----------------------------------------------------------------------------

async fn fresh_db() -> (TempDir, SqlitePool) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let pool = db::open(&path).await.unwrap();
    (dir, pool)
}

async fn insert_folder(pool: &SqlitePool, id: i64, name: &str, parent_id: Option<i64>) {
    sqlx::query(
        "INSERT INTO folder (id, parent_id, path, name, sort_key, mtime, seen_at, slug)
         VALUES (?, ?, ?, ?, ?, 0, 0, ?)",
    )
    .bind(id)
    .bind(parent_id)
    .bind(format!("/{}/{}", id, name))
    .bind(name)
    .bind(name.to_lowercase())
    .bind(format!("slug{:016x}", id as u64))
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_book(pool: &SqlitePool, id: i64, name: &str, folder_id: i64) {
    sqlx::query(
        "INSERT INTO book (id, folder_id, hash, path, name, sort_key, format, file_size, mtime, page_count, added_at, seen_at)
         VALUES (?, ?, ?, ?, ?, ?, 'cbz', 0, 0, 1, 0, 0)",
    )
    .bind(id)
    .bind(folder_id)
    .bind(format!("hash{:016x}", id as u64))
    .bind(format!("/{}/{}.cbz", folder_id, name))
    .bind(name)
    .bind(name.to_lowercase())
    .execute(pool)
    .await
    .unwrap();
}

async fn seed(pool: &SqlitePool) {
    insert_folder(pool, 1, "Library", None).await;
    insert_folder(pool, 2, "Star Wars", Some(1)).await;
    insert_folder(pool, 3, "Star Wars (2015)", Some(2)).await;
    insert_folder(pool, 4, "Star Trek - The Millennium Wars", Some(1)).await;
    insert_folder(pool, 5, "DMZ", Some(1)).await;
    insert_folder(pool, 6, "X-Men", Some(1)).await;

    insert_book(pool, 1, "Star Wars Vol. 1", 3).await;
    insert_book(pool, 2, "X-Men 1", 6).await;
    insert_book(pool, 3, "Detective Comics 001", 5).await;
    insert_book(pool, 4, "100% Free Comic", 5).await;
}

#[tokio::test]
async fn substring_match_finds_book_by_partial_name() {
    let (_dir, pool) = fresh_db().await;
    seed(&pool).await;

    let (folders, books) = search::run(&pool, "men").await.unwrap();
    let folder_names: Vec<&str> = folders.iter().map(|f| f.name.as_str()).collect();
    let book_names: Vec<&str> = books.iter().map(|b| b.name.as_str()).collect();
    assert!(folder_names.contains(&"X-Men"));
    assert!(book_names.contains(&"X-Men 1"));
}

#[tokio::test]
async fn match_is_case_insensitive() {
    let (_dir, pool) = fresh_db().await;
    seed(&pool).await;

    let (folders, _) = search::run(&pool, "STAR WARS").await.unwrap();
    assert!(folders.iter().any(|f| f.name == "Star Wars"));
}

#[tokio::test]
async fn star_wildcard_spans_arbitrary_content() {
    let (_dir, pool) = fresh_db().await;
    seed(&pool).await;

    let (folders, _) = search::run(&pool, "star*wars").await.unwrap();
    let names: Vec<&str> = folders.iter().map(|f| f.name.as_str()).collect();
    assert!(names.contains(&"Star Wars"));
    assert!(names.contains(&"Star Wars (2015)"));
    assert!(names.contains(&"Star Trek - The Millennium Wars"));
}

#[tokio::test]
async fn folder_at_any_depth_is_searchable() {
    let (_dir, pool) = fresh_db().await;
    seed(&pool).await;

    let (folders, _) = search::run(&pool, "2015").await.unwrap();
    assert!(folders.iter().any(|f| f.name == "Star Wars (2015)"));
}

#[tokio::test]
async fn empty_query_returns_no_results() {
    let (_dir, pool) = fresh_db().await;
    seed(&pool).await;

    let (folders, books) = search::run(&pool, "").await.unwrap();
    assert!(folders.is_empty());
    assert!(books.is_empty());
}

#[tokio::test]
async fn literal_percent_in_query_does_not_act_as_wildcard() {
    let (_dir, pool) = fresh_db().await;
    seed(&pool).await;

    // `100%` must match the book named "100% Free Comic" but should NOT be
    // treated as a free wildcard that matches every other book.
    let (_, books) = search::run(&pool, "100%").await.unwrap();
    let book_names: Vec<&str> = books.iter().map(|b| b.name.as_str()).collect();
    assert!(book_names.contains(&"100% Free Comic"));
    assert!(!book_names.contains(&"X-Men 1"));
    assert!(!book_names.contains(&"Detective Comics 001"));
}

#[tokio::test]
async fn no_matches_returns_empty_vectors() {
    let (_dir, pool) = fresh_db().await;
    seed(&pool).await;

    let (folders, books) = search::run(&pool, "nonexistent-zzz").await.unwrap();
    assert!(folders.is_empty());
    assert!(books.is_empty());
}

#[tokio::test]
async fn results_are_sorted_by_sort_key() {
    let (_dir, pool) = fresh_db().await;
    seed(&pool).await;

    let (folders, _) = search::run(&pool, "*").await.unwrap();
    let mut names: Vec<String> = folders.iter().map(|f| f.name.to_lowercase()).collect();
    let original = names.clone();
    names.sort();
    assert_eq!(original, names, "folders should arrive sorted by sort_key");
}
