// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::path::PathBuf;
use std::sync::Arc;

use sqlx::SqlitePool;
use tokio::sync::mpsc;

use crate::archive;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub data_dir: Arc<PathBuf>,
    /// Canonical library root. Every path read from the DB at request time is
    /// validated against this prefix before being served.
    pub library_root: Arc<PathBuf>,
    pub scan_tx: mpsc::Sender<()>,
    pub page_thumb_default_width: u32,
    pub archive_cache: Arc<archive::Cache>,
}
