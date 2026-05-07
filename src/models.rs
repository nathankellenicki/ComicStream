// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use serde::Serialize;
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Folder {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub path: String,
    pub name: String,
    pub sort_key: String,
    pub cover_path: Option<String>,
    pub mtime: i64,
    pub seen_at: i64,
    pub cover_version: Option<String>,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Book {
    pub id: i64,
    pub folder_id: i64,
    pub hash: String,
    pub path: String,
    pub name: String,
    pub sort_key: String,
    pub format: String,
    pub file_size: i64,
    pub mtime: i64,
    pub page_count: i64,
    pub added_at: i64,
    pub seen_at: i64,
}
