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
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::mpsc;
use tokio_util::io::ReaderStream;
use tracing::{info, warn};

use std::sync::Arc;

use crate::archive;
use crate::auth;
use crate::models::{Book, Folder};
use crate::opds::{build_feed, FeedCtx};
use crate::peer_ip::ProxyConfig;
use crate::rate_limit::Limiter;
use crate::state::AppState;
use crate::thumb;

pub struct AuthSetup {
    pub creds: Arc<auth::Credentials>,
    pub limiter: Arc<Limiter>,
    pub proxy: Arc<ProxyConfig>,
}

pub fn router(state: AppState, auth_setup: Option<AuthSetup>) -> Router {
    let public = Router::new().route("/health", get(health));

    let mut protected = Router::new()
        .route("/", get(opds_root))
        .route("/opds", get(opds_root))
        .route("/opds/", get(opds_root))
        .route("/opds/search", get(opds_search))
        .route("/opds/opensearch.xml", get(opensearch_descriptor))
        .route("/opds/folders/:slug", get(opds_folder))
        .route("/folders/:slug/cover", get(folder_cover))
        .route("/books/:hash/pages/:n", get(book_page))
        .route("/books/:hash/cover", get(book_cover))
        .route("/books/:hash/thumbnail", get(book_thumbnail))
        .route("/books/:hash/file", get(book_file))
        .route("/admin/rescan", post(admin_rescan))
        .with_state(state);

    if let Some(setup) = auth_setup {
        let creds = setup.creds;
        let limiter = setup.limiter;
        let proxy = setup.proxy;
        protected = protected.layer(axum::middleware::from_fn(move |req, next| {
            let creds = creds.clone();
            let limiter = limiter.clone();
            let proxy = proxy.clone();
            auth::require_basic(creds, limiter, proxy, req, next)
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
    apply_feed_response_headers(&mut headers);
    Ok((StatusCode::OK, headers, xml).into_response())
}

#[derive(Deserialize)]
struct SearchParams {
    #[serde(default)]
    q: String,
}

async fn opds_search(
    State(st): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Response, AppError> {
    let q = params.q.trim();
    let (folders, books) = crate::search::run(&st.pool, q)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let q_encoded: String =
        percent_encoding::utf8_percent_encode(q, percent_encoding::NON_ALPHANUMERIC).collect();
    let self_href = format!("/opds/search?q={}", q_encoded);
    let feed_id = format!(
        "urn:comicstream:search:{:016x}",
        xxhash_rust::xxh3::xxh3_64(q.as_bytes())
    );
    let title = if q.is_empty() {
        "Search".to_string()
    } else {
        format!("Search: {}", q)
    };

    let ctx = FeedCtx {
        self_href: &self_href,
        up_href: None,
        feed_id,
        title,
        updated: Utc::now(),
        kind_acquisition: true,
    };
    let xml = build_feed(&ctx, &folders, &books);

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/atom+xml;profile=opds-catalog;kind=acquisition"),
    );
    apply_feed_response_headers(&mut headers);
    Ok((StatusCode::OK, headers, xml).into_response())
}

async fn opensearch_descriptor() -> Result<Response, AppError> {
    let body = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<OpenSearchDescription xmlns=\"http://a9.com/-/spec/opensearch/1.1/\">\n\
  <ShortName>ComicStream</ShortName>\n\
  <Description>Search comic library by name</Description>\n\
  <InputEncoding>UTF-8</InputEncoding>\n\
  <Url type=\"application/atom+xml;profile=opds-catalog;kind=acquisition\" template=\"/opds/search?q={searchTerms}\"/>\n\
</OpenSearchDescription>\n";

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/opensearchdescription+xml"),
    );
    apply_feed_response_headers(&mut headers);
    Ok((StatusCode::OK, headers, body).into_response())
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
    let idx = if params.one_based {
        // `n` is client-supplied; i64::MIN would overflow a plain subtraction.
        n.checked_sub(1).ok_or(AppError::NotFound)?
    } else {
        n
    };
    if idx < 0 || idx >= book.page_count {
        return Err(AppError::NotFound);
    }
    let idx = idx as usize;

    if let Some(width) = parse_thumbnail_pref(&headers, st.page_thumb_default_width) {
        // Snap rather than clamp: the client picks the width, and one cache
        // entry per distinct value is a disk-fill vector.
        let width = crate::thumb::snap_width(width);
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
        let mut resp = serve_file(&cached, "image/jpeg").await?;
        vary_on_prefer(&mut resp);
        return Ok(resp);
    }

    let arch = open_archive(&st, &hash, &book.path).await?;
    let (bytes, mime) = tokio::task::spawn_blocking(move || arch.read_page(idx))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let mut resp = image_response(mime, bytes);
    vary_on_prefer(&mut resp);
    Ok(resp)
}

/// `/books/:hash/pages/:n` returns either the full page or a downscaled
/// thumbnail for the *same* URL depending on the request's `Prefer` header, so
/// caches must key on that header as well as on `Authorization`. Without this,
/// a client that cached one variant would keep serving it for the other for the
/// full 24h `max-age`.
fn vary_on_prefer(resp: &mut Response) {
    resp.headers_mut().insert(
        header::VARY,
        HeaderValue::from_static("Authorization, Prefer"),
    );
}

/// Get an opened archive from the cache (or open it and cache it). The open
/// itself is wrapped in spawn_blocking so it doesn't stall the runtime when
/// the archive isn't already cached.
///
/// Refuses any DB-stored path that doesn't canonicalize back inside the
/// configured library root — defense in depth against a tampered DB row
/// pointing at, say, `/etc/passwd`.
async fn open_archive(
    st: &AppState,
    hash: &str,
    path: &str,
) -> Result<std::sync::Arc<dyn archive::Book>, AppError> {
    let safe = crate::safe_path::under(&st.library_root, std::path::Path::new(path))
        .ok_or(AppError::NotFound)?;
    let cache = st.archive_cache.clone();
    let hash = hash.to_string();
    tokio::task::spawn_blocking(move || cache.get_or_open(&hash, &safe))
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
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let book = lookup_book(&st, &hash).await?;
    let safe = crate::safe_path::under(&st.library_root, Path::new(&book.path))
        .ok_or(AppError::NotFound)?;
    let mime = crate::opds::book_mime(&book.format);
    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok());
    serve_file_with_range(&safe, mime, range).await
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
        if let Some(safe) = crate::safe_path::under(&st.library_root, Path::new(cover)) {
            let mime = mime_guess::from_path(&safe)
                .first_raw()
                .unwrap_or("image/jpeg");
            return serve_file(&safe, mime).await;
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

/// Look up a book by content hash.
///
/// Several rows may share a hash when the same file exists at more than one
/// path. Their bytes are identical, so any of them serves the request equally
/// well; `ORDER BY id` just makes the choice deterministic across calls.
async fn lookup_book(st: &AppState, hash: &str) -> Result<Book, AppError> {
    sqlx::query_as("SELECT * FROM book WHERE hash = ? ORDER BY id LIMIT 1")
        .bind(hash)
        .fetch_optional(&st.pool)
        .await?
        .ok_or(AppError::NotFound)
}

/// What a `Range` header resolved to against a known file length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeSpec {
    /// Send the whole representation (no header, or one we deliberately ignore).
    Full,
    /// Send `start..=end_inclusive`.
    Partial { start: u64, end_inclusive: u64 },
    /// Syntactically valid but outside the file — must become a 416.
    Unsatisfiable,
}

/// Resolve a `Range` header against `len`.
///
/// Only single byte-ranges are honoured. Multi-range requests and unknown range
/// units fall back to [`RangeSpec::Full`], which RFC 7233 explicitly permits —
/// serving the whole representation is always a valid answer to a range request.
/// Malformed values are ignored the same way rather than rejected.
pub fn parse_range(raw: Option<&str>, len: u64) -> RangeSpec {
    let raw = match raw {
        Some(r) => r.trim(),
        None => return RangeSpec::Full,
    };
    let spec = match raw.get(..6) {
        Some(unit) if unit.eq_ignore_ascii_case("bytes=") => raw[6..].trim(),
        _ => return RangeSpec::Full,
    };
    // Multi-range would require a multipart/byteranges body; sending everything
    // is a legal alternative and keeps this simple.
    if spec.contains(',') {
        return RangeSpec::Full;
    }
    let (from, to) = match spec.split_once('-') {
        Some((f, t)) => (f.trim(), t.trim()),
        None => return RangeSpec::Full,
    };

    if from.is_empty() {
        // Suffix form: `-N` means the last N bytes.
        let n: u64 = match to.parse() {
            Ok(n) => n,
            Err(_) => return RangeSpec::Full,
        };
        if n == 0 || len == 0 {
            return RangeSpec::Unsatisfiable;
        }
        let start = len.saturating_sub(n);
        return RangeSpec::Partial {
            start,
            end_inclusive: len - 1,
        };
    }

    let start: u64 = match from.parse() {
        Ok(s) => s,
        Err(_) => return RangeSpec::Full,
    };
    if start >= len {
        return RangeSpec::Unsatisfiable;
    }
    let end_inclusive = if to.is_empty() {
        len - 1
    } else {
        match to.parse::<u64>() {
            Ok(e) => e.min(len - 1),
            Err(_) => return RangeSpec::Full,
        }
    };
    if end_inclusive < start {
        return RangeSpec::Unsatisfiable;
    }
    RangeSpec::Partial {
        start,
        end_inclusive,
    }
}

fn content_type_value(content_type: &str) -> HeaderValue {
    HeaderValue::from_str(content_type)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"))
}

/// Stream a file to the client without buffering it in memory.
///
/// Reading the whole file up front would let a handful of concurrent requests
/// for large archives (up to MAX_ARCHIVE_BYTES each) exhaust RAM, so the body is
/// a `ReaderStream` over the open handle instead.
async fn serve_file(path: &Path, content_type: &str) -> Result<Response, AppError> {
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let len = file
        .metadata()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .len();

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, content_type_value(content_type));
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(len));
    apply_immutable_response_headers(&mut headers);

    let body = Body::from_stream(ReaderStream::new(file));
    Ok((StatusCode::OK, headers, body).into_response())
}

/// Stream a file with single-range `Range` support, for the acquisition
/// endpoint where clients want to resume interrupted downloads.
async fn serve_file_with_range(
    path: &Path,
    content_type: &str,
    range: Option<&str>,
) -> Result<Response, AppError> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let len = file
        .metadata()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .len();

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, content_type_value(content_type));
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    apply_immutable_response_headers(&mut headers);

    match parse_range(range, len) {
        RangeSpec::Full => {
            headers.insert(header::CONTENT_LENGTH, HeaderValue::from(len));
            let body = Body::from_stream(ReaderStream::new(file));
            Ok((StatusCode::OK, headers, body).into_response())
        }
        RangeSpec::Partial {
            start,
            end_inclusive,
        } => {
            file.seek(std::io::SeekFrom::Start(start))
                .await
                .map_err(|e| AppError::Internal(e.to_string()))?;
            let count = end_inclusive - start + 1;
            if let Ok(v) =
                HeaderValue::from_str(&format!("bytes {}-{}/{}", start, end_inclusive, len))
            {
                headers.insert(header::CONTENT_RANGE, v);
            }
            headers.insert(header::CONTENT_LENGTH, HeaderValue::from(count));
            let body = Body::from_stream(ReaderStream::new(file.take(count)));
            Ok((StatusCode::PARTIAL_CONTENT, headers, body).into_response())
        }
        RangeSpec::Unsatisfiable => {
            headers.remove(header::CONTENT_TYPE);
            if let Ok(v) = HeaderValue::from_str(&format!("bytes */{}", len)) {
                headers.insert(header::CONTENT_RANGE, v);
            }
            Ok((
                StatusCode::RANGE_NOT_SATISFIABLE,
                headers,
                Body::empty(),
            )
                .into_response())
        }
    }
}

fn image_response(mime: &'static str, bytes: Vec<u8>) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
    apply_immutable_response_headers(&mut headers);
    (StatusCode::OK, headers, Body::from(bytes)).into_response()
}

/// 24h is fine for content-keyed URLs (book pages, covers, page thumbnails) —
/// the URL itself changes when the content does, so stale cache hits are
/// impossible.
const IMMUTABLE_CACHE_CONTROL: &str = "private, max-age=86400";

/// Short max-age for OPDS feed XML — same URL can return different bytes when
/// books are renamed/added/removed, so we want clients to refetch fairly soon.
const FEED_CACHE_CONTROL: &str = "private, max-age=60";

pub fn apply_immutable_response_headers(headers: &mut HeaderMap) {
    apply_common_security_headers(headers, IMMUTABLE_CACHE_CONTROL);
}

pub fn apply_feed_response_headers(headers: &mut HeaderMap) {
    apply_common_security_headers(headers, FEED_CACHE_CONTROL);
}

fn apply_common_security_headers(headers: &mut HeaderMap, cache_control: &'static str) {
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control),
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
        // Sanitize so a header value containing CR/LF/ESC/NUL can't forge log
        // lines or hijack a terminal viewing them.
        crate::logsafe::sanitize(value.to_str().unwrap_or("<binary>"))
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
