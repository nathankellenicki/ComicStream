// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};
use sqlx::SqlitePool;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use walkdir::WalkDir;

use crate::archive;
use crate::hash::file_hash;
use crate::models::Book;
use crate::natsort;
use crate::thumb;

const COVER_NAMES: &[&str] = &[
    "cover.jpg",
    "cover.jpeg",
    "cover.png",
    "folder.jpg",
    "folder.png",
];
const IGNORED_DIRS: &[&str] = &["@eaDir", "__MACOSX", ".thumbnails", "__Panels"];

pub struct ScanOptions {
    pub library: PathBuf,
    pub library_name: Option<String>,
    pub data_dir: PathBuf,
    pub prewarm_thumbnails: bool,
    pub page_thumb_width: u32,
}

pub fn spawn_loop(pool: SqlitePool, opts: ScanOptions) -> mpsc::Sender<()> {
    let (tx, mut rx) = mpsc::channel::<()>(1);
    let initial = tx.clone();
    tokio::spawn(async move {
        let _ = initial.try_send(());
        drop(initial);
        while rx.recv().await.is_some() {
            while rx.try_recv().is_ok() {}
            if let Err(e) = scan(
                &pool,
                &opts.library,
                opts.library_name.as_deref(),
                &opts.data_dir,
                opts.prewarm_thumbnails,
                opts.page_thumb_width,
            )
            .await
            {
                error!("scan failed: {:#}", e);
            }
        }
    });
    tx
}

pub async fn scan(
    pool: &SqlitePool,
    library_root: &Path,
    library_name: Option<&str>,
    data_dir: &Path,
    prewarm_thumbnails: bool,
    page_thumb_width: u32,
) -> Result<()> {
    let started = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    info!(library = %library_root.display(), "scan starting");

    let canonical = library_root
        .canonicalize()
        .with_context(|| format!("canonicalizing {}", library_root.display()))?;

    let mut path_to_id: HashMap<PathBuf, i64> = HashMap::new();

    let root_id = upsert_folder(pool, &canonical, None, library_name, started).await?;
    path_to_id.insert(canonical.clone(), root_id);

    let mut walker = WalkDir::new(&canonical)
        .follow_links(false)
        .sort_by(|a, b| {
            natsort::cmp(
                &a.file_name().to_string_lossy(),
                &b.file_name().to_string_lossy(),
            )
        })
        .into_iter();

    let mut book_count = 0usize;
    let mut folder_count = 0usize;

    while let Some(entry) = walker.next() {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                warn!("walk error: {}", e);
                continue;
            }
        };

        let path = entry.path();
        if path == canonical {
            continue;
        }

        let file_name = entry.file_name().to_string_lossy();
        if entry.file_type().is_dir() && IGNORED_DIRS.iter().any(|n| file_name == *n) {
            walker.skip_current_dir();
            continue;
        }
        if file_name.starts_with('.') {
            if entry.file_type().is_dir() {
                walker.skip_current_dir();
            }
            continue;
        }

        if entry.file_type().is_dir() {
            let parent = path.parent().unwrap();
            let parent_id = match path_to_id.get(parent) {
                Some(id) => *id,
                None => {
                    warn!("orphan folder: {}", path.display());
                    continue;
                }
            };
            let id = upsert_folder(pool, path, Some(parent_id), None, started).await?;
            path_to_id.insert(path.to_path_buf(), id);
            folder_count += 1;
        } else if entry.file_type().is_file() {
            if !archive::is_supported(path) {
                continue;
            }
            let parent = path.parent().unwrap();
            let parent_id = match path_to_id.get(parent) {
                Some(id) => *id,
                None => {
                    warn!("orphan book: {}", path.display());
                    continue;
                }
            };
            if let Err(e) = upsert_book(pool, path, parent_id, started).await {
                warn!("book {} failed: {:#}", path.display(), e);
                continue;
            }
            book_count += 1;
        }
    }

    let removed_books: Vec<(String,)> = sqlx::query_as("SELECT path FROM book WHERE seen_at < ?")
        .bind(started)
        .fetch_all(pool)
        .await?;
    for (p,) in &removed_books {
        info!(path = %p, "book removed");
    }
    let pruned_books = sqlx::query("DELETE FROM book WHERE seen_at < ?")
        .bind(started)
        .execute(pool)
        .await?
        .rows_affected();

    let removed_folders: Vec<(String,)> =
        sqlx::query_as("SELECT path FROM folder WHERE seen_at < ?")
            .bind(started)
            .fetch_all(pool)
            .await?;
    for (p,) in &removed_folders {
        info!(path = %p, "folder removed");
    }
    let pruned_folders = sqlx::query("DELETE FROM folder WHERE seen_at < ?")
        .bind(started)
        .execute(pool)
        .await?
        .rows_affected();

    refresh_cover_versions(pool).await?;

    info!(
        books = book_count,
        folders = folder_count,
        pruned_books,
        pruned_folders,
        "scan complete"
    );

    if prewarm_thumbnails {
        prewarm(pool, data_dir, page_thumb_width).await?;
    }

    Ok(())
}

async fn prewarm(pool: &SqlitePool, data_dir: &Path, page_thumb_width: u32) -> Result<()> {
    let books: Vec<Book> = sqlx::query_as("SELECT * FROM book").fetch_all(pool).await?;
    let total_books = books.len();
    let mut covers_generated = 0usize;
    let mut covers_skipped = 0usize;
    let mut covers_failed = 0usize;
    let mut pages_generated = 0usize;
    let mut pages_skipped = 0usize;
    let mut pages_failed = 0usize;

    for b in books {
        let path = b.path.clone();
        let hash = b.hash.clone();
        let dd = data_dir.to_path_buf();
        let page_count = b.page_count as usize;

        let res =
            tokio::task::spawn_blocking(move || -> (usize, usize, usize, usize, usize, usize) {
                let mut cov_g = 0;
                let mut cov_s = 0;
                let mut cov_f = 0;
                let mut pg_g = 0;
                let mut pg_f = 0;

                let cover_cached = thumb::cache_path(&dd, &hash).exists();
                let pages_needed: Vec<usize> = (0..page_count)
                    .filter(|n| !thumb::page_thumb_path(&dd, &hash, *n, page_thumb_width).exists())
                    .collect();
                let pg_s = page_count - pages_needed.len();
                if cover_cached {
                    cov_s = 1;
                }

                if cover_cached && pages_needed.is_empty() {
                    return (cov_g, cov_s, cov_f, pg_g, pg_s, pg_f);
                }

                let arch = match archive::open(Path::new(&path)) {
                    Ok(a) => a,
                    Err(e) => {
                        warn!("prewarm: opening {} failed: {:#}", path, e);
                        if !cover_cached {
                            cov_f = 1;
                        }
                        pg_f = pages_needed.len();
                        return (cov_g, cov_s, cov_f, pg_g, pg_s, pg_f);
                    }
                };

                if !cover_cached {
                    match arch.read_page(0) {
                        Ok((bytes, _)) => match thumb::ensure_thumbnail(&dd, &hash, &bytes) {
                            Ok(_) => cov_g = 1,
                            Err(e) => {
                                warn!("prewarm: cover for {} failed: {:#}", path, e);
                                cov_f = 1;
                            }
                        },
                        Err(e) => {
                            warn!("prewarm: read cover page for {} failed: {:#}", path, e);
                            cov_f = 1;
                        }
                    }
                }

                for n in pages_needed {
                    match arch.read_page(n) {
                        Ok((bytes, _)) => {
                            match thumb::ensure_page_thumbnail(
                                &dd,
                                &hash,
                                n,
                                page_thumb_width,
                                &bytes,
                            ) {
                                Ok(_) => pg_g += 1,
                                Err(e) => {
                                    warn!("prewarm: page {} of {} failed: {:#}", n, path, e);
                                    pg_f += 1;
                                }
                            }
                        }
                        Err(e) => {
                            warn!("prewarm: read page {} of {} failed: {:#}", n, path, e);
                            pg_f += 1;
                        }
                    }
                }

                (cov_g, cov_s, cov_f, pg_g, pg_s, pg_f)
            })
            .await;

        match res {
            Ok((cov_g, cov_s, cov_f, pg_g, pg_s, pg_f)) => {
                covers_generated += cov_g;
                covers_skipped += cov_s;
                covers_failed += cov_f;
                pages_generated += pg_g;
                pages_skipped += pg_s;
                pages_failed += pg_f;
            }
            Err(e) => warn!("prewarm task panicked: {}", e),
        }
    }

    info!(
        books = total_books,
        covers_generated,
        covers_skipped,
        covers_failed,
        pages_generated,
        pages_skipped,
        pages_failed,
        "prewarm complete"
    );
    Ok(())
}

async fn upsert_folder(
    pool: &SqlitePool,
    path: &Path,
    parent_id: Option<i64>,
    name_override: Option<&str>,
    seen_at: i64,
) -> Result<i64> {
    let path_str = path.to_string_lossy().into_owned();
    let name = name_override.map(str::to_owned).unwrap_or_else(|| {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path_str.clone())
    });
    let sort_key = natsort::key(&name);
    let mtime = mtime_secs(path);
    let cover_path = find_cover(path);
    let slug = crate::slug::for_path(&path_str);

    let row: Option<(i64,)> = sqlx::query_as("SELECT id FROM folder WHERE path = ?")
        .bind(&path_str)
        .fetch_optional(pool)
        .await?;

    if let Some((id,)) = row {
        sqlx::query(
            "UPDATE folder SET parent_id=?, name=?, sort_key=?, cover_path=?, mtime=?, seen_at=?, slug=? WHERE id=?",
        )
        .bind(parent_id)
        .bind(&name)
        .bind(&sort_key)
        .bind(&cover_path)
        .bind(mtime)
        .bind(seen_at)
        .bind(&slug)
        .bind(id)
        .execute(pool)
        .await?;
        Ok(id)
    } else {
        let res = sqlx::query(
            "INSERT INTO folder (parent_id, path, name, sort_key, cover_path, mtime, seen_at, slug)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(parent_id)
        .bind(&path_str)
        .bind(&name)
        .bind(&sort_key)
        .bind(&cover_path)
        .bind(mtime)
        .bind(seen_at)
        .bind(&slug)
        .execute(pool)
        .await?;
        info!(path = %path_str, "folder added");
        Ok(res.last_insert_rowid())
    }
}

async fn upsert_book(pool: &SqlitePool, path: &Path, folder_id: i64, seen_at: i64) -> Result<()> {
    let path_str = path.to_string_lossy().into_owned();
    let meta = std::fs::metadata(path)?;
    let mtime = mtime_secs(path);
    let file_size = meta.len() as i64;

    let existing: Option<(i64, i64, i64, String)> =
        sqlx::query_as("SELECT id, mtime, file_size, hash FROM book WHERE path = ?")
            .bind(&path_str)
            .fetch_optional(pool)
            .await?;

    if let Some((id, old_mtime, old_size, _hash)) = &existing {
        if *old_mtime == mtime && *old_size == file_size {
            sqlx::query("UPDATE book SET seen_at=?, folder_id=? WHERE id=?")
                .bind(seen_at)
                .bind(folder_id)
                .bind(*id)
                .execute(pool)
                .await?;
            return Ok(());
        }
    }

    let hash = file_hash(path)?;
    let kind = archive::detect(path)?;
    let book = archive::open(path)?;
    let page_count = book.page_count() as i64;
    if page_count == 0 {
        debug!("skipping {}: no images", path.display());
        return Ok(());
    }

    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path_str.clone());
    let sort_key = natsort::key(&stem);
    let format = match kind {
        archive::Kind::Zip => "cbz",
        archive::Kind::Rar => "cbr",
    }
    .to_string();

    if let Some((id, _, _, old_hash)) = &existing {
        sqlx::query(
            "UPDATE book SET folder_id=?, hash=?, name=?, sort_key=?, format=?, file_size=?, mtime=?, page_count=?, seen_at=? WHERE id=?",
        )
        .bind(folder_id)
        .bind(&hash)
        .bind(&stem)
        .bind(&sort_key)
        .bind(&format)
        .bind(file_size)
        .bind(mtime)
        .bind(page_count)
        .bind(seen_at)
        .bind(*id)
        .execute(pool)
        .await?;
        if old_hash != &hash {
            info!(path = %path_str, "book content changed");
        }
    } else {
        // Detect rename: same hash, different path means the existing row is
        // about to have its path overwritten by the ON CONFLICT clause.
        let renamed_from: Option<(String,)> =
            sqlx::query_as("SELECT path FROM book WHERE hash = ?")
                .bind(&hash)
                .fetch_optional(pool)
                .await?;

        sqlx::query(
            "INSERT INTO book (folder_id, hash, path, name, sort_key, format, file_size, mtime, page_count, added_at, seen_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(hash) DO UPDATE SET
                folder_id=excluded.folder_id,
                path=excluded.path,
                name=excluded.name,
                sort_key=excluded.sort_key,
                format=excluded.format,
                file_size=excluded.file_size,
                mtime=excluded.mtime,
                page_count=excluded.page_count,
                seen_at=excluded.seen_at",
        )
        .bind(folder_id)
        .bind(&hash)
        .bind(&path_str)
        .bind(&stem)
        .bind(&sort_key)
        .bind(&format)
        .bind(file_size)
        .bind(mtime)
        .bind(page_count)
        .bind(seen_at)
        .bind(seen_at)
        .execute(pool)
        .await?;

        match renamed_from {
            Some((from,)) => info!(from = %from, to = %path_str, "book renamed"),
            None => info!(path = %path_str, "book added"),
        }
    }

    Ok(())
}

fn mtime_secs(p: &Path) -> i64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Compute a content-stable token per folder reflecting whatever `folder_cover`
/// would currently serve. Goes into `?v=` on the OPDS thumbnail URL so clients
/// can't keep showing a stale image when ID assignment shifts (e.g. after a
/// data-dir wipe).
async fn refresh_cover_versions(pool: &SqlitePool) -> Result<()> {
    let folders: Vec<(i64, Option<String>)> = sqlx::query_as("SELECT id, cover_path FROM folder")
        .fetch_all(pool)
        .await?;

    for (id, cover_path) in folders {
        let version = cover_version_for(pool, id, cover_path.as_deref()).await?;
        sqlx::query("UPDATE folder SET cover_version = ? WHERE id = ?")
            .bind(version)
            .bind(id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

async fn cover_version_for(
    pool: &SqlitePool,
    folder_id: i64,
    cover_path: Option<&str>,
) -> Result<Option<String>> {
    if let Some(p) = cover_path {
        if let Ok(meta) = std::fs::metadata(p) {
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            return Ok(Some(format!("p{}", mtime)));
        }
    }

    let direct: Option<(String,)> =
        sqlx::query_as("SELECT hash FROM book WHERE folder_id = ? ORDER BY sort_key LIMIT 1")
            .bind(folder_id)
            .fetch_optional(pool)
            .await?;
    if let Some((hash,)) = direct {
        return Ok(Some(hash));
    }

    let descendant: Option<(String,)> = sqlx::query_as(
        "WITH RECURSIVE sub(id) AS (
            SELECT id FROM folder WHERE id = ?
            UNION ALL
            SELECT f.id FROM folder f JOIN sub s ON f.parent_id = s.id
         )
         SELECT b.hash FROM book b JOIN sub s ON b.folder_id = s.id ORDER BY b.sort_key LIMIT 1",
    )
    .bind(folder_id)
    .fetch_optional(pool)
    .await?;
    if let Some((hash,)) = descendant {
        return Ok(Some(hash));
    }
    Ok(None)
}

fn find_cover(folder: &Path) -> Option<String> {
    for name in COVER_NAMES {
        let candidate = folder.join(name);
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    None
}
