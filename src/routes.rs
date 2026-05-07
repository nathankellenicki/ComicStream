// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::path::Path;

use axum::body::Body;
use axum::extract::{Path as AxPath, Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{info, warn};

use std::sync::Arc;

use crate::archive;
use crate::auth;
use crate::models::{Book, Folder};
use crate::opds::{build_feed, FeedCtx};
use crate::state::AppState;
use crate::thumb;

pub fn router(state: AppState, creds: Option<Arc<auth::Credentials>>) -> Router {
    let public = Router::new().route("/health", get(health));

    let mut protected = Router::new()
        .route("/", get(opds_root))
        .route("/opds", get(opds_root))
        .route("/opds/", get(opds_root))
        .route("/opds/folders/:slug", get(opds_folder))
        .route("/folders/:slug/cover", get(folder_cover))
        .route("/books/:hash/pages/:n", get(book_page))
        .route("/books/:hash/cover", get(book_cover))
        .route("/books/:hash/thumbnail", get(book_thumbnail))
        .route("/books/:hash/file", get(book_file))
        .route("/admin/rescan", post(admin_rescan))
        .with_state(state);

    if let Some(creds) = creds {
        protected = protected.layer(axum::middleware::from_fn(move |req, next| {
            let creds = creds.clone();
            auth::require_basic(creds, req, next)
        }));
    }

    public.merge(protected)
}

async fn health() -> &'static str {
    "ok"
}

pub async fn log_request(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let headers: Vec<String> = req
        .headers()
        .iter()
        .map(|(k, v)| format!("{}={}", k, display_header_value(k, v)))
        .collect();
    info!(method = %method, uri = %uri, headers = %headers.join(" | "), "request");
    next.run(req).await
}

async fn admin_rescan(State(st): State<AppState>) -> (StatusCode, &'static str) {
    match st.scan_tx.try_send(()) {
        Ok(_) => (StatusCode::ACCEPTED, "rescan queued\n"),
        Err(mpsc::error::TrySendError::Full(_)) => {
            (StatusCode::ACCEPTED, "rescan already pending\n")
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            (StatusCode::SERVICE_UNAVAILABLE, "scanner unavailable\n")
        }
    }
}

async fn opds_root(State(st): State<AppState>) -> Result<Response, AppError> {
    let root: Folder = sqlx::query_as("SELECT * FROM folder WHERE parent_id IS NULL LIMIT 1")
        .fetch_one(&st.pool)
        .await?;
    serve_folder_feed(&st, &root, "/opds").await
}

async fn opds_folder(
    State(st): State<AppState>,
    AxPath(slug): AxPath<String>,
) -> Result<Response, AppError> {
    let folder: Folder = sqlx::query_as("SELECT * FROM folder WHERE slug = ?")
        .bind(&slug)
        .fetch_optional(&st.pool)
        .await?
        .ok_or(AppError::NotFound)?;
    let self_href = format!("/opds/folders/{}", slug);
    serve_folder_feed(&st, &folder, &self_href).await
}

async fn serve_folder_feed(
    st: &AppState,
    folder: &Folder,
    self_href: &str,
) -> Result<Response, AppError> {
    let subfolders: Vec<Folder> =
        sqlx::query_as("SELECT * FROM folder WHERE parent_id = ? ORDER BY sort_key")
            .bind(folder.id)
            .fetch_all(&st.pool)
            .await?;

    let books: Vec<Book> =
        sqlx::query_as("SELECT * FROM book WHERE folder_id = ? ORDER BY sort_key")
            .bind(folder.id)
            .fetch_all(&st.pool)
            .await?;

    let up_href = match folder.parent_id {
        Some(pid) => {
            let parent_slug: Option<(String,)> =
                sqlx::query_as("SELECT slug FROM folder WHERE id = ?")
                    .bind(pid)
                    .fetch_optional(&st.pool)
                    .await?;
            parent_slug.map(|(s,)| format!("/opds/folders/{}", s))
        }
        None => None,
    };

    let ctx = FeedCtx {
        self_href,
        up_href: up_href.as_deref(),
        feed_id: format!("urn:comicstream:folder:{}", folder.slug),
        title: folder.name.clone(),
        updated: DateTime::<Utc>::from_timestamp(folder.mtime, 0).unwrap_or_else(Utc::now),
        kind_acquisition: !books.is_empty(),
    };

    let xml = build_feed(&ctx, &subfolders, &books);

    let kind_type = if books.is_empty() {
        "application/atom+xml;profile=opds-catalog;kind=navigation"
    } else {
        "application/atom+xml;profile=opds-catalog;kind=acquisition"
    };

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(kind_type));
    apply_authenticated_response_headers(&mut headers);
    Ok((StatusCode::OK, headers, xml).into_response())
}

#[derive(Deserialize)]
struct PageParams {
    #[serde(default)]
    one_based: bool,
}

async fn book_page(
    State(st): State<AppState>,
    AxPath((hash, n)): AxPath<(String, i64)>,
    Query(params): Query<PageParams>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let book = lookup_book(&st, &hash).await?;
    let idx = if params.one_based { n - 1 } else { n };
    if idx < 0 || idx >= book.page_count {
        return Err(AppError::NotFound);
    }
    let idx = idx as usize;

    if let Some(width) = parse_thumbnail_pref(&headers, st.page_thumb_default_width) {
        let width = width.min(crate::thumb::MAX_PAGE_THUMB_WIDTH);
        let cached = crate::thumb::page_thumb_path(&st.data_dir, &hash, idx, width);
        if !cached.exists() {
            let arch = open_archive(&st, &hash, &book.path).await?;
            let data_dir = st.data_dir.clone();
            let h = hash.clone();
            tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                let (bytes, _) = arch.read_page(idx)?;
                crate::thumb::ensure_page_thumbnail(&data_dir, &h, idx, width, &bytes)?;
                Ok(())
            })
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?
            .map_err(|e| AppError::Internal(e.to_string()))?;
        }
        return serve_file(&cached, "image/jpeg").await;
    }

    let arch = open_archive(&st, &hash, &book.path).await?;
    let (bytes, mime) = tokio::task::spawn_blocking(move || arch.read_page(idx))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(image_response(mime, bytes))
}

/// Get an opened archive from the cache (or open it and cache it). The open
/// itself is wrapped in spawn_blocking so it doesn't stall the runtime when
/// the archive isn't already cached.
async fn open_archive(
    st: &AppState,
    hash: &str,
    path: &str,
) -> Result<std::sync::Arc<dyn archive::Book>, AppError> {
    let cache = st.archive_cache.clone();
    let hash = hash.to_string();
    let path = path.to_string();
    tokio::task::spawn_blocking(move || cache.get_or_open(&hash, std::path::Path::new(&path)))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .map_err(|e| AppError::Internal(e.to_string()))
}

/// Parse `Prefer: variant=thumbnail [; width=N]` per RFC 7240.
/// Returns Some(width) when `variant=thumbnail` is present; `width=N` overrides
/// the configured default if specified.
pub fn parse_thumbnail_pref(headers: &HeaderMap, default_width: u32) -> Option<u32> {
    let raw = headers
        .get(header::HeaderName::from_static("prefer"))?
        .to_str()
        .ok()?;
    let lower = raw.to_ascii_lowercase();
    let mut want_thumb = false;
    let mut width: Option<u32> = None;
    for token in lower.split([',', ';']) {
        let t = token.trim();
        let (k, v) = match t.split_once('=') {
            Some((k, v)) => (k.trim(), v.trim().trim_matches('"')),
            None => continue,
        };
        match k {
            "variant" if v == "thumbnail" => want_thumb = true,
            "width" => {
                if let Ok(w) = v.parse::<u32>() {
                    width = Some(w);
                }
            }
            _ => {}
        }
    }
    if want_thumb {
        Some(width.unwrap_or(default_width))
    } else {
        None
    }
}

async fn book_cover(
    State(st): State<AppState>,
    AxPath(hash): AxPath<String>,
) -> Result<Response, AppError> {
    let book = lookup_book(&st, &hash).await?;
    let arch = open_archive(&st, &hash, &book.path).await?;
    let (bytes, mime) = tokio::task::spawn_blocking(move || arch.read_page(0))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(image_response(mime, bytes))
}

async fn book_thumbnail(
    State(st): State<AppState>,
    AxPath(hash): AxPath<String>,
) -> Result<Response, AppError> {
    let book = lookup_book(&st, &hash).await?;
    let cached = thumb::cache_path(&st.data_dir, &book.hash);
    if !cached.exists() {
        let arch = open_archive(&st, &book.hash, &book.path).await?;
        let data_dir = st.data_dir.clone();
        let hash_owned = book.hash.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let (src, _) = arch.read_page(0)?;
            thumb::ensure_thumbnail(&data_dir, &hash_owned, &src)?;
            Ok(())
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .map_err(|e| AppError::Internal(e.to_string()))?;
    }
    serve_file(&cached, "image/jpeg").await
}

async fn book_file(
    State(st): State<AppState>,
    AxPath(hash): AxPath<String>,
) -> Result<Response, AppError> {
    let book = lookup_book(&st, &hash).await?;
    let mime = crate::opds::book_mime(&book.format);
    serve_file(Path::new(&book.path), mime).await
}

async fn folder_cover(
    State(st): State<AppState>,
    AxPath(slug): AxPath<String>,
) -> Result<Response, AppError> {
    let folder: Folder = sqlx::query_as("SELECT * FROM folder WHERE slug = ?")
        .bind(&slug)
        .fetch_optional(&st.pool)
        .await?
        .ok_or(AppError::NotFound)?;

    if let Some(cover) = folder.cover_path.as_ref() {
        let p = Path::new(cover);
        if p.is_file() {
            let mime = mime_guess::from_path(p).first_raw().unwrap_or("image/jpeg");
            return serve_file(p, mime).await;
        }
    }

    let book: Option<Book> =
        sqlx::query_as("SELECT * FROM book WHERE folder_id = ? ORDER BY sort_key LIMIT 1")
            .bind(folder.id)
            .fetch_optional(&st.pool)
            .await?;

    if let Some(b) = book {
        return serve_descendant_thumb(&st, &b).await;
    }

    let book_in_descendant: Option<Book> = sqlx::query_as(
        "WITH RECURSIVE sub(id) AS (
            SELECT id FROM folder WHERE id = ?
            UNION ALL
            SELECT f.id FROM folder f JOIN sub s ON f.parent_id = s.id
         )
         SELECT b.* FROM book b
         JOIN sub s ON b.folder_id = s.id
         ORDER BY b.sort_key LIMIT 1",
    )
    .bind(folder.id)
    .fetch_optional(&st.pool)
    .await?;

    if let Some(b) = book_in_descendant {
        return serve_descendant_thumb(&st, &b).await;
    }

    Err(AppError::NotFound)
}

async fn serve_descendant_thumb(st: &AppState, b: &Book) -> Result<Response, AppError> {
    let cached = thumb::cache_path(&st.data_dir, &b.hash);
    if !cached.exists() {
        let arch = open_archive(st, &b.hash, &b.path).await?;
        let data_dir = st.data_dir.clone();
        let hash_owned = b.hash.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let (src, _) = arch.read_page(0)?;
            thumb::ensure_thumbnail(&data_dir, &hash_owned, &src)?;
            Ok(())
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .map_err(|e| AppError::Internal(e.to_string()))?;
    }
    serve_file(&cached, "image/jpeg").await
}

async fn lookup_book(st: &AppState, hash: &str) -> Result<Book, AppError> {
    sqlx::query_as("SELECT * FROM book WHERE hash = ?")
        .bind(hash)
        .fetch_optional(&st.pool)
        .await?
        .ok_or(AppError::NotFound)
}

async fn serve_file(path: &Path, content_type: &str) -> Result<Response, AppError> {
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(content_type).unwrap(),
    );
    apply_authenticated_response_headers(&mut headers);
    Ok((StatusCode::OK, headers, Body::from(bytes)).into_response())
}

fn image_response(mime: &'static str, bytes: Vec<u8>) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
    apply_authenticated_response_headers(&mut headers);
    (StatusCode::OK, headers, Body::from(bytes)).into_response()
}

pub fn apply_authenticated_response_headers(headers: &mut HeaderMap) {
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=86400"),
    );
    headers.insert(header::VARY, HeaderValue::from_static("Authorization"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
}

pub fn display_header_value(name: &header::HeaderName, value: &HeaderValue) -> String {
    if is_sensitive_header(name) {
        "<redacted>".to_string()
    } else {
        value.to_str().unwrap_or("<binary>").to_string()
    }
}

fn is_sensitive_header(name: &header::HeaderName) -> bool {
    matches!(
        name.as_str(),
        "authorization" | "proxy-authorization" | "cookie" | "set-cookie"
    )
}

#[derive(Debug)]
pub enum AppError {
    NotFound,
    Internal(String),
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::RowNotFound => AppError::NotFound,
            other => AppError::Internal(other.to_string()),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::NotFound => (StatusCode::NOT_FOUND, "not found").into_response(),
            AppError::Internal(msg) => {
                warn!("internal error: {}", msg);
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
            }
        }
    }
}
