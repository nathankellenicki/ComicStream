// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::fs::File;
use std::io::Read;
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use lru::LruCache;
use parking_lot::Mutex;

mod cbr;
mod cbz;

pub use cbr::Cbr;
pub use cbz::Cbz;

/// Maximum decompressed bytes for a single image page. CBZ/CBR entries whose
/// declared uncompressed size exceeds this are rejected at archive-open time;
/// `read_page` also enforces it via `Read::take` so a header understating its
/// real size still can't blow up memory.
pub const MAX_PAGE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct PageEntry {
    pub name: String,
    pub mime: &'static str,
}

pub trait Book: Send + Sync {
    fn page_count(&self) -> usize;
    fn read_page(&self, index: usize) -> Result<(Vec<u8>, &'static str)>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Zip,
    Rar,
}

pub fn detect(path: &Path) -> Result<Kind> {
    let mut f = File::open(path)?;
    let mut buf = [0u8; 8];
    let n = f.read(&mut buf)?;
    let head = &buf[..n];
    if head.starts_with(b"PK\x03\x04")
        || head.starts_with(b"PK\x05\x06")
        || head.starts_with(b"PK\x07\x08")
    {
        Ok(Kind::Zip)
    } else if head.starts_with(b"Rar!\x1a\x07\x00") || head.starts_with(b"Rar!\x1a\x07\x01") {
        Ok(Kind::Rar)
    } else {
        Err(anyhow!("unknown archive magic in {}", path.display()))
    }
}

pub fn open(path: &Path) -> Result<Arc<dyn Book>> {
    match detect(path)? {
        Kind::Zip => Ok(Arc::new(Cbz::open(path)?) as Arc<dyn Book>),
        Kind::Rar => Ok(Arc::new(Cbr::open(path)?) as Arc<dyn Book>),
    }
}

/// LRU cache of opened archives, keyed by content hash. The expensive part of
/// `open()` is parsing the archive's directory (cheap for ZIP, expensive for
/// RAR), so caching avoids redoing that on every page request.
pub struct Cache {
    inner: Mutex<LruCache<String, Arc<dyn Book>>>,
}

impl Cache {
    pub fn new(capacity: usize) -> Self {
        let cap = NonZeroUsize::new(capacity.max(1)).unwrap();
        Self {
            inner: Mutex::new(LruCache::new(cap)),
        }
    }

    pub fn get_or_open(&self, hash: &str, path: &Path) -> Result<Arc<dyn Book>> {
        if let Some(book) = self.inner.lock().get(hash).cloned() {
            return Ok(book);
        }
        // Open without holding the lock; tolerate the rare race where two
        // requests open the same archive concurrently.
        let book = open(path)?;
        self.inner.lock().put(hash.to_string(), book.clone());
        Ok(book)
    }
}

pub fn is_supported(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("cbz") | Some("zip") | Some("cbr") | Some("rar")
    )
}

pub(crate) fn image_mime(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    let ext = std::path::Path::new(&lower).extension()?.to_str()?;
    Some(match ext {
        "jpg" | "jpeg" | "jpe" | "jfif" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => return None,
    })
}
